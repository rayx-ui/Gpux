//! Lane E's Android commands: `android build` (the `android_host` conformance APK) and
//! `android run` (install, launch and follow it on a device or emulator).
//!
//! The APK needs no Gradle: `android_host` is a NativeActivity whose only Java code is gpux's
//! own helper (`crates/gpui_mobile/java`, the `dev.gpux.mobile` package), which `javac` compiles
//! against the pinned `android.jar` and `d8` turns into `classes.dex`. `aapt2` links
//! `tooling/android/AndroidManifest.xml`, the dex and the native libraries are added uncompressed,
//! and `zipalign` (16 KB pages) and `apksigner` (a local debug key) finish it. The SDK, NDK and
//! platform versions are pinned below; `sdkmanager` installs them.

use crate::{
    Environment,
    cli::Selection,
    lanes::{Invocation, OutputKind},
    results::{self, Tally},
    run::{self, Runner},
};
use gpux_conformance::{CATALOG, Lane};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The APK's package name.
pub const PACKAGE: &str = "dev.gpux.conformance";
/// The activity `am start` launches: gpux's NativeActivity, which forwards `onTrimMemory`.
pub const ACTIVITY: &str = "dev.gpux.mobile.GpuxNativeActivity";
/// The library NativeActivity loads, `lib<name>.so`.
pub const LIBRARY: &str = "android_host";
/// The lowest Android API level the host runs on (Vulkan and `libnativewindow`).
pub const MIN_SDK: u32 = 26;
/// The platform the APK links against.
pub const COMPILE_SDK: u32 = 36;
/// The pinned NDK.
pub const NDK_VERSION: &str = "28.0.12674087";
/// gpux's Java sources, inside the `gpui_mobile` crate so the crate stays publishable; consumers
/// add them to their own build.
pub const JAVA_SOURCES: &str = "crates/gpui_mobile/java";

/// The pinned SDK build tools (`aapt2`, `zipalign`, `apksigner`).
pub const BUILD_TOOLS_VERSION: &str = "36.0.0";
/// The logcat tag `gpui_mobile` logs under, where the host's lines appear.
pub const LOG_TAG: &str = "gpui-android";
/// The prefix of the host's requests and markers (result lines use `gpux-conformance:`).
pub const HOST_MARKER: &str = "gpux-conformance-host:";

/// An Android ABI the host builds for, with its Rust target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Abi {
    /// The Android ABI name (`x86_64`, `arm64-v8a`).
    pub name: &'static str,
    /// The Rust target triple.
    pub triple: &'static str,
}

/// The ABIs the host builds for: emulators and devices.
pub const ABIS: [Abi; 2] = [
    Abi {
        name: "x86_64",
        triple: "x86_64-linux-android",
    },
    Abi {
        name: "arm64-v8a",
        triple: "aarch64-linux-android",
    },
];

/// The ABI named `name`, as `getprop ro.product.cpu.abi` or `--abi` gives it.
pub fn abi(name: &str) -> Option<Abi> {
    ABIS.iter().copied().find(|abi| abi.name == name.trim())
}

/// Where the Android SDK pieces live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sdk {
    /// The SDK root.
    pub root: PathBuf,
    /// The NDK cargo-ndk builds with.
    pub ndk: PathBuf,
    /// The build-tools folder.
    pub build_tools: PathBuf,
    /// The platform's `android.jar`.
    pub android_jar: PathBuf,
}

impl Sdk {
    /// A build-tools program, with the platform's executable suffix.
    pub fn tool(&self, name: &str, windows: bool) -> PathBuf {
        self.build_tools.join(if windows {
            format!("{name}.exe")
        } else {
            name.into()
        })
    }

    /// `adb`, from the SDK's platform-tools.
    pub fn adb(&self, windows: bool) -> PathBuf {
        self.root
            .join("platform-tools")
            .join(if windows { "adb.exe" } else { "adb" })
    }
}

