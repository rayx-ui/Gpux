//! The fast-mode report: parsing the `fast_mode` bench's output, the per-scenario tables with
//! their deltas and counters, the per-mechanism ranking, `--compare` between two runs, the
//! `perf fast-mode` command, and lane G's bench plan running the suite.

use gpux_testkit::{
    Environment, Os,
    cli::{self, Command, PerfCommand},
    execute,
    fast_mode::{self, Estimate},
    lanes::Invocation,
    local_gpu,
    run::{RunOutput, Runner},
};
use std::path::{Path, PathBuf};

const CURRENT: &str = include_str!("fixtures/fast_mode_current.txt");
const BASELINE: &str = include_str!("fixtures/fast_mode_baseline.txt");
const TEST_MODE: &str = include_str!("fixtures/fast_mode_test_mode.txt");

fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= expected.abs() * 1e-9 + 1e-9
}

#[test]
fn parses_estimates_reports_and_counters() {
    let run = fast_mode::parse(CURRENT);
    assert_eq!(run.samples.len(), 15);
    assert_eq!(
        run.scenarios(),
        ["still-frame", "table-stream", "scroll-list"]
    );

    let off = run.sample("still-frame", "off").unwrap();
    assert_eq!(off.id, "fast/still-frame/off");
    let Estimate {
        low,
        estimate,
        high,
    } = off.time.unwrap();
    assert!(close(low, 1.1e6) && close(estimate, 1.2e6) && close(high, 1.3e6));
    assert!(close(off.instructions.unwrap(), 331.334e6));
    assert!(close(off.allocations.unwrap(), 71.383e3));
    assert_eq!(off.counter("frames"), 20);
    assert_eq!(off.counter("views_built"), 1240);
    assert_eq!(off.per_frame("views_built"), 62.);
    assert_eq!(off.counter("layer_bypasses.moved"), 0);

    // An id too long for Criterion's column is printed on its own line above `time:`.
    let table = run.sample("table-stream", "views").unwrap();
    assert!(close(table.time.unwrap().estimate, 420e3));

    let views = run.sample("still-frame", "views").unwrap();
    assert_eq!(views.counter("views_reused"), 1200);

    // Units parse as Criterion and the bench report print them.
    assert_eq!(fast_mode::parse_duration("1.5", "µs"), Some(1500.));
    assert_eq!(fast_mode::parse_duration("2", "s"), Some(2e9));
    assert_eq!(fast_mode::parse_duration("2", "furlongs"), None);
    assert_eq!(fast_mode::parse_count("2.5 K allocations"), Some(2500.));
    assert_eq!(fast_mode::parse_count("7 allocations"), Some(7.));
    assert_eq!(
        fast_mode::split_id("fast/scroll-list/layers"),
        Some(("scroll-list", "layers"))
    );
    assert_eq!(fast_mode::split_id("fast/scroll-list/fastest"), None);
    assert_eq!(
        fast_mode::split_id("RandomizedTree/unchanged/tree/any/seed-0"),
        None
    );
}

#[test]
fn renders_tables_deltas_and_the_mechanism_ranking() {
    let run = fast_mode::parse(CURRENT);
    let report = fast_mode::render(&run, Some("Commit: abc123\nMachine: test"), None);
    assert!(report.contains("- Commit: abc123"), "{report}");

    // One table per scenario, a row per mode with its deltas.
    assert!(report.contains("\n## still-frame\n"), "{report}");
    assert!(
        report.contains(
            "| views | 250.00 µs | 200.00 µs – 300.00 µs | -79.2% | -76.2% | 40.00 M | 2.1 K |"
        ),
        "{report}"
    );
    assert!(
        report.contains("| off | 1.200 ms | 1.100 ms – 1.300 ms | +0.0% | – | 331.33 M | 71.4 K |"),
        "{report}"
    );
    // The counters per frame, by mode.
    assert!(
        report.contains("| views_built | 62 | 62 | 1 | 1 | 1 |"),
        "{report}"
    );
    assert!(
        report.contains("| views_reused | 0 | 0 | 60 | 60 | 60 |"),
        "{report}"
    );

    // Each mechanism ranks the scenarios it sped up and flags the ones it slowed down.
    let views = fast_mode::ranking(&run, "views");
    assert_eq!(views[0].scenario, "still-frame");
    assert!(views.iter().all(|benefit| !benefit.regression));
    let splice = fast_mode::ranking(&run, "splice");
    let table = splice
        .iter()
        .find(|benefit| benefit.scenario == "table-stream")
        .unwrap();
    assert!(table.regression && table.delta > 50., "{table:?}");
    let layers = fast_mode::ranking(&run, "layers");
    assert_eq!(layers[0].scenario, "scroll-list");
    assert!(
        report.contains("| view retention (`views`) | 3 | 0 | still-frame (-76.2%) | none |"),
        "{report}"
    );
    assert!(
        report.contains(
            "| splice (`splice`) | 1 | 1 | still-frame (-4.0%) | **table-stream (+52.4%)** |"
        ),
        "{report}"
    );
    assert!(
        report.contains("table-stream | +52.4% ⚠ regression |"),
        "{report}"
    );
    assert_eq!(fast_mode::ranking(&run, "off"), Vec::new());
}

