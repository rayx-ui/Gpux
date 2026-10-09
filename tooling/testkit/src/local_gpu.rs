//! Lane G: the developer's real GPU.
//!
//! `gpux-testkit lane g [--headed]` runs on the machine's hardware adapter:
//!
//! - lane B's specs in the `local-gpu` profile, all of them, `@timing` and `@dpr2` included
//!   (`--headed` shows the browser);
//! - lanes C and D with relative probes only, since no committed golden matches a GPU, and with
//!   `GPUX_EXPECT_BACKEND=hardware`, so a failing GPU cannot leave them on a software rasterizer;
//! - the `#[gpux::bench]` Criterion suites of `gpux-platform`, `fast_mode` among them.
//!
//! It refuses to run under CI, and writes a timing summary to
//! `artifacts-temp/testkit/g/timing.md` and the fast-mode report to
//! `artifacts-temp/testkit/g/fast-mode.md`.

use crate::{
    Environment, Os,
    cli::{LaneArgs, Selection},
    lanes::{self, Invocation, LaneId, LanePlan, OutputKind, PlanError},
    prereq,
    results::{self, Tally},
    run::{self, Runner},
    web,
};
use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

/// The Playwright profile lane G runs lane B's specs in.
pub const PROFILE: &str = "local-gpu";

/// `gpux-platform`'s benchmark targets: the `#[gpux::bench]` Criterion suites.
pub const BENCHES: [&str; 3] = [
    "randomized_element_tree",
    "inspector_render",
    crate::fast_mode::BENCH,
];

/// The prefix of the lines timing specs print for the summary.
pub const TIMING_PREFIX: &str = "gpux-timing:";

/// The environment variable CI services set.
pub const CI_ENV_VAR: &str = "CI";

/// Why lane G refuses to run in this environment, if it does: CI runners have no GPU, and
/// software adapters make timing meaningless.
pub fn refusal(environment: &dyn Environment) -> Option<String> {
    let value = environment.var(CI_ENV_VAR)?;
    let value = value.trim();
    if value.is_empty() || value == "0" || value.eq_ignore_ascii_case("false") {
        return None;
    }
    Some(format!(
        "lane g runs on the developer's GPU and refuses to run under CI (`{CI_ENV_VAR}={value}`): \
         hosted runners have no GPU, and software adapters make timing meaningless; CI runs \
         lanes a to e"
    ))
}

/// The adapter variables that put headless rendering on the GPU. macOS renders through Metal on
/// the system GPU without them.
pub fn adapter_env(os: Os) -> Vec<(String, String)> {
    if os == Os::MacOs {
        Vec::new()
    } else {
        vec![("GPUX_WGPU_ADAPTER".into(), "hardware".into())]
    }
}

/// Lanes C and D on the hardware adapter, with relative probes only.
pub fn hardware_plans(selection: &Selection, os: Os) -> Result<Vec<LanePlan>, PlanError> {
    let mut c = lanes::plan(LaneId::C, selection)?;
    let mut c_env = adapter_env(os);
    c_env.push((
        "GPUX_EXPECT_BACKEND".into(),
        if os == Os::MacOs { "metal" } else { "hardware" }.into(),
    ));
    c_env.push(("GPUX_PROBES_ONLY".into(), "1".into()));
    for invocation in c
        .invocations
        .iter_mut()
        .chain(c.listings.iter_mut().map(|listing| &mut listing.invocation))
    {
        invocation.label = format!("hardware-{}", invocation.label);
        invocation.env.extend(c_env.iter().cloned());
    }
    c.lane = LaneId::G;

    // Desktop windows never compare goldens; their own adapter choice prefers the GPU, and the
    // host checks that it got one.
    let mut d = lanes::plan(LaneId::D, selection)?;
    for invocation in &mut d.invocations {
        invocation.label = format!("hardware-{}", invocation.label);
        invocation
            .env
            .push(("GPUX_EXPECT_BACKEND".into(), "hardware".into()));
    }
    if !d.invocations.is_empty() {
        d.tests.insert(0, "desktop_host::desktop-backend".into());
    }
    d.lane = LaneId::G;
    Ok(vec![c, d])
}

/// The benchmark run: every `#[gpux::bench]` suite of `gpux-platform`, on the GPU.
pub fn bench_plan(os: Os) -> LanePlan {
    let mut args: Vec<String> = [
        "bench",
        "-p",
        "gpux-platform",
        "--features",
        "bench-support",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    for bench in BENCHES {
        args.extend(["--bench".into(), bench.into()]);
    }
    LanePlan {
        lane: LaneId::G,
        invocations: vec![Invocation {
            label: "benches".into(),
            program: "cargo".into(),
            args,
            env: adapter_env(os),
            output: OutputKind::Criterion,
        }],
        tests: BENCHES
            .iter()
            .map(|bench| format!("bench::{bench}"))
            .collect(),
        listings: Vec::new(),
    }
}

/// One Criterion result: a benchmark id and its `[low estimate high]` values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchResult {
    /// The benchmark id, such as `tree/wide/full_refresh`.
    pub id: String,
    /// The lower bound of the estimate's confidence interval.
    pub low: String,
    /// The estimate.
    pub estimate: String,
    /// The upper bound.
    pub high: String,
}