/// Finds the pinned SDK pieces, or explains how to install what is missing.
pub fn find_sdk(environment: &dyn Environment) -> Result<Sdk, String> {
    let root = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .find_map(|name| environment.var(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .ok_or(
            "set ANDROID_HOME to the Android SDK (install it with Android Studio or the \
                command-line tools)",
        )?;
    let install = |package: &str| {
        format!(
            "install it with `sdkmanager \"{package}\"` (in {}/cmdline-tools/latest/bin)",
            root.display()
        )
    };
    let pinned = root.join("ndk").join(NDK_VERSION);
    let overridden = environment
        .var("ANDROID_NDK_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let ndk = overridden.clone().unwrap_or_else(|| pinned.clone());
    if !ndk.is_dir() {
        return Err(format!(
            "the NDK {NDK_VERSION} is missing at {}: {}",
            ndk.display(),
            install(&format!("ndk;{NDK_VERSION}"))
        ));
    }
    // CI images and developer shells often set ANDROID_NDK_HOME to another NDK; building with
    // it would silently change the toolchain (r28 is the first to align ELF segments for 16 KB
    // pages by default).
    let is_pinned =
        |path: &Path| std::fs::canonicalize(path).ok() == std::fs::canonicalize(&pinned).ok();
    if overridden.is_some() && !is_pinned(&ndk) {
        let revision = ndk_revision(&ndk);
        if revision.as_deref() != Some(NDK_VERSION) {
            return Err(format!(
                "ANDROID_NDK_HOME points at NDK {} ({}), but lane E pins {NDK_VERSION}: point it \
                 at {} ({}) or unset it",
                revision.as_deref().unwrap_or("of unknown version"),
                ndk.display(),
                pinned.display(),
                install(&format!("ndk;{NDK_VERSION}"))
            ));
        }
    }
    let build_tools = root.join("build-tools").join(BUILD_TOOLS_VERSION);
    if !build_tools.is_dir() {
        return Err(format!(
            "the build tools {BUILD_TOOLS_VERSION} are missing: {}",
            install(&format!("build-tools;{BUILD_TOOLS_VERSION}"))
        ));
    }
    let android_jar = root
        .join("platforms")
        .join(format!("android-{COMPILE_SDK}"))
        .join("android.jar");
    if !android_jar.is_file() {
        return Err(format!(
            "the android-{COMPILE_SDK} platform is missing: {}",
            install(&format!("platforms;android-{COMPILE_SDK}"))
        ));
    }
    Ok(Sdk {
        root,
        ndk,
        build_tools,
        android_jar,
    })
}

/// Where the Android build lives under the workspace root.
pub fn out_dir(root: &Path) -> PathBuf {
    root.join("target").join("gpux-android")
}

/// The signed APK `android build` produces.
pub fn apk_path(root: &Path) -> PathBuf {
    out_dir(root).join("gpux-conformance.apk")
}

/// The cargo-ndk invocation that builds the host library for `abi`.
pub fn cargo_ndk(abi: Abi, sdk: &Sdk) -> Invocation {
    Invocation {
        label: format!("android-build-{}", abi.name),
        program: "cargo".into(),
        args: [
            "ndk",
            "-t",
            abi.name,
            "-P",
            &MIN_SDK.to_string(),
            "build",
            "-p",
            "gpux-conformance",
            "--no-default-features",
            "--features",
            "app-host,diagnostics",
            "--example",
            LIBRARY,
            "--release",
        ]
        .iter()
        .map(|arg| arg.to_string())
        .collect(),
        env: vec![
            ("ANDROID_NDK_HOME".into(), sdk.ndk.display().to_string()),
            // A test host: optimized but quick to link, and small enough to install fast. Its
            // symbols stay, so panic backtraces in logcat name their functions.
            ("CARGO_PROFILE_RELEASE_LTO".into(), "false".into()),
            ("CARGO_PROFILE_RELEASE_CODEGEN_UNITS".into(), "16".into()),
            ("CARGO_PROFILE_RELEASE_DEBUG".into(), "false".into()),
            ("CARGO_PROFILE_RELEASE_STRIP".into(), "debuginfo".into()),
        ],
        output: OutputKind::Listing,
    }
}

/// The host library cargo-ndk built for `abi`.
pub fn built_library(root: &Path, abi: Abi) -> PathBuf {
    root.join("target")
        .join(abi.triple)
        .join("release")
        .join("examples")
        .join(format!("lib{LIBRARY}.so"))
}

pub(crate) fn tool_invocation(label: &str, program: &Path, args: Vec<String>) -> Invocation {
    Invocation {
        label: label.into(),
        program: program.display().to_string(),
        args,
        env: Vec::new(),
        output: OutputKind::Listing,
    }
}

pub(crate) fn run_tool(
    invocation: &Invocation,
    program: &Path,
    workdir: &Path,
    runner: &mut dyn Runner,
    log: &Path,
) -> Result<run::RunOutput, String> {
    let result = runner
        .run(invocation, program, workdir, log)
        .map_err(|error| format!("{error:#}"))?;
    if !result.success {
        return Err(format!(
            "`{}` failed; see {}",
            invocation.command_line(),
            log.display()
        ));
    }
    Ok(result)
}

/// The `.java` files under [`JAVA_SOURCES`], sorted.
pub fn java_sources(root: &Path) -> Vec<PathBuf> {
    files_with_extension(&root.join(JAVA_SOURCES), "java")
}

/// Every file under `dir` with the extension `extension`, sorted.
pub fn files_with_extension(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|found| found == extension) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// `javac` compiling `sources` against the pinned `android.jar` into `classes`.
pub fn javac(sdk: &Sdk, javac: &Path, sources: &[PathBuf], classes: &Path) -> Invocation {
    let mut args = vec![
        "--release".to_string(),
        "11".into(),
        "-classpath".into(),
        sdk.android_jar.display().to_string(),
        "-d".into(),
        classes.display().to_string(),
    ];
    args.extend(sources.iter().map(|source| source.display().to_string()));
    tool_invocation("javac", javac, args)
}

/// The build tools' `d8` (run on `java`) turning `class_files` into `out/classes.dex` for the
/// minimum API level.
pub fn d8(sdk: &Sdk, java: &Path, class_files: &[PathBuf], out: &Path) -> Invocation {
    let mut args = vec![
        "-cp".to_string(),
        sdk.build_tools
            .join("lib")
            .join("d8.jar")
            .display()
            .to_string(),
        "com.android.tools.r8.D8".into(),
        "--release".into(),
        "--min-api".into(),
        MIN_SDK.to_string(),
        "--lib".into(),
        sdk.android_jar.display().to_string(),
        "--output".into(),
        out.display().to_string(),
    ];
    args.extend(class_files.iter().map(|file| file.display().to_string()));
    tool_invocation("d8", java, args)
}

/// Appends each `(path in the APK, file)` uncompressed, so libraries load in place.
pub fn add_stored(apk: &Path, entries: &[(String, PathBuf)]) -> Result<(), String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(apk)
        .map_err(|error| format!("could not open {}: {error}", apk.display()))?;
    let mut zip = zip::ZipWriter::new_append(file)
        .map_err(|error| format!("{} is not a zip archive: {error}", apk.display()))?;
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, source) in entries {
        let bytes = std::fs::read(source)
            .map_err(|error| format!("could not read {}: {error}", source.display()))?;
        zip.start_file(name.as_str(), options)
            .and_then(|()| zip.write_all(&bytes).map_err(Into::into))
            .map_err(|error| format!("could not add {name}: {error}"))?;
    }
    zip.finish()
        .map_err(|error| format!("could not finish {}: {error}", apk.display()))?;
    Ok(())
}

/// Builds the host for `abis` and assembles the signed APK at [`apk_path`].
pub fn build(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    abis: &[Abi],
    out: &mut dyn Write,
) -> Result<PathBuf, String> {
    let sdk = find_sdk(environment)?;
    let windows = environment.os() == crate::Os::Windows;
    let java = environment
        .find_program("java")
        .ok_or("`java` is not on PATH: install JDK 21 and put its `bin` on PATH")?;
    let keytool = environment
        .find_program("keytool")
        .ok_or("`keytool` is not on PATH: it comes with the JDK; put the JDK's `bin` on PATH")?;
    let cargo = environment
        .find_program("cargo")
        .ok_or("`cargo` is not on PATH: install Rust with rustup (https://rustup.rs)")?;
    if environment.find_program("cargo-ndk").is_none() {
        return Err("cargo-ndk is missing: install it with `cargo install cargo-ndk`".into());
    }
    let javac_program = environment.find_program("javac").ok_or(
        "`javac` is not on PATH: it comes with the JDK (install JDK 21); put its `bin` on PATH",
    )?;
    if !sdk.build_tools.join("lib").join("d8.jar").is_file() {
        return Err(format!(
            "the build tools {BUILD_TOOLS_VERSION} have no `lib/d8.jar`: reinstall them with \
             `sdkmanager \"build-tools;{BUILD_TOOLS_VERSION}\"`"
        ));
    }
    let logs = run::artifacts_dir(root, "e");
    let out_dir = out_dir(root);
    std::fs::create_dir_all(&out_dir)
        .map_err(|error| format!("could not create {}: {error}", out_dir.display()))?;

    let mut libraries = Vec::new();
    for &abi in abis {
        let invocation = cargo_ndk(abi, &sdk);
        writeln!(out, "android: {}", invocation.command_line()).ok();
        out.flush().ok();
        run_tool(
            &invocation,
            &cargo,
            root,
            runner,
            &logs.join(format!("{}.log", invocation.label)),
        )
        .map_err(|error| {
            format!(
                "{error}. The build needs the Rust target: `rustup target add {}`",
                abi.triple
            )
        })?;
        libraries.push((
            format!("lib/{}/lib{LIBRARY}.so", abi.name),
            built_library(root, abi),
        ));
    }

    // gpux's Java helper: javac, then d8.
    let classes = out_dir.join("classes");
    let dex = out_dir.join("dex");
    for dir in [&classes, &dex] {
        std::fs::remove_dir_all(dir).ok();
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    }
    let sources = java_sources(root);
    if sources.is_empty() {
        return Err(format!("no Java sources under {JAVA_SOURCES}"));
    }
    let compile = javac(&sdk, &javac_program, &sources, &classes);
    writeln!(out, "android: {}", compile.command_line()).ok();
    run_tool(
        &compile,
        &javac_program,
        root,
        runner,
        &logs.join("javac.log"),
    )?;
    let dexing = d8(&sdk, &java, &files_with_extension(&classes, "class"), &dex);
    writeln!(out, "android: {}", dexing.command_line()).ok();
    run_tool(&dexing, &java, root, runner, &logs.join("d8.log"))?;
    libraries.push(("classes.dex".into(), dex.join("classes.dex")));

    let unsigned = out_dir.join("unsigned.apk");
    let aapt2 = sdk.tool("aapt2", windows);
    let link = tool_invocation(
        "aapt2-link",
        &aapt2,
        vec![
            "link".into(),
            "-o".into(),
            unsigned.display().to_string(),
            "--manifest".into(),
            root.join("tooling/android/AndroidManifest.xml")
                .display()
                .to_string(),
            "-I".into(),
            sdk.android_jar.display().to_string(),
            "--min-sdk-version".into(),
            MIN_SDK.to_string(),
            "--target-sdk-version".into(),
            COMPILE_SDK.to_string(),
            "--version-code".into(),
            "1".into(),
            "--version-name".into(),
            env!("CARGO_PKG_VERSION").into(),
        ],
    );
    writeln!(out, "android: {}", link.command_line()).ok();
    run_tool(&link, &aapt2, root, runner, &logs.join("aapt2-link.log"))?;
    if unsigned.is_file() {
        add_stored(&unsigned, &libraries)?;
    }

    let aligned = out_dir.join("aligned.apk");
    let zipalign = sdk.tool("zipalign", windows);
    let align = tool_invocation(
        "zipalign",
        &zipalign,
        vec![
            "-P".into(),
            "16".into(),
            "-f".into(),
            "4".into(),
            unsigned.display().to_string(),
            aligned.display().to_string(),
        ],
    );
    writeln!(out, "android: {}", align.command_line()).ok();
    run_tool(&align, &zipalign, root, runner, &logs.join("zipalign.log"))?;

    // A local debug key, as Android Studio makes one: these are test credentials.
    let keystore = out_dir.join("debug.keystore");
    if !keystore.is_file() {
        let genkey = tool_invocation(
            "keytool",
            &keytool,
            [
                "-genkeypair",
                "-keystore",
                &keystore.display().to_string(),
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-alias",
                "androiddebugkey",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-dname",
                "CN=Android Debug,O=Android,C=US",
            ]
            .iter()
            .map(|arg| arg.to_string())
            .collect(),
        );
        run_tool(&genkey, &keytool, root, runner, &logs.join("keytool.log"))?;
    }
    let apk = apk_path(root);
    let sign = tool_invocation(
        "apksigner",
        &java,
        vec![
            "-jar".into(),
            sdk.build_tools
                .join("lib")
                .join("apksigner.jar")
                .display()
                .to_string(),
            "sign".into(),
            "--ks".into(),
            keystore.display().to_string(),
            "--ks-pass".into(),
            "pass:android".into(),
            "--key-pass".into(),
            "pass:android".into(),
            "--ks-key-alias".into(),
            "androiddebugkey".into(),
            "--out".into(),
            apk.display().to_string(),
            aligned.display().to_string(),
        ],
    );
    writeln!(out, "android: {}", sign.command_line()).ok();
    run_tool(&sign, &java, root, runner, &logs.join("apksigner.log"))?;
    writeln!(out, "android: built {}", apk.display()).ok();
    Ok(apk)
}

/// What the host asks of the testkit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// `adb shell input tap <x> <y>`.
    Tap(i32, i32),
    /// `adb shell input swipe <x1> <y1> <x2> <y2> <ms>`.
    Swipe(i32, i32, i32, i32, u32),
    /// `adb shell am send-trim-memory <package> <level>`: the system's `onTrimMemory`.
    TrimMemory(String),
    /// The home key.
    Background,
    /// The activity relaunched.
    Foreground,
    /// `adb screencap`, written to the app's `files/screens/<n>.png`.
    Screenshot(u32),
    /// `uiautomator dump` of the screen's accessibility nodes, written to the app's
    /// `files/a11y/<n>.xml`.
    A11yDump(u32),
    /// The run is over.
    Done,
}

