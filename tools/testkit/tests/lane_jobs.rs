//! `gpux-testkit lane c --jobs <n>`: the partition of lane C's test groups by weight, the
//! single-process plan that `--jobs 1` keeps, one block per process in a fixed order, and failure
//! propagation into the lane's one summary.

use gpux_testkit::{
    Environment, Os,
    cli::{Command, Selection, parse},
    execute,
    lanes::{Invocation, LaneId, partition, plan, plan_with_jobs, weight},
    run::{RunJob, RunOutput, Runner},
    summary::{EXIT_FAILED, EXIT_USAGE, Summary},
    timings::TimedLine,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn units(weights: &[(&str, u64)]) -> Vec<(String, u64)> {
    weights
        .iter()
        .map(|(name, weight)| (name.to_string(), *weight))
        .collect()
}

#[test]
fn the_heaviest_units_go_to_separate_processes_and_the_rest_to_the_lightest() {
    let split = partition(
        &units(&[("a", 10), ("b", 7), ("c", 5), ("d", 4), ("e", 1)]),
        2,
    );
    assert_eq!(split, vec![vec!["a", "d"], vec!["b", "c", "e"]]);
    // Equal weights go to the lowest worker first, so the result is the same on every run.
    let equal = partition(&units(&[("a", 3), ("b", 3), ("c", 3), ("d", 3)]), 2);
    assert_eq!(equal, vec![vec!["a", "c"], vec!["b", "d"]]);
}

#[test]
fn a_partition_never_has_an_empty_process_or_more_than_asked() {
    let all = units(&[("a", 5), ("b", 4), ("c", 3)]);
    assert_eq!(partition(&all, 8).len(), 3);
    assert_eq!(partition(&all, 1), vec![vec!["a", "b", "c"]]);
    assert_eq!(partition(&all, 2).len(), 2);
    assert!(partition(&all, 2).iter().all(|worker| !worker.is_empty()));
}

#[test]
fn every_lane_c_unit_has_a_weight() {
    let one = plan(LaneId::C, &Selection::default()).unwrap();
    let mut groups: Vec<String> = one.invocations[0]
        .args
        .windows(2)
        .filter(|pair| pair[0] == "--test")
        .map(|pair| pair[1].clone())
        .collect();
    groups.sort();
    assert!(groups.len() >= 8, "{groups:?}");
    for group in groups {
        assert!(
            weight(&group).is_some(),
            "`{group}` has no weight in LANE_C_WEIGHTS"
        );
    }
}

fn test_groups(invocation: &Invocation) -> Vec<&str> {
    invocation
        .args
        .windows(2)
        .filter(|pair| pair[0] == "--test")
        .map(|pair| pair[1].as_str())
        .collect()
}

#[test]
fn the_slowest_groups_do_not_share_a_process_while_there_are_processes_to_spare() {
    let three = plan_with_jobs(LaneId::C, &Selection::default(), 3).unwrap();
    let slow = [
        "lane_c_layer_scales",
        "lane_c_scroll_layers",
        "lane_c_list_hover",
    ];
    let home: Vec<usize> = slow
        .iter()
        .map(|group| {
            three
                .invocations
                .iter()
                .position(|invocation| test_groups(invocation).contains(group))
                .unwrap()
        })
        .collect();
    assert_eq!(home.len(), 3);
    assert!(
        home[0] != home[1] && home[1] != home[2] && home[0] != home[2],
        "{home:?}"
    );
    // With two processes the heaviest group is alone: the others weigh less than it does.
    let two = plan_with_jobs(LaneId::C, &Selection::default(), 2).unwrap();
    assert_eq!(test_groups(&two.invocations[1]), ["lane_c_layer_scales"]);
}

#[test]
fn every_group_runs_once_in_a_process_that_is_serial_inside() {
    let plan = plan_with_jobs(LaneId::C, &Selection::default(), 4).unwrap();
    let mut groups: Vec<&str> = plan.invocations.iter().flat_map(test_groups).collect();
    let single = self::plan(LaneId::C, &Selection::default()).unwrap();
    let mut expected = test_groups(&single.invocations[0]);
    groups.sort();
    expected.sort();
    assert_eq!(groups, expected);
    for (index, invocation) in plan.invocations.iter().enumerate() {
        assert!(
            invocation
                .command_line()
                .ends_with("--no-fail-fast -- --test-threads=1"),
            "{}",
            invocation.command_line()
        );
        assert_eq!(invocation.label, format!("lane-c-job-{}", index + 1));
    }
    // One build runs before the workers.
    let build = plan.prebuild.as_ref().unwrap();
    assert!(build.args.contains(&"--no-run".to_string()));
    assert_eq!(plan.tests, single.tests);
    assert_eq!(plan.listings, single.listings);
}

#[test]
fn one_job_is_exactly_the_plan_the_lane_runs_today() {
    let today = plan(LaneId::C, &Selection::default()).unwrap();
    let one = plan_with_jobs(LaneId::C, &Selection::default(), 1).unwrap();
    assert_eq!(today, one);
    assert!(one.prebuild.is_none());
    assert_eq!(one.invocations.len(), 1);
}

#[test]
fn a_selection_that_fits_in_fewer_processes_uses_fewer() {
    let scene = gpux_tests_conformance::CATALOG
        .iter()
        .find(|scene| scene.runs_in(gpux_tests_conformance::Lane::C))
        .unwrap();
    let narrowed = Selection {
        scene: Some(scene.id.to_string()),
        grep: None,
    };
    // A narrowed lane runs the scene suite alone: one process, as `--jobs 1` plans it.
    let plan = plan_with_jobs(LaneId::C, &narrowed, 4).unwrap();
    assert_eq!(plan.invocations.len(), 1);
    assert!(plan.prebuild.is_none());
    assert_eq!(plan, self::plan(LaneId::C, &narrowed).unwrap());
    let many = plan_with_jobs(LaneId::C, &Selection::default(), 64).unwrap();
    assert!(many.invocations.len() <= 11);
    assert!(many.invocations.len() > 3);
}

#[test]
fn jobs_is_parsed_and_must_be_a_positive_number() {
    let lane = |line: &str| parse(line.split_whitespace().map(str::to_string));
    let Command::Lane(args) = lane("lane c --jobs 3 --full --grep layout").unwrap() else {
        panic!("a lane command");
    };
    assert_eq!((args.jobs, args.full), (3, true));
    assert_eq!(args.selection.grep.as_deref(), Some("layout"));
    let Command::Lane(args) = lane("lane c").unwrap() else {
        panic!("a lane command");
    };
    assert_eq!(args.jobs, 1);
    for bad in ["lane c --jobs", "lane c --jobs 0", "lane c --jobs many"] {
        assert!(lane(bad).is_err(), "{bad}");
    }
}

struct Host {
    vars: HashMap<String, String>,
    root: PathBuf,
}

impl Environment for Host {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
    fn find_program(&self, name: &str) -> Option<PathBuf> {
        (name == "cargo").then(|| PathBuf::from("/fake/bin/cargo"))
    }
    fn os(&self) -> Os {
        Os::MacOs
    }
    fn workspace_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
}

/// Answers the build with `build` and the workers with `workers`, in order.
struct Workers {
    build: RunOutput,
    workers: Vec<RunOutput>,
    seen: Vec<Invocation>,
}

fn output(success: bool, text: &str) -> RunOutput {
    RunOutput {
        success,
        code: Some(if success { 0 } else { 101 }),
        output: text.to_string(),
    }
}

fn passed(count: usize) -> RunOutput {
    output(
        true,
        &format!(
            "test result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
        ),
    )
}

impl Runner for Workers {
    fn run(
        &mut self,
        invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        self.seen.push(invocation.clone());
        Ok(self.build.clone())
    }

    fn run_concurrently(
        &mut self,
        jobs: &[RunJob<'_>],
    ) -> Vec<(anyhow::Result<RunOutput>, Vec<TimedLine>)> {
        jobs.iter()
            .enumerate()
            .map(|(index, job)| {
                self.seen.push(job.invocation.clone());
                (Ok(self.workers[index].clone()), Vec::new())
            })
            .collect()
    }
}

fn run(line: &str, mut runner: Workers) -> (i32, Vec<String>, Workers) {
    let root = tempfile::tempdir().unwrap();
    let host = Host {
        vars: HashMap::new(),
        root: root.path().to_path_buf(),
    };
    let mut out = Vec::new();
    let code = execute(
        line.split_whitespace().map(str::to_string),
        &host,
        &mut runner,
        &mut out,
    );
    let text = String::from_utf8(out).unwrap();
    (code, text.lines().map(str::to_string).collect(), runner)
}

fn workers(outputs: Vec<RunOutput>) -> Workers {
    Workers {
        build: output(true, ""),
        workers: outputs,
        seen: Vec::new(),
    }
}

#[test]
fn each_process_prints_one_block_in_order_and_the_lane_has_one_summary() {
    let (code, lines, runner) = run(
        "lane c --jobs 3",
        workers(vec![passed(12), passed(30), passed(5)]),
    );
    assert_eq!(code, 0, "{lines:?}");
    let blocks: Vec<usize> = (1..=3)
        .map(|index| {
            lines
                .iter()
                .position(|line| line.starts_with(&format!("lane c: ---- job {index}/3")))
                .unwrap()
        })
        .collect();
    assert!(blocks[0] < blocks[1] && blocks[1] < blocks[2], "{lines:?}");
    // A block holds its own process's output.
    assert!(lines[blocks[1] + 1].contains("30 passed"), "{lines:?}");
    let summaries: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("gpux-testkit:"))
        .collect();
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        Summary::parse(summaries[0]),
        Some(Summary {
            passed: 47,
            failed: 0,
            skipped: 0
        })
    );
    // The concurrency level is reported, the build ran before the workers.
    assert!(
        lines
            .iter()
            .any(|line| line.contains("3 concurrent processes")),
        "{lines:?}"
    );
    assert_eq!(runner.seen[0].label, "lane-c-build");
    assert_eq!(runner.seen.len(), 4);
}

