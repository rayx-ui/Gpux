//! Lane E's Android commands: argument parsing, the pinned SDK checks, the host's requests and
//! the adb commands that carry them out, device selection, result counting, and APK assembly.

use gpux_testkit::{
    Environment, Os,
    android::{self, ABIS, Request},
    cli::{AndroidCommand, Command, Selection, parse},
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
    let background = android::shell_commands(&Request::Background);
    assert_eq!(background[0], ["input", "keyevent", "KEYCODE_HOME"]);
    assert_eq!(
        background.last().unwrap(),
        &[
            "am",
            "start",
            "-n",
            "dev.gpux.conformance/android.app.NativeActivity"
        ]
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
    assert_eq!(&expected[..5], android::PLATFORM_CHECKS);
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
}

impl Environment for FakeEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
    fn find_program(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    fn os(&self) -> Os {
        Os::Linux
    }
    fn workspace_root(&self) -> Option<PathBuf> {
        None
    }
}

#[test]
fn missing_sdk_pieces_name_the_sdkmanager_package() {
    let sdk = tempfile::tempdir().unwrap();
    let environment = |vars: &[(&'static str, &Path)]| FakeEnvironment {
        vars: vars
            .iter()
            .map(|(name, path)| (*name, path.display().to_string()))
            .collect(),
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
            "--features app-host",
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