/// The request in a logcat line, whatever prefix the line carries.
pub fn request(line: &str) -> Option<Request> {
    let rest = line.split_once(HOST_MARKER)?.1.trim();
    let mut words = rest.split_whitespace();
    let numbers = |words: std::str::SplitWhitespace<'_>| -> Option<Vec<i64>> {
        words.map(|word| word.parse().ok()).collect()
    };
    match words.next()? {
        "tap" => match numbers(words)?.as_slice() {
            [x, y] => Some(Request::Tap(*x as i32, *y as i32)),
            _ => None,
        },
        "swipe" => match numbers(words)?.as_slice() {
            [x1, y1, x2, y2, ms] => Some(Request::Swipe(
                *x1 as i32, *y1 as i32, *x2 as i32, *y2 as i32, *ms as u32,
            )),
            _ => None,
        },
        "trim-memory" => {
            let level = words.next()?;
            let known = [
                "HIDDEN",
                "RUNNING_MODERATE",
                "BACKGROUND",
                "RUNNING_LOW",
                "MODERATE",
                "RUNNING_CRITICAL",
                "COMPLETE",
            ];
            (known.contains(&level) && words.next().is_none())
                .then(|| Request::TrimMemory(level.to_string()))
        }
        "background" => Some(Request::Background),
        "foreground" => Some(Request::Foreground),
        "screenshot" => match numbers(words)?.as_slice() {
            [number] => Some(Request::Screenshot(u32::try_from(*number).ok()?)),
            _ => None,
        },
        "a11y-dump" => match numbers(words)?.as_slice() {
            [number] => Some(Request::A11yDump(u32::try_from(*number).ok()?)),
            _ => None,
        },
        "done" => Some(Request::Done),
        _ => None,
    }
}

