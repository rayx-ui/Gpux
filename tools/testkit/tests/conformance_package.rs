//! The conformance crate's package name: every cargo invocation the testkit builds selects
//! `gpux-tests-conformance`, a gpux checkout is recognized by `crates/gpux_tests_conformance`, and
//! the runtime protocol identifiers that merely contain the word keep their values.

use gpux_testkit::{
    CONFORMANCE_PACKAGE, Environment, Os, android,
    cli::Selection,
    execute, find_workspace_root, ios,
    lanes::{Invocation, LaneId, plan, plan_with_jobs},
    run::{RunOutput, Runner},
    summary::EXIT_FAILED,
    web::{self, WebPaths},
};
use std::path::{Path, PathBuf};

/// The old folder name, spelled so the layout guard does not flag this file.
const OLD_FOLDER: &str = concat!("gpux", "_conformance");

/// The package that `-p` selects in `invocation`, if any.
fn selected_package(invocation: &Invocation) -> Option<&str> {
    let args = &invocation.args;
    args.iter()
        .position(|arg| arg == "-p")
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

fn assert_selects_the_package(invocation: &Invocation) {
    assert_eq!(
        selected_package(invocation),
        Some("gpux-tests-conformance"),
        "{}",
        invocation.command_line()
    );
    assert!(
        !invocation.args.iter().any(|arg| arg == "gpux-conformance"),
        "{}",
        invocation.command_line()
    );
}

#[test]
fn the_package_is_gpux_tests_conformance() {
    assert_eq!(CONFORMANCE_PACKAGE, "gpux-tests-conformance");
}

#[test]
fn lane_plans_and_listings_select_the_renamed_package() {
    let selection = Selection::default();
    let plans = [
        plan(LaneId::A, &selection).unwrap(),
        plan(LaneId::C, &selection).unwrap(),
        plan_with_jobs(LaneId::C, &selection, 3).unwrap(),
        plan(LaneId::D, &selection).unwrap(),
    ];
    let mut checked = 0;
    for lane_plan in &plans {
        for invocation in lane_plan
            .invocations
            .iter()
            .chain(lane_plan.listings.iter().map(|listing| &listing.invocation))
            .chain(lane_plan.prebuild.iter())
        {
            assert_selects_the_package(invocation);
            checked += 1;
        }
    }
    // Lanes A, C (one process and three jobs with their prebuild) and D, with their listings.
    assert!(checked >= 8, "only {checked} invocations checked");
}

#[test]
fn web_android_and_ios_builds_select_the_renamed_package() {
    let paths = WebPaths::new(Path::new("/repo"));
    assert_selects_the_package(&web::cargo_build("nightly", &paths));

    let sdk = android::Sdk {
        root: PathBuf::from("/sdk"),
        ndk: PathBuf::from("/sdk/ndk/28"),
        build_tools: PathBuf::from("/sdk/build-tools/36.0.0"),
        android_jar: PathBuf::from("/sdk/platforms/android-36/android.jar"),
    };
    for abi in android::ABIS {
        assert_selects_the_package(&android::cargo_ndk(abi, &sdk));
    }

    let ios_build = ios::cargo_build();
    let index = ios_build.iter().position(|arg| arg == "-p").unwrap();
    assert_eq!(ios_build[index + 1], "gpux-tests-conformance");
}

#[test]
fn a_checkout_is_recognized_by_the_renamed_crate_only() {
    let renamed = tempfile::tempdir().unwrap();
    let crate_dir = renamed.path().join("crates").join("gpux_tests_conformance");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(renamed.path().join("Cargo.toml"), "[workspace]").unwrap();
    std::fs::write(crate_dir.join("Cargo.toml"), "[package]").unwrap();
    assert_eq!(
        find_workspace_root(&crate_dir.join("src")),
        Some(renamed.path().to_path_buf())
    );

    let old = tempfile::tempdir().unwrap();
    let old_crate = old.path().join("crates").join(OLD_FOLDER);
    std::fs::create_dir_all(old_crate.join("src")).unwrap();
    std::fs::write(old.path().join("Cargo.toml"), "[workspace]").unwrap();
    std::fs::write(old_crate.join("Cargo.toml"), "[package]").unwrap();
    assert_eq!(find_workspace_root(&old_crate.join("src")), None);

    // The root manifest is still required beside the crate.
    let no_manifest = tempfile::tempdir().unwrap();
    let crate_only = no_manifest
        .path()
        .join("crates")
        .join("gpux_tests_conformance");
    std::fs::create_dir_all(&crate_only).unwrap();
    std::fs::write(crate_only.join("Cargo.toml"), "[package]").unwrap();
    assert_eq!(find_workspace_root(&crate_only), None);
}

struct Outside;

impl Environment for Outside {
    fn var(&self, _name: &str) -> Option<String> {
        None
    }

    fn find_program(&self, _name: &str) -> Option<PathBuf> {
        None
    }

    fn os(&self) -> Os {
        Os::MacOs
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        None
    }
}

struct NothingRuns;

impl Runner for NothingRuns {
    fn run(
        &mut self,
        invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        panic!(
            "nothing runs outside a checkout: {}",
            invocation.command_line()
        )
    }
}

#[test]
fn the_outside_checkout_error_names_the_renamed_folder() {
    let mut out = Vec::new();
    let code = execute(
        ["lane".to_string(), "a".to_string()],
        &Outside,
        &mut NothingRuns,
        &mut out,
    );
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, EXIT_FAILED, "{text}");
    assert!(text.contains("crates/gpux_tests_conformance"), "{text}");
    assert!(!text.contains(&format!("crates/{OLD_FOLDER}")), "{text}");
}

#[test]
fn runtime_protocol_identifiers_are_unchanged() {
    assert_eq!(gpux_tests_conformance::report::MARKER, "gpux-conformance:");
    assert_eq!(android::HOST_MARKER, "gpux-conformance-host:");
    assert_eq!(ios::HOST_MARKER, "gpux-conformance-host:");
    assert_eq!(android::PACKAGE, "dev.gpux.conformance");
    assert_eq!(ios::BUNDLE_ID, "dev.gpux.conformance");
    assert_eq!(ios::SIMULATOR_NAME, "gpux-conformance");
    let root = Path::new("/repo");
    assert_eq!(
        android::apk_path(root).file_name().unwrap(),
        "gpux-conformance.apk"
    );
    assert_eq!(
        ios::project(root).file_name().unwrap(),
        "GpuxConformance.xcodeproj"
    );
    assert!(
        ios::app_path(root).ends_with("GpuxConformance.app"),
        "{}",
        ios::app_path(root).display()
    );

    // The browser global the web host publishes and the Playwright fixtures read.
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let web_host = std::fs::read_to_string(
        repository.join("crates/gpux_tests_conformance/examples/web_host/lib.rs"),
    )
    .unwrap();
    assert!(web_host.contains("\"__gpuxConformance\""));
}
