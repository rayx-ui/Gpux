//! Lane F, the iOS Simulator: what the testkit builds and launches, how it finds or creates the
//! pinned Simulator, the host's requests, and how results are counted. Running it needs macOS;
//! everything here is the host-independent part.

use gpux_testkit::{
    Environment, Os,
    cli::{Command, IosCommand, Selection, parse},
    execute,
    ios::{self, Request},
    lanes::Invocation,
    run::{RunOutput, Runner},
    summary::EXIT_FAILED,
};
use std::path::{Path, PathBuf};

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

#[test]
fn ios_commands_need_the_simulator_flag() {
    assert_eq!(
        parse(args("ios build --simulator")).unwrap(),
        Command::Ios(IosCommand::Build)
    );
    assert_eq!(
        parse(args("ios run --simulator --scene paint-borders")).unwrap(),
        Command::Ios(IosCommand::Run {
            selection: Selection {
                scene: Some("paint-borders".into()),
                grep: None,
            },
        })
    );
    let error = parse(args("ios run")).unwrap_err().to_string();
    assert!(error.contains("needs `--simulator`"), "{error}");
    assert!(error.contains("docs/testing.md"), "{error}");
    let error = parse(args("ios install --simulator"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("`build` and `run`"), "{error}");
    let error = parse(args("ios build --simulator --scene x"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown option `--scene`"), "{error}");
}

#[test]
fn the_build_links_the_simulator_library_into_an_unsigned_app() {
    let root = Path::new("/repo");
    assert_eq!(
        ios::cargo_build().join(" "),
        "build -p gpux-conformance --no-default-features --features app-host --example ios_host \
         --target aarch64-apple-ios-sim"
    );
    let library = ios::host_library(root);
    assert!(library.ends_with("target/aarch64-apple-ios-sim/debug/examples/libios_host.a"));
    let xcodebuild = ios::xcodebuild(root, &library).join(" ");
    for part in [
        "-sdk iphonesimulator",
        "CODE_SIGNING_ALLOWED=NO",
        "GPUX_IOS_HOST_LIB=",
        "GpuxConformance.xcodeproj",
    ] {
        assert!(xcodebuild.contains(part), "{xcodebuild}");
    }
    assert!(xcodebuild.ends_with(" build"), "{xcodebuild}");
    assert!(ios::app_path(root).ends_with("products/Debug-iphonesimulator/GpuxConformance.app"));
}

const DEVICES: &str = "\
== Devices ==
-- iOS 18.2 --
    iPhone 16 Pro (1A2B3C4D-0000-0000-0000-000000000001) (Shutdown)
    gpux-conformance (5E6F7A8B-0000-0000-0000-000000000002) (Booted)
-- iOS 17.5 --
    iPhone 15 (9C0D1E2F-0000-0000-0000-000000000003) (Shutdown)
";

const RUNTIMES: &str = "\
== Runtimes ==
iOS 17.5 (17.5 - 21F79) - com.apple.CoreSimulator.SimRuntime.iOS-17-5
iOS 18.2 (18.2 - 22C150) - com.apple.CoreSimulator.SimRuntime.iOS-18-2
iOS 18.10 (18.10 - 22Z1) - com.apple.CoreSimulator.SimRuntime.iOS-18-10
watchOS 11.2 (11.2 - 22S99) - com.apple.CoreSimulator.SimRuntime.watchOS-11-2
";

#[test]
fn the_pinned_simulator_is_found_or_created_on_the_newest_runtime() {
    assert_eq!(
        ios::simulator_device(DEVICES, "gpux-conformance"),
        Some(("5E6F7A8B-0000-0000-0000-000000000002".into(), true))
    );
    assert_eq!(
        ios::simulator_device(DEVICES, "iPhone 16 Pro"),
        Some(("1A2B3C4D-0000-0000-0000-000000000001".into(), false))
    );
    assert_eq!(ios::simulator_device(DEVICES, "iPhone 16"), None);
    // Versions compare as numbers: 18.10 is newer than 18.2.
    assert_eq!(
        ios::newest_ios_runtime(RUNTIMES).as_deref(),
        Some("com.apple.CoreSimulator.SimRuntime.iOS-18-10")
    );
    assert_eq!(ios::newest_ios_runtime("== Runtimes ==\n"), None);
}

#[test]
fn the_host_launches_with_its_console_and_selection() {
    let selection = Selection {
        scene: None,
        grep: Some("text".into()),
    };
    assert_eq!(
        ios::launch("UDID", &selection).join(" "),
        "simctl launch --console-pty --terminate-running-process UDID dev.gpux.conformance \
         --grep text"
    );
}

#[test]
fn host_requests_bring_another_app_forward_and_reopen_the_host() {
    assert_eq!(
        ios::request("2026-10-07 gpux-conformance-host: background"),
        Some(Request::Background)
    );
    assert_eq!(
        ios::request("gpux-conformance-host: done"),
        Some(Request::Done)
    );
    assert_eq!(ios::request("gpux-conformance-host: tap 1 2"), None);
    assert_eq!(ios::request("gpux-conformance: PASS ios-startup"), None);
    let commands: Vec<String> = ios::simctl_commands(&Request::Background, "UDID")
        .iter()
        .map(|command| command.join(" "))
        .collect();
    assert_eq!(
        commands,
        [
            "simctl launch UDID com.apple.Preferences",
            "simctl openurl UDID gpux-conformance://foreground",
        ]
    );
}

#[test]
fn a_host_that_panics_or_fails_to_load_ends_the_run() {
    let unresolved = "dyld[68780]: symbol not found in flat namespace '_CGLErrorString'";
    assert_eq!(
        ios::fatal(unresolved),
        Some(format!("the app did not load: {unresolved}"))
    );
    assert!(
        ios::fatal("dyld[1]: Library not loaded: @rpath/libios_host.dylib")
            .is_some_and(|failure| failure.starts_with("the app did not load"))
    );
    assert!(
        ios::fatal("thread 'main' panicked at examples/ios_host/lib.rs:9:5")
            .is_some_and(|failure| failure.starts_with("the host panicked"))
    );
    assert_eq!(ios::fatal("dev.gpux.conformance: 68780"), None);
    assert_eq!(ios::fatal("gpux-conformance: PASS ios-startup"), None);
    assert_eq!(
        ios::fatal("[WARN wgpu_core::instance] Missing downlevel flags"),
        None
    );
}

#[test]
fn results_count_the_platform_checks_and_lane_f_scenes() {
    let everything = Selection::default();
    let expected = ios::expected_results(&everything);
    assert_eq!(&expected[..5], &ios::PLATFORM_CHECKS);
    assert!(expected.len() > 5, "lane F runs catalog scenes too");
    let output: String = expected
        .iter()
        .map(|id| format!("gpux-conformance: PASS {id} ok\n"))
        .collect();
    let tally = ios::tally(&output, &everything);
    assert_eq!((tally.passed, tally.failed), (expected.len(), 0));
    // A host that stops early fails every result it never reported.
    let tally = ios::tally("gpux-conformance: PASS ios-startup ok\n", &everything);
    assert_eq!(tally.failed, expected.len() - 1);

    let narrowed = Selection {
        scene: None,
        grep: Some("ios-text".into()),
    };
    assert_eq!(ios::expected_results(&narrowed), ["ios-text-metrics"]);
}

struct WindowsHost(PathBuf);

impl Environment for WindowsHost {
    fn var(&self, _: &str) -> Option<String> {
        None
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("/bin/{name}")))
    }

    fn os(&self) -> Os {
        Os::Windows
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

struct NoRunner;

impl Runner for NoRunner {
    fn run(
        &mut self,
        invocation: &Invocation,
        _: &Path,
        _: &Path,
        _: &Path,
    ) -> anyhow::Result<RunOutput> {
        panic!("nothing may run off macOS: {}", invocation.command_line());
    }
}

#[test]
fn lane_f_explains_that_it_needs_macos_and_lists_anywhere() {
    let root = tempfile::tempdir().unwrap();
    let host = WindowsHost(root.path().to_path_buf());
    let mut out = Vec::new();
    let code = execute(args("lane f"), &host, &mut NoRunner, &mut out);
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, EXIT_FAILED);
    assert!(text.contains("needs macOS with Xcode"), "{text}");

    let mut out = Vec::new();
    let code = execute(args("lane f --list"), &host, &mut NoRunner, &mut out);
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("  ios-metal"), "{text}");
}
