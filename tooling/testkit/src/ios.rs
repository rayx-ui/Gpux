//! Lane F: the iOS conformance host on a Simulator.
//!
//! `gpux-testkit ios build --simulator` builds the `ios_host` example of `gpux-conformance` as a
//! static library for the Simulator, generates the Xcode project of `tooling/ios` with XcodeGen,
//! and builds the app with `xcodebuild`, unsigned. `gpux-testkit ios run --simulator` (and
//! `lane f`) boots the pinned Simulator, installs the app, launches it with its console attached
//! (`simctl launch --console-pty`), performs the host's requests, copies the frames failing pixel
//! steps saved, and counts the result lines. Devices are run by hand with the developer's
//! signing team (see `docs/testing.md`).

use crate::{
    Environment, Os,
    android::{run_tool, tool_invocation},
    cli::Selection,
    results::{self, Tally},
    run::{self, Runner},
};
use gpux_conformance::{CATALOG, Lane};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

/// The app's bundle identifier (`tooling/ios/project.yml`).
pub const BUNDLE_ID: &str = "dev.gpux.conformance";
/// The Rust target of the Simulator build.
pub const SIMULATOR_TARGET: &str = "aarch64-apple-ios-sim";
/// The Simulator lane F runs on, created on first use.
pub const SIMULATOR_NAME: &str = "gpux-conformance";
/// The pinned device type of that Simulator.
pub const DEVICE_TYPE: &str = "com.apple.CoreSimulator.SimDeviceType.iPhone-16";
/// The app that comes to the front when the host asks to go to the background.
pub const OTHER_APP: &str = "com.apple.Preferences";
/// The prefix of the host's requests.
pub const HOST_MARKER: &str = "gpux-conformance-host:";
/// How long a run may take before it counts as hung.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// The ids of the iOS platform checks the host reports, in order.
pub const PLATFORM_CHECKS: [&str; 13] = [
    "ios-startup",
    "ios-metal",
    "ios-text-metrics",
    "ios-touch-tap",
    "ios-lifecycle",
    "composition-ios-order",
    "composition-ios-input",
    "composition-ios-shadow",
    "scroll-layers-ios-div",
    "scroll-layers-ios-list",
    "scroll-layers-ios-resume",
    "scroll-layers-ios-timing",
    "platform-view-ios",
];

/// Every result `ios run` expects for `selection`: the platform checks and the lane-F scenes it
/// selects.
pub fn expected_results(selection: &Selection) -> Vec<String> {
    PLATFORM_CHECKS
        .iter()
        .copied()
        .chain(
            CATALOG
                .iter()
                .filter(|scene| scene.runs_in(Lane::F))
                .map(|scene| scene.id),
        )
        .filter(|id| {
            gpux_conformance::selects(id, selection.scene.as_deref(), selection.grep.as_deref())
        })
        .map(str::to_string)
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

/// What the host asks the testkit for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Another app comes to the front.
    Background,
    /// The host comes back to the front.
    Foreground,
    /// A screenshot of the Simulator's screen is written to this path, which the host reads
    /// back (a Simulator app's files are files of the Mac).
    Screenshot(String),
    /// The run is over.
    Done,
}

/// The request in a console line, whatever prefix the line carries.
pub fn request(line: &str) -> Option<Request> {
    match line.split_once(HOST_MARKER)?.1.trim() {
        "background" => Some(Request::Background),
        "foreground" => Some(Request::Foreground),
        "done" => Some(Request::Done),
        other => other
            .strip_prefix("screenshot ")
            .map(|path| Request::Screenshot(path.trim().to_string())),
    }
}

/// Why the run is over when a console line reports that the host died: a panic, or dyld failing
/// to load the app (an unresolved symbol or a missing library), after which `simctl launch
/// --console-pty` keeps waiting on an app that never started.
pub fn fatal(line: &str) -> Option<String> {
    if line.contains("panicked at") {
        Some(format!("the host panicked: {line}"))
    } else if line.trim_start().starts_with("dyld[") {
        Some(format!("the app did not load: {line}"))
    } else {
        None
    }
}

