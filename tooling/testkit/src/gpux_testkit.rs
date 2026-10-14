//! `gpux-testkit`, the single entry point for gpux's test lanes.
//!
//! `gpux-testkit lane <lane>... [--list] [--scene <id>] [--grep <pattern>] [--headed]` selects
//! tests by lane, runs them through the lane's host (cargo test targets for lanes A and C, the
//! desktop conformance host for lane D, Playwright for lane B, an Android device for lane E, and
//! all of these on the local GPU for lane G), parses their results, and ends with one summary line:
//! `gpux-testkit: <passed> passed, <failed> failed, <skipped> skipped`. It exits nonzero on any
//! failure and when nothing was selected. Raw output goes to `artifacts-temp/testkit/<lane>/`.

pub mod android;
pub mod cli;
pub mod fast_mode;
pub mod ios;
pub mod lanes;
pub mod local_gpu;
pub mod prereq;
pub mod programs;
pub mod results;
pub mod run;
pub mod serve;
pub mod summary;
pub mod timings;
pub mod wasm_verify;
pub mod web;

use cli::{AndroidCommand, Command, IosCommand, LaneArgs, PerfCommand, WebCommand};
use lanes::{Invocation, LaneId, LanePlan, OutputKind};
use results::Tally;
use run::{RunOutput, Runner};
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

/// Why a command needs the workspace and found none.
const OUTSIDE_CHECKOUT: &str = "gpux-testkit runs inside a gpux checkout: run it from the \
                                repository (the folder with `crates/gpux_conformance`) or a \
                                subfolder";

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
        Command::Android(command) => android_command(&command, environment, runner, out),
        Command::Ios(command) => ios_command(&command, environment, runner, out),
        Command::Perf(command) => perf_command(&command, environment, runner, out),
    }
}

