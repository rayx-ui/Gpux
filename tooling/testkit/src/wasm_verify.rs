//! Verifies a threaded browser build: shared WebAssembly memory, the `wasm_thread_entry_point`
//! export web workers start through, atomic instructions, and the explicit constructor path.
//!
//! `gpui_platform::web_init` calls `__wasm_call_ctors` itself. That explicit reference makes
//! wasm-ld treat the module as a reactor, so it does not wrap every export in a
//! `<name>.command_export` function that reruns the constructors, which would rerun them on
//! every web worker's entry. A verified build has exactly one constructor call site (in release
//! builds `web_init` is usually inlined into the app's entry function) and no wrappers.

use std::fmt;
use wasmparser::{BinaryReaderError, ExternalKind, Name, Operator, Parser, Payload, TypeRef};

/// What a verified threaded build contains.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verification {
    /// Whether the module's memory is shared.
    pub shared_memory: bool,
    /// Whether it exports `wasm_thread_entry_point`.
    pub thread_entry_point: bool,
    /// How many atomic instructions it contains.
    pub atomic_instructions: usize,
    /// The functions that call `__wasm_call_ctors`, by name.
    pub constructor_callers: Vec<String>,
}

impl fmt::Display for Verification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "shared memory, wasm_thread_entry_point, {} atomic instructions, constructors called once (from {}), no export wrappers",
            self.atomic_instructions,
            self.constructor_callers.join(", ")
        )
    }
}

/// Reads `bytes` and checks it is a threaded build that calls its constructors explicitly.
pub fn verify_threaded(bytes: &[u8]) -> Result<Verification, String> {
    let module =
        inspect(bytes).map_err(|error| format!("not a valid WebAssembly module: {error}"))?;
    if !module.shared_memory {
        return Err(
            "the module's memory is not shared: build with `+atomics` and `--shared-memory`".into(),
        );
    }
    if !module.thread_entry_point {
        return Err(
            "the module does not export `wasm_thread_entry_point`, which web workers start through"
                .into(),
        );
    }
    if module.atomic_instructions == 0 {
        return Err("the module contains no atomic instructions: build std with `-Z build-std` and `+atomics`".into());
    }
    let Some(_) = module.constructors else {
        return Err("the module has no `__wasm_call_ctors` in its name section: keep the name section (do not strip it)".into());
    };
    if !module.command_export_wrappers.is_empty() {
        return Err(format!(
            "wasm-ld wrapped the exports in constructor calls ({}): call `gpui_platform::web_init`              first, whose explicit `__wasm_call_ctors` call makes the module a reactor",
            module.command_export_wrappers.join(", ")
        ));
    }
    match module.constructor_callers.as_slice() {
        [_] => {}
        [] => {
            return Err(
                "nothing calls `__wasm_call_ctors`: initialize through `gpui_platform::web_init`"
                    .into(),
            );
        }
        callers => {
            return Err(format!(
                "`__wasm_call_ctors` must have one call site, `gpui_platform::web_init`; it is                  called from {}",
                callers.join(", ")
            ));
        }
    }
    Ok(Verification {
        shared_memory: module.shared_memory,
        thread_entry_point: module.thread_entry_point,
        atomic_instructions: module.atomic_instructions,
        constructor_callers: module.constructor_callers,
    })
}

#[derive(Default)]
struct Module {
    shared_memory: bool,
    thread_entry_point: bool,
    atomic_instructions: usize,
    constructors: Option<u32>,
    constructor_callers: Vec<String>,
    command_export_wrappers: Vec<String>,
}

fn inspect(bytes: &[u8]) -> Result<Module, BinaryReaderError> {
    let mut module = Module::default();
    let mut imported_functions = 0u32;
    let mut names = std::collections::HashMap::new();
    let mut bodies = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    match import?.ty {
                        TypeRef::Memory(memory) => module.shared_memory |= memory.shared,
                        TypeRef::Func(_) => imported_functions += 1,
                        _ => {}
                    }
                }
            }
            Payload::MemorySection(reader) => {
                for memory in reader {
                    module.shared_memory |= memory?.shared;
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    if export.kind == ExternalKind::Func && export.name == "wasm_thread_entry_point"
                    {
                        module.thread_entry_point = true;
                    }
                }
            }
            Payload::CodeSectionEntry(body) => bodies.push(body),
            Payload::CustomSection(section) if section.name() == "name" => {
                let reader = wasmparser::NameSectionReader::new(wasmparser::BinaryReader::new(
                    section.data(),
                    section.data_offset(),
                ));
                for name in reader {
                    if let Name::Function(map) = name? {
                        for naming in map {
                            let naming = naming?;
                            names.insert(naming.index, naming.name.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    module.command_export_wrappers = names
        .values()
        .filter(|name| name.ends_with(".command_export"))
        .cloned()
        .collect();
    module.command_export_wrappers.sort();
    module.constructors = names
        .iter()
        .find(|(_, name)| name.as_str() == "__wasm_call_ctors")
        .map(|(index, _)| *index);
    for (offset, body) in bodies.iter().enumerate() {
        let index = imported_functions + offset as u32;
        let mut calls_constructors = false;
        let mut reader = body.get_operators_reader()?;
        while !reader.eof() {
            let operator = reader.read()?;
            if is_atomic(&operator) {
                module.atomic_instructions += 1;
            }
            if let (Operator::Call { function_index }, Some(constructors)) =
                (&operator, module.constructors)
                && *function_index == constructors
            {
                calls_constructors = true;
            }
        }
        if calls_constructors {
            module.constructor_callers.push(
                names
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| format!("function {index}")),
            );
        }
    }
    Ok(module)
}

fn is_atomic(operator: &Operator<'_>) -> bool {
    matches!(
        operator,
        Operator::MemoryAtomicNotify { .. }
            | Operator::MemoryAtomicWait32 { .. }
            | Operator::MemoryAtomicWait64 { .. }
            | Operator::AtomicFence
            | Operator::I32AtomicLoad { .. }
            | Operator::I64AtomicLoad { .. }
            | Operator::I32AtomicStore { .. }
            | Operator::I64AtomicStore { .. }
            | Operator::I32AtomicRmwAdd { .. }
            | Operator::I64AtomicRmwAdd { .. }
            | Operator::I32AtomicRmwCmpxchg { .. }
            | Operator::I64AtomicRmwCmpxchg { .. }
            | Operator::I32AtomicRmwXchg { .. }
            | Operator::I64AtomicRmwXchg { .. }
    )
}
