//! Where the testkit finds the mobile app shells after their move to `tools/`: lane E's manifest
//! in `tools/android` and lane F's XcodeGen spec and generated project in `tools/ios`.

#![allow(
    clippy::disallowed_methods,
    reason = "`git ls-files` is a synchronous child process; waiting on it is the test"
)]

use gpux_testkit::{
    Environment, Os,
    android::{self, Abi},
    ios,
    lanes::Invocation,
    run::{RunOutput, Runner},
};
use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn the_android_manifest_is_in_tools_android() {
    let root = Path::new("/repo");
    assert_eq!(
        android::manifest(root),
        root.join("tools")
            .join("android")
            .join("AndroidManifest.xml")
    );
    assert!(android::manifest(&repository()).is_file());
}

#[test]
fn the_ios_spec_and_project_are_in_tools_ios() {
    let root = Path::new("/repo");
    let shell = root.join("tools").join("ios");
    assert_eq!(ios::spec(root), shell.join("project.yml"));
    assert_eq!(ios::project(root), shell.join("GpuxConformance.xcodeproj"));
    assert!(ios::spec(&repository()).is_file());
    // XcodeGen writes the project beside the spec, under the spec's project name.
    let spec = std::fs::read_to_string(ios::spec(&repository())).unwrap();
    assert!(spec.contains("name: GpuxConformance"), "{spec}");
}

/// The files git tracks under `path` in the checkout.
fn tracked(path: &str) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "--", path])
        .current_dir(repository())
        .output()
        .expect("git runs in the checkout");
    assert!(output.status.success(), "git ls-files failed");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn the_shells_moved_out_of_tooling() {
    // Ignored leftovers (a generated Xcode project) may stay in older checkouts; tracked files
    // may not.
    assert_eq!(tracked("tooling"), Vec::<String>::new());
    assert!(tracked("tools/android").contains(&"tools/android/AndroidManifest.xml".to_string()));
    assert!(tracked("tools/ios").contains(&"tools/ios/project.yml".to_string()));
}

struct Mac;

impl Environment for Mac {
    fn var(&self, _name: &str) -> Option<String> {
        None
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("/fake/bin/{name}")))
    }

    fn os(&self) -> Os {
        Os::MacOs
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        None
    }
}

/// Succeeds at everything and records the command lines.
#[derive(Default)]
struct Recorder {
    ran: Vec<Invocation>,
}

impl Runner for Recorder {
    fn run(
        &mut self,
        invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        self.ran.push(invocation.clone());
        Ok(RunOutput {
            success: true,
            code: Some(0),
            output: String::new(),
        })
    }
}

#[test]
fn ios_build_generates_the_project_from_the_moved_spec() {
    let root = tempfile::tempdir().unwrap();
    let mut recorder = Recorder::default();
    let mut out = Vec::new();
    // Nothing is really built, so the build ends at the missing app; the steps before it ran.
    let result = ios::build(root.path(), &Mac, &mut recorder, &mut out);
    assert!(result.is_err());
    let xcodegen = recorder
        .ran
        .iter()
        .find(|invocation| invocation.label == "xcodegen")
        .expect("xcodegen ran");
    assert_eq!(
        xcodegen.args,
        vec![
            "generate".to_string(),
            "--spec".into(),
            ios::spec(root.path()).display().to_string(),
        ]
    );
    let xcodebuild = recorder
        .ran
        .iter()
        .find(|invocation| invocation.label == "xcodebuild")
        .expect("xcodebuild ran");
    let project = ios::project(root.path()).display().to_string();
    assert!(xcodebuild.args.contains(&project), "{:?}", xcodebuild.args);
}

/// An Android SDK with the pinned pieces `android build` checks for.
fn android_sdk() -> tempfile::TempDir {
    let sdk = tempfile::tempdir().unwrap();
    let ndk = sdk.path().join("ndk").join(android::NDK_VERSION);
    std::fs::create_dir_all(&ndk).unwrap();
    std::fs::write(
        ndk.join("source.properties"),
        format!("Pkg.Revision = {}\n", android::NDK_VERSION),
    )
    .unwrap();
    let tools = sdk
        .path()
        .join("build-tools")
        .join(android::BUILD_TOOLS_VERSION);
    std::fs::create_dir_all(tools.join("lib")).unwrap();
    std::fs::write(tools.join("lib").join("d8.jar"), b"").unwrap();
    let platform = sdk
        .path()
        .join("platforms")
        .join(format!("android-{}", android::COMPILE_SDK));
    std::fs::create_dir_all(&platform).unwrap();
    std::fs::write(platform.join("android.jar"), b"").unwrap();
    sdk
}

struct AndroidHost {
    sdk: PathBuf,
}

impl Environment for AndroidHost {
    fn var(&self, name: &str) -> Option<String> {
        (name == "ANDROID_HOME").then(|| self.sdk.display().to_string())
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("/fake/bin/{name}")))
    }

    fn os(&self) -> Os {
        Os::Linux
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        None
    }
}

/// Records each invocation and stops the build at `aapt2 link`, the step that reads the manifest.
#[derive(Default)]
struct UntilLink {
    ran: Vec<Invocation>,
}

impl Runner for UntilLink {
    fn run(
        &mut self,
        invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        self.ran.push(invocation.clone());
        Ok(RunOutput {
            success: invocation.label != "aapt2-link",
            code: Some(0),
            output: String::new(),
        })
    }
}

#[test]
fn android_build_links_the_moved_manifest() {
    let sdk = android_sdk();
    let root = tempfile::tempdir().unwrap();
    let java = root
        .path()
        .join(android::JAVA_SOURCES)
        .join("dev/gpux/mobile");
    std::fs::create_dir_all(&java).unwrap();
    std::fs::write(
        java.join("Composition.java"),
        "package dev.gpux.mobile; public final class Composition {}",
    )
    .unwrap();
    let mut runner = UntilLink::default();
    let abis: [Abi; 1] = [android::abi("x86_64").unwrap()];
    let result = android::build(
        root.path(),
        &AndroidHost {
            sdk: sdk.path().to_path_buf(),
        },
        &mut runner,
        &abis,
        &mut Vec::new(),
    );
    assert!(result.is_err(), "the build stops at the failed link");
    let link = runner
        .ran
        .iter()
        .find(|invocation| invocation.label == "aapt2-link")
        .expect("aapt2 link ran");
    let index = link
        .args
        .iter()
        .position(|arg| arg == "--manifest")
        .unwrap();
    assert_eq!(
        link.args[index + 1],
        android::manifest(root.path()).display().to_string()
    );
}
