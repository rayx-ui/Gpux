//! Lane E's Gradle-free Java step: `gpux-testkit android build` compiles gpux's Java helper
//! (`crates/gpui_mobile/java`) with `javac` against the pinned `android.jar`, turns it into
//! `classes.dex` with the build tools' `d8`, and puts the dex in the APK; a missing JDK or
//! `d8.jar` is reported with what installs it.

use gpux_testkit::{
    Environment, Os,
    android::{self},
    lanes::Invocation,
    run::{RunOutput, Runner},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

const ALL_PROGRAMS: [&str; 5] = ["java", "javac", "keytool", "cargo", "cargo-ndk"];

/// An SDK with every pinned piece and `d8.jar` when `with_d8`, in a temporary folder.
fn sdk(with_d8: bool) -> tempfile::TempDir {
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
    if with_d8 {
        std::fs::write(tools.join("lib").join("d8.jar"), b"").unwrap();
    }
    let platform = sdk
        .path()
        .join("platforms")
        .join(format!("android-{}", android::COMPILE_SDK));
    std::fs::create_dir_all(&platform).unwrap();
    std::fs::write(platform.join("android.jar"), b"").unwrap();
    sdk
}

/// A checkout with one Java source in the helper's folder.
fn checkout() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let package = root
        .path()
        .join(android::JAVA_SOURCES)
        .join("dev/gpux/mobile");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("Composition.java"),
        "package dev.gpux.mobile; public final class Composition {}",
    )
    .unwrap();
    root
}

/// The value after `flag` in `args`.
fn after<'a>(args: &'a [String], flag: &str) -> &'a str {
    let index = args.iter().position(|arg| arg == flag).unwrap();
    &args[index + 1]
}

/// Records each invocation and leaves the file each tool would write.
#[derive(Default)]
struct Recording {
    invocations: Vec<Invocation>,
    root: PathBuf,
}

impl Runner for Recording {
    fn run(
        &mut self,
        invocation: &Invocation,
        _: &Path,
        _: &Path,
        _: &Path,
    ) -> anyhow::Result<RunOutput> {
        let args = &invocation.args;
        let write = |path: &Path, bytes: &[u8]| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        match invocation.label.as_str() {
            "javac" => write(
                &Path::new(after(args, "-d")).join("dev/gpux/mobile/Composition.class"),
                b"\xca\xfe\xba\xbe",
            ),
            "d8" => write(
                &Path::new(after(args, "--output")).join("classes.dex"),
                b"dex\n035",
            ),
            // An empty zip stands in for aapt2's APK.
            "aapt2-link" => write(
                Path::new(after(args, "-o")),
                &[
                    0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
            ),
            "zipalign" => {
                std::fs::copy(&args[args.len() - 2], &args[args.len() - 1]).unwrap();
            }
            "keytool" => write(Path::new(after(args, "-keystore")), b"keystore"),
            "apksigner" => {
                std::fs::copy(&args[args.len() - 1], after(args, "--out")).unwrap();
            }
            label if label.starts_with("android-build") => write(
                &android::built_library(&self.root, android::abi("x86_64").unwrap()),
                b"\x7fELF",
            ),
            _ => {}
        }
        self.invocations.push(invocation.clone());
        Ok(RunOutput {
            success: true,
            code: Some(0),
            output: String::new(),
        })
    }
}

#[test]
fn the_java_helper_is_compiled_dexed_and_packaged() {
    let sdk = sdk(true);
    let root = checkout();
    let environment = FakeEnvironment {
        vars: [("ANDROID_HOME", sdk.path().display().to_string())]
            .into_iter()
            .collect(),
        programs: ALL_PROGRAMS.to_vec(),
    };
    let mut runner = Recording {
        root: root.path().to_path_buf(),
        ..Default::default()
    };
    let apk = android::build(
        root.path(),
        &environment,
        &mut runner,
        &[android::abi("x86_64").unwrap()],
        &mut Vec::new(),
    )
    .unwrap();

    let android_jar = sdk
        .path()
        .join(format!(
            "platforms/android-{}/android.jar",
            android::COMPILE_SDK
        ))
        .display()
        .to_string();
    let labels: Vec<&str> = runner
        .invocations
        .iter()
        .map(|invocation| invocation.label.as_str())
        .collect();
    let position = |label: &str| labels.iter().position(|found| *found == label).unwrap();
    assert!(position("javac") < position("d8") && position("d8") < position("aapt2-link"));

    let javac = &runner.invocations[position("javac")];
    assert_eq!(after(&javac.args, "-classpath"), android_jar);
    assert!(
        javac
            .args
            .iter()
            .any(|arg| arg.ends_with("dev/gpux/mobile/Composition.java")),
        "{:?}",
        javac.args
    );

    let d8 = &runner.invocations[position("d8")];
    assert_eq!(d8.program, "/bin/java");
    assert!(d8.args[1].ends_with("lib/d8.jar"), "{:?}", d8.args);
    assert_eq!(after(&d8.args, "--lib"), android_jar);
    assert_eq!(after(&d8.args, "--min-api"), android::MIN_SDK.to_string());
    assert!(
        d8.args
            .iter()
            .any(|arg| arg.ends_with("dev/gpux/mobile/Composition.class"))
    );

    // The dex is in the signed APK, next to the native library.
    let bytes = std::fs::read(apk).unwrap();
    let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|window| window == needle);
    assert!(contains(b"classes.dex") && contains(b"dex\n035"));
    assert!(contains(b"lib/x86_64/libandroid_host.so"));
}

#[test]
fn a_missing_jdk_or_d8_names_what_installs_it() {
    let root = checkout();
    let build = |sdk: &tempfile::TempDir, programs: Vec<&'static str>| {
        let environment = FakeEnvironment {
            vars: [("ANDROID_HOME", sdk.path().display().to_string())]
                .into_iter()
                .collect(),
            programs,
        };
        let mut runner = Recording {
            root: root.path().to_path_buf(),
            ..Default::default()
        };
        let result = android::build(
            root.path(),
            &environment,
            &mut runner,
            &[android::abi("x86_64").unwrap()],
            &mut Vec::new(),
        );
        (result, runner.invocations.len())
    };

    let complete = sdk(true);
    let without_javac = ALL_PROGRAMS
        .into_iter()
        .filter(|program| *program != "javac")
        .collect();
    let (result, ran) = build(&complete, without_javac);
    let error = result.unwrap_err();
    assert!(
        error.contains("`javac`") && error.contains("JDK"),
        "{error}"
    );
    assert_eq!(ran, 0, "nothing runs without the JDK");

    let (result, ran) = build(&sdk(false), ALL_PROGRAMS.to_vec());
    let error = result.unwrap_err();
    assert!(
        error.contains("d8.jar")
            && error.contains(&format!("build-tools;{}", android::BUILD_TOOLS_VERSION)),
        "{error}"
    );
    assert_eq!(ran, 0, "nothing runs without d8");
}

#[test]
fn the_helper_sources_live_in_the_gpui_mobile_crate() {
    // The real checkout: the helper ships inside the crate, so the crate stays publishable and
    // consumers add the same sources to their build.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sources = android::java_sources(&root);
    assert!(
        sources
            .iter()
            .any(|source| source.ends_with("dev/gpux/mobile/Composition.java")),
        "{sources:?}"
    );
    assert!(
        sources.iter().all(|source| {
            source.starts_with(root.join(android::JAVA_SOURCES))
                && source
                    .extension()
                    .is_some_and(|extension| extension == "java")
        }),
        "{sources:?}"
    );
}
