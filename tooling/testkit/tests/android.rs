//! Lane E's Android commands: argument parsing, the pinned SDK checks, the host's requests and
//! the adb commands that carry them out, device selection, result counting, and APK assembly.

use gpux_testkit::{
    Environment, Os,
    android::{self, ABIS, Request},
    cli::{AndroidCommand, Command, Selection, parse},
    lanes::Invocation,
    run::{RunOutput, Runner},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

#[test]
fn android_commands_parse() {
    assert_eq!(
        parse(args("android build")).unwrap(),
        Command::Android(AndroidCommand::Build {
            abis: ABIS.to_vec()
        })
    );
    assert_eq!(
        parse(args("android build --abi x86_64 --abi x86_64")).unwrap(),
        Command::Android(AndroidCommand::Build {
            abis: vec![android::abi("x86_64").unwrap()]
        })
    );
    assert_eq!(
        parse(args("android run --serial emulator-5554 --grep touch")).unwrap(),
        Command::Android(AndroidCommand::Run {
            serial: Some("emulator-5554".into()),
            selection: Selection {
                scene: None,
                grep: Some("touch".into()),
            },
        })
    );
    let error = |line: &str| parse(args(line)).unwrap_err().to_string();
    assert!(error("android").contains("`build` or `run`"));
    assert!(error("android build --abi mips").contains("unknown ABI `mips`"));
    assert!(error("android run --abi x86_64").contains("unknown option `--abi`"));
    assert!(error("android flash").contains("unknown android command"));
}

#[test]
fn host_requests_parse_from_logcat_lines() {
    assert_eq!(
        android::request("android_host::android: gpux-conformance-host: tap 540 1200"),
        Some(Request::Tap(540, 1200))
    );
    assert_eq!(
        android::request("gpux-conformance-host: swipe 10 900 10 780 40"),
        Some(Request::Swipe(10, 900, 10, 780, 40))
    );
    assert_eq!(
        android::request("gpux-conformance-host: background"),
        Some(Request::Background)
    );
    assert_eq!(
        android::request("gpux-conformance-host: foreground"),
        Some(Request::Foreground)
    );
    assert_eq!(
        android::request("x gpux-conformance-host: done"),
        Some(Request::Done)
    );
    assert_eq!(android::request("gpux-conformance-host: tap 1"), None);
    assert_eq!(
        android::request("gpux-conformance: PASS layout-flex 3 steps"),
        None
    );
}

#[test]
fn requests_become_adb_shell_commands() {
    assert_eq!(
        android::shell_commands(&Request::Swipe(1, 2, 3, 4, 50)),
        [["input", "swipe", "1", "2", "3", "4", "50"]]
    );
    // Going to the background and coming back are separate requests: the host asks to come back
    // once the system took its surface, however long a slow device takes to stop the activity.
    assert_eq!(
        android::shell_commands(&Request::Background),
        [["input", "keyevent", "KEYCODE_HOME"]]
    );
    assert_eq!(
        android::shell_commands(&Request::Foreground),
        [[
            "am",
            "start",
            "-n",
            "dev.gpux.conformance/android.app.NativeActivity"
        ]]
    );
    assert!(android::shell_commands(&Request::Done).is_empty());
}

#[test]
fn the_first_attached_device_or_the_named_one_runs() {
    let devices = "* daemon started successfully\nList of devices attached\n\
                   emulator-5554\tdevice\nR58M\tunauthorized\n0123\tdevice\n\n";
    assert_eq!(
        android::pick_device(devices, None).unwrap(),
        "emulator-5554"
    );
    assert_eq!(android::pick_device(devices, Some("0123")).unwrap(), "0123");
    let error = android::pick_device(devices, Some("R58M")).unwrap_err();
    assert!(
        error.contains("not attached") && error.contains("emulator-5554, 0123"),
        "{error}"
    );
    let error = android::pick_device("List of devices attached\n", None).unwrap_err();
    assert!(error.contains("start an emulator"), "{error}");
}

#[test]
fn every_check_and_lane_e_scene_must_report() {
    let everything = Selection::default();
    let expected = android::expected_results(&everything);
    assert_eq!(
        &expected[..android::PLATFORM_CHECKS.len()],
        android::PLATFORM_CHECKS
    );
    assert!(expected.contains(&"layout-flex".to_string()));
    let mut output: String = expected
        .iter()
        .map(|id| format!("gpui_android: gpux-conformance: PASS {id} ok\n"))
        .collect();
    let tally = android::tally(&output, &everything);
    assert_eq!((tally.passed, tally.failed), (expected.len(), 0));

    // A host that stops early fails every result it never reported.
    output = "gpux-conformance: FAIL android-startup no adapter\n".to_string();
    let tally = android::tally(&output, &everything);
    assert_eq!(tally.failed, expected.len());
    assert!(
        tally
            .failures
            .iter()
            .any(|failure| failure.contains("layout-flex: no result was reported"))
    );
}

struct FakeEnvironment {
    vars: HashMap<&'static str, String>,
    programs: Vec<&'static str>,
}

impl Environment for FakeEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.programs
            .contains(&name)
            .then(|| PathBuf::from(format!("/bin/{name}")))
    }
    fn os(&self) -> Os {
        Os::Linux
    }
    fn workspace_root(&self) -> Option<PathBuf> {
        None
    }
}

