//! gpux names its diagnostics surface itself: a `diagnostics` feature, `__gpux*` browser globals
//! and the `?gpux-html-island=1` opt-in. The former names keep working as aliases until consumers
//! move: `rayx_diagnostics` enables `diagnostics`, `__rayx*` globals are still read (and published
//! under the alias feature), and `?rayx-html-island=1` is still honoured.

use std::path::{Path, PathBuf};

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf()
}

/// The crates that carry diagnostics: `gpux`, `gpux-wgpu-renderer` and `gpux-web`.
const DIAGNOSTICS_CRATES: [&str; 3] = ["gpui", "gpui_wgpu", "gpui_web"];

/// Every browser global these crates published as `__rayx<Name>` before the gpux names.
const GLOBALS: [&str; 12] = [
    "FontFallbackDiagnostics",
    "HtmlElementDiagnostics",
    "HtmlIslandDiagnostics",
    "HtmlIslandEnabled",
    "HtmlIslandInjectUploadFailureId",
    "MediaQueryDiagnosticsEnabled",
    "MediaQueryDiagnosticsResetEpoch",
    "MediaQueryPipeline",
    "TextRendererDiagnostics",
    "TextRenderingDiagnosticsEnabled",
    "WasmTextScaleDiagnostics",
    "WgpuFrameDiagnostics",
];

fn sources(crate_name: &str) -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, files: &mut Vec<(PathBuf, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                files.push((path, text));
            }
        }
    }
    let mut files = Vec::new();
    walk(&crates_dir().join(crate_name).join("src"), &mut files);
    files
}

fn feature_line<'a>(manifest: &'a str, feature: &str) -> Option<&'a str> {
    manifest
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{feature} = [")))
}

#[test]
fn rayx_diagnostics_is_an_alias_of_diagnostics_in_every_manifest() {
    for crate_name in DIAGNOSTICS_CRATES {
        let manifest =
            std::fs::read_to_string(crates_dir().join(crate_name).join("Cargo.toml")).unwrap();
        assert!(
            feature_line(&manifest, "diagnostics").is_some(),
            "{crate_name} has no `diagnostics` feature"
        );
        let alias = feature_line(&manifest, "rayx_diagnostics")
            .unwrap_or_else(|| panic!("{crate_name} dropped the `rayx_diagnostics` alias"));
        assert!(
            alias.contains("\"diagnostics\""),
            "{crate_name}'s `rayx_diagnostics` must enable `diagnostics`: {alias}"
        );
    }
}

#[test]
fn collectors_gate_on_the_diagnostics_feature() {
    for crate_name in DIAGNOSTICS_CRATES {
        for (path, text) in sources(crate_name) {
            let legacy_gates = text.matches("feature = \"rayx_diagnostics\"").count();
            let allowed = usize::from(path.ends_with("web_globals.rs"));
            assert!(
                legacy_gates <= allowed,
                "{} gates on `rayx_diagnostics`; collectors gate on `diagnostics`",
                path.display()
            );
        }
    }
}

#[test]
fn every_former_global_goes_through_the_gpux_names() {
    let mut used = String::new();
    for crate_name in ["gpui_wgpu", "gpui_web"] {
        for (path, text) in sources(crate_name) {
            if path.ends_with("web_globals.rs") {
                assert!(text.contains("pub const PREFIX: &str = \"__gpux\";"));
                assert!(text.contains("pub const LEGACY_PREFIX: &str = \"__rayx\";"));
                continue;
            }
            for line in text.lines() {
                let code = line.split("//").next().unwrap_or_default();
                assert!(
                    !code.contains("\"__rayx"),
                    "{} names a `__rayx` global directly; use `web_globals`: {line}",
                    path.display()
                );
            }
            used.push_str(&text);
        }
    }
    for name in GLOBALS {
        let through_helper = ["publish(", "flag(", "read(", "clear("]
            .iter()
            .any(|call| used.contains(&format!("{call}\"{name}\"")));
        assert!(
            through_helper,
            "the former `__rayx{name}` global is not read or published through `web_globals`"
        );
    }
}

#[test]
fn both_html_island_opt_ins_are_honoured() {
    let source =
        std::fs::read_to_string(crates_dir().join("gpui_web/src/html_in_canvas.rs")).unwrap();
    for opt_in in ["\"gpux-html-island\"", "\"rayx-html-island\""] {
        assert!(
            source.contains(&format!("query_flag_enabled(&search, {opt_in})")),
            "the {opt_in} URL opt-in is not honoured"
        );
    }
    assert!(source.contains("web_globals::flag(\"HtmlIslandEnabled\")"));
}

#[test]
fn only_diagnostics_publish_browser_globals() {
    let source =
        std::fs::read_to_string(crates_dir().join("gpui_wgpu/src/web_globals.rs")).unwrap();
    let source = source.replace("\r\n", "\n");
    // Ungated, a publisher compiles into production builds; gating `publish` makes every caller
    // that is not itself behind `diagnostics` fail to build without the feature.
    assert!(
        source.contains("#[cfg(feature = \"diagnostics\")]\npub fn publish("),
        "`web_globals::publish` must exist only with the `diagnostics` feature"
    );
    assert!(
        !source.contains("publish_with_legacy"),
        "no global may be published whatever the features"
    );
}