#[test]
fn a_run_without_timings_reports_its_counters() {
    let run = fast_mode::parse(TEST_MODE);
    assert_eq!(run.samples.len(), 2);
    assert!(run.samples.iter().all(|sample| sample.time.is_none()));
    let report = fast_mode::render(&run, None, None);
    assert!(report.contains("timed nothing"), "{report}");
    assert!(
        report.contains("| off | – | – | – | – | 331.33 M | 71.4 K |"),
        "{report}"
    );
    assert!(report.contains("| views_built | 62 | 62 |"), "{report}");
    let empty = fast_mode::render(&fast_mode::parse("nothing here"), None, None);
    assert!(empty.contains("no `fast/<scenario>/<mode>` id"), "{empty}");
}

#[test]
fn compare_diffs_two_runs() {
    let baseline = fast_mode::parse(BASELINE);
    let current = fast_mode::parse(CURRENT);
    let section = fast_mode::compare_section("baseline-s01", &baseline, &current);
    assert!(
        section.contains("## Compared with baseline-s01"),
        "{section}"
    );
    assert!(
        section
            .contains("| fast/still-frame/off | 1.000 ms | 1.200 ms | +20.0% ⚠ | -5.3% | -0.9% |"),
        "{section}"
    );
    assert!(
        section.contains("| fast/table-stream/off | 1.050 ms | 950.00 µs | -9.5% |"),
        "{section}"
    );
    assert!(
        section.contains("| fast/still-frame/views | – | | new | | |"),
        "{section}"
    );
    assert!(
        section.contains("| fast/chat-scroll/off | | – | gone | | |"),
        "{section}"
    );

    // A change within the intervals is not flagged.
    let reversed = fast_mode::compare_section("current", &current, &baseline);
    assert!(
        reversed.contains("| fast/scroll-list/off | 2.100 ms | 2.100 ms | +0.0% |"),
        "{reversed}"
    );

    let report = fast_mode::render(&current, None, Some(("baseline-s01", &baseline)));
    assert!(report.contains("## Compared with baseline-s01"), "{report}");
}

#[test]
fn lane_g_benches_the_fast_mode_suite() {
    assert!(local_gpu::BENCHES.contains(&fast_mode::BENCH));
    let plan = local_gpu::bench_plan(Os::Linux);
    let args = &plan.invocations[0].args;
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--bench" && pair[1] == "fast_mode"),
        "{args:?}"
    );
    assert!(plan.tests.contains(&"bench::fast_mode".to_string()));

    let alone = fast_mode::bench_invocation(Os::Linux);
    assert_eq!(
        fast_mode::bench_invocation(Os::MacOs).command_line(),
        "cargo bench -p gpux-platform --features bench-support --bench fast_mode"
    );
    assert!(
        alone
            .env
            .iter()
            .any(|(name, value)| name == "GPUX_WGPU_ADAPTER" && value == "hardware")
    );
    assert!(fast_mode::bench_invocation(Os::MacOs).env.is_empty());
}

#[test]
fn the_command_line_parses_perf_fast_mode() {
    let parse = |line: &str| cli::parse(line.split_whitespace().map(str::to_string));
    assert_eq!(
        parse("perf fast-mode --name baseline-s01 --compare artifacts-temp/testkit/perf/old"),
        Ok(Command::Perf(PerfCommand::FastMode {
            name: "baseline-s01".into(),
            compare: Some(PathBuf::from("artifacts-temp/testkit/perf/old")),
        }))
    );
    let Ok(Command::Perf(PerfCommand::FastMode { name, compare })) = parse("perf fast-mode") else {
        panic!("perf fast-mode does not parse");
    };
    assert!(
        name.starts_with("fast-mode-") && compare.is_none(),
        "{name}"
    );
    assert!(parse("perf").is_err());
    assert!(parse("perf slow-mode").is_err());
    assert!(parse("perf fast-mode --name a/b").is_err());
    assert!(parse("perf fast-mode --name x --name y").is_err());
    assert!(parse("perf fast-mode --compare").is_err());
    assert!(cli::USAGE.contains("perf fast-mode"));
}

struct FakeEnvironment {
    root: PathBuf,
}

impl Environment for FakeEnvironment {
    fn var(&self, _: &str) -> Option<String> {
        None
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        (name == "cargo" || name == "git").then(|| PathBuf::from(format!("/fake/bin/{name}")))
    }

