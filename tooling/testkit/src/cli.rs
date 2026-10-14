//! Command-line parsing.

use crate::lanes::LaneId;
use std::fmt;

/// The usage text printed by `--help` and after usage errors.
pub const USAGE: &str = "\
usage: gpux-testkit lane <lane>... [--list] [--scene <id>] [--grep <pattern>] [--headed] [--full]
       gpux-testkit web build [--verify]
       gpux-testkit web serve [--port <port>]
       gpux-testkit web e2e [--profile <profile>] [--grep <pattern>] [--headed]
       gpux-testkit android build [--abi x86_64|arm64-v8a]...
       gpux-testkit android run [--serial <serial>] [--scene <id>] [--grep <pattern>]
       gpux-testkit ios build --simulator
       gpux-testkit ios run --simulator [--scene <id>] [--grep <pattern>]
       gpux-testkit perf fast-mode [--name <run>] [--compare <run-dir>]

lanes:
  a  behaviour on the test platform (no GPU or display)
  b  a real browser through Playwright (GPUX_WEB_PROFILE picks the profile)
  c  offscreen pixels through the headless renderer
  d  real desktop windows
  e  Android on a device or emulator through adb (ANDROID_SERIAL picks the device)
  f  iOS on a Simulator (macOS with Xcode and XcodeGen)
  g  the local real GPU: lane b's every spec, lanes c and d with probes only, and the
     benchmarks, on the hardware adapter; refuses to run under CI

options:
  --list            print the exact test selection without running it
  --scene <id>      select one conformance scene
  --grep <pattern>  select the tests whose names contain the pattern
  --headed          show the browser of lanes b and g
  --full            lane c: run every frame of the scroll-layer suites (the default samples them;
                    GPUX_LANE_C_FULL=1 does the same); lane g always runs them in full

web (lane b's browser host):
  build    build the threaded `web_host` WASM app into target/gpux-web/dist;
           --verify checks shared memory, the thread entry point and constructors
  serve    serve the build with cross-origin isolation headers (default port 8000)
  e2e      build, serve and run the Playwright specs in tools-node; profiles are
           local-gpu (default), ci-linux-swiftshader and windows-software

android (lane e's conformance host):
  build    build the android_host APK (both ABIs unless --abi narrows them)
  run      build for the device's ABI, install, run and follow the host on the first
           attached device or emulator (or --serial)

ios (lane f's conformance host; devices are signed and run by hand, see docs/testing.md):
  build    build the ios_host app for the Simulator, unsigned
  run      build, boot the pinned Simulator, install, run and follow the host

perf (timing on the local machine, like lane g):
  fast-mode  run the `fast_mode` bench of every scenario in every fast mode into
             artifacts-temp/testkit/perf/<run>/ (--name, default fast-mode-<seconds>) and
             write its fast-mode.md; --compare <run-dir> diffs it against an earlier run";

/// A parsed command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Prints the usage.
    Help,
    /// Runs or lists lanes.
    Lane(LaneArgs),
    /// Builds, serves or tests lane B's browser host.
    Web(WebCommand),
    /// Builds or runs lane E's Android host.
    Android(AndroidCommand),
    /// Builds or runs lane F's iOS host.
    Ios(IosCommand),
    /// Runs a performance comparison.
    Perf(PerfCommand),
}

/// The `perf` subcommands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PerfCommand {
    /// Runs the `fast_mode` bench and writes its report.
    FastMode {
        /// The run directory's name under `artifacts-temp/testkit/perf/`.
        name: String,
        /// An earlier run directory to compare with.
        compare: Option<std::path::PathBuf>,
    },
}

/// The `ios` subcommands, all on the Simulator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IosCommand {
    /// Builds the app.
    Build,
    /// Runs the host.
    Run {
        /// The checks and scenes to run.
        selection: Selection,
    },
}

/// The `android` subcommands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AndroidCommand {
    /// Builds the APK for these ABIs.
    Build {
        /// The ABIs, every one when none is named.
        abis: Vec<crate::android::Abi>,
    },
    /// Runs the host on a device.
    Run {
        /// The device's serial; the first attached device otherwise.
        serial: Option<String>,
        /// The checks and scenes to run.
        selection: Selection,
    },
}

/// The `web` subcommands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebCommand {
    /// Builds the threaded browser host; `verify` checks the binary too.
    Build {
        /// Whether to verify the threaded binary.
        verify: bool,
    },
    /// Serves the build.
    Serve {
        /// The port; `0` picks a free one.
        port: u16,
    },
    /// Builds, serves and runs the Playwright specs.
    E2e(E2eArgs),
}

/// The arguments of `web e2e`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct E2eArgs {
    /// The Playwright project.
    pub profile: String,
    /// A Playwright `--grep` pattern, for example `@canary`.
    pub grep: Option<String>,
    /// Runs the browser headed.
    pub headed: bool,
}