/// The `adb shell` commands that carry out a request.
pub fn shell_commands(request: &Request) -> Vec<Vec<String>> {
    let words = |words: &[&str]| {
        words
            .iter()
            .map(|word| word.to_string())
            .collect::<Vec<_>>()
    };
    match request {
        Request::Tap(x, y) => vec![words(&["input", "tap", &x.to_string(), &y.to_string()])],
        Request::Swipe(x1, y1, x2, y2, ms) => vec![words(&[
            "input",
            "swipe",
            &x1.to_string(),
            &y1.to_string(),
            &x2.to_string(),
            &y2.to_string(),
            &ms.to_string(),
        ])],
        Request::TrimMemory(level) => vec![words(&["am", "send-trim-memory", PACKAGE, level])],
        Request::Background => vec![words(&["input", "keyevent", "KEYCODE_HOME"])],
        Request::Foreground => vec![words(&[
            "am",
            "start",
            "-n",
            &format!("{PACKAGE}/{ACTIVITY}"),
        ])],
        Request::Screenshot(_) | Request::A11yDump(_) | Request::Done => Vec::new(),
    }
}

/// The revision an NDK's `source.properties` declares, without a pre-release suffix: its
/// `Pkg.BaseRevision`, or its `Pkg.Revision` up to the first `-` (r28-beta2 declares
/// `28.0.12674087-beta2` and the base revision `28.0.12674087`).
pub fn ndk_revision(ndk: &Path) -> Option<String> {
    let properties = std::fs::read_to_string(ndk.join("source.properties")).ok()?;
    let value = |name: &str| {
        properties.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == name).then(|| value.trim().to_string())
        })
    };
    value("Pkg.BaseRevision").or_else(|| {
        value("Pkg.Revision")
            .map(|revision| revision.split('-').next().unwrap_or_default().to_string())
    })
}