#[test]
fn a_failing_process_fails_the_lane_and_names_its_test() {
    let failing = output(
        false,
        "test at_1x::tooltip ... FAILED\ntest result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n",
    );
    let (code, lines, _) = run("lane c --jobs 2", workers(vec![passed(10), failing]));
    assert_eq!(code, EXIT_FAILED, "{lines:?}");
    assert!(
        lines.iter().any(|line| line.trim() == "at_1x::tooltip"),
        "{lines:?}"
    );
    assert_eq!(
        Summary::parse(lines.last().unwrap()),
        Some(Summary {
            passed: 14,
            failed: 1,
            skipped: 0
        })
    );
}

#[test]
fn a_process_that_dies_without_results_fails_the_lane() {
    let crashed = output(false, "error: process didn't exit successfully\n");
    let (code, lines, _) = run("lane c --jobs 2", workers(vec![passed(10), crashed]));
    assert_eq!(code, EXIT_FAILED, "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("exited with code 101 before reporting its results")),
        "{lines:?}"
    );
}

#[test]
fn a_run_in_which_no_test_ran_fails() {
    let empty = || output(true, "");
    let (code, lines, _) = run("lane c --jobs 2", workers(vec![empty(), empty()]));
    assert_eq!(code, EXIT_FAILED, "{lines:?}");
    assert_eq!(
        Summary::parse(lines.last().unwrap()),
        Some(Summary::default())
    );
}