/// The `simctl` invocations that carry out a request on `udid`.
///
/// Launching the running host only brings it to the front. Opening a URL of its own would too,
/// but a Simulator that has not opened one before asks "Open in …?" and waits for a tap that
/// never comes, with the host suspended behind the other app.
pub fn simctl_commands(request: &Request, udid: &str) -> Vec<Vec<String>> {
    let words = |words: &[&str]| words.iter().map(|word| word.to_string()).collect();
    match request {
        Request::Background => vec![words(&["simctl", "launch", udid, OTHER_APP])],
        Request::Foreground => vec![words(&["simctl", "launch", udid, BUNDLE_ID])],
        Request::Screenshot(path) => vec![words(&["simctl", "io", udid, "screenshot", path])],
        Request::Done => Vec::new(),
    }
}

/// Where lane F's build goes.
pub fn out_dir(root: &Path) -> PathBuf {
    root.join("target").join("gpux-ios")
}

/// The host's static library for the Simulator.
pub fn host_library(root: &Path) -> PathBuf {
    root.join("target")
        .join(SIMULATOR_TARGET)
        .join("debug")
        .join("examples")
        .join("libios_host.a")
}

/// The Xcode project XcodeGen generates beside its spec.
pub fn project(root: &Path) -> PathBuf {
    root.join("tooling")
        .join("ios")
        .join("GpuxConformance.xcodeproj")
}

/// The built app.
pub fn app_path(root: &Path) -> PathBuf {
    out_dir(root)
        .join("products")
        .join("Debug-iphonesimulator")
        .join("GpuxConformance.app")
}

/// `cargo build` of the host's static library for the Simulator.
pub fn cargo_build() -> Vec<String> {
    [
        "build",
        "-p",
        "gpux-conformance",
        "--no-default-features",
        "--features",
        "app-host,diagnostics",
        "--example",
        "ios_host",
        "--target",
        SIMULATOR_TARGET,
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect()
}

/// `xcodebuild` of the app for the Simulator, unsigned, linking `library`.
pub fn xcodebuild(root: &Path, library: &Path) -> Vec<String> {
    let out = out_dir(root);
    vec![
        "-project".into(),
        project(root).display().to_string(),
        "-target".into(),
        "GpuxConformance".into(),
        "-configuration".into(),
        "Debug".into(),
        "-sdk".into(),
        "iphonesimulator".into(),
        format!("SYMROOT={}", out.join("products").display()),
        format!("OBJROOT={}", out.join("intermediates").display()),
        "CODE_SIGNING_ALLOWED=NO".into(),
        format!("GPUX_IOS_HOST_LIB={}", library.display()),
        "build".into(),
    ]
}

/// The UDID of the available Simulator named `name` in `xcrun simctl list devices available`,
/// and whether it is booted.
pub fn simulator_device(list: &str, name: &str) -> Option<(String, bool)> {
    list.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(name)?.strip_prefix(" (")?;
        let (udid, state) = rest.split_once(") (")?;
        Some((
            udid.to_string(),
            state.trim_end_matches(')').trim() == "Booted",
        ))
    })
}

/// The identifier of the newest iOS runtime in `xcrun simctl list runtimes available`.
pub fn newest_ios_runtime(list: &str) -> Option<String> {
    list.lines()
        .filter_map(|line| {
            let line = line.trim();
            let version = line.strip_prefix("iOS ")?.split_whitespace().next()?;
            let version: Vec<u32> = version
                .split('.')
                .map(|part| part.parse().ok())
                .collect::<Option<_>>()?;
            let identifier = line.rsplit_once(" - ")?.1.trim();
            identifier
                .starts_with("com.apple.CoreSimulator.SimRuntime.iOS")
                .then(|| (version, identifier.to_string()))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, identifier)| identifier)
}