/// Whether `adb install` failed because the device holds the host signed with another key. Each
/// checkout signs with its own debug key, so the test app is then replaced rather than updated.
pub fn signed_with_another_key(install_output: &str) -> bool {
    install_output.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE")
}

/// The ids of the Android platform checks the host reports, in order.
pub const PLATFORM_CHECKS: [&str; 18] = [
    "android-startup",
    "android-touch-tap",
    "android-touch-pan",
    "android-touch-fling",
    "android-lifecycle",
    "composition-android-order",
    "composition-android-input",
    "composition-android-pan",
    "composition-android-shadow",
    "scroll-layers-android-div",
    "scroll-layers-android-list",
    "scroll-layers-android-resume",
    "scroll-layers-android-timing",
    "platform-view-android",
    "platform-view-android-text",
    "screen-reader-android-tree",
    "screen-reader-android-click",
    "screen-reader-android-adjust",
];

/// Every result `android run` expects for `selection`: the platform checks and the lane-E
/// scenes it selects.
pub fn expected_results(selection: &Selection) -> Vec<String> {
    PLATFORM_CHECKS
        .iter()
        .copied()
        .chain(
            CATALOG
                .iter()
                .filter(|scene| scene.runs_in(Lane::E))
                .map(|scene| scene.id),
        )
        .filter(|id| {
            gpux_conformance::selects(id, selection.scene.as_deref(), selection.grep.as_deref())
        })
        .map(str::to_string)
        .collect()
}

/// The environment variable that lets the host pick the emulator's software Vulkan adapter, which
/// it skips otherwise: lane E then proves the Vulkan path where no hardware Vulkan is at hand.
pub const SOFTWARE_VULKAN_VAR: &str = "GPUX_ANDROID_SOFTWARE_VULKAN";

/// The `adb shell setprop` that carries [`SOFTWARE_VULKAN_VAR`] to the host: `1`, or empty so an
/// earlier run's setting does not linger.
pub fn software_vulkan_property(environment: &dyn Environment) -> Vec<String> {
    let on = environment.var(SOFTWARE_VULKAN_VAR).as_deref() == Some("1");
    vec![
        "setprop".into(),
        "debug.gpux.software_vulkan".into(),
        if on { "1" } else { "''" }.into(),
    ]
}

