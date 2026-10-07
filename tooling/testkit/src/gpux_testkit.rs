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
pub mod serve;
pub mod summary;
pub mod wasm_verify;
pub mod web;

use cli::{Command, LaneArgs, WebCommand};
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
        Command::Web(command) => web_command(&command, environment, runner, out),
    }
}

fn web_command(
    command: &WebCommand,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> i32 {
    let tally = match environment.workspace_root() {
        None => Tally::failure(
            "gpux-testkit runs inside a gpux checkout: run it from the repository (the folder              with `crates/gpux_conformance`) or a subfolder",
        ),
        Some(root) => match command {
            WebCommand::Build { verify } => {
                match web::build(&root, environment, runner, *verify, out) {
                    Ok(_) => Tally {
                        passed: 1,
                        ..Tally::default()
                    },
                    Err(error) => Tally::failure(format!("web build: {error}")),
                }
            }
            WebCommand::Serve { port } => serve_build(&root, *port, out),
            WebCommand::E2e(args) => web::e2e(
                &root,
                environment,
                runner,
                &args.profile,
                args.grep.as_deref(),
                args.headed,
                out,
            ),
        },
    };
    finish(&tally, 0, out)
}

/// Serves the existing build until the process is stopped.
fn serve_build(root: &Path, port: u16, out: &mut dyn Write) -> Tally {
    let dist = web::WebPaths::new(root).dist;
    if !dist.join("index.html").is_file() {
        return Tally::failure(format!(
            "web serve: {} holds no build; run `gpux-testkit web build` first",
            dist.display()
        ));
    }
    match serve::start(dist.clone(), port) {
        Ok((address, server)) => {
            writeln!(out, "web: serving {} at http://{address}/", dist.display()).ok();
            writeln!(
                out,
                "web: open http://{address}/?scene=<id>; press Ctrl+C to stop"
            )
            .ok();
            out.flush().ok();
            server.join().ok();
            Tally::failure("web serve: the server stopped")
        }
        Err(error) => Tally::failure(format!(
            "web serve: could not listen on port {port}: {error}"
        )),
    }
}

/// Prints the failures and the summary line, and returns the exit code.
fn finish(tally: &Tally, listed: usize, out: &mut dyn Write) -> i32 {
    let summary = Summary::of(tally);
    if !tally.failures.is_empty() {
        writeln!(out, "failures:").ok();
        for failure in &tally.failures {
            writeln!(out, "  {failure}").ok();
        }
    }
    writeln!(out, "{summary}").ok();
    summary.exit_code(listed)
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
        if args.list && plan.listings.is_empty() {
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
        if args.list {
            match list_exact(&plan, root, environment, runner, out) {
                Ok(count) => listed += count,
                Err(failure) => tally.add(failure),
            }
            continue;
        }
        tally.add(run_plan(&plan, root, environment, runner, out));
    }
    finish(&tally, listed, out)
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

/// Prints the exact tests libtest would run for each of the lane's targets, without running
/// them, and returns how many there are.
fn list_exact(
    plan: &LanePlan,
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Result<usize, Tally> {
    let lane = plan.lane;
    let artifacts = run::artifacts_dir(root, lane.as_str());
    let mut count = 0;
    for listing in &plan.listings {
        let invocation = &listing.invocation;
        let program = environment
            .find_program(&invocation.program)
            .ok_or_else(|| {
                Tally::failure(format!(
                    "lane {lane}: `{}` is not on PATH",
                    invocation.program
                ))
            })?;
        writeln!(out, "lane {lane}: {}", invocation.command_line()).ok();
        out.flush().ok();
        let log = artifacts.join(format!("{}.log", invocation.label));
        let result = runner
            .run(invocation, &program, root, &log)
            .map_err(|error| Tally::failure(format!("lane {lane}: {error:#}")))?;
        if !result.success {
            return Err(Tally::failure(format!(
                "lane {lane}: listing `{}` failed; see {}",
                invocation.command_line(),
                log.display()
            )));
        }
        for name in results::libtest_list(&result.output) {
            writeln!(out, "  {}::{name}", listing.target).ok();
            count += 1;
        }
    }
    if count == 0 {
        writeln!(out, "lane {lane}: nothing matches the selection").ok();
    }
    Ok(count)
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
            OutputKind::Libtest | OutputKind::Listing => results::tally_libtest(&result.output),
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