/// `simctl launch` of the host on `udid` with its console attached and `selection` as launch
/// arguments.
pub fn launch(udid: &str, selection: &Selection) -> Vec<String> {
    let mut args: Vec<String> = [
        "simctl",
        "launch",
        "--console-pty",
        "--terminate-running-process",
        udid,
        BUNDLE_ID,
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    if let Some(scene) = &selection.scene {
        args.extend(["--scene".into(), scene.clone()]);
    }
    if let Some(grep) = &selection.grep {
        args.extend(["--grep".into(), grep.clone()]);
    }
    args
}

/// What the iOS lane needs from the host: macOS with Xcode's tools, XcodeGen and cargo.
fn tools(environment: &dyn Environment) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), String> {
    if environment.os() != Os::MacOs {
        return Err(
            "lane f runs the iOS Simulator, which needs macOS with Xcode; run it on a Mac or \
             through the lane-f CI job"
                .into(),
        );
    }
    let find = |name: &str, install: &str| {
        environment
            .find_program(name)
            .ok_or_else(|| format!("`{name}` is not on PATH: {install}"))
    };
    Ok((
        find("cargo", "install Rust with rustup")?,
        find("xcodegen", "install it with `brew install xcodegen`")?,
        find(
            "xcodebuild",
            "install Xcode and run `xcode-select --install`",
        )?,
        find("xcrun", "install Xcode and run `xcode-select --install`")?,
    ))
}

