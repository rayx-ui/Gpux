//! The fast-mode report: what the `fast_mode` bench of `gpux-platform` measured, scenario by
//! scenario and mode by mode.
//!
//! The bench prints, for every id `fast/<scenario>/<mode>`, Criterion's estimate, the GPUI bench
//! report's per-iteration instructions and allocations where `BENCH_MEASUREMENT` gathers them,
//! and one `gpux-fast-stats: <id> <counter>=<n> ...` line with the work counters. This module
//! parses that output and renders `fast-mode.md`:
//!
//! - one table per scenario: per mode, the median and its confidence interval, the delta
//!   against `off` and against the previous mode, instructions, allocations, and the counters;
//! - a per-mechanism summary ranking the scenarios each mechanism sped up and flagging any it
//!   slowed down;
//! - with `--compare <run-dir>`, every id against an earlier run, such as the pre-optimization
//!   baseline.
//!
//! `gpux-testkit lane g` runs the bench with the other benchmarks and writes
//! `artifacts-temp/testkit/g/fast-mode.md`; `gpux-testkit perf fast-mode` runs it alone into
//! `artifacts-temp/testkit/perf/<run>/`.

use crate::{
    Environment, Os,
    lanes::{Invocation, LaneId, LanePlan, OutputKind},
    local_gpu,
    results::Tally,
    run::{self, Runner},
};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

/// The bench target.
pub const BENCH: &str = "fast_mode";

/// The prefix of the counter lines the bench prints.
pub const STATS_PREFIX: &str = "gpux-fast-stats:";

/// The cumulative modes, in order.
pub const MODES: [&str; 5] = ["off", "layout", "views", "splice", "layers"];

/// The mechanism each mode after `off` adds.
pub const MECHANISMS: [(&str, &str); 4] = [
    ("layout", "layout retention"),
    ("views", "view retention"),
    ("splice", "splice"),
    ("layers", "scroll layers"),
];

/// The counters a scenario table shows beside the timings, when they are not zero.
pub const KEY_COUNTERS: [&str; 10] = [
    "views_built",
    "views_reused",
    "views_spliced",
    "cached_views_replayed",
    "style_writes",
    "nodes_claimed",
    "lines_shaped",
    "primitives_painted",
    "primitives_copied",
    "layers_composited",
];

/// The report file's name.
pub const REPORT: &str = "fast-mode.md";

/// The raw bench output's name in a run directory.
pub const LOG: &str = "fast_mode.log";

/// What a run directory records about where it ran.
pub const RUN_INFO: &str = "run.txt";

/// The medians a run directory keeps from Criterion's estimates, one
/// `gpux-fast-median: <id> <low> <median> <high>` line per id, in nanoseconds.
pub const MEDIANS: &str = "medians.txt";

/// The prefix of the lines in [`MEDIANS`].
pub const MEDIAN_PREFIX: &str = "gpux-fast-median:";

/// A Criterion estimate, in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    /// The confidence interval's lower bound.
    pub low: f64,
    /// The estimate.
    pub estimate: f64,
    /// The confidence interval's upper bound.
    pub high: f64,
}

/// One bench id's measurements.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// `fast/<scenario>/<mode>`.
    pub id: String,
    /// The scenario.
    pub scenario: String,
    /// The mode.
    pub mode: String,
    /// Criterion's time estimate, when it timed the id (not under `--test`): the point
    /// estimate and interval of its `time:` line.
    pub time: Option<Estimate>,
    /// The median and its interval from Criterion's `estimates.json`, when found.
    pub median: Option<Estimate>,
    /// Instructions per iteration, when measured.
    pub instructions: Option<f64>,
    /// Heap allocations per iteration, when measured.
    pub allocations: Option<f64>,
    /// The work counters of the id's last run, in the order the bench printed them.
    pub counters: Vec<(String, u64)>,
}

impl Sample {
    /// The timing the report compares: the median when Criterion's estimates were found,
    /// its point estimate otherwise.
    pub fn timing(&self) -> Option<Estimate> {
        self.median.or(self.time)
    }

