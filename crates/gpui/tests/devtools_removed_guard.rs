//! The per-element DevKit inspection layer is gone: gpux keeps one semantic tree, AccessKit's.
//! These tests scan `crates/` for the retired types, methods, frame records and gates, and for the
//! retired `devtools` Cargo feature, and fail naming each file and symbol they find. The
//! `gpux-devtools` crates, which inspect apps through the AccessKit tree, are not retired.
//!
//! The symbols are spelled in pieces here so this file does not name them itself.

use std::path::{Path, PathBuf};

/// The retired symbols, each matched as a substring of a file's text.
fn retired_symbols() -> Vec<String> {
    let devtools = "Devtools";
    let snake = "devtools_";
    let mut symbols: Vec<String> = [
        "Semantics",
        "WritableProperty",
        "PropertySetter",
        "PropertyCallback",
        "PointerAction",
        "FrameSnapshot",
        "ElementSnapshot",
        "EffectSnapshot",
        "Bounds",
        "ElementSource",
        "FrameNode",
    ]
    .iter()
    .map(|name| format!("{devtools}{name}"))
    .collect();
    // `InspectableElement` covers `InspectableElementExt`.
    symbols.push(format!("Inspectable{}", "Element"));
    symbols.extend(
        [
            "frame_snapshot",
            "effect_snapshot",
            // Covers `set_devtools_semantics` and `set_devtools_semantics_with`.
            "semantics",
            // Covers `on_devtools_property`, `set_devtools_property` and
            // `register_devtools_property_setter`.
            "property",
            // Covers the frames' node records and `PrepaintStateIndex`'s index into them.
            "nodes",
            "node_id",
            "node_stack",
            "node_indices",
        ]
        .iter()
        .map(|name| format!("{snake}{name}")),
    );
    symbols.extend(
        [
            ("record_", "element"),
            ("push_", "node"),
            ("push_existing_", "node"),
            ("pop_", "node"),
            ("update_", "element_after_paint"),
        ]
        .iter()
        .map(|(verb, rest)| format!("{verb}{snake}{rest}")),
    );
    symbols.push(format!("settled_{}", "frame_snapshot"));
    // Devtools pointer actions are gone, and with them the scope that hid them.
    symbols.push(format!("with_pointer_{}", "semantics_suppressed"));
    // The `DEVTOOLS` gate and its uses; a `DEVTOOLS` scene of another type is not one.
    let gate = "DEVTOOLS";
    symbols.push(format!("const {gate}: bool"));
    symbols.push(format!("window::{gate}"));
    symbols.push(format!("if {gate} "));
    symbols.push(format!("!{gate} "));
    symbols
}

/// The retired symbols `text` holds, one finding each.
fn symbol_findings(text: &str) -> Vec<String> {
    retired_symbols()
        .into_iter()
        .filter(|symbol| text.contains(symbol.as_str()))
        .collect()
}

/// The `devtools` feature a Cargo manifest's `text` defines in `[features]` or requests from a
/// dependency, one finding per line.
fn feature_findings(text: &str) -> Vec<String> {
    let feature = "devtools";
    let mut findings = Vec::new();
    let mut in_features = false;
    for (line_number, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_features = trimmed == "[features]";
            continue;
        }
        // Package metadata names the word, not a feature.
        if trimmed.starts_with("keywords") || trimmed.starts_with("categories") {
            continue;
        }
        let defines = in_features
            && trimmed
                .strip_prefix(feature)
                .is_some_and(|rest| rest.trim_start().starts_with('='));
        let requests = trimmed.contains(&format!("\"{feature}\""))
            || trimmed.contains(&format!("/{feature}\""));
        if defines || requests {
            findings.push(format!("line {}: feature `{feature}`", line_number + 1));
        }
    }
    findings
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/gpui sits in crates/")
        .to_path_buf()
}

/// Every file under `dir`, skipping build output and vendored sources.
fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), "target" | "vendor" | "node_modules" | ".git") {
                continue;
            }
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Every finding under `crates/`, as `file: symbol`, and how many files were read.
fn scan_crates() -> (Vec<String>, usize) {
    let crates = crates_dir();
    let guard = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/devtools_removed_guard.rs");
    let mut paths = Vec::new();
    files(&crates, &mut paths);
    let mut findings = Vec::new();
    let mut read = 0;
    for path in paths {
        if path == guard {
            continue;
        }
        // Binary files (fonts, images) hold no source.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        read += 1;
        let shown = path.strip_prefix(&crates).unwrap_or(&path).display();
        for symbol in symbol_findings(&text) {
            findings.push(format!("crates/{shown}: {symbol}"));
        }
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            for finding in feature_findings(&text) {
                findings.push(format!("crates/{shown}: {finding}"));
            }
        }
    }
    (findings, read)
}

#[test]
fn crates_hold_no_retired_devtools_symbol_or_feature() {
    let (findings, read) = scan_crates();
    // The scan reaches the sources: gpux alone has hundreds of files.
    assert!(read > 300, "the scan read only {read} files");
    assert!(
        findings.is_empty(),
        "retired DevKit inspection symbols remain:\n{}",
        findings.join("\n")
    );
}

#[test]
fn gpux_defines_no_devtools_feature() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("crates/gpui/Cargo.toml");
    assert!(manifest.contains("[features]"));
    let findings = feature_findings(&manifest);
    assert!(findings.is_empty(), "crates/gpui/Cargo.toml: {findings:?}");
}

#[test]
fn the_scan_finds_a_seeded_occurrence_of_each_symbol() {
    for symbol in retired_symbols() {
        let seeded = format!("fn seeded() {{\n    let _ = {symbol}x;\n}}\n");
        assert_eq!(
            symbol_findings(&seeded),
            std::slice::from_ref(&symbol),
            "seeded `{symbol}` is found, and only it"
        );
    }
    let feature = "devtools";
    let defined = format!("[features]\ndefault = []\n{feature} = []\n");
    assert_eq!(feature_findings(&defined).len(), 1, "{defined}");
    let in_default = format!("[features]\ndefault = [\"font-kit\", \"{feature}\"]\n");
    assert_eq!(feature_findings(&in_default).len(), 1, "{in_default}");
    let forwarded = format!("[features]\ntest-support = [\"gpux/{feature}\"]\n");
    assert_eq!(feature_findings(&forwarded).len(), 1, "{forwarded}");
    let requested =
        format!("[dependencies]\ngpux = {{ workspace = true, features = [\"{feature}\"] }}\n");
    assert_eq!(feature_findings(&requested).len(), 1, "{requested}");
}

#[test]
fn the_scan_leaves_the_accesskit_devtools_alone() {
    let legitimate = "\
use gpux_devtools::Options;
const TOKEN_ENV: &str = \"GPUX_DEVTOOLS_TOKEN\";
pub const DEVTOOLS: SceneSpec = SceneSpec { id: \"web-devtools\" };
/// Tests and the devtools read it through `Self::a11y_tree`.
";
    assert_eq!(symbol_findings(legitimate), Vec::<String>::new());
    let manifest = "\
[package]
name = \"gpux-devtools\"
keywords = [\"gpux\", \"devtools\", \"accessibility\"]

[features]
app-host = [\"dep:gpux_devtools\", \"dep:gpux_fonts\"]

[dependencies]
gpux_devtools = { workspace = true, optional = true }
";
    assert_eq!(feature_findings(manifest), Vec::<String>::new());
}