/// The results in Criterion's output. Criterion prints `<id> time: [low estimate high]`, or, when
/// the id is long, the id on a line of its own right above an indented `time:` line. Ids may
/// contain spaces (`RandomizedTree/full refresh/tree/any/seed-0`).
pub fn bench_results(output: &str) -> Vec<BenchResult> {
    let mut results = Vec::new();
    // The last line with text, unless it was indented.
    let mut previous: Option<&str> = None;
    for line in output
        .split(['\n', '\r'])
        .filter(|line| !line.trim().is_empty())
    {
        let Some(at) = line.find("time:") else {
            previous = (!line.starts_with(char::is_whitespace)).then(|| line.trim_end());
            continue;
        };
        let named = line[..at].trim();
        let id = if named.is_empty() {
            previous.take()
        } else {
            Some(named)
        };
        previous = None;
        let Some(id) = id.map(str::to_string) else {
            continue;
        };
        let values: Vec<&str> = line[at..]
            .split(['[', ']'])
            .nth(1)
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        if let [low, low_unit, estimate, estimate_unit, high, high_unit] = values[..] {
            results.push(BenchResult {
                id,
                low: format!("{low} {low_unit}"),
                estimate: format!("{estimate} {estimate_unit}"),
                high: format!("{high} {high_unit}"),
            });
        }
    }
    results
}

/// The tally of a benchmark run: one passed test per result.
pub fn tally_benches(output: &str) -> Tally {
    Tally {
        passed: bench_results(output).len(),
        ..Tally::default()
    }
}

/// What timing specs printed for the summary, without the prefix.
pub fn timing_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_once(TIMING_PREFIX))
        .map(|(_, timing)| timing.trim().to_string())
        .collect()
}

/// One part of a lane G run, for the summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// What ran.
    pub name: String,
    /// Its passed, failed and skipped counts.
    pub tally: Tally,
    /// How long it took.
    pub duration: Duration,
}

/// The timing summary: each part's results and duration, the timing specs' measurements and the
/// benchmark estimates.
pub fn summary(
    parts: &[Part],
    gpu: Option<&str>,
    timings: &[String],
    benches: &[BenchResult],
) -> String {
    let mut text = String::from("# gpux lane G timing summary\n\n");
    text.push_str(&format!(
        "GPU: {}\n\n",
        gpu.unwrap_or("not reported (macOS windows name no adapter; lane C ran on Metal)")
    ));
    text.push_str("| Part | Passed | Failed | Skipped | Duration |\n|---|---|---|---|---|\n");
    for part in parts {
        text.push_str(&format!(
            "| {} | {} | {} | {} | {:.1} s |\n",
            part.name,
            part.tally.passed,
            part.tally.failed,
            part.tally.skipped,
            part.duration.as_secs_f64()
        ));
    }
    if !timings.is_empty() {
        text.push_str("\n## Frame timing (lane B `@timing` specs)\n\n");
        for timing in timings {
            text.push_str(&format!("- {timing}\n"));
        }
    }
    if !benches.is_empty() {
        text.push_str("\n## Benchmarks\n\n| Benchmark | Estimate | Interval |\n|---|---|---|\n");
        for bench in benches {
            text.push_str(&format!(
                "| {} | {} | {} – {} |\n",
                bench.id, bench.estimate, bench.low, bench.high
            ));
        }
    }
    text
}

/// The GPU the desktop windows reported in their `desktop-backend` result.
fn reported_gpu(output: &str) -> Option<String> {
    results::result_lines(output)
        .into_iter()
        .find(|line| line.scene == "desktop-backend")
        .and_then(|line| {
            line.detail
                .strip_prefix("windows render on ")
                .map(str::to_string)
        })
}

/// The browser specs lane G selects: all of them, or the selection's.
fn browser_grep(selection: &Selection) -> Option<String> {
    (!selection.is_everything()).then(|| web::lane_grep(selection))
}

fn read_log(root: &Path, lane: &str, name: &str) -> String {
    std::fs::read_to_string(run::artifacts_dir(root, lane).join(name)).unwrap_or_default()
}