    /// The counter `name`, zero when absent.
    pub fn counter(&self, name: &str) -> u64 {
        self.counters
            .iter()
            .find_map(|(counter, value)| (counter == name).then_some(*value))
            .unwrap_or(0)
    }

    /// A counter per measured frame.
    pub fn per_frame(&self, name: &str) -> f64 {
        let frames = self.counter("frames").max(1);
        self.counter(name) as f64 / frames as f64
    }
}

/// A `fast_mode` bench run: its samples, scenario by scenario in the order the bench ran them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// Every id the run reported.
    pub samples: Vec<Sample>,
}

impl Run {
    /// The scenarios, in run order.
    pub fn scenarios(&self) -> Vec<&str> {
        let mut scenarios: Vec<&str> = Vec::new();
        for sample in &self.samples {
            if !scenarios.contains(&sample.scenario.as_str()) {
                scenarios.push(&sample.scenario);
            }
        }
        scenarios
    }

    /// The sample of `scenario` in `mode`.
    pub fn sample(&self, scenario: &str, mode: &str) -> Option<&Sample> {
        self.samples
            .iter()
            .find(|sample| sample.scenario == scenario && sample.mode == mode)
    }

    /// The sample with `id`.
    pub fn by_id(&self, id: &str) -> Option<&Sample> {
        self.samples.iter().find(|sample| sample.id == id)
    }
}

/// A Criterion duration such as `1.2345 ms`, in nanoseconds.
pub fn parse_duration(value: &str, unit: &str) -> Option<f64> {
    let value: f64 = value.parse().ok()?;
    let scale = match unit {
        "ps" => 1e-3,
        "ns" => 1.,
        "µs" | "us" | "μs" => 1e3,
        "ms" => 1e6,
        "s" => 1e9,
        _ => return None,
    };
    Some(value * scale)
}

/// A count such as `331.334 M` (the unit noun after it is ignored).
pub fn parse_count(text: &str) -> Option<f64> {
    let mut words = text.split_whitespace();
    let value: f64 = words.next()?.parse().ok()?;
    let scale = match words.next() {
        Some("K") => 1e3,
        Some("M") => 1e6,
        Some("G") => 1e9,
        _ => 1.,
    };
    Some(value * scale)
}

/// The median of an id from Criterion's `estimates.json` text: its point estimate and 95%
/// interval, in the measurement's unit (nanoseconds for wall time).
pub fn parse_median(estimates: &str) -> Option<Estimate> {
    let median = &estimates[estimates.find("\"median\":")?..];
    // The median's own object ends where the next estimate begins.
    let median = &median[..median.find("\"median_abs_dev\"").unwrap_or(median.len())];
    let number = |key: &str| -> Option<f64> {
        let start = median.find(key)? + key.len();
        let rest = &median[start..];
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        rest[..end].trim().parse().ok()
    };
    Some(Estimate {
        low: number("\"lower_bound\":")?,
        estimate: number("\"point_estimate\":")?,
        high: number("\"upper_bound\":")?,
    })
}

/// Fills in each sample's median from Criterion's output directory (`target/criterion`),
/// where `fast/<scenario>/<mode>` keeps `new/estimates.json`.
pub fn apply_medians(run: &mut Run, criterion_dir: &Path) {
    for sample in &mut run.samples {
        let estimates = criterion_dir
            .join(&sample.id)
            .join("new")
            .join("estimates.json");
        if let Ok(text) = std::fs::read_to_string(estimates) {
            sample.median = parse_median(&text);
        }
    }
}

/// Splits `fast/<scenario>/<mode>` into its scenario and mode.
pub fn split_id(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix("fast/")?;
    let (scenario, mode) = rest.rsplit_once('/')?;
    MODES.contains(&mode).then_some((scenario, mode))
}