fn perf_command(
    command: &PerfCommand,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> i32 {
    let tally = match environment.workspace_root() {
        None => Tally::failure(OUTSIDE_CHECKOUT),
        Some(root) => match command {
            PerfCommand::FastMode { name, compare } => {
                fast_mode::run(&root, name, compare.as_deref(), environment, runner, out)
            }
        },
    };
    finish(&tally, 0, out)
}

fn ios_command(
    command: &IosCommand,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> i32 {
    let tally = match environment.workspace_root() {
        None => Tally::failure(OUTSIDE_CHECKOUT),
        Some(root) => match command {
            IosCommand::Build => match ios::build(&root, environment, runner, out) {
                Ok(_) => Tally {
                    passed: 1,
                    ..Tally::default()
                },
                Err(error) => Tally::failure(format!("ios build: {error}")),
            },
            IosCommand::Run { selection } => ios::run(&root, environment, runner, selection, out),
        },
    };
    finish(&tally, 0, out)
}

fn android_command(
    command: &AndroidCommand,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> i32 {
    let tally = match environment.workspace_root() {
        None => Tally::failure(OUTSIDE_CHECKOUT),
        Some(root) => match command {
            AndroidCommand::Build { abis } => {
                match android::build(&root, environment, runner, abis, out) {
                    Ok(_) => Tally {
                        passed: 1,
                        ..Tally::default()
                    },
                    Err(error) => Tally::failure(format!("android build: {error}")),
                }
            }
            AndroidCommand::Run { serial, selection } => android::run(
                &root,
                environment,
                runner,
                serial.as_deref(),
                selection,
                out,
            ),
        },
    };
    finish(&tally, 0, out)
}

fn web_command(
    command: &WebCommand,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> i32 {
    let tally = match environment.workspace_root() {
        None => Tally::failure(OUTSIDE_CHECKOUT),
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
    let mut mode = None;
    let root = environment.workspace_root();
    for &lane in &args.lanes {
        if args.jobs > 1 && lane != LaneId::C {
            writeln!(
                out,
                "lane {lane}: --jobs {} applies to the full lane c only; lane {lane} runs as \
                 usual",
                args.jobs
            )
            .ok();
        }
        if lane == LaneId::G {
            // The local-GPU lane runs lanes B, C and D and the benchmarks on the hardware GPU.
            let Some(root) = root.as_deref() else {
                writeln!(out, "lane g: {OUTSIDE_CHECKOUT}").ok();
                tally.add(Tally::failure(format!("lane g: {OUTSIDE_CHECKOUT}")));
                continue;
            };
            if args.list {
                let (count, failures) = local_gpu::list(root, args, environment, runner, out);
                listed += count;
                tally.add(failures);
            } else {
                tally.add(local_gpu::run(root, args, environment, runner, out));
            }
            continue;
        }
        if lane == LaneId::F {
            // The iOS lane runs on a Simulator through simctl instead of cargo targets.
            let Some(root) = root.as_deref() else {
                writeln!(out, "lane f: {OUTSIDE_CHECKOUT}").ok();
                tally.add(Tally::failure(format!("lane f: {OUTSIDE_CHECKOUT}")));
                continue;
            };
            if args.list {
                for id in ios::expected_results(&args.selection) {
                    writeln!(out, "  {id}").ok();
                    listed += 1;
                }
            } else {
                tally.add(ios::run(root, environment, runner, &args.selection, out));
            }
            continue;
        }
        if lane == LaneId::E {
            // The Android lane runs on a device through adb instead of cargo targets.
            let Some(root) = root.as_deref() else {
                writeln!(out, "lane e: {OUTSIDE_CHECKOUT}").ok();
                tally.add(Tally::failure(format!("lane e: {OUTSIDE_CHECKOUT}")));
                continue;
            };
            if args.list {
                for id in android::expected_results(&args.selection) {
                    writeln!(out, "  {id}").ok();
                    listed += 1;
                }
            } else {
                let serial = environment
                    .var("ANDROID_SERIAL")
                    .filter(|serial| !serial.is_empty());
                tally.add(android::run(
                    root,
                    environment,
                    runner,
                    serial.as_deref(),
                    &args.selection,
                    out,
                ));
            }
            continue;
        }
        if lane == LaneId::B {
            // The browser lane runs Playwright specs instead of cargo targets.
            let Some(root) = root.as_deref() else {
                writeln!(out, "lane b: {OUTSIDE_CHECKOUT}").ok();
                tally.add(Tally::failure(format!("lane b: {OUTSIDE_CHECKOUT}")));
                continue;
            };
            let profile = web::lane_profile(environment);
            let grep = web::lane_grep(&args.selection);
            if args.list {
                match web::list(root, environment, runner, &profile, Some(&grep), out) {
                    Ok(count) => listed += count,
                    Err(failure) => tally.add(failure),
                }
            } else {
                tally.add(web::e2e(
                    root,
                    environment,
                    runner,
                    &profile,
                    Some(&grep),
                    args.headed,
                    out,
                ));
            }
            continue;
        }
        let mut plan = match lanes::plan_with_jobs(lane, &args.selection, args.jobs) {
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
            writeln!(out, "lane {lane}: {OUTSIDE_CHECKOUT}").ok();
            tally.add(Tally::failure(format!("lane {lane}: {OUTSIDE_CHECKOUT}")));
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
        if lane == LaneId::C && args.jobs > 1 && plan.prebuild.is_none() {
            writeln!(
                out,
                "lane c: --jobs {} applies to the full lane c only; this selection runs as one \
                 process",
                args.jobs
            )
            .ok();
        }
        if lane == LaneId::C && args.selection.is_everything() {
            // The scroll-layer suites run with the full lane; say which mode they are in.
            let full = args.full
                || lanes::full_requested(environment.var(lanes::FULL_VARIABLE).as_deref());
            if full {
                lanes::set_full(&mut plan);
            }
            let line = lanes::mode_line(full);
            writeln!(out, "{line}").ok();
            mode = Some(line);
        }
        tally.add(run_plan(&plan, root, environment, runner, out));
    }
    if let Some(line) = mode {
        // The mode is repeated beside the summary: a log read from its end still says it.
        writeln!(out, "{line}").ok();
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
    let mut timelines = Vec::new();
    if plan.prebuild.is_some() {
        run_concurrently(
            plan,
            root,
            environment,
            runner,
            out,
            &mut tally,
            &mut timelines,
        );
        write_timings(lane, &artifacts, &timelines, out);
        return tally;
    }
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
        let timeline = runner.take_timeline();
        if invocation.output == OutputKind::Libtest {
            timelines.push(timings::ProcessTimeline {
                label: invocation.label.clone(),
                lines: timeline,
            });
        }
        tally.add(account(plan, invocation, &result, &log));
    }
    write_timings(lane, &artifacts, &timelines, out);
    tally
}

/// Runs a plan's workers side by side after its one build, and prints each worker's output as one
/// block, in worker order.
fn run_concurrently(
    plan: &LanePlan,
    root: &Path,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
    tally: &mut Tally,
    timelines: &mut Vec<timings::ProcessTimeline>,
) {
    let lane = plan.lane;
    let artifacts = run::artifacts_dir(root, lane.as_str());
    let mut programs = Vec::new();
    for invocation in plan.prebuild.iter().chain(&plan.invocations) {
        match environment.find_program(&invocation.program) {
            Some(program) => programs.push(program),
            None => {
                tally.add(Tally::failure(format!(
                    "lane {lane}: `{}` is not on PATH",
                    invocation.program
                )));
                return;
            }
        }
    }
    let count = plan.invocations.len();
    writeln!(
        out,
        "lane {lane}: {count} concurrent processes (--jobs), each with --test-threads=1 and its \
         own device"
    )
    .ok();
    if let Some(build) = &plan.prebuild {
        writeln!(out, "lane {lane}: {}", build.command_line()).ok();
        out.flush().ok();
        let log = artifacts.join(format!("{}.log", build.label));
        match runner.run(build, &programs[0], root, &log) {
            Ok(result) if result.success => {}
            Ok(result) => {
                tally.add(Tally::failure(format!(
                    "lane {lane}: `{}` exited with {} before the workers started; see {}",
                    build.command_line(),
                    result
                        .code
                        .map_or_else(|| "a signal".to_string(), |code| format!("code {code}")),
                    log.display()
                )));
                return;
            }
            Err(error) => {
                tally.add(Tally::failure(format!("lane {lane}: {error:#}")));
                return;
            }
        }
        runner.take_timeline();
    }
    let jobs: Vec<run::RunJob<'_>> = plan
        .invocations
        .iter()
        .zip(&programs[usize::from(plan.prebuild.is_some())..])
        .map(|(invocation, program)| run::RunJob {
            invocation,
            program,
            workdir: root,
            log: artifacts.join(format!("{}.log", invocation.label)),
        })
        .collect();
    for (index, job) in jobs.iter().enumerate() {
        writeln!(
            out,
            "lane {lane}: job {}/{count}: {}",
            index + 1,
            job.invocation.command_line()
        )
        .ok();
    }
    out.flush().ok();
    let results = runner.run_concurrently(&jobs);
    for (index, (job, (result, timeline))) in jobs.iter().zip(results).enumerate() {
        writeln!(
            out,
            "lane {lane}: ---- job {}/{count} ({}) ----",
            index + 1,
            job.invocation.label
        )
        .ok();
        match result {
            Ok(result) => {
                writeln!(out, "{}", result.output.trim_end()).ok();
                timelines.push(timings::ProcessTimeline {
                    label: job.invocation.label.clone(),
                    lines: timeline,
                });
                tally.add(account(plan, job.invocation, &result, &job.log));
            }
            Err(error) => {
                writeln!(out, "{error:#}").ok();
                tally.add(Tally::failure(format!(
                    "lane {lane}: job {}/{count}: {error:#}",
                    index + 1
                )));
            }
        }
    }
}

/// What `result` of `invocation` counts for the lane: its reported tests, and a failure for a
/// process that stopped without reporting one.
fn account(plan: &LanePlan, invocation: &Invocation, result: &RunOutput, log: &Path) -> Tally {
    let lane = plan.lane;
    let mut counted = match invocation.output {
        OutputKind::Libtest | OutputKind::Listing => results::tally_libtest(&result.output),
        OutputKind::ResultLines => results::tally_result_lines(&result.output),
        OutputKind::Criterion => {
            let mut benches = local_gpu::tally_benches(&result.output);
            if result.success && benches.passed == 0 {
                benches.add(Tally::failure(format!(
                    "lane {lane}: `{}` reported no benchmark results; see {}",
                    invocation.command_line(),
                    log.display()
                )));
            }
            benches
        }
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
    counted
}

/// Writes `timings.md` beside a lane's artifacts when libtest processes ran.
fn write_timings(
    lane: LaneId,
    artifacts: &Path,
    timelines: &[timings::ProcessTimeline],
    out: &mut dyn Write,
) {
    if timelines.is_empty() {
        return;
    }
    let path = artifacts.join("timings.md");
    let report = timings::report(lane.as_str(), timelines);
    match std::fs::create_dir_all(artifacts).and_then(|()| std::fs::write(&path, report)) {
        Ok(()) => {
            writeln!(out, "lane {lane}: test timings in {}", path.display()).ok();
        }
        Err(error) => {
            writeln!(
                out,
                "lane {lane}: could not write {}: {error}",
                path.display()
            )
            .ok();
        }
    }
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