#[test]
fn a_failed_build_stops_before_any_worker_starts() {
    let mut runner = workers(vec![passed(1)]);
    runner.build = output(false, "error[E0432]: unresolved import\n");
    let (code, lines, runner) = run("lane c --jobs 2", runner);
    assert_eq!(code, EXIT_FAILED, "{lines:?}");
    assert_eq!(runner.seen.len(), 1, "only the build ran");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("before the workers started")),
        "{lines:?}"
    );
    assert_ne!(code, EXIT_USAGE);
}

#[test]
fn full_reaches_every_process_and_the_mode_is_reported_with_the_level() {
    let (code, lines, runner) = run(
        "lane c --jobs 2 --full",
        workers(vec![passed(3), passed(4)]),
    );
    assert_eq!(code, 0, "{lines:?}");
    let full = ("GPUX_LANE_C_FULL".to_string(), "1".to_string());
    let seen_workers: Vec<&Invocation> = runner.seen.iter().skip(1).collect();
    assert_eq!(seen_workers.len(), 2);
    assert!(
        seen_workers
            .iter()
            .all(|invocation| invocation.env.contains(&full))
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "lane c: full scroll-layer suites")
    );
}

#[test]
fn a_cold_build_before_the_workers_is_echoed_like_any_lane_run() {
    let plan = plan_with_jobs(LaneId::C, &Selection::default(), 2).unwrap();
    let build = plan.prebuild.unwrap();
    assert_ne!(
        build.output,
        gpux_testkit::lanes::OutputKind::Listing,
        "a listing's output is not echoed, so a failing or slow build would be silent"
    );
}

