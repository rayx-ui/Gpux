//! Per-test durations of a libtest stream: deltas between completion lines, the slowest-first
//! table with shares, the empty-run message and per-process blocks.

use gpux_testkit::{
    Environment, Os, execute,
    lanes::Invocation,
    run::{RunOutput, Runner},
    timings::{ProcessTimeline, TimedLine, durations, report},
};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn stream(lines: &[(u64, &str)]) -> Vec<TimedLine> {
    lines
        .iter()
        .map(|(millis, line)| TimedLine {
            at: Duration::from_millis(*millis),
            line: (*line).to_string(),
        })
        .collect()
}

fn lane_c_stream() -> Vec<TimedLine> {
    stream(&[
        (
            50,
            "   Running tests/lane_c.rs (target/debug/deps/lane_c-abc)",
        ),
        (900, "running 3 tests"),
        (1_000, "test at_1x::layout_flex ... ok"),
        (1_400, "test at_1x::tooltip ... ok"),
        (1_400, "test at_2x::ignored_one ... ignored"),
        (2_400, "test at_2x::list_variable ... FAILED"),
        (
            2_400,
            "test result: FAILED. 2 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out",
        ),
        (
            3_000,
            "   Running tests/lane_c_scroll_layers.rs (target/debug/deps/lane_c_scroll_layers-def)",
        ),
        (9_000, "running 1 test"),
        (19_000, "test layers_match_reference ... ok"),
    ])
}

#[test]
fn durations_are_the_deltas_between_completion_lines() {
    let timings = durations(&lane_c_stream());
    let seen: Vec<(String, u128, bool)> = timings
        .iter()
        .map(|timing| {
            (
                format!("{}::{}", timing.target, timing.name),
                timing.duration.as_millis(),
                timing.passed,
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            ("lane_c::at_1x::layout_flex".to_string(), 100, true),
            ("lane_c::at_1x::tooltip".to_string(), 400, true),
            ("lane_c::at_2x::list_variable".to_string(), 1_000, false),
            // The next binary's own clock starts at its `running` line, so its build and start-up
            // are not charged to its first test.
            (
                "lane_c_scroll_layers::layers_match_reference".to_string(),
                10_000,
                true
            ),
        ]
    );
}

#[test]
fn the_report_lists_the_slowest_tests_first_with_their_share() {
    let text = report(
        "c",
        &[ProcessTimeline {
            label: "lane-c".into(),
            lines: lane_c_stream(),
        }],
    );
    let slowest = text
        .find("lane_c_scroll_layers::layers_match_reference")
        .unwrap();
    let middle = text.find("lane_c::at_2x::list_variable").unwrap();
    let fastest = text.find("lane_c::at_1x::layout_flex").unwrap();
    assert!(slowest < middle && middle < fastest, "{text}");
    // 10 s of 11.5 s in total.
    assert!(
        text.contains("| `lane_c_scroll_layers::layers_match_reference` | 10.00 s | 87.0% |"),
        "{text}"
    );
    assert!(text.contains("(failed)"), "{text}");
    // The slowest test target comes first in the group table.
    let groups = &text[text.find("| Group |").unwrap()..];
    assert!(
        groups
            .find("`lane_c_scroll_layers` | 1 | 10.00 s | 87.0%")
            .unwrap()
            < groups.find("`lane_c` | 3 | 1.50 s | 13.0%").unwrap(),
        "{text}"
    );
}

#[test]
fn a_run_with_no_completed_test_has_no_table_and_says_why() {
    let text = report(
        "c",
        &[ProcessTimeline {
            label: "lane-c".into(),
            lines: stream(&[
                (10, "error[E0432]: unresolved import"),
                (20, "error: could not compile"),
            ]),
        }],
    );
    assert!(!text.contains("| Test |"), "{text}");
    assert!(text.contains("No test completed in this process"), "{text}");
    assert!(text.contains("build error"), "{text}");
}

#[test]
fn each_process_gets_its_own_block_with_its_own_durations() {
    let first = stream(&[(0, "running 1 test"), (2_000, "test one ... ok")]);
    let second = stream(&[
        (0, "running 2 tests"),
        (500, "test two ... ok"),
        (1_500, "test three ... ok"),
    ]);
    let text = report(
        "c",
        &[
            ProcessTimeline {
                label: "lane-c-job-1".into(),
                lines: first,
            },
            ProcessTimeline {
                label: "lane-c-job-2".into(),
                lines: second,
            },
        ],
    );
    let one = text.find("## lane-c-job-1").unwrap();
    let two = text.find("## lane-c-job-2").unwrap();
    assert!(one < two, "{text}");
    assert!(
        text[one..two].contains("| `one` | 2.00 s | 100.0% |"),
        "{text}"
    );
    assert!(
        text[two..].contains("| `three` | 1.00 s | 66.7% |"),
        "{text}"
    );
    assert!(text[two..].contains("| `two` | 0.50 s | 33.3% |"), "{text}");
}

struct FakeEnvironment {
    root: PathBuf,
}

impl Environment for FakeEnvironment {
    fn var(&self, _name: &str) -> Option<String> {
        None
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

struct TimedRunner {
    timeline: Vec<TimedLine>,
}

impl Runner for TimedRunner {
    fn run(
        &mut self,
        _invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        Ok(RunOutput {
            success: true,
            code: Some(0),
            output: "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
                .to_string(),
        })
    }

    fn take_timeline(&mut self) -> Vec<TimedLine> {
        std::mem::take(&mut self.timeline)
    }
}

#[test]
fn a_lane_run_writes_timings_md_beside_its_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let environment = FakeEnvironment {
        root: root.path().to_path_buf(),
    };
    let mut runner = TimedRunner {
        timeline: stream(&[
            (0, "running 2 tests"),
            (300, "test at_1x::tooltip ... ok"),
            (3_300, "test at_1x::layout_flex ... ok"),
        ]),
    };
    let mut out = Vec::new();
    let code = execute(
        ["lane".to_string(), "c".to_string()],
        &environment,
        &mut runner,
        &mut out,
    );
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{text}");
    let path = root.path().join("artifacts-temp/testkit/c/timings.md");
    let report = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{error}: {text}"));
    assert!(
        report.contains("| `at_1x::layout_flex` | 3.00 s | 90.9% |"),
        "{report}"
    );
    assert!(text.contains("test timings in"), "{text}");
}