/// The system properties that carry `selection` to the host, as `adb shell setprop` arguments.
/// Unset parts are set empty, so an earlier run's selection does not linger.
pub fn selection_properties(selection: &Selection) -> Result<Vec<Vec<String>>, String> {
    [
        ("debug.gpux.scene", selection.scene.as_deref()),
        ("debug.gpux.grep", selection.grep.as_deref()),
    ]
    .iter()
    .map(|(name, value)| {
        let value = value.unwrap_or_default();
        if value.contains('\'') || value.len() > 91 {
            return Err(format!(
                "`{value}` cannot travel to the device as a system property"
            ));
        }
        // The device shell parses the command line; quote the value for it.
        Ok(vec![
            "setprop".into(),
            name.to_string(),
            format!("'{value}'"),
        ])
    })
    .collect()
}

/// Counts the host's result lines, and every expected result it never reported as a failure.
pub fn tally(output: &str, selection: &Selection) -> Tally {
    let mut tally = results::tally_result_lines(output);
    let reported: Vec<String> = results::result_lines(output)
        .into_iter()
        .map(|line| line.scene)
        .collect();
    for id in expected_results(selection) {
        if !reported.contains(&id) {
            tally.add(Tally::failure(format!("{id}: no result was reported")));
        }
    }
    tally
}

/// The serial of the device to run on: `serial` when it is attached, or the first attached one.
pub fn pick_device(devices_output: &str, serial: Option<&str>) -> Result<String, String> {
    let attached: Vec<&str> = devices_output
        .lines()
        .skip_while(|line| !line.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let (serial, state) = line.split_once(char::is_whitespace)?;
            (state.trim() == "device").then_some(serial)
        })
        .collect();
    match serial {
        Some(serial) if attached.contains(&serial) => Ok(serial.to_string()),
        Some(serial) => Err(format!(
            "device `{serial}` is not attached; attached: {}",
            if attached.is_empty() {
                "none".into()
            } else {
                attached.join(", ")
            }
        )),
        None => attached
            .first()
            .map(|serial| serial.to_string())
            .ok_or_else(|| {
                "no Android device is attached: start an emulator (`emulator -avd <name>`) or \
             connect a device with USB debugging"
                    .into()
            }),
    }
}

/// How long `android run` follows the host before giving up on it.
pub const RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45 * 60);

/// Installs the host on a device (the first attached one, or `serial`), runs it, carries out
/// its requests, pulls the frames failing probes saved, and counts its results.
pub fn run(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    serial: Option<&str>,
    selection: &Selection,
    out: &mut dyn Write,
) -> Tally {
    match run_on_device(root, environment, runner, serial, selection, out) {
        Ok(tally) => tally,
        Err(error) => Tally::failure(format!("lane e: {error}")),
    }
}