/// Parses a `fast_mode` bench run's output.
pub fn parse(output: &str) -> Run {
    let mut samples: BTreeMap<String, Sample> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    fn entry<'a>(
        samples: &'a mut BTreeMap<String, Sample>,
        order: &mut Vec<String>,
        id: &str,
    ) -> Option<&'a mut Sample> {
        let (scenario, mode) = split_id(id)?;
        if !samples.contains_key(id) {
            order.push(id.to_string());
            samples.insert(
                id.to_string(),
                Sample {
                    id: id.to_string(),
                    scenario: scenario.to_string(),
                    mode: mode.to_string(),
                    time: None,
                    median: None,
                    instructions: None,
                    allocations: None,
                    counters: Vec::new(),
                },
            );
        }
        samples.get_mut(id)
    }

    for result in local_gpu::bench_results(output) {
        let estimate = |text: &str| {
            let (value, unit) = text.split_once(' ')?;
            parse_duration(value, unit)
        };
        if let (Some(low), Some(mid), Some(high)) = (
            estimate(&result.low),
            estimate(&result.estimate),
            estimate(&result.high),
        ) && let Some(sample) = entry(&mut samples, &mut order, &result.id)
        {
            sample.time = Some(Estimate {
                low,
                estimate: mid,
                high,
            });
        }
    }

    // The GPUI bench report names the id, then indents its per-iteration metrics.
    let mut reporting: Option<String> = None;
    for line in output.lines() {
        if let Some((_, id)) = line.split_once("GPUI bench report (all observed iterations): ") {
            reporting = Some(id.trim().to_string());
            continue;
        }
        if let Some((_, stats)) = line.split_once(STATS_PREFIX) {
            reporting = None;
            let mut words = stats.split_whitespace();
            let Some(id) = words.next() else { continue };
            let counters: Vec<(String, u64)> = words
                .filter_map(|word| {
                    let (name, value) = word.split_once('=')?;
                    Some((name.to_string(), value.parse().ok()?))
                })
                .collect();
            if let Some(sample) = entry(&mut samples, &mut order, id) {
                // The last run of an id is the measured one.
                sample.counters = counters;
            }
            continue;
        }
        let Some(id) = reporting.as_deref() else {
            continue;
        };
        if !line.starts_with(char::is_whitespace) {
            reporting = None;
            continue;
        }
        let metric = line.trim();
        let median = |prefix: &str| {
            metric
                .strip_prefix(prefix)
                .and_then(|rest| rest.trim().strip_prefix("median "))
                .and_then(|rest| rest.split(" (").next())
                .and_then(parse_count)
        };
        if let Some(value) = median("instructions per iteration:") {
            if let Some(sample) = entry(&mut samples, &mut order, id) {
                sample.instructions = Some(value);
            }
        } else if let Some(value) = median("allocations per iteration:")
            && let Some(sample) = entry(&mut samples, &mut order, id)
        {
            sample.allocations = Some(value);
        }
    }

    Run {
        samples: order
            .into_iter()
            .filter_map(|id| samples.remove(&id))
            .collect(),
    }
}

/// `later` against `earlier` as a signed percentage, `+` slower.
fn delta(later: f64, earlier: f64) -> Option<f64> {
    (earlier > 0.).then(|| (later - earlier) / earlier * 100.)
}

fn format_delta(delta: Option<f64>) -> String {
    match delta {
        Some(delta) => format!("{delta:+.1}%"),
        None => "–".into(),
    }
}

/// A duration in the unit that suits it.
pub fn format_duration(nanos: f64) -> String {
    if nanos >= 1e9 {
        format!("{:.3} s", nanos / 1e9)
    } else if nanos >= 1e6 {
        format!("{:.3} ms", nanos / 1e6)
    } else if nanos >= 1e3 {
        format!("{:.2} µs", nanos / 1e3)
    } else {
        format!("{nanos:.1} ns")
    }
}

/// A count with a K, M or G prefix.
pub fn format_count(count: f64) -> String {
    if count >= 1e9 {
        format!("{:.2} G", count / 1e9)
    } else if count >= 1e6 {
        format!("{:.2} M", count / 1e6)
    } else if count >= 1e3 {
        format!("{:.1} K", count / 1e3)
    } else {
        format!("{count:.0}")
    }
}

/// A slowdown smaller than this, in percent, is run-to-run noise on a developer machine even
/// when Criterion's intervals do not overlap: identical modes differ by up to about 3%.
pub const REGRESSION_FLOOR_PERCENT: f64 = 3.;

