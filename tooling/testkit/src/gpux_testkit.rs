//! `gpux-testkit`, the single entry point for gpux's test lanes.
//!
//! `gpux-testkit lane <lane>... [--list] [--scene <id>] [--grep <pattern>]` selects tests by
//! lane, runs them through the lane's host (cargo test targets for lanes A and C, the desktop
//! conformance host for lane D), parses their results, and ends with one summary line:
//! `gpux-testkit: <passed> passed, <failed> failed, <skipped> skipped`. It exits nonzero on any
//! failure and when nothing was selected. Raw output goes to `artifacts-temp/testkit/<lane>/`.

pub mod cli;
pub mod lanes;
pub mod prereq;
pub mod programs;
pub mod results;
pub mod run;
pub mod summary;

use cli::{Command, LaneArgs};
use lanes::{LaneId, LanePlan, OutputKind};
use results::Tally;
use run::Runner;
use std::io::Write;
use std::path::{Path, PathBuf};
use summary::{EXIT_USAGE, Summary};

/// The operating system the lanes run on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    /// Windows.
    Windows,
    /// Linux.
    Linux,
    /// macOS.
    MacOs,
    /// Anything else.
    Other,
}

/// What the testkit needs from its host.
pub trait Environment {
    /// An environment variable.
    fn var(&self, name: &str) -> Option<String>;
    /// The resolved path of a program on `PATH`.
    fn find_program(&self, name: &str) -> Option<PathBuf>;
    /// The operating system.
    fn os(&self) -> Os;
    /// The gpux workspace root, where lanes run.
    fn workspace_root(&self) -> Option<PathBuf>;
}

/// The real host.
pub struct SystemEnvironment;

impl Environment for SystemEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        let pathext = std::env::var("PATHEXT").ok();
        programs::resolve(name, &path, pathext.as_deref(), cfg!(windows))
    }

    fn os(&self) -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Other
        }
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        let start = std::env::current_dir().ok()?;
        find_workspace_root(&start)
    }
}

/// The nearest ancestor of `start` that holds the gpux workspace (its manifest and the
/// conformance crate).
pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| {
            dir.join("Cargo.toml").is_file()
                && dir
                    .join("crates")
                    .join("gpux_conformance")
                    .join("Cargo.toml")
                    .is_file()
        })
        .map(Path::to_path_buf)
}

/// Runs a command line and returns the process exit code. Every outcome, including usage
/// errors, ends with the summary line on `out`.
pub fn execute(
    args: impl IntoIterator<Item = String>,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> i32 {
    let command = match cli::parse(args) {
        Ok(command) => command,
        Err(error) => {
            writeln!(out, "error: {error}\n\n{}", cli::USAGE).ok();
            writeln!(out, "{}", Summary::default()).ok();
            return EXIT_USAGE;
        }
    };
    match command {
        Command::Help => {
            writeln!(out, "{}", cli::USAGE).ok();
            writeln!(out, "{}", Summary::default()).ok();
            0
        }
        Command::Lane(args) => lane(&args, environment, runner, out),
    }
}

fn lane(
    args: &LaneArgs,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> i32 {
    let mut tally = Tally::default();
    let mut listed = 0;
    let root = environment.workspace_root();
    for &lane in &args.lanes {
        let plan = match lanes::plan(lane, &args.selection) {
            Ok(plan) => plan,
            Err(error) => {
                writeln!(out, "lane {lane}: {error}").ok();
                tally.add(Tally::failure(format!("lane {lane}: {error}")));
                continue;
            }
        };
        if args.list {
            listed += list(&plan, out);
            continue;
        }
        let Some(root) = root.as_deref() else {
            let message = "gpux-testkit runs inside a gpux checkout: run it from the repository \
                           (the folder with `crates/gpux_conformance`) or a subfolder";
            writeln!(out, "lane {lane}: {message}").ok();
            tally.add(Tally::failure(format!("lane {lane}: {message}")));
            continue;
        };
        let problems = prereq::check(lane, environment);
        if !problems.is_empty() {
            for problem in &problems {
                writeln!(out, "lane {lane}: {problem}").ok();
                tally.add(Tally::failure(format!("lane {lane}: {problem}")));
            }
            continue;
        }
        tally.add(run_plan(&plan, root, environment, runner, out));
    }
    let summary = Summary::of(&tally);
    if !tally.failures.is_empty() {
        writeln!(out, "failures:").ok();
        for failure in &tally.failures {
            writeln!(out, "  {failure}").ok();
        }
    }
    writeln!(out, "{summary}").ok();
    summary.exit_code(listed)
}

/// Prints a lane's selection and returns how many tests it selects.
fn list(plan: &LanePlan, out: &mut dyn Write) -> usize {
    for invocation in &plan.invocations {
        writeln!(out, "lane {}: {}", plan.lane, invocation.command_line()).ok();
    }
    if plan.tests.is_empty() {
        writeln!(out, "lane {}: nothing matches the selection", plan.lane).ok();
    }
    for test in &plan.tests {
        writeln!(out, "  {test}").ok();
    }
    plan.tests.len()
}

fn run_plan(
    plan: &LanePlan,
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Tally {
    let mut tally = Tally::default();
    let lane = plan.lane;
    if plan.invocations.is_empty() {
        writeln!(out, "lane {lane}: nothing matches the selection").ok();
        return tally;
    }
    let artifacts = run::artifacts_dir(root, lane.as_str());
    for invocation in &plan.invocations {
        let Some(program) = environment.find_program(&invocation.program) else {
            tally.add(Tally::failure(format!(
                "lane {lane}: `{}` is not on PATH",
                invocation.program
            )));
            continue;
        };
        writeln!(out, "lane {lane}: {}", invocation.command_line()).ok();
        out.flush().ok();
        let log = artifacts.join(format!("{}.log", invocation.label));
        let result = match runner.run(invocation, &program, root, &log) {
            Ok(result) => result,
            Err(error) => {
                tally.add(Tally::failure(format!("lane {lane}: {error:#}")));
                continue;
            }
        };
        let mut counted = match invocation.output {
            OutputKind::Libtest => results::tally_libtest(&result.output),
            OutputKind::ResultLines => results::tally_result_lines(&result.output),
        };
        let reported_failure = counted.failed > 0;
        let mut missing = 0;
        if invocation.output == OutputKind::ResultLines {
            // A host that stops early must not pass for one that ran less.
            let reported: Vec<String> = results::result_lines(&result.output)
                .into_iter()
                .map(|line| line.scene)
                .collect();
            for test in &plan.tests {
                let id = test.rsplit("::").next().unwrap_or(test);
                if !reported.iter().any(|scene| scene == id) {
                    counted.add(Tally::failure(format!("{test}: no result was reported")));
                    missing += 1;
                }
            }
        }
        if !result.success && !reported_failure {
            // The process failed without reporting a failed test: a build error or a crash.
            // Its unreported tests already count as failures; otherwise the run itself does.
            let message = format!(
                "lane {lane}: `{}` exited with {} before reporting its results; see {}",
                invocation.command_line(),
                result
                    .code
                    .map_or_else(|| "a signal".to_string(), |code| format!("code {code}")),
                log.display()
            );
            if missing > 0 {
                counted.failures.push(message);
            } else {
                counted.add(Tally::failure(message));
            }
        }
        tally.add(counted);
    }
    tally
}

/// The lanes `gpux-testkit lane` accepts, for documentation and tests.
pub const LANES: [LaneId; 7] = [
    LaneId::A,
    LaneId::B,
    LaneId::C,
    LaneId::D,
    LaneId::E,
    LaneId::F,
    LaneId::G,
];
