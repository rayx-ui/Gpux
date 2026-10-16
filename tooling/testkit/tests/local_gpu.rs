//! Lane G, the local real GPU: it refuses CI, runs lanes C and D on the hardware adapter with
//! probes only, runs the `#[gpux::bench]` suites, and summarizes timing.

use gpux_testkit::{
    Environment, Os,
    cli::Selection,
    execute,
    lanes::{Invocation, LaneId, OutputKind},
    local_gpu::{self, BenchResult, Part},
    results::Tally,
    run::{RunOutput, Runner},
    summary::EXIT_FAILED,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

struct FakeEnvironment {
    vars: HashMap<String, String>,
    os: Os,
    root: PathBuf,
}

impl FakeEnvironment {
    fn new(root: &Path) -> Self {
        Self {
            vars: HashMap::new(),
            os: Os::Windows,
            root: root.to_path_buf(),
        }
    }
}

impl Environment for FakeEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        (name == "cargo").then(|| PathBuf::from("/fake/bin/cargo"))
    }

    fn os(&self) -> Os {
        self.os
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
}

/// Answers each invocation with a canned output, recording what ran.
#[derive(Default)]
struct FakeRunner {
    outputs: Vec<RunOutput>,
    ran: Vec<String>,
}

impl Runner for FakeRunner {
    fn run(
        &mut self,
        invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        self.ran.push(invocation.command_line());
        Ok(self.outputs.remove(0))
    }
}

fn run(line: &str, environment: &FakeEnvironment, runner: &mut FakeRunner) -> (i32, String) {
    let mut out = Vec::new();
    let code = execute(
        line.split_whitespace().map(str::to_string),
        environment,
        runner,
        &mut out,
    );
    (code, String::from_utf8(out).unwrap())
}

fn env_of(invocation: &Invocation, name: &str) -> Option<String> {
    invocation
        .env
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
}

#[test]
fn lane_g_refuses_to_run_under_ci() {
    let root = tempfile::tempdir().unwrap();
    let mut environment = FakeEnvironment::new(root.path());
    environment.vars.insert("CI".into(), "true".into());
    let mut runner = FakeRunner::default();
    let (code, text) = run("lane g", &environment, &mut runner);
    assert_eq!(code, EXIT_FAILED);
    assert!(
        text.contains("refuses to run under CI (`CI=true`)"),
        "{text}"
    );
    assert!(runner.ran.is_empty(), "{:?}", runner.ran);

    for value in ["", "0", "false", "FALSE"] {
        environment.vars.insert("CI".into(), value.into());
        assert_eq!(local_gpu::refusal(&environment), None, "CI={value}");
    }
    environment.vars.remove("CI");
    assert_eq!(local_gpu::refusal(&environment), None);
}

#[test]
fn lanes_c_and_d_run_on_the_hardware_adapter_with_probes_only() {
    let plans = local_gpu::hardware_plans(&Selection::default(), Os::Windows).unwrap();
    let [c, d] = &plans[..] else {
        panic!("{plans:?}")
    };
    assert_eq!((c.lane, d.lane), (LaneId::G, LaneId::G));

    let render = &c.invocations[0];
    assert_eq!(render.label, "hardware-lane-c");
    assert!(render.args.iter().any(|arg| arg == "lane_c"), "{render:?}");
    assert_eq!(
        env_of(render, "GPUX_WGPU_ADAPTER").as_deref(),
        Some("hardware")
    );
    assert_eq!(
        env_of(render, "GPUX_EXPECT_BACKEND").as_deref(),
        Some("hardware")
    );
    assert_eq!(env_of(render, "GPUX_PROBES_ONLY").as_deref(), Some("1"));
    // The listing compiles the same test target with the same environment.
    assert_eq!(
        env_of(&c.listings[0].invocation, "GPUX_PROBES_ONLY").as_deref(),
        Some("1")
    );

    let windows = &d.invocations[0];
    assert_eq!(windows.label, "hardware-lane-d");
    assert_eq!(windows.output, OutputKind::ResultLines);
    assert_eq!(
        env_of(windows, "GPUX_EXPECT_BACKEND").as_deref(),
        Some("hardware")
    );
    // The backend check must report, so a host that skips it fails.
    assert_eq!(d.tests[0], "desktop_host::desktop-backend");
    assert!(d.tests.len() > 1);

    // macOS renders lane C through Metal on the system GPU, so it expects Metal.
    let plans = local_gpu::hardware_plans(&Selection::default(), Os::MacOs).unwrap();
    let render = &plans[0].invocations[0];
    assert_eq!(env_of(render, "GPUX_WGPU_ADAPTER"), None);
    assert_eq!(
        env_of(render, "GPUX_EXPECT_BACKEND").as_deref(),
        Some("metal")
    );
    assert_eq!(env_of(render, "GPUX_PROBES_ONLY").as_deref(), Some("1"));
}

#[test]
fn the_benchmarks_are_the_gpux_bench_suites_of_gpux_platform() {
    let plan = local_gpu::bench_plan(Os::Linux);
    let bench = &plan.invocations[0];
    assert_eq!(bench.output, OutputKind::Criterion);
    assert_eq!(
        bench.command_line(),
        "GPUX_WGPU_ADAPTER=hardware cargo bench -p gpux-platform --features bench-support \
         --bench randomized_element_tree --bench inspector_render --bench fast_mode"
    );
    assert_eq!(
        plan.tests,
        [
            "bench::randomized_element_tree",
            "bench::inspector_render",
            "bench::fast_mode"
        ]
    );
}