/// The port `web serve` uses by default.
pub const DEFAULT_PORT: u16 = 8000;

/// The arguments of `lane`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneArgs {
    /// The lanes, in the order given, without repeats.
    pub lanes: Vec<LaneId>,
    /// Prints the selection instead of running it.
    pub list: bool,
    /// The selection within each lane.
    pub selection: Selection,
    /// Runs the browser specs of lanes B and G headed.
    pub headed: bool,
    /// Runs lane C's scroll-layer suites frame for frame instead of sampled.
    pub full: bool,
}

/// Which tests of a lane to run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// One conformance scene id.
    pub scene: Option<String>,
    /// A substring of test or scene names.
    pub grep: Option<String>,
}

impl Selection {
    /// Whether nothing narrows the lane.
    pub fn is_everything(&self) -> bool {
        self.scene.is_none() && self.grep.is_none()
    }
}

/// A command line that cannot run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageError(pub String);

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

/// Parses the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None | Some("help" | "--help" | "-h") => Ok(Command::Help),
        Some("lane") => parse_lane(args).map(Command::Lane),
        Some("web") => parse_web(args).map(Command::Web),
        Some("android") => parse_android(args).map(Command::Android),
        Some("ios") => parse_ios(args).map(Command::Ios),
        Some("perf") => parse_perf(args).map(Command::Perf),
        Some(other) => Err(UsageError(format!(
            "unknown command `{other}`; the commands are `lane`, `web`, `android`, `ios` and \
             `perf`"
        ))),
    }
}

fn value(arg: &str, args: &mut impl Iterator<Item = String>) -> Result<String, UsageError> {
    args.next()
        .filter(|value| !value.starts_with("--") && !value.is_empty())
        .ok_or_else(|| UsageError(format!("`{arg}` needs a value")))
}

fn parse_android(mut args: impl Iterator<Item = String>) -> Result<AndroidCommand, UsageError> {
    let subcommand = args
        .next()
        .ok_or_else(|| UsageError("name an android command: `build` or `run`".into()))?;
    let mut abis = Vec::new();
    let mut serial = None;
    let mut selection = Selection::default();
    while let Some(arg) = args.next() {
        match (subcommand.as_str(), arg.as_str()) {
            ("build", "--abi") => {
                let name = value(&arg, &mut args)?;
                let abi = crate::android::abi(&name).ok_or_else(|| {
                    UsageError(format!(
                        "unknown ABI `{name}`; the ABIs are x86_64 and arm64-v8a"
                    ))
                })?;
                if !abis.contains(&abi) {
                    abis.push(abi);
                }
            }
            ("run", "--serial" | "--scene" | "--grep") => {
                let slot = match arg.as_str() {
                    "--serial" => &mut serial,
                    "--scene" => &mut selection.scene,
                    _ => &mut selection.grep,
                };
                if slot.replace(value(&arg, &mut args)?).is_some() {
                    return Err(UsageError(format!("`{arg}` is given twice")));
                }
            }
            _ => {
                return Err(UsageError(format!(
                    "unknown option `{arg}` for `android {subcommand}`"
                )));
            }
        }
    }
    match subcommand.as_str() {
        "build" => Ok(AndroidCommand::Build {
            abis: if abis.is_empty() {
                crate::android::ABIS.to_vec()
            } else {
                abis
            },
        }),
        "run" => Ok(AndroidCommand::Run { serial, selection }),
        other => Err(UsageError(format!(
            "unknown android command `{other}`; the android commands are `build` and `run`"
        ))),
    }
}

fn parse_perf(mut args: impl Iterator<Item = String>) -> Result<PerfCommand, UsageError> {
    let subcommand = args
        .next()
        .ok_or_else(|| UsageError("name a perf command: `fast-mode`".into()))?;
    if subcommand != "fast-mode" {
        return Err(UsageError(format!(
            "unknown perf command `{subcommand}`; the perf command is `fast-mode`"
        )));
    }
    let mut name = None;
    let mut compare = None;
    while let Some(arg) = args.next() {
        let slot = match arg.as_str() {
            "--name" => &mut name,
            "--compare" => &mut compare,
            _ => {
                return Err(UsageError(format!(
                    "unknown option `{arg}` for `perf fast-mode`"
                )));
            }
        };
        if slot.replace(value(&arg, &mut args)?).is_some() {
            return Err(UsageError(format!("`{arg}` is given twice")));
        }
    }
    if let Some(name) = &name
        && (name.contains(['/', '\\']) || name == "." || name == "..")
    {
        return Err(UsageError(format!(
            "`--name {name}` must be a plain directory name"
        )));
    }
    Ok(PerfCommand::FastMode {
        name: name.unwrap_or_else(|| {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            format!("fast-mode-{seconds}")
        }),
        compare: compare.map(std::path::PathBuf::from),
    })
}