#[test]
fn jobs_one_through_the_lane_command_runs_one_process_and_no_build() {
    let (code, lines, runner) = run("lane c --jobs 1", workers(vec![]));
    // The fake answers the one process with the build's empty output: nothing ran.
    assert_eq!(code, EXIT_FAILED, "{lines:?}");
    assert_eq!(runner.seen.len(), 1);
    assert_eq!(
        runner.seen[0].command_line(),
        "cargo test -p gpux-tests-conformance --test lane_c --test lane_g --test lane_c_fast_parity \
         --test lane_c_scroll_layers --test lane_c_layer_scales --test lane_c_list_hover \
         --test lane_c_profile --test lane_c_compare --test lane_c_sampling --test lane_c_a11y_layers \
         --test lane_c_paint_contract_scales --no-fail-fast -- \
         --test-threads=1"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("concurrent processes"))
    );
}

#[test]
fn jobs_that_cannot_take_effect_say_so() {
    let (_, lines, _) = run("lane c --jobs 4 --grep layout", workers(vec![passed(1)]));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("--jobs 4 applies to the full lane c only")),
        "{lines:?}"
    );
    let (_, lines, _) = run("lane a --jobs 2", workers(vec![passed(1)]));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("lane a: --jobs 2 applies to the full lane c only")),
        "{lines:?}"
    );
    let (_, lines, _) = run("lane c --jobs 2", workers(vec![passed(1), passed(1)]));
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("applies to the full lane c only"))
    );
}

#[test]
fn the_process_runner_runs_jobs_side_by_side_and_returns_their_results_in_order() {
    let cargo = PathBuf::from(std::env::var("CARGO").expect("cargo sets CARGO for its tests"));
    let job = |label: &str, arg: &str| Invocation {
        label: label.to_string(),
        program: "cargo".to_string(),
        args: vec![arg.to_string()],
        env: Vec::new(),
        output: gpux_testkit::lanes::OutputKind::Libtest,
    };
    let version = job("version", "--version");
    let list = job("list", "--list");
    let missing = job("missing", "--version");
    let logs = tempfile::tempdir().unwrap();
    let nowhere = PathBuf::from("/definitely/not/a/program");
    let jobs: Vec<RunJob<'_>> = [(&version, &cargo), (&list, &cargo), (&missing, &nowhere)]
        .into_iter()
        .map(|(invocation, program)| RunJob {
            invocation,
            program,
            workdir: logs.path(),
            log: logs.path().join(format!("{}.log", invocation.label)),
        })
        .collect();
    let mut runner = gpux_testkit::run::ProcessRunner::default();
    let results = runner.run_concurrently(&jobs);
    assert_eq!(results.len(), 3);
    let first = results[0].0.as_ref().unwrap();
    assert!(
        first.success && first.output.starts_with("cargo "),
        "{first:?}"
    );
    let second = results[1].0.as_ref().unwrap();
    assert!(
        second.success && second.output.contains("build"),
        "{second:?}"
    );
    assert!(
        results[2].0.is_err(),
        "a program that cannot start fails its own slot only"
    );
    // Each job's own timed output comes back with it, and each keeps its own log.
    assert!(
        results[0]
            .1
            .iter()
            .any(|timed| timed.line.starts_with("cargo "))
    );
    assert!(logs.path().join("version.log").is_file() && logs.path().join("list.log").is_file());
}