/// Whether `later` is slower than `earlier` beyond both confidence intervals and beyond
/// [`REGRESSION_FLOOR_PERCENT`].
pub fn regressed(later: &Estimate, earlier: &Estimate) -> bool {
    later.low > earlier.high
        && delta(later.estimate, earlier.estimate)
            .is_some_and(|delta| delta > REGRESSION_FLOOR_PERCENT)
}

/// One scenario's benefit from a mechanism: its time against the mode before.
#[derive(Clone, Debug, PartialEq)]
pub struct Benefit {
    /// The scenario.
    pub scenario: String,
    /// The delta against the previous mode, `-` faster.
    pub delta: f64,
    /// Whether it is slower beyond the confidence intervals.
    pub regression: bool,
}

/// What the mechanism `mode` adds did to each scenario, most sped up first.
pub fn ranking(run: &Run, mode: &str) -> Vec<Benefit> {
    let Some(index) = MODES.iter().position(|candidate| *candidate == mode) else {
        return Vec::new();
    };
    let Some(previous) = index.checked_sub(1).map(|index| MODES[index]) else {
        return Vec::new();
    };
    let mut benefits: Vec<Benefit> = run
        .scenarios()
        .into_iter()
        .filter_map(|scenario| {
            let now = run.sample(scenario, mode)?.timing()?;
            let before = run.sample(scenario, previous)?.timing()?;
            Some(Benefit {
                scenario: scenario.to_string(),
                delta: delta(now.estimate, before.estimate)?,
                regression: regressed(&now, &before),
            })
        })
        .collect();
    benefits.sort_by(|a, b| a.delta.total_cmp(&b.delta));
    benefits
}