#[test]
fn results_come_from_whichever_log_kept_more_of_them() {
    let pass =
        |scene: &str| format!("android_host::android: gpux-conformance: PASS {scene} 4 steps");
    let full = [
        pass("paint-canvas-path"),
        "android_host::android: gpux-conformance-host: screenshot 27".into(),
        pass("paint-borders"),
    ]
    .join("\n");
    // The stream lost the first two lines while adb transferred a screenshot.
    let streamed = pass("paint-borders");
    assert_eq!(android::fuller_log(&streamed, &full), full);
    // A buffer that wrapped, or no buffer at all, leaves the stream.
    assert_eq!(android::fuller_log(&full, &streamed), full);
    assert_eq!(android::fuller_log(&streamed, ""), streamed);
}

#[test]
fn missing_sdk_pieces_name_the_sdkmanager_package() {
    let sdk = tempfile::tempdir().unwrap();
    let environment = |vars: &[(&'static str, &Path)]| FakeEnvironment {
        vars: vars
            .iter()
            .map(|(name, path)| (*name, path.display().to_string()))
            .collect(),
        programs: Vec::new(),
    };
    let error = android::find_sdk(&environment(&[])).unwrap_err();
    assert!(error.contains("ANDROID_HOME"), "{error}");

    let with_sdk = environment(&[("ANDROID_HOME", sdk.path())]);
    let error = android::find_sdk(&with_sdk).unwrap_err();
    assert!(
        error.contains(&format!("sdkmanager \"ndk;{}\"", android::NDK_VERSION)),
        "{error}"
    );

    std::fs::create_dir_all(sdk.path().join("ndk").join(android::NDK_VERSION)).unwrap();
    let error = android::find_sdk(&with_sdk).unwrap_err();
    assert!(
        error.contains(&format!("build-tools;{}", android::BUILD_TOOLS_VERSION)),
        "{error}"
    );

    std::fs::create_dir_all(
        sdk.path()
            .join("build-tools")
            .join(android::BUILD_TOOLS_VERSION),
    )
    .unwrap();
    let error = android::find_sdk(&with_sdk).unwrap_err();
    assert!(
        error.contains(&format!("platforms;android-{}", android::COMPILE_SDK)),
        "{error}"
    );

    let platform = sdk
        .path()
        .join("platforms")
        .join(format!("android-{}", android::COMPILE_SDK));
    std::fs::create_dir_all(&platform).unwrap();
    std::fs::write(platform.join("android.jar"), b"").unwrap();
    let found = android::find_sdk(&with_sdk).unwrap();
    assert_eq!(found.ndk, sdk.path().join("ndk").join(android::NDK_VERSION));
}

#[test]
fn native_libraries_are_added_uncompressed() {
    let dir = tempfile::tempdir().unwrap();
    let apk = dir.path().join("base.apk");
    // aapt2's output is a zip; an empty archive stands in for it.
    std::fs::write(
        &apk,
        [
            0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    )
    .unwrap();
    let library = dir.path().join("libandroid_host.so");
    std::fs::write(&library, b"\x7fELF test library").unwrap();
    android::add_stored(&apk, &[("lib/x86_64/libandroid_host.so".into(), library)]).unwrap();
    let bytes = std::fs::read(&apk).unwrap();
    // The stored entry keeps the library's bytes verbatim, so it can load in place.
    assert!(
        bytes
            .windows(b"\x7fELF test library".len())
            .any(|window| window == b"\x7fELF test library")
    );
    assert!(
        bytes
            .windows(b"lib/x86_64/libandroid_host.so".len())
            .any(|window| window == b"lib/x86_64/libandroid_host.so")
    );
}

#[test]
fn the_build_targets_every_abi_at_the_minimum_api_level() {
    let sdk = android::Sdk {
        root: PathBuf::from("/sdk"),
        ndk: PathBuf::from("/sdk/ndk/28"),
        build_tools: PathBuf::from("/sdk/build-tools/36.0.0"),
        android_jar: PathBuf::from("/sdk/platforms/android-36/android.jar"),
    };
    for abi in ABIS {
        let line = android::cargo_ndk(abi, &sdk).command_line();
        let ndk = format!("cargo ndk -t {} -P 26 build", abi.name);
        for part in [
            ndk.as_str(),
            "--example android_host",
            "--features app-host,diagnostics",
            "--release",
            "ANDROID_NDK_HOME=/sdk/ndk/28",
        ] {
            assert!(line.contains(part), "{part} is missing from {line}");
        }
    }
    assert_eq!(
        android::built_library(Path::new("/repo"), android::abi("arm64-v8a").unwrap()),
        PathBuf::from("/repo/target/aarch64-linux-android/release/examples/libandroid_host.so")
    );
}

#[test]
fn a_selection_narrows_the_run_on_the_device() {
    let selection = Selection {
        scene: None,
        grep: Some("touch".into()),
    };
    assert_eq!(
        android::expected_results(&selection),
        [
            "android-touch-tap",
            "android-touch-pan",
            "android-touch-fling"
        ]
    );
    let one = Selection {
        scene: Some("layout-flex".into()),
        grep: None,
    };
    assert_eq!(android::expected_results(&one), ["layout-flex"]);
    // The host reads the selection from system properties; unset parts are cleared.
    assert_eq!(
        android::selection_properties(&one).unwrap(),
        [
            vec!["setprop", "debug.gpux.scene", "'layout-flex'"],
            vec!["setprop", "debug.gpux.grep", "''"],
        ]
    );
    let quoted = Selection {
        scene: None,
        grep: Some("it's".into()),
    };
    assert!(android::selection_properties(&quoted).is_err());
}

#[test]
fn a_host_signed_with_another_key_is_replaced() {
    // Each checkout signs with its own debug key; adb then refuses the update.
    assert!(android::signed_with_another_key(
        "Performing Streamed Install\nadb.exe: failed to install gpux-conformance.apk: Failure \
         [INSTALL_FAILED_UPDATE_INCOMPATIBLE: Existing package dev.gpux.conformance signatures do \
         not match newer version; ignoring!]"
    ));
    // Other failures stay failures.
    assert!(!android::signed_with_another_key(
        "adb.exe: failed to install gpux-conformance.apk: Failure [INSTALL_FAILED_INSUFFICIENT_STORAGE]"
    ));
    assert!(!android::signed_with_another_key("Success"));
}

/// An SDK with every pinned piece, in a temporary folder.
fn complete_sdk() -> tempfile::TempDir {
    let sdk = tempfile::tempdir().unwrap();
    let ndk = sdk.path().join("ndk").join(android::NDK_VERSION);
    std::fs::create_dir_all(&ndk).unwrap();
    // The pinned package's own file: a beta, whose revision carries a suffix.
    std::fs::write(
        ndk.join("source.properties"),
        format!(
            "Pkg.Desc = Android NDK\nPkg.Revision = {0}-beta2\nPkg.BaseRevision = {0}\n\
             Pkg.ReleaseName = r28-beta2\n",
            android::NDK_VERSION
        ),
    )
    .unwrap();
    std::fs::create_dir_all(
        sdk.path()
            .join("build-tools")
            .join(android::BUILD_TOOLS_VERSION),
    )
    .unwrap();
    let platform = sdk
        .path()
        .join("platforms")
        .join(format!("android-{}", android::COMPILE_SDK));
    std::fs::create_dir_all(&platform).unwrap();
    std::fs::write(platform.join("android.jar"), b"").unwrap();
    sdk
}

#[test]
fn an_ndk_override_must_be_the_pinned_ndk() {
    let sdk = complete_sdk();
    let other = sdk.path().join("ndk").join("27.3.13750724");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("source.properties"),
        "Pkg.Revision = 27.3.13750724\n",
    )
    .unwrap();
    let with = |ndk: &Path| FakeEnvironment {
        vars: [
            ("ANDROID_HOME", sdk.path().display().to_string()),
            ("ANDROID_NDK_HOME", ndk.display().to_string()),
        ]
        .into_iter()
        .collect(),
        programs: Vec::new(),
    };
    // A runner image's own NDK is refused, naming the pinned one.
    let error = android::find_sdk(&with(&other)).unwrap_err();
    assert!(
        error.contains("27.3.13750724") && error.contains(android::NDK_VERSION),
        "{error}"
    );
    // An override pointing at the pinned NDK is used, as CI sets it.
    let pinned = sdk.path().join("ndk").join(android::NDK_VERSION);
    assert_eq!(android::find_sdk(&with(&pinned)).unwrap().ndk, pinned);
    assert_eq!(
        android::ndk_revision(&pinned).as_deref(),
        Some(android::NDK_VERSION)
    );
    // A copy of the pinned NDK elsewhere is recognised by its base revision.
    let copy = sdk.path().join("copy-of-ndk");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::copy(
        pinned.join("source.properties"),
        copy.join("source.properties"),
    )
    .unwrap();
    assert_eq!(android::find_sdk(&with(&copy)).unwrap().ndk, copy);
    // Without a base revision, the pre-release suffix is dropped.
    let suffixed = sdk.path().join("suffixed");
    std::fs::create_dir_all(&suffixed).unwrap();
    std::fs::write(
        suffixed.join("source.properties"),
        format!("Pkg.Revision = {}-rc1\n", android::NDK_VERSION),
    )
    .unwrap();
    assert_eq!(
        android::ndk_revision(&suffixed).as_deref(),
        Some(android::NDK_VERSION)
    );
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
        panic!(
            "nothing may run without the build prerequisites: {}",
            invocation.command_line()
        );
    }
}

#[test]
fn missing_build_tools_on_path_are_named() {
    let sdk = complete_sdk();
    let root = tempfile::tempdir().unwrap();
    let with = |programs: Vec<&'static str>| FakeEnvironment {
        vars: [("ANDROID_HOME", sdk.path().display().to_string())]
            .into_iter()
            .collect(),
        programs,
    };
    let build = |environment: &FakeEnvironment| {
        android::build(
            root.path(),
            environment,
            &mut NoRunner,
            &[android::abi("x86_64").unwrap()],
            &mut Vec::new(),
        )
        .unwrap_err()
    };
    let error = build(&with(Vec::new()));
    assert!(
        error.contains("`java` is not on PATH") && error.contains("JDK 21"),
        "{error}"
    );
    let error = build(&with(vec!["java"]));
    assert!(error.contains("`keytool` is not on PATH"), "{error}");
    let error = build(&with(vec!["java", "keytool", "cargo"]));
    assert_eq!(
        error,
        "cargo-ndk is missing: install it with `cargo install cargo-ndk`"
    );
}