    fn os(&self) -> Os {
        Os::MacOs
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
}

/// Answers the bench with `bench`, `git` with a commit, and writes what it answers to the log.
struct FakeRunner {
    bench: &'static str,
    ran: Vec<String>,
}

impl Runner for FakeRunner {
    fn run(
        &mut self,
        invocation: &Invocation,
        _: &Path,
        _: &Path,
        log: &Path,
    ) -> anyhow::Result<RunOutput> {
        self.ran.push(invocation.command_line());
        let output = match (invocation.program.as_str(), invocation.args[0].as_str()) {
            ("cargo", _) => self.bench.to_string(),
            ("git", "rev-parse") => "0123456789abcdef\n".to_string(),
            ("git", "status") => " M crates/gpui/src/window.rs\n?? new.rs\n".to_string(),
            (other, _) => panic!("unexpected {other}"),
        };
        std::fs::create_dir_all(log.parent().unwrap())?;
        std::fs::write(log, &output)?;
        Ok(RunOutput {
            success: true,
            code: Some(0),
            output,
        })
    }
}

#[test]
fn perf_fast_mode_writes_a_run_directory_and_compares() {
    let root = tempfile::tempdir().unwrap();
    let environment = FakeEnvironment {
        root: root.path().to_path_buf(),
    };
    let mut runner = FakeRunner {
        bench: BASELINE,
        ran: Vec::new(),
    };
    let mut out = Vec::new();
    let code = execute(
        "perf fast-mode --name baseline-s01"
            .split_whitespace()
            .map(str::to_string),
        &environment,
        &mut runner,
        &mut out,
    );
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("gpux-testkit: 4 passed, 0 failed, 0 skipped"),
        "{text}"
    );
    let baseline = fast_mode::run_dir(root.path(), "baseline-s01");
    assert!(baseline.join(fast_mode::LOG).is_file());
    let info = std::fs::read_to_string(baseline.join(fast_mode::RUN_INFO)).unwrap();
    assert!(info.contains("Commit: 0123456789abcdef"), "{info}");
    assert!(info.contains("Tree: 2 uncommitted changes"), "{info}");
    let report = std::fs::read_to_string(baseline.join(fast_mode::REPORT)).unwrap();
    assert!(report.contains("- Commit: 0123456789abcdef"), "{report}");

    runner.bench = CURRENT;
    let mut out = Vec::new();
    let code = execute(
        "perf fast-mode --name now --compare artifacts-temp/testkit/perf/baseline-s01"
            .split_whitespace()
            .map(str::to_string),
        &environment,
        &mut runner,
        &mut out,
    );
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{text}");
    let report =
        std::fs::read_to_string(fast_mode::run_dir(root.path(), "now").join(fast_mode::REPORT))
            .unwrap();
    assert!(report.contains("## Compared with"), "{report}");
    assert!(
        report.contains("| fast/still-frame/off | 1.000 ms | 1.200 ms | +20.0% ⚠"),
        "{report}"
    );

    // Comparing with a directory that holds no run fails before benching.
    let ran = runner.ran.len();
    let mut out = Vec::new();
    let code = execute(
        "perf fast-mode --compare artifacts-temp/testkit/perf/missing"
            .split_whitespace()
            .map(str::to_string),
        &environment,
        &mut runner,
        &mut out,
    );
    assert_ne!(code, 0);
    assert_eq!(runner.ran.len(), ran);
}

#[test]
fn medians_come_from_criterion_estimates() {
    let estimates = include_str!("fixtures/criterion/fast/still-frame/off/new/estimates.json");
    let median = fast_mode::parse_median(estimates).unwrap();
    assert!(close(median.estimate, 20863576.333333336), "{median:?}");
    assert!(
        close(median.low, 20779708.5) && close(median.high, 20941701.5),
        "{median:?}"
    );
    assert_eq!(fast_mode::parse_median("{\"mean\":{}}"), None);

    let mut run = fast_mode::parse(CURRENT);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/criterion");
    fast_mode::apply_medians(&mut run, &fixtures);
    let off = run.sample("still-frame", "off").unwrap();
    assert_eq!(off.median, Some(median));
    assert_eq!(off.timing(), Some(median));
    // Ids without estimates keep Criterion's point estimate.
    let layout = run.sample("still-frame", "layout").unwrap();
    assert_eq!(layout.median, None);
    assert_eq!(layout.timing(), layout.time);

    let report = fast_mode::render(&run, None, None);
    assert!(report.contains("| Mode | Median | Interval |"), "{report}");
    assert!(
        report.contains("| off | 20.864 ms | 20.780 ms – 20.942 ms |"),
        "{report}"
    );
    let plain = fast_mode::render(&fast_mode::parse(CURRENT), None, None);
    assert!(plain.contains("| Mode | Estimate | Interval |"), "{plain}");
    assert!(
        plain.contains("point estimates rather than medians"),
        "{plain}"
    );

    // A run directory keeps its medians for later comparisons.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(fast_mode::LOG), CURRENT).unwrap();
    fast_mode::write_report(dir.path(), CURRENT, Some(&fixtures), None, None).unwrap();
    let reread = fast_mode::read_run_dir(dir.path()).unwrap();
    assert_eq!(
        reread.sample("still-frame", "off").unwrap().median,
        Some(median)
    );
}