/// Renders the report of `run`; `compare` adds a section against an earlier run.
pub fn render(run: &Run, info: Option<&str>, compare: Option<(&str, &Run)>) -> String {
    let mut text = String::from("# gpux fast-mode report\n\n");
    if let Some(info) = info.filter(|info| !info.trim().is_empty()) {
        for line in info.lines().filter(|line| !line.trim().is_empty()) {
            text.push_str(&format!("- {}\n", line.trim()));
        }
        text.push('\n');
    }
    if run.samples.is_empty() {
        text.push_str("The run reported no `fast/<scenario>/<mode>` id.\n");
        return text;
    }
    let timed = run.samples.iter().any(|sample| sample.timing().is_some());
    let medians = run.samples.iter().any(|sample| sample.median.is_some());
    if !timed {
        text.push_str(
            "The run timed nothing (Criterion's `--test` mode); the tables show the counters.\n\n",
        );
    } else if !medians {
        text.push_str(
            "Criterion's `estimates.json` were not found, so timings are its point estimates \
             rather than medians.\n\n",
        );
    }

    text.push_str("## Mechanisms\n\n");
    text.push_str(
        "Each mechanism against the mode before it, scenarios ranked by benefit. A regression is \
         slower beyond both confidence intervals and by more than 3%.\n\n",
    );
    text.push_str("| Mechanism | Faster | Slower | Best | Regressions |\n|---|---|---|---|---|\n");
    for (mode, mechanism) in MECHANISMS {
        let ranking = ranking(run, mode);
        let faster = ranking.iter().filter(|benefit| benefit.delta < 0.).count();
        let slower = ranking.iter().filter(|benefit| benefit.delta > 0.).count();
        let best = ranking
            .first()
            .filter(|benefit| benefit.delta < 0.)
            .map(|benefit| format!("{} ({:+.1}%)", benefit.scenario, benefit.delta))
            .unwrap_or_else(|| "–".into());
        let regressions: Vec<String> = ranking
            .iter()
            .filter(|benefit| benefit.regression)
            .map(|benefit| format!("**{} ({:+.1}%)**", benefit.scenario, benefit.delta))
            .collect();
        text.push_str(&format!(
            "| {mechanism} (`{mode}`) | {faster} | {slower} | {best} | {} |\n",
            if regressions.is_empty() {
                "none".to_string()
            } else {
                regressions.join(", ")
            }
        ));
    }
    for (mode, mechanism) in MECHANISMS {
        let ranking = ranking(run, mode);
        if ranking.is_empty() {
            continue;
        }
        text.push_str(&format!(
            "\n### {mechanism}\n\n| Rank | Scenario | Δ previous |\n|---|---|---|\n"
        ));
        for (rank, benefit) in ranking.iter().enumerate() {
            let flag = if benefit.regression {
                " ⚠ regression"
            } else {
                ""
            };
            text.push_str(&format!(
                "| {} | {} | {:+.1}%{flag} |\n",
                rank + 1,
                benefit.scenario,
                benefit.delta
            ));
        }
    }

    for scenario in run.scenarios() {
        text.push_str(&format!("\n## {scenario}\n\n"));
        text.push_str(&format!(
            "| Mode | {} | Interval | Δ off | Δ previous | Instructions | Allocations |\n\
             |---|---|---|---|---|---|---|\n",
            if medians { "Median" } else { "Estimate" }
        ));
        let off = run.sample(scenario, "off").and_then(Sample::timing);
        let mut previous: Option<Estimate> = None;
        for mode in MODES {
            let Some(sample) = run.sample(scenario, mode) else {
                continue;
            };
            let (median, interval, against_off, against_previous) = match sample.timing() {
                Some(time) => (
                    format_duration(time.estimate),
                    format!(
                        "{} – {}",
                        format_duration(time.low),
                        format_duration(time.high)
                    ),
                    format_delta(off.and_then(|off| delta(time.estimate, off.estimate))),
                    format_delta(
                        previous.and_then(|previous| delta(time.estimate, previous.estimate)),
                    ),
                ),
                None => ("–".into(), "–".into(), "–".into(), "–".into()),
            };
            previous = sample.timing().or(previous);
            text.push_str(&format!(
                "| {mode} | {median} | {interval} | {against_off} | {against_previous} | {} | {} |\n",
                sample.instructions.map(format_count).unwrap_or_else(|| "–".into()),
                sample.allocations.map(format_count).unwrap_or_else(|| "–".into()),
            ));
        }
        // The counters, per frame, that any mode recorded.
        let modes: Vec<&Sample> = MODES
            .iter()
            .filter_map(|mode| run.sample(scenario, mode))
            .collect();
        let mut names: Vec<&str> = Vec::new();
        for sample in &modes {
            for (name, value) in &sample.counters {
                if name != "frames" && *value > 0 && !names.contains(&name.as_str()) {
                    names.push(name);
                }
            }
        }
        names.sort_by_key(|name| {
            KEY_COUNTERS
                .iter()
                .position(|key| key == name)
                .unwrap_or(KEY_COUNTERS.len())
        });
        if !names.is_empty() {
            text.push_str("\n| Counter per frame |");
            for sample in &modes {
                text.push_str(&format!(" {} |", sample.mode));
            }
            text.push_str("\n|---|");
            for _ in &modes {
                text.push_str("---|");
            }
            text.push('\n');
            for name in names {
                text.push_str(&format!("| {name} |"));
                for sample in &modes {
                    let value = sample.per_frame(name);
                    if value.fract() == 0. {
                        text.push_str(&format!(" {value:.0} |"));
                    } else {
                        text.push_str(&format!(" {value:.2} |"));
                    }
                }
                text.push('\n');
            }
        }
    }

    if let Some((label, earlier)) = compare {
        text.push_str(&compare_section(label, earlier, run));
    }
    text
}

