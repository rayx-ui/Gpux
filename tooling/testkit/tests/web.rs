//! Lane B's browser commands: `web` argument parsing, the threaded-binary verification, the
//! cross-origin isolated server, and the build steps `web build` runs.

use gpux_testkit::{
    Environment, Os,
    cli::{Command, E2eArgs, WebCommand, parse},
    execute,
    lanes::Invocation,
    run::{RunOutput, Runner},
    serve,
    summary::{EXIT_FAILED, EXIT_USAGE},
    wasm_verify::verify_threaded,
    web,
};
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, MemArg, MemoryType, Module, NameMap, NameSection, TypeSection,
};

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

#[test]
fn web_commands_parse() {
    assert_eq!(
        parse(args("web build --verify")).unwrap(),
        Command::Web(WebCommand::Build { verify: true })
    );
    assert_eq!(
        parse(args("web build")).unwrap(),
        Command::Web(WebCommand::Build { verify: false })
    );
    assert_eq!(
        parse(args("web serve")).unwrap(),
        Command::Web(WebCommand::Serve { port: 8000 })
    );
    assert_eq!(
        parse(args("web serve --port 9123")).unwrap(),
        Command::Web(WebCommand::Serve { port: 9123 })
    );
    assert_eq!(
        parse(args(
            "web e2e --profile ci-linux-swiftshader --grep @canary --headed"
        ))
        .unwrap(),
        Command::Web(WebCommand::E2e(E2eArgs {
            profile: "ci-linux-swiftshader".into(),
            grep: Some("@canary".into()),
            headed: true,
        }))
    );
    assert_eq!(
        parse(args("web e2e")).unwrap(),
        Command::Web(WebCommand::E2e(E2eArgs {
            profile: "local-gpu".into(),
            grep: None,
            headed: false,
        }))
    );
}

#[test]
fn bad_web_command_lines_fail_with_actionable_messages() {
    let error = |line: &str| parse(args(line)).unwrap_err().to_string();
    assert!(error("web").contains("`build`, `serve` or `e2e`"));
    assert!(error("web deploy").contains("unknown web command `deploy`"));
    assert!(error("web build --port 1").contains("unknown option `--port` for `web build`"));
    assert!(error("web serve --port nine").contains("not a port number"));
    assert!(error("web serve --port").contains("needs a value"));
    assert!(error("web e2e --grep a --grep b").contains("twice"));
}

/// A module laid out the way wasm-ld links a threaded build.
struct ModuleSpec {
    shared: bool,
    thread_entry_point: bool,
    atomics: bool,
    names: bool,
    /// Exported functions besides the thread entry point, with whether each calls the
    /// constructors.
    exports: Vec<(&'static str, bool)>,
}

impl Default for ModuleSpec {
    fn default() -> Self {
        Self {
            shared: true,
            thread_entry_point: true,
            atomics: true,
            names: true,
            exports: vec![("run", true)],
        }
    }
}

/// Function 0 is an imported host function, function 1 is `__wasm_call_ctors`, then come the
/// exports and the thread entry point.
fn module(spec: &ModuleSpec) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([], []);
    module.section(&types);