fn parse_ios(mut args: impl Iterator<Item = String>) -> Result<IosCommand, UsageError> {
    let subcommand = args
        .next()
        .ok_or_else(|| UsageError("name an ios command: `build` or `run`".into()))?;
    if !matches!(subcommand.as_str(), "build" | "run") {
        return Err(UsageError(format!(
            "unknown ios command `{subcommand}`; the ios commands are `build` and `run`"
        )));
    }
    let mut simulator = false;
    let mut selection = Selection::default();
    while let Some(arg) = args.next() {
        match (subcommand.as_str(), arg.as_str()) {
            (_, "--simulator") => simulator = true,
            ("run", "--scene" | "--grep") => {
                let slot = if arg == "--scene" {
                    &mut selection.scene
                } else {
                    &mut selection.grep
                };
                if slot.replace(value(&arg, &mut args)?).is_some() {
                    return Err(UsageError(format!("`{arg}` is given twice")));
                }
            }
            _ => {
                return Err(UsageError(format!(
                    "unknown option `{arg}` for `ios {subcommand}`"
                )));
            }
        }
    }
    if !simulator {
        return Err(UsageError(format!(
            "`ios {subcommand}` needs `--simulator`: devices need your signing team, so they \
             are built and run by hand (docs/testing.md)"
        )));
    }
    Ok(match subcommand.as_str() {
        "build" => IosCommand::Build,
        _ => IosCommand::Run { selection },
    })
}

fn parse_web(mut args: impl Iterator<Item = String>) -> Result<WebCommand, UsageError> {
    let subcommand = args
        .next()
        .ok_or_else(|| UsageError("name a web command: `build`, `serve` or `e2e`".into()))?;
    let mut verify = false;
    let mut port = None;
    let mut profile = None;
    let mut grep = None;
    let mut headed = false;
    while let Some(arg) = args.next() {
        let accepted = match (subcommand.as_str(), arg.as_str()) {
            ("build", "--verify") => {
                verify = true;
                true
            }
            ("serve", "--port") => {
                let text = value(&arg, &mut args)?;
                let parsed = text
                    .parse::<u16>()
                    .map_err(|_| UsageError(format!("`--port {text}` is not a port number")))?;
                port.replace(parsed).is_none()
            }
            ("e2e", "--profile") => profile.replace(value(&arg, &mut args)?).is_none(),
            ("e2e", "--grep") => grep.replace(value(&arg, &mut args)?).is_none(),
            ("e2e", "--headed") => {
                headed = true;
                true
            }
            _ => {
                return Err(UsageError(format!(
                    "unknown option `{arg}` for `web {subcommand}`"
                )));
            }
        };
        if !accepted {
            return Err(UsageError(format!("`{arg}` is given twice")));
        }
    }
    match subcommand.as_str() {
        "build" => Ok(WebCommand::Build { verify }),
        "serve" => Ok(WebCommand::Serve {
            port: port.unwrap_or(DEFAULT_PORT),
        }),
        "e2e" => Ok(WebCommand::E2e(E2eArgs {
            profile: profile.unwrap_or_else(|| "local-gpu".into()),
            grep,
            headed,
        })),
        other => Err(UsageError(format!(
            "unknown web command `{other}`; the web commands are `build`, `serve` and `e2e`"
        ))),
    }
}

fn parse_lane(mut args: impl Iterator<Item = String>) -> Result<LaneArgs, UsageError> {
    let mut lanes = Vec::new();
    let mut list = false;
    let mut headed = false;
    let mut full = false;
    let mut selection = Selection::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--headed" => headed = true,
            "--full" => full = true,
            "--scene" | "--grep" => {
                let value = args
                    .next()
                    .filter(|value| !value.starts_with("--") && !value.is_empty())
                    .ok_or_else(|| UsageError(format!("`{arg}` needs a value")))?;
                let slot = if arg == "--scene" {
                    &mut selection.scene
                } else {
                    &mut selection.grep
                };
                if slot.replace(value).is_some() {
                    return Err(UsageError(format!("`{arg}` is given twice")));
                }
            }
            flag if flag.starts_with('-') => {
                return Err(UsageError(format!("unknown option `{flag}`")));
            }
            lane => {
                let lane = LaneId::parse(lane)?;
                if !lanes.contains(&lane) {
                    lanes.push(lane);
                }
            }
        }
    }
    if lanes.is_empty() {
        return Err(UsageError(
            "name at least one lane, for example `gpux-testkit lane a c`".into(),
        ));
    }
    Ok(LaneArgs {
        lanes,
        list,
        selection,
        headed,
        full,
    })
}