fn run_on_device(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    serial: Option<&str>,
    selection: &Selection,
    out: &mut dyn Write,
) -> Result<Tally, String> {
    let mut properties = selection_properties(selection)?;
    properties.push(software_vulkan_property(environment));
    let sdk = find_sdk(environment)?;
    let windows = environment.os() == crate::Os::Windows;
    let adb = sdk.adb(windows);
    if !adb.is_file() {
        return Err(format!(
            "adb is missing at {}: install it with `sdkmanager \"platform-tools\"`",
            adb.display()
        ));
    }
    let logs = run::artifacts_dir(root, "e");
    let adb_run = |label: &str, args: Vec<String>, runner: &mut dyn Runner| {
        let invocation = tool_invocation(label, &adb, args);
        run_tool(
            &invocation,
            &adb,
            root,
            runner,
            &logs.join(format!("{label}.log")),
        )
    };
    let devices = adb_run("adb-devices", vec!["devices".into()], runner)?;
    let serial = pick_device(&devices.output, serial)?;
    let on_device = |args: &[&str]| -> Vec<String> {
        ["-s", serial.as_str()]
            .iter()
            .chain(args)
            .map(|arg| arg.to_string())
            .collect()
    };
    let abi_name = adb_run(
        "adb-abi",
        on_device(&["shell", "getprop", "ro.product.cpu.abi"]),
        runner,
    )?
    .output;
    let device_abi = abi(&abi_name).ok_or_else(|| {
        format!(
            "device {serial} reports ABI `{}`; the host builds for {}",
            abi_name.trim(),
            ABIS.iter()
                .map(|abi| abi.name)
                .collect::<Vec<_>>()
                .join(" and ")
        )
    })?;
    writeln!(out, "lane e: device {serial} ({})", device_abi.name).ok();

    let apk = build(root, environment, runner, &[device_abi], out)?;
    let install = on_device(&["install", "-r", "-t", &apk.display().to_string()]);
    let invocation = tool_invocation("adb-install", &adb, install.clone());
    let installed = runner
        .run(&invocation, &adb, root, &logs.join("adb-install.log"))
        .map_err(|error| format!("{error:#}"))?;
    if !installed.success {
        if !signed_with_another_key(&installed.output) {
            return Err(format!(
                "`{}` failed; see {}",
                invocation.command_line(),
                logs.join("adb-install.log").display()
            ));
        }
        writeln!(
            out,
            "lane e: the device has the host signed with another debug key (another checkout's, \
             or one from before `target/` was cleaned); replacing it"
        )
        .ok();
        adb_run("adb-uninstall", on_device(&["uninstall", PACKAGE]), runner)?;
        adb_run("adb-install", install, runner)?;
    }
    adb_run("adb-logcat-clear", on_device(&["logcat", "-c"]), runner)?;
    for property in properties {
        let mut args = on_device(&["shell"]);
        args.extend(property);
        adb_run("adb-select", args, runner)?;
    }
    adb_run(
        "adb-stop",
        on_device(&["shell", "am", "force-stop", PACKAGE]),
        runner,
    )?;
    adb_run(
        "adb-start",
        on_device(&[
            "shell",
            "am",
            "start",
            "-n",
            &format!("{PACKAGE}/{ACTIVITY}"),
        ]),
        runner,
    )?;
    writeln!(out, "lane e: following logcat ({LOG_TAG})").ok();
    out.flush().ok();

    let follow = tool_invocation(
        "logcat",
        &adb,
        on_device(&["logcat", "-v", "raw", "-s", &format!("{LOG_TAG}:I")]),
    );
    let mut collected = String::new();
    let mut failures = Vec::new();
    // The host repeats a screenshot or dump request until the file arrives.
    let mut screenshots = std::collections::HashSet::new();
    let mut dumps = std::collections::HashSet::new();
    let mut handle = |line: &str| -> bool {
        collected.push_str(line);
        collected.push('\n');
        if line.contains(gpux_conformance::report::MARKER)
            || line.contains(HOST_MARKER)
            || line.contains("PANIC")
        {
            writeln!(out, "{line}").ok();
            out.flush().ok();
        }
        if line.contains("PANIC") {
            // gpui_mobile logs the panic; the host cannot report anything after it.
            failures.push(format!("the host panicked: {line}"));
            return false;
        }
        match request(line) {
            Some(Request::Done) => return false,
            Some(Request::Screenshot(number)) => {
                if !screenshots.insert(number) {
                    return true;
                }
                if let Err(error) = screenshot(&adb, &serial, &logs, number, runner, root) {
                    failures.push(format!("could not take screenshot {number}: {error}"));
                }
            }
            Some(Request::A11yDump(number)) => {
                if !dumps.insert(number) {
                    return true;
                }
                if let Err(error) = a11y_dump(&adb, &serial, &logs, number, runner, root) {
                    failures.push(format!(
                        "could not dump the accessibility nodes {number}: {error}"
                    ));
                }
            }
            Some(request) => {
                for command in shell_commands(&request) {
                    let mut args = on_device(&["shell"]);
                    args.extend(command);
                    if let Err(error) = adb_run("adb-request", args, runner) {
                        failures.push(format!("could not carry out {request:?}: {error}"));
                    }
                }
            }
            None => {}
        }
        true
    };
    let finished = run::follow_lines(
        &follow,
        &adb,
        root,
        &logs.join("logcat.log"),
        RUN_TIMEOUT,
        &mut handle,
    )
    .map_err(|error| format!("could not follow logcat: {error:#}"))?;

    pull_frames(&adb, &serial, &logs, runner, root, out);
    // `adb logcat` can drop lines from its stream (it did while transferring screenshots); the
    // device's log buffer keeps them.
    let device_log = device_log(&adb, &serial, &logs, runner, root);
    let mut tally = tally(fuller_log(&collected, &device_log), selection);
    for failure in failures {
        tally.add(Tally::failure(failure));
    }
    // A panic also stops the follow; only silence until the deadline is a timeout.
    if !finished {
        tally.add(Tally::failure(format!(
            "the host did not finish within {} minutes; see {}",
            RUN_TIMEOUT.as_secs() / 60,
            logs.join("logcat.log").display()
        )));
    }
    Ok(tally)
}

/// The host's lines in the device's log buffer (`adb logcat -d`), or nothing if adb fails.
fn device_log(
    adb: &Path,
    serial: &str,
    logs: &Path,
    runner: &mut dyn Runner,
    root: &Path,
) -> String {
    let dump = tool_invocation(
        "adb-logcat-dump",
        adb,
        ["-s", serial, "logcat", "-d", "-v", "raw", "-s"]
            .iter()
            .map(|arg| arg.to_string())
            .chain([format!("{LOG_TAG}:I")])
            .collect(),
    );
    match runner.run(&dump, adb, root, &logs.join("adb-logcat-dump.log")) {
        Ok(result) if result.success => result.output,
        _ => String::new(),
    }
}

/// Of the streamed log and the device's buffer of the same run, the one that reports more
/// results: the stream when the buffer is missing or wrapped, the buffer when the stream dropped
/// lines.
pub fn fuller_log<'a>(streamed: &'a str, device: &'a str) -> &'a str {
    let reported = |log: &str| {
        results::result_lines(log)
            .into_iter()
            .map(|line| line.scene)
            .collect::<std::collections::HashSet<_>>()
            .len()
    };
    if reported(device) > reported(streamed) {
        device
    } else {
        streamed
    }
}