    let mut imports = ImportSection::new();
    imports.import("env", "host", EntityType::Function(0));
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: Some(16),
            memory64: false,
            shared: spec.shared,
            page_size_log2: None,
        }),
    );
    module.section(&imports);

    let mut bodies: Vec<(String, Function)> = vec![("__wasm_call_ctors".into(), {
        let mut ctors = Function::new([]);
        ctors.instruction(&Instruction::End);
        ctors
    })];
    for (name, calls_ctors) in &spec.exports {
        let mut body = Function::new([]);
        if *calls_ctors {
            body.instruction(&Instruction::Call(1));
        }
        body.instruction(&Instruction::Call(0));
        body.instruction(&Instruction::End);
        bodies.push((name.to_string(), body));
    }
    if spec.thread_entry_point {
        let mut entry = Function::new([]);
        if spec.atomics {
            entry.instruction(&Instruction::I32Const(0));
            entry.instruction(&Instruction::I32Const(1));
            entry.instruction(&Instruction::I32AtomicRmwAdd(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
            entry.instruction(&Instruction::Drop);
        }
        entry.instruction(&Instruction::End);
        bodies.push(("wasm_thread_entry_point".into(), entry));
    }

    let mut functions = FunctionSection::new();
    for _ in &bodies {
        functions.function(0);
    }
    module.section(&functions);

    let mut exports = ExportSection::new();
    for (index, (name, _)) in bodies.iter().enumerate().skip(1) {
        // A wrapper stands in for its export; the tests keep the export on the original.
        if !name.ends_with(".command_export") {
            exports.export(name, ExportKind::Func, index as u32 + 1);
        }
    }
    module.section(&exports);

    let mut code = CodeSection::new();
    for (_, body) in &bodies {
        code.function(body);
    }
    module.section(&code);

    if spec.names {
        let mut map = NameMap::new();
        map.append(0, "host");
        for (index, (name, _)) in bodies.iter().enumerate() {
            map.append(index as u32 + 1, name);
        }
        let mut names = NameSection::new();
        names.functions(&map);
        module.section(&names);
    }
    module.finish()
}

#[test]
fn a_threaded_reactor_build_verifies() {
    let verification = verify_threaded(&module(&ModuleSpec::default())).unwrap();
    assert!(verification.shared_memory && verification.thread_entry_point);
    assert_eq!(verification.atomic_instructions, 1);
    assert_eq!(verification.constructor_callers, ["run"]);
    assert!(
        verification
            .to_string()
            .contains("constructors called once (from run)")
    );
}

#[test]
fn verification_rejects_builds_that_cannot_run_threads() {
    let reject = |spec: ModuleSpec| verify_threaded(&module(&spec)).unwrap_err();
    assert!(
        reject(ModuleSpec {
            shared: false,
            ..ModuleSpec::default()
        })
        .contains("memory is not shared")
    );
    assert!(
        reject(ModuleSpec {
            thread_entry_point: false,
            ..ModuleSpec::default()
        })
        .contains("wasm_thread_entry_point")
    );
    assert!(
        reject(ModuleSpec {
            atomics: false,
            ..ModuleSpec::default()
        })
        .contains("no atomic instructions")
    );
    assert!(
        reject(ModuleSpec {
            names: false,
            ..ModuleSpec::default()
        })
        .contains("name section")
    );
    assert!(
        verify_threaded(b"not wasm")
            .unwrap_err()
            .contains("not a valid WebAssembly")
    );
}

#[test]
fn verification_rejects_implicit_constructor_paths() {
    // Nothing calls the constructors: the app skipped `web_init`.
    let error = verify_threaded(&module(&ModuleSpec {
        exports: vec![("run", false)],
        ..ModuleSpec::default()
    }))
    .unwrap_err();
    assert!(
        error.contains("nothing calls `__wasm_call_ctors`"),
        "{error}"
    );

    // wasm-ld's command wrappers rerun the constructors on every export, worker entries included.
    let error = verify_threaded(&module(&ModuleSpec {
        exports: vec![("run", false), ("run.command_export", true)],
        ..ModuleSpec::default()
    }))
    .unwrap_err();
    assert_eq!(
        error,
        "wasm-ld wrapped the exports in constructor calls (run.command_export): call \
         `gpui_platform::web_init` first, whose explicit `__wasm_call_ctors` call makes the module \
         a reactor"
    );

    // Two call sites run the constructors twice.
    let error = verify_threaded(&module(&ModuleSpec {
        exports: vec![("run", true), ("start", true)],
        ..ModuleSpec::default()
    }))
    .unwrap_err();
    assert_eq!(
        error,
        "`__wasm_call_ctors` must have one call site, `gpui_platform::web_init`; it is called \
         from run, start"
    );
}

fn request(address: SocketAddr, line: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(address).unwrap();
    write!(stream, "{line}\r\nHost: localhost\r\nAccept: */*\r\n\r\n").unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a header block");
    let head = String::from_utf8(response[..split].to_vec()).unwrap();
    (head, response[split + 4..].to_vec())
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

#[test]
fn the_server_isolates_the_page_and_falls_back_to_index() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.html"), "<!doctype html>page").unwrap();
    std::fs::write(root.path().join("web_host_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    std::fs::create_dir(root.path().join("snippets")).unwrap();
    std::fs::write(root.path().join("snippets").join("worker.js"), "export {}").unwrap();
    let (address, _server) = serve::start(root.path().to_path_buf(), 0).unwrap();

    let (head, body) = request(address, "GET / HTTP/1.1");
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert_eq!(body, b"<!doctype html>page");
    assert_eq!(
        header(&head, "Content-Type"),
        Some("text/html; charset=utf-8")
    );
    for (name, value) in serve::ISOLATION_HEADERS {
        assert_eq!(header(&head, name), Some(value), "{head}");
    }
    assert_eq!(header(&head, "Cache-Control"), Some("no-store"));

    let (head, body) = request(address, "GET /web_host_bg.wasm HTTP/1.1");
    assert_eq!(header(&head, "Content-Type"), Some("application/wasm"));
    assert_eq!(body, b"\0asm\x01\0\0\0");
    let (head, _) = request(address, "GET /snippets/worker.js?v=1 HTTP/1.1");
    assert_eq!(
        header(&head, "Content-Type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(
        header(&head, "Cross-Origin-Embedder-Policy"),
        Some("require-corp")
    );

    // Routes without an extension are the app's; missing assets are not.
    let (head, body) = request(address, "GET /scenes/paint-borders?scene=x HTTP/1.1");
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert_eq!(body, b"<!doctype html>page");
    let (head, _) = request(address, "GET /missing.js HTTP/1.1");
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");

    let (head, _) = request(address, "GET /../Cargo.toml HTTP/1.1");
    assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    let (head, body) = request(address, "HEAD /index.html HTTP/1.1");
    assert!(
        head.starts_with("HTTP/1.1 200 OK") && body.is_empty(),
        "{head}"
    );
    assert_eq!(header(&head, "Content-Length"), Some("19"));
    let (head, _) = request(address, "POST / HTTP/1.1");
    assert!(head.starts_with("HTTP/1.1 405"), "{head}");
}

#[test]
fn the_build_uses_the_locked_wasm_bindgen_and_threaded_flags() {
    let lock = "[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\nsource = \"registry\"\n\n\
                [[package]]\nname = \"wasm-bindgen-futures\"\nversion = \"0.4.50\"\n";
    assert_eq!(
        web::locked_wasm_bindgen_version(lock).as_deref(),
        Some("0.2.129")
    );
    assert_eq!(
        web::parse_wasm_bindgen_version("wasm-bindgen 0.2.129\n").as_deref(),
        Some("0.2.129")
    );
    assert_eq!(
        web::parse_wasm_bindgen_version("wasm-bindgen 0.2.100 (abc123)").as_deref(),
        Some("0.2.100")
    );

    let paths = web::WebPaths::new(Path::new("/repo"));
    let build = web::cargo_build("nightly-2026-09-30", &paths).command_line();
    for part in [
        "cargo +nightly-2026-09-30 build",
        "--example web_host",
        "--target wasm32-unknown-unknown",
        "--release",
        "-Z build-std=std,panic_abort",
        "+atomics,+bulk-memory,+mutable-globals",
        "--shared-memory",
        "--import-memory",
        "--export=__wasm_init_tls",
    ] {
        assert!(build.contains(part), "{part} is missing from {build}");
    }

    let catalog = web::catalog_json();
    assert!(
        catalog.starts_with(
            "[
  {\"id\": \""
        ),
        "{catalog}"
    );
    assert!(
        catalog.contains(
            "{\"id\": \"overlay-anchored\", \"scales\": [1, 2], \"areas\": [\"AnchoredOverlay\"], \"clock\": false}"
        ),
        "{catalog}"
    );
    assert_eq!(
        catalog.matches("\"id\"").count(),
        gpux_conformance::CATALOG
            .iter()
            .filter(|scene| scene.runs_in(gpux_conformance::Lane::B))
            .count()
    );

    let page = web::index_html();
    for part in [
        "import('./web_host.js')",
        "'gpux-html-island'",
        "'rayx-html-island'",
        "crossOriginIsolated",
        "run();",
    ] {
        assert!(page.contains(part), "{part} is missing from the page");
    }
}

struct FakeEnvironment {
    root: PathBuf,
    programs: Vec<&'static str>,
}

impl Environment for FakeEnvironment {
    fn var(&self, _name: &str) -> Option<String> {
        None
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.programs
            .contains(&name)
            .then(|| PathBuf::from(format!("/fake/bin/{name}")))
    }

    fn os(&self) -> Os {
        Os::Linux
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
}

#[derive(Default)]
struct FakeRunner {
    outputs: Vec<RunOutput>,
    ran: Vec<String>,
}

impl Runner for FakeRunner {
    fn run(
        &mut self,
        invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        self.ran.push(invocation.command_line());
        Ok(self.outputs.remove(0))
    }
}

fn output(text: &str) -> RunOutput {
    RunOutput {
        success: true,
        code: Some(0),
        output: text.to_string(),
    }
}

fn run(line: &str, environment: &FakeEnvironment, runner: &mut FakeRunner) -> (i32, String) {
    let mut out = Vec::new();
    let code = execute(args(line), environment, runner, &mut out);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn web_build_runs_cargo_then_wasm_bindgen_and_writes_the_page() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Cargo.lock"),
        "[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n",
    )
    .unwrap();
    let environment = FakeEnvironment {
        root: root.path().to_path_buf(),
        programs: vec!["cargo", "wasm-bindgen"],
    };
    let mut runner = FakeRunner {
        outputs: vec![output("wasm-bindgen 0.2.129"), output(""), output("")],
        ..FakeRunner::default()
    };
    let (code, text) = run("web build", &environment, &mut runner);
    assert_eq!(code, 0, "{text}");
    assert_eq!(runner.ran.len(), 3);
    assert!(runner.ran[0].ends_with("wasm-bindgen --version"));
    assert!(runner.ran[1].contains("cargo +nightly build"));
    assert!(runner.ran[2].contains("--target web --out-dir"));
    let page = root.path().join("target/gpux-web/dist/index.html");
    assert_eq!(std::fs::read_to_string(page).unwrap(), web::index_html());
    let dist = root.path().join("target/gpux-web/dist");
    assert!(dist.join("catalog.json").is_file());
    assert_eq!(
        std::fs::read_to_string(dist.join("assets/conformance.svg")).unwrap(),
        web::CONFORMANCE_SVG
    );
    assert_eq!(
        text.lines().last(),
        Some("gpux-testkit: 1 passed, 0 failed, 0 skipped")
    );
}

#[test]
fn web_commands_report_missing_prerequisites() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Cargo.lock"),
        "[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n",
    )
    .unwrap();
    let environment = FakeEnvironment {
        root: root.path().to_path_buf(),
        programs: vec!["cargo", "wasm-bindgen"],
    };

    // A CLI that does not match the lock is refused before anything builds.
    let mut runner = FakeRunner {
        outputs: vec![output("wasm-bindgen 0.2.100")],
        ..FakeRunner::default()
    };
    let (code, text) = run("web build --verify", &environment, &mut runner);
    assert_eq!(code, EXIT_FAILED);
    assert!(
        text.contains("wasm-bindgen 0.2.100 is installed but Cargo.lock pins 0.2.129")
            && text.contains("cargo install wasm-bindgen-cli --version 0.2.129"),
        "{text}"
    );
    assert_eq!(runner.ran.len(), 1);

    let (code, text) = run(
        "web e2e --profile gpu",
        &environment,
        &mut FakeRunner::default(),
    );
    assert_eq!(code, EXIT_FAILED);
    assert!(text.contains("unknown profile `gpu`"), "{text}");

    let (code, text) = run("web e2e", &environment, &mut FakeRunner::default());
    assert_eq!(code, EXIT_FAILED);
    assert!(
        text.contains("Playwright is not installed") && text.contains("pnpm install"),
        "{text}"
    );

    let (code, text) = run("web serve", &environment, &mut FakeRunner::default());
    assert_eq!(code, EXIT_FAILED);
    assert!(
        text.contains("run `gpux-testkit web build` first"),
        "{text}"
    );

    let (code, _) = run("web", &environment, &mut FakeRunner::default());
    assert_eq!(code, EXIT_USAGE);
}

#[test]
fn lane_b_selection_maps_to_a_playwright_grep() {
    use gpux_testkit::cli::Selection;
    let selection = |scene: Option<&str>, grep: Option<&str>| Selection {
        scene: scene.map(str::to_string),
        grep: grep.map(str::to_string),
    };
    assert_eq!(web::lane_grep(&selection(None, None)), "@canary|@pr");
    assert_eq!(web::lane_grep(&selection(None, Some("@dpr2"))), "@dpr2");
    assert_eq!(
        web::lane_grep(&selection(Some("layout-flex"), None)),
        "layout-flex passes"
    );
    let environment = FakeEnvironment {
        root: PathBuf::new(),
        programs: Vec::new(),
    };
    assert_eq!(web::lane_profile(&environment), "local-gpu");
    assert_eq!(
        web::playwright_list(
            "Listing tests:\n  [local-gpu] › specs/canary.spec.ts:6:1 › WebGPU starts @canary\n\
             Total: 1 test in 1 file\n"
        ),
        ["[local-gpu] › specs/canary.spec.ts:6:1 › WebGPU starts @canary"]
    );
}

/// A checkout with the locked wasm-bindgen, Playwright installed, and a verified build in place.
fn browser_checkout() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Cargo.lock"),
        "[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.path().join("tools-node/node_modules/@playwright/test")).unwrap();
    let dist = root.path().join("target/gpux-web/dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(
        dist.join("web_host_bg.wasm"),
        module(&ModuleSpec::default()),
    )
    .unwrap();
    root
}

fn build_outputs() -> Vec<RunOutput> {
    vec![output("wasm-bindgen 0.2.129"), output(""), output("")]
}

#[test]
fn lane_b_lists_its_specs_without_running_them() {
    let root = browser_checkout();
    let environment = FakeEnvironment {
        root: root.path().to_path_buf(),
        programs: vec!["cargo", "wasm-bindgen", "pnpm"],
    };
    let mut outputs = build_outputs();
    outputs.push(output(
        "Listing tests:\n  [local-gpu] › specs/canary.spec.ts:6:1 › WebGPU starts @canary\n  \
         [local-gpu] › specs/scenes.spec.ts:13:3 › layout-flex passes its script @pr\n\
         Total: 2 tests in 2 files\n",
    ));
    let mut runner = FakeRunner {
        outputs,
        ..FakeRunner::default()
    };
    let (code, text) = run("lane b --list", &environment, &mut runner);
    assert_eq!(code, 0, "{text}");
    assert!(
        runner.ran[3]
            .contains("pnpm exec playwright test --project local-gpu --grep @canary|@pr --list"),
        "{}",
        runner.ran[3]
    );
    assert!(text.contains("layout-flex passes its script @pr"), "{text}");
    assert_eq!(
        text.lines().last(),
        Some("gpux-testkit: 0 passed, 0 failed, 0 skipped")
    );
}

#[test]
fn lane_b_runs_a_scenes_specs_and_counts_their_results() {
    let root = browser_checkout();
    let environment = FakeEnvironment {
        root: root.path().to_path_buf(),
        programs: vec!["cargo", "wasm-bindgen", "pnpm"],
    };
    let mut outputs = build_outputs();
    outputs.push(output(
        "Running 2 tests using 2 workers\n  ok 1 [local-gpu] › specs/scenes.spec.ts:13:3 › \
         layout-flex passes its script @pr (3s)\n  ok 2 [local-gpu] › specs/scenes.spec.ts:27:5 › \
         layout-flex passes its script at DPR 2 @dpr2 (3s)\n  2 passed (4s)\n",
    ));
    let mut runner = FakeRunner {
        outputs,
        ..FakeRunner::default()
    };
    let (code, text) = run("lane b --scene layout-flex", &environment, &mut runner);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("threaded wasm verified"), "{text}");
    assert!(
        runner.ran[3].starts_with("GPUX_BASE_URL=http://127.0.0.1:")
            && runner.ran[3].ends_with(
                "pnpm exec playwright test --project local-gpu --grep layout-flex passes"
            ),
        "{}",
        runner.ran[3]
    );
    assert_eq!(
        text.lines().last(),
        Some("gpux-testkit: 2 passed, 0 failed, 0 skipped")
    );
}