/// Every id of `current` against `earlier`.
pub fn compare_section(label: &str, earlier: &Run, current: &Run) -> String {
    let mut text = format!(
        "\n## Compared with {label}\n\n| Id | Earlier | Now | Δ | Instructions Δ | Allocations Δ |\n\
         |---|---|---|---|---|---|\n"
    );
    for sample in &current.samples {
        let Some(before) = earlier.by_id(&sample.id) else {
            text.push_str(&format!("| {} | – | | new | | |\n", sample.id));
            continue;
        };
        let time = |sample: &Sample| {
            sample
                .timing()
                .map(|time| format_duration(time.estimate))
                .unwrap_or_else(|| "–".into())
        };
        let change = match (sample.timing(), before.timing()) {
            (Some(now), Some(then)) => {
                let flag = if regressed(&now, &then) { " ⚠" } else { "" };
                format!("{}{flag}", format_delta(delta(now.estimate, then.estimate)))
            }
            _ => "–".into(),
        };
        let metric = |now: Option<f64>, then: Option<f64>| match (now, then) {
            (Some(now), Some(then)) => format_delta(delta(now, then)),
            _ => "–".into(),
        };
        text.push_str(&format!(
            "| {} | {} | {} | {change} | {} | {} |\n",
            sample.id,
            time(before),
            time(sample),
            metric(sample.instructions, before.instructions),
            metric(sample.allocations, before.allocations),
        ));
    }
    for sample in &earlier.samples {
        if current.by_id(&sample.id).is_none() {
            text.push_str(&format!("| {} | | – | gone | | |\n", sample.id));
        }
    }
    text
}

/// The `cargo bench` invocation of the `fast_mode` suite alone.
pub fn bench_invocation(os: Os) -> Invocation {
    let args = [
        "bench",
        "-p",
        "gpux-platform",
        "--features",
        "bench-support",
        "--bench",
        BENCH,
    ];
    Invocation {
        label: "fast-mode".into(),
        program: "cargo".into(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        env: local_gpu::adapter_env(os),
        output: OutputKind::Criterion,
    }
}

/// The plan `perf fast-mode` runs.
pub fn plan(os: Os) -> LanePlan {
    LanePlan {
        lane: LaneId::G,
        invocations: vec![bench_invocation(os)],
        tests: vec![format!("bench::{BENCH}")],
        listings: Vec::new(),
    }
}

/// `artifacts-temp/testkit/perf/<name>/`.
pub fn run_dir(root: &Path, name: &str) -> PathBuf {
    run::artifacts_dir(root, "perf").join(name)
}

/// Reads an earlier run directory's bench output and the medians it kept.
pub fn read_run_dir(dir: &Path) -> anyhow::Result<Run> {
    let log = dir.join(LOG);
    let output = std::fs::read_to_string(&log)
        .map_err(|error| anyhow::anyhow!("could not read {}: {error}", log.display()))?;
    let mut run = parse(&output);
    if let Ok(medians) = std::fs::read_to_string(dir.join(MEDIANS)) {
        for line in medians.lines() {
            let Some(rest) = line.strip_prefix(MEDIAN_PREFIX) else {
                continue;
            };
            let words: Vec<&str> = rest.split_whitespace().collect();
            if let [id, low, median, high] = words[..]
                && let (Ok(low), Ok(estimate), Ok(high)) =
                    (low.parse(), median.parse(), high.parse())
                && let Some(sample) = run.samples.iter_mut().find(|sample| sample.id == id)
            {
                sample.median = Some(Estimate {
                    low,
                    estimate,
                    high,
                });
            }
        }
    }
    Ok(run)
}

/// Where Criterion writes its estimates: `$CARGO_TARGET_DIR/criterion`, or the workspace's
/// `target/criterion`.
pub fn criterion_dir(root: &Path, environment: &dyn Environment) -> PathBuf {
    environment
        .var("CARGO_TARGET_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .map(|dir| {
            if dir.is_absolute() {
                dir
            } else {
                root.join(dir)
            }
        })
        .unwrap_or_else(|| root.join("target"))
        .join("criterion")
}