/// Copies the frames failing pixel probes saved on the device into `artifacts-temp/testkit/e/`.
fn pull_frames(
    adb: &Path,
    serial: &str,
    logs: &Path,
    runner: &mut dyn Runner,
    root: &Path,
    out: &mut dyn Write,
) {
    let listing = tool_invocation(
        "adb-frames",
        adb,
        [
            "-s",
            serial,
            "shell",
            "run-as",
            PACKAGE,
            "ls",
            "files/frames",
        ]
        .iter()
        .map(|arg| arg.to_string())
        .collect(),
    );
    let Ok(result) = runner.run(&listing, adb, root, &logs.join("adb-frames.log")) else {
        return;
    };
    if !result.success {
        return;
    }
    for name in result
        .output
        .split_whitespace()
        .filter(|name| name.ends_with(".png"))
    {
        let destination = logs.join("frames").join(name);
        let args: Vec<String> = ["-s", serial, "exec-out", "run-as", PACKAGE, "cat"]
            .iter()
            .map(|arg| arg.to_string())
            .chain([format!("files/frames/{name}")])
            .collect();
        match run::save_output(adb, &args, &destination) {
            Ok(()) => writeln!(out, "lane e: pulled {}", destination.display()).ok(),
            Err(error) => writeln!(out, "lane e: could not pull {name}: {error:#}").ok(),
        };
    }
}

/// Takes screenshot `number` of the device, keeps it in `artifacts-temp/testkit/e/screens/`, and
/// puts it in the app's `files/screens/<number>.png`, where the host waits for it. The file goes
/// through `/data/local/tmp` and `run-as cp`: adb on Windows reads standard input as text, which
/// ends a PNG at its signature's 0x1A byte.
/// `uiautomator dump` of the screen's accessibility nodes, kept in the logs and handed to the
/// host as its `files/a11y/<number>.xml`.
fn a11y_dump(
    adb: &Path,
    serial: &str,
    logs: &Path,
    number: u32,
    runner: &mut dyn Runner,
    root: &Path,
) -> Result<(), String> {
    let args = |args: &[&str]| -> Vec<String> {
        ["-s", serial]
            .iter()
            .chain(args)
            .map(|arg| arg.to_string())
            .collect()
    };
    let staged = format!("/data/local/tmp/gpux-a11y-{number}.xml");
    let local = logs.join("a11y").join(format!("{number}.xml"));
    std::fs::create_dir_all(logs.join("a11y")).map_err(|error| error.to_string())?;
    let target = format!("files/a11y/{number}.xml");
    let partial = format!("{target}.part");
    for command in [
        args(&["shell", "uiautomator", "dump", &staged]),
        args(&["pull", &staged, &local.display().to_string()]),
        args(&["shell", "chmod", "644", &staged]),
        args(&["shell", "run-as", PACKAGE, "mkdir", "-p", "files/a11y"]),
        // Copy under a temporary name and rename, so the host never reads a partial file.
        args(&["shell", "run-as", PACKAGE, "cp", &staged, &partial]),
        args(&["shell", "run-as", PACKAGE, "mv", &partial, &target]),
        args(&["shell", "rm", "-f", &staged]),
    ] {
        let invocation = tool_invocation("adb-a11y-dump", adb, command);
        run_tool(
            &invocation,
            adb,
            root,
            runner,
            &logs.join("adb-a11y-dump.log"),
        )?;
    }
    Ok(())
}

fn screenshot(
    adb: &Path,
    serial: &str,
    logs: &Path,
    number: u32,
    runner: &mut dyn Runner,
    root: &Path,
) -> Result<(), String> {
    let local = logs.join("screens").join(format!("{number}.png"));
    let args = |args: &[&str]| -> Vec<String> {
        ["-s", serial]
            .iter()
            .chain(args)
            .map(|arg| arg.to_string())
            .collect()
    };
    run::save_output(adb, &args(&["exec-out", "screencap", "-p"]), &local)
        .map_err(|error| format!("{error:#}"))?;
    let staged = format!("/data/local/tmp/gpux-screen-{number}.png");
    let target = format!("files/screens/{number}.png");
    let partial = format!("{target}.part");
    for command in [
        args(&["push", &local.display().to_string(), &staged]),
        args(&["shell", "chmod", "644", &staged]),
        // Copy under a temporary name and rename, so the host never reads a partial file.
        args(&["shell", "run-as", PACKAGE, "cp", &staged, &partial]),
        args(&["shell", "run-as", PACKAGE, "mv", &partial, &target]),
        args(&["shell", "rm", "-f", &staged]),
    ] {
        let invocation = tool_invocation("adb-screenshot", adb, command);
        run_tool(
            &invocation,
            adb,
            root,
            runner,
            &logs.join("adb-screenshot.log"),
        )?;
    }
    Ok(())
}