/// Builds the app for the Simulator and returns its path.
pub fn build(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Result<PathBuf, String> {
    let (cargo, xcodegen, xcodebuild_program, _) = tools(environment)?;
    let logs = run::artifacts_dir(root, "f");
    let steps: [(&str, &Path, Vec<String>); 3] = [
        ("ios-host", &cargo, cargo_build()),
        (
            "xcodegen",
            &xcodegen,
            vec![
                "generate".into(),
                "--spec".into(),
                root.join("tooling")
                    .join("ios")
                    .join("project.yml")
                    .display()
                    .to_string(),
            ],
        ),
        (
            "xcodebuild",
            &xcodebuild_program,
            xcodebuild(root, &host_library(root)),
        ),
    ];
    for (label, program, args) in steps {
        let invocation = tool_invocation(label, program, args);
        writeln!(out, "ios: {}", invocation.command_line()).ok();
        out.flush().ok();
        run_tool(
            &invocation,
            program,
            root,
            runner,
            &logs.join(format!("{label}.log")),
        )
        .map_err(|error| {
            if label == "ios-host" {
                format!("{error}; the Simulator build needs `rustup target add {SIMULATOR_TARGET}`")
            } else {
                error
            }
        })?;
    }
    let app = app_path(root);
    if !app.is_dir() {
        return Err(format!("xcodebuild did not produce {}", app.display()));
    }
    writeln!(out, "ios: built {}", app.display()).ok();
    Ok(app)
}

/// Builds, installs and runs the host on the Simulator, and counts its results.
pub fn run(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    selection: &Selection,
    out: &mut dyn Write,
) -> Tally {
    match run_on_simulator(root, environment, runner, selection, out) {
        Ok(tally) => tally,
        Err(error) => Tally::failure(format!("lane f: {error}")),
    }
}

fn run_on_simulator(
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    selection: &Selection,
    out: &mut dyn Write,
) -> Result<Tally, String> {
    let app = build(root, environment, runner, out)?;
    let (_, _, _, xcrun) = tools(environment)?;
    let logs = run::artifacts_dir(root, "f");
    let xcrun_run = |label: &str, args: Vec<String>, runner: &mut dyn Runner| {
        let invocation = tool_invocation(label, &xcrun, args);
        run_tool(
            &invocation,
            &xcrun,
            root,
            runner,
            &logs.join(format!("{label}.log")),
        )
    };
    let words =
        |words: &[&str]| -> Vec<String> { words.iter().map(|word| word.to_string()).collect() };

    let devices = xcrun_run(
        "simctl-devices",
        words(&["simctl", "list", "devices", "available"]),
        runner,
    )?;
    let udid = match simulator_device(&devices.output, SIMULATOR_NAME) {
        Some((udid, _)) => udid,
        None => {
            let runtimes = xcrun_run(
                "simctl-runtimes",
                words(&["simctl", "list", "runtimes", "available"]),
                runner,
            )?;
            let runtime = newest_ios_runtime(&runtimes.output).ok_or(
                "Xcode has no iOS Simulator runtime: install one with `xcodebuild -downloadPlatform iOS`",
            )?;
            xcrun_run(
                "simctl-create",
                words(&["simctl", "create", SIMULATOR_NAME, DEVICE_TYPE, &runtime]),
                runner,
            )?
            .output
            .trim()
            .to_string()
        }
    };
    writeln!(out, "lane f: Simulator {SIMULATOR_NAME} ({udid})").ok();
    // Boots the Simulator when it is not booted, and waits until it is.
    xcrun_run(
        "simctl-boot",
        words(&["simctl", "bootstatus", &udid, "-b"]),
        runner,
    )?;
    xcrun_run(
        "simctl-install",
        words(&["simctl", "install", &udid, &app.display().to_string()]),
        runner,
    )?;
    writeln!(out, "lane f: following the host's console").ok();
    out.flush().ok();

    let console = tool_invocation("console", &xcrun, launch(&udid, selection));
    let mut collected = String::new();
    let mut failures = Vec::new();
    let mut handle = |line: &str| -> bool {
        collected.push_str(line);
        collected.push('\n');
        if line.contains(gpux_conformance::report::MARKER) || line.contains(HOST_MARKER) {
            writeln!(out, "{line}").ok();
            out.flush().ok();
        }
        if let Some(failure) = fatal(line) {
            writeln!(out, "{line}").ok();
            failures.push(failure);
            return false;
        }
        match request(line) {
            Some(Request::Done) => return false,
            Some(request) => {
                for args in simctl_commands(&request, &udid) {
                    if let Err(error) = xcrun_run("simctl-request", args, runner) {
                        failures.push(format!("could not carry out {request:?}: {error}"));
                    }
                }
            }
            None => {}
        }
        true
    };
    let finished = run::follow_lines(
        &console,
        &xcrun,
        root,
        &logs.join("console.log"),
        RUN_TIMEOUT,
        &mut handle,
    )
    .map_err(|error| format!("could not follow the host's console: {error:#}"))?;

    copy_frames(&xcrun, &udid, &logs, runner, root, out);
    let mut tally = tally(&collected, selection);
    for failure in failures {
        tally.add(Tally::failure(failure));
    }
    if !finished {
        tally.add(Tally::failure(format!(
            "the host did not finish within {} minutes; see {}",
            RUN_TIMEOUT.as_secs() / 60,
            logs.join("console.log").display()
        )));
    }
    Ok(tally)
}

/// Copies the frames failing pixel steps saved in the app's `Documents/frames/` into
/// `artifacts-temp/testkit/f/frames/`.
fn copy_frames(
    xcrun: &Path,
    udid: &str,
    logs: &Path,
    runner: &mut dyn Runner,
    root: &Path,
    out: &mut dyn Write,
) {
    let invocation = tool_invocation(
        "simctl-container",
        xcrun,
        ["simctl", "get_app_container", udid, BUNDLE_ID, "data"]
            .iter()
            .map(|arg| arg.to_string())
            .collect(),
    );
    let Ok(container) = run_tool(
        &invocation,
        xcrun,
        root,
        runner,
        &logs.join("simctl-container.log"),
    ) else {
        return;
    };
    let frames = PathBuf::from(container.output.trim())
        .join("Documents")
        .join("frames");
    let Ok(entries) = std::fs::read_dir(&frames) else {
        return;
    };
    let destination = logs.join("frames");
    std::fs::create_dir_all(&destination).ok();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "png")
            && let Some(name) = path.file_name()
            && std::fs::copy(&path, destination.join(name)).is_ok()
        {
            writeln!(out, "lane f: saved {}", destination.join(name).display()).ok();
        }
    }
}