/// Runs lane G and writes its timing summary.
pub fn run(
    root: &Path,
    args: &LaneArgs,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Tally {
    if let Some(reason) = refusal(environment) {
        writeln!(out, "lane g: {reason}").ok();
        return Tally::failure(format!("lane g: {reason}"));
    }
    let mut tally = Tally::default();
    let mut parts = Vec::new();

    let started = Instant::now();
    let browser = web::e2e(
        root,
        environment,
        runner,
        PROFILE,
        browser_grep(&args.selection).as_deref(),
        args.headed,
        out,
    );
    parts.push(Part {
        name: format!("lane B in `{PROFILE}`"),
        tally: browser.clone(),
        duration: started.elapsed(),
    });
    tally.add(browser);
    let timings = timing_lines(&read_log(root, "b", "web-e2e.log"));

    let mut gpu = None;
    match hardware_plans(&args.selection, environment.os()) {
        Ok(plans) => {
            let names = [
                (LaneId::C, "lane C on the GPU, probes only"),
                (LaneId::D, "lane D on the GPU"),
            ];
            for (plan, (lane, name)) in plans.iter().zip(names) {
                let started = Instant::now();
                let problems = prereq::check(lane, environment);
                let part = if problems.is_empty() {
                    crate::run_plan(plan, root, environment, runner, out)
                } else {
                    let mut failed = Tally::default();
                    for problem in problems {
                        writeln!(out, "lane g: {problem}").ok();
                        failed.add(Tally::failure(format!("lane g: {problem}")));
                    }
                    failed
                };
                if lane == LaneId::D {
                    gpu = reported_gpu(&read_log(root, "g", "hardware-lane-d.log"));
                }
                parts.push(Part {
                    name: name.into(),
                    tally: part.clone(),
                    duration: started.elapsed(),
                });
                tally.add(part);
            }
        }
        Err(error) => {
            writeln!(out, "lane g: {error}").ok();
            tally.add(Tally::failure(format!("lane g: {error}")));
        }
    }

    let mut benches = Vec::new();
    if args.selection.is_everything() {
        let started = Instant::now();
        let part = crate::run_plan(
            &bench_plan(environment.os()),
            root,
            environment,
            runner,
            out,
        );
        let bench_output = read_log(root, "g", "benches.log");
        benches = bench_results(&bench_output);
        // The `fast_mode` suite's report: its ids by scenario and mode, with their counters.
        match crate::fast_mode::write_report(
            &run::artifacts_dir(root, "g"),
            &bench_output,
            Some(&crate::fast_mode::criterion_dir(root, environment)),
            None,
            None,
        ) {
            Ok((path, ids)) if ids > 0 => {
                writeln!(out, "lane g: fast-mode report in {}", path.display()).ok();
            }
            Ok(_) => {}
            Err(error) => tally.add(Tally::failure(format!(
                "lane g: could not write the fast-mode report: {error:#}"
            ))),
        }
        parts.push(Part {
            name: "`gpux-platform` benchmarks".into(),
            tally: part.clone(),
            duration: started.elapsed(),
        });
        tally.add(part);
    } else {
        writeln!(
            out,
            "lane g: the benchmarks run only without --scene or --grep"
        )
        .ok();
    }

    let path = run::artifacts_dir(root, "g").join("timing.md");
    let text = summary(&parts, gpu.as_deref(), &timings, &benches);
    match std::fs::create_dir_all(run::artifacts_dir(root, "g"))
        .and_then(|()| std::fs::write(&path, text))
    {
        Ok(()) => {
            writeln!(out, "lane g: timing summary in {}", path.display()).ok();
        }
        Err(error) => tally.add(Tally::failure(format!(
            "lane g: could not write {}: {error}",
            path.display()
        ))),
    }
    tally
}

/// Prints lane G's selection without running it, and returns how many tests it holds.
pub fn list(
    root: &Path,
    args: &LaneArgs,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> (usize, Tally) {
    let mut listed = 0;
    let mut tally = Tally::default();
    match web::list(
        root,
        environment,
        runner,
        PROFILE,
        browser_grep(&args.selection).as_deref(),
        out,
    ) {
        Ok(count) => listed += count,
        Err(failure) => tally.add(failure),
    }
    match hardware_plans(&args.selection, environment.os()) {
        Ok(plans) => {
            for plan in &plans {
                if plan.listings.is_empty() {
                    listed += crate::list(plan, out);
                } else {
                    match crate::list_exact(plan, root, environment, runner, out) {
                        Ok(count) => listed += count,
                        Err(failure) => tally.add(failure),
                    }
                }
            }
        }
        Err(error) => tally.add(Tally::failure(format!("lane g: {error}"))),
    }
    if args.selection.is_everything() {
        listed += crate::list(&bench_plan(environment.os()), out);
    }
    (listed, tally)
}