/// Writes `fast-mode.md` for `output` into `dir`, with `compare` against an earlier run
/// directory. Returns the report's path and how many ids it covers.
pub fn write_report(
    dir: &Path,
    output: &str,
    criterion_dir: Option<&Path>,
    info: Option<&str>,
    compare: Option<&Path>,
) -> anyhow::Result<(PathBuf, usize)> {
    let mut current = parse(output);
    if let Some(criterion_dir) = criterion_dir {
        apply_medians(&mut current, criterion_dir);
        // Keep the medians beside the raw output, so `--compare` against this run reads them.
        let medians: String = current
            .samples
            .iter()
            .filter_map(|sample| {
                let median = sample.median?;
                Some(format!(
                    "{MEDIAN_PREFIX} {} {} {} {}\n",
                    sample.id, median.low, median.estimate, median.high
                ))
            })
            .collect();
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(MEDIANS), medians)?;
    }
    let earlier = match compare {
        Some(dir) => Some((dir.display().to_string(), read_run_dir(dir)?)),
        None => None,
    };
    let text = render(
        &current,
        info,
        earlier.as_ref().map(|(label, run)| (label.as_str(), run)),
    );
    std::fs::create_dir_all(dir)?;
    let path = dir.join(REPORT);
    std::fs::write(&path, text)?;
    Ok((path, current.samples.len()))
}

/// Runs `perf fast-mode`: the bench into `artifacts-temp/testkit/perf/<name>/`, its report, and
/// the comparison with `compare` when given.
pub fn run(
    root: &Path,
    name: &str,
    compare: Option<&Path>,
    environment: &dyn Environment,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Tally {
    let dir = run_dir(root, name);
    let compare = compare.map(|path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        }
    });
    if let Some(compare) = &compare
        && !compare.join(LOG).is_file()
    {
        let message = format!(
            "perf fast-mode: {} holds no {LOG} to compare with",
            compare.display()
        );
        writeln!(out, "{message}").ok();
        return Tally::failure(message);
    }
    let invocation = bench_invocation(environment.os());
    let Some(program) = environment.find_program(&invocation.program) else {
        return Tally::failure("perf fast-mode: `cargo` is not on PATH".to_string());
    };
    writeln!(out, "perf fast-mode: {}", invocation.command_line()).ok();
    out.flush().ok();
    let log = dir.join(LOG);
    let result = match runner.run(&invocation, &program, root, &log) {
        Ok(result) => result,
        Err(error) => return Tally::failure(format!("perf fast-mode: {error:#}")),
    };
    let mut info = format!(
        "Bench: `{}`\nOS: {} {}\n",
        invocation.command_line(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let mut probe = |label: &str, program: &str, args: &[&str]| -> Option<String> {
        let found = environment.find_program(program)?;
        let invocation = Invocation {
            label: label.to_string(),
            program: program.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            env: Vec::new(),
            output: OutputKind::Listing,
        };
        let log = dir.join(format!("{label}.log"));
        runner
            .run(&invocation, &found, root, &log)
            .ok()
            .filter(|probed| probed.success)
            .map(|probed| probed.output)
    };
    if let Some(commit) = probe("commit", "git", &["rev-parse", "HEAD"]) {
        info.push_str(&format!("Commit: {}\n", commit.trim()));
    }
    // A baseline must name the source it measured: uncommitted changes make the commit alone
    // not enough.
    if let Some(status) = probe("tree", "git", &["status", "--porcelain"]) {
        let changed = status
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        info.push_str(&if changed == 0 {
            "Tree: clean\n".to_string()
        } else {
            format!("Tree: {changed} uncommitted changes (`git status --porcelain`)\n")
        });
    }
    if let Some(machine) = probe("machine", "uname", &["-mnrs"]) {
        info.push_str(&format!("Machine: {}\n", machine.trim()));
    }
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(dir.join(RUN_INFO), &info).ok();

    let mut tally = Tally::default();
    let criterion = criterion_dir(root, environment);
    match write_report(
        &dir,
        &result.output,
        Some(&criterion),
        Some(&info),
        compare.as_deref(),
    ) {
        Ok((path, ids)) => {
            writeln!(
                out,
                "perf fast-mode: {ids} ids, report in {}",
                path.display()
            )
            .ok();
            if ids == 0 {
                tally.add(Tally::failure(format!(
                    "perf fast-mode: the bench reported no id; see {}",
                    log.display()
                )));
            } else {
                tally.passed += ids;
            }
        }
        Err(error) => tally.add(Tally::failure(format!("perf fast-mode: {error:#}"))),
    }
    if !result.success {
        tally.add(Tally::failure(format!(
            "perf fast-mode: the bench failed; see {}",
            log.display()
        )));
    }
    tally
}