const CRITERION: &str = "\
Benchmarking tree/wide/full_refresh\r
Benchmarking tree/wide/full_refresh: Warming up for 500.00 ms\r
Benchmarking tree/wide/full_refresh: Collecting 20 samples in estimated 1.0034 s (210 iterations)\r
Benchmarking tree/wide/full_refresh: Analyzing
tree/wide/full_refresh  time:   [4.7520 ms 4.7798 ms 4.8126 ms]
                        change: [-1.2041% +0.3127% +1.9012%] (p = 0.72 > 0.05)
                        No change in performance detected.
Benchmarking RandomizedTree/full refresh/tree/any/seed-0: Analyzing
RandomizedTree/full refresh/tree/any/seed-0
                        time:   [812.40 µs 815.02 µs 818.77 µs]
Found 2 outliers among 20 measurements (10.00%)
GPUI bench report (all observed iterations): RandomizedTree/full refresh/tree/any/seed-0
  window draw:
    p95: 1.409ms
";

#[test]
fn criterion_results_are_read_with_short_and_long_ids() {
    let results = local_gpu::bench_results(CRITERION);
    assert_eq!(
        results,
        [
            BenchResult {
                id: "tree/wide/full_refresh".into(),
                low: "4.7520 ms".into(),
                estimate: "4.7798 ms".into(),
                high: "4.8126 ms".into(),
            },
            BenchResult {
                id: "RandomizedTree/full refresh/tree/any/seed-0".into(),
                low: "812.40 µs".into(),
                estimate: "815.02 µs".into(),
                high: "818.77 µs".into(),
            },
        ]
    );
    assert_eq!(local_gpu::tally_benches(CRITERION).passed, 2);
}

#[test]
fn the_summary_lists_parts_frame_timing_and_benchmarks() {
    let timings = local_gpu::timing_lines(
        "  ok 1 [local-gpu] › specs/timing.spec.ts:40:1 › @timing scrolling\n\
         gpux-timing: list-uniform scroll frame work p95 0.86 ms, max 1.61 ms over 239 frames\n",
    );
    assert_eq!(
        timings,
        ["list-uniform scroll frame work p95 0.86 ms, max 1.61 ms over 239 frames"]
    );
    let parts = [Part {
        name: "lane B in `local-gpu`".into(),
        tally: Tally {
            passed: 112,
            ..Tally::default()
        },
        duration: Duration::from_millis(95_300),
    }];
    let text = local_gpu::summary(
        &parts,
        Some("hardware (NVIDIA GeForce RTX 4070)"),
        &timings,
        &local_gpu::bench_results(CRITERION),
    );
    assert!(
        text.contains("GPU: hardware (NVIDIA GeForce RTX 4070)"),
        "{text}"
    );
    assert!(
        text.contains("| lane B in `local-gpu` | 112 | 0 | 0 | 95.3 s |"),
        "{text}"
    );
    assert!(
        text.contains("- list-uniform scroll frame work p95 0.86 ms"),
        "{text}"
    );
    assert!(
        text.contains("| tree/wide/full_refresh | 4.7798 ms | 4.7520 ms – 4.8126 ms |"),
        "{text}"
    );
}

#[test]
fn listing_lane_g_shows_the_benchmarks_and_the_hardware_runs() {
    let root = tempfile::tempdir().unwrap();
    let environment = FakeEnvironment::new(root.path());
    let mut runner = FakeRunner {
        outputs: vec![
            RunOutput {
                success: true,
                code: Some(0),
                output: "at_1x::paint_borders: test\nat_2x::paint_borders: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "probes_only_skips_goldens_and_keeps_the_probes: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "paint_borders_matches_with_fast_mode: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "text_rows_scroll_from_tiles: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "transformed_bordered_content_is_equal_to_the_reference: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "a_list_shows_each_frames_hover: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "debug_assertions_are_on_in_the_lane_c_test_build: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "equal_images_pass: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "full_mode_compares_every_frame_of_the_script: test\n".into(),
            },
            RunOutput {
                success: true,
                code: Some(0),
                output: "a_scrolled_div_carries_its_content_s_nodes: test\n".into(),
            },
        ],
        ..FakeRunner::default()
    };
    let (_, text) = run("lane g --list", &environment, &mut runner);
    // Without Playwright in tools-node, the browser part reports what to install.
    assert!(text.contains("Playwright is not installed"), "{text}");
    assert!(text.contains("  lane_c::at_1x::paint_borders"), "{text}");
    assert!(
        text.contains("  lane_g::probes_only_skips_goldens_and_keeps_the_probes"),
        "{text}"
    );
    assert!(text.contains("  desktop_host::desktop-backend"), "{text}");
    assert!(
        text.contains(
            "cargo bench -p gpux-platform --features bench-support --bench randomized_element_tree"
        ),
        "{text}"
    );
    assert!(text.contains("  bench::randomized_element_tree"), "{text}");
    assert!(
        text.contains("  lane_c_scroll_layers::text_rows_scroll_from_tiles"),
        "{text}"
    );
    assert!(
        text.contains("  lane_c_sampling::full_mode_compares_every_frame_of_the_script"),
        "{text}"
    );
    assert_eq!(runner.ran.len(), 10, "{:?}", runner.ran);
    assert!(
        runner
            .ran
            .iter()
            .all(|command| command.contains("GPUX_PROBES_ONLY=1")),
        "{:?}",
        runner.ran
    );
}
