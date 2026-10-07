//! Whole commands against a fake host: the summary line, exit codes, prerequisites, lanes that
//! cannot run, and where output is kept.

use gpux_testkit::{
    Environment, Os,
    cli::Selection,
    execute, find_workspace_root,
    lanes::{Invocation, LaneId, plan},
    run::{RunOutput, Runner},
    summary::{EXIT_FAILED, EXIT_USAGE, Summary},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

struct FakeEnvironment {
    vars: HashMap<String, String>,
    programs: Vec<&'static str>,
    os: Os,
    root: Option<PathBuf>,
}

impl FakeEnvironment {
    fn new(root: &Path) -> Self {
        Self {
            vars: HashMap::new(),
            programs: vec!["cargo"],
            os: Os::Windows,
            root: Some(root.to_path_buf()),
        }
    }
}

impl Environment for FakeEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.programs
            .contains(&name)
            .then(|| PathBuf::from(format!("/fake/bin/{name}")))
    }

    fn os(&self) -> Os {
        self.os
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        self.root.clone()
    }
}

/// Answers each invocation with a canned output, recording what ran.
#[derive(Default)]
struct FakeRunner {
    outputs: Vec<RunOutput>,
    ran: Vec<(String, PathBuf)>,
}

impl Runner for FakeRunner {
    fn run(
        &mut self,
        invocation: &Invocation,
        program: &Path,
        _workdir: &Path,
        log: &Path,
    ) -> anyhow::Result<RunOutput> {
        assert_eq!(program, Path::new("/fake/bin/cargo"));
        self.ran
            .push((invocation.command_line(), log.to_path_buf()));
        Ok(self.outputs.remove(0))
    }
}

fn output(success: bool, text: &str) -> RunOutput {
    RunOutput {
        success,
        code: Some(if success { 0 } else { 101 }),
        output: text.to_string(),
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

fn last_line(text: &str) -> &str {
    text.lines().last().unwrap()
}

#[test]
fn passing_runs_end_with_the_summary_and_exit_zero() {
    let root = tempfile::tempdir().unwrap();
    let environment = FakeEnvironment::new(root.path());
    let mut runner = FakeRunner {
        outputs: vec![output(
            true,
            "test result: ok. 60 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 1s",
        )],
        ..FakeRunner::default()
    };
    let (code, text) = run("lane a", &environment, &mut runner);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        last_line(&text),
        "gpux-testkit: 60 passed, 0 failed, 2 skipped"
    );
    assert_eq!(
        Summary::parse(last_line(&text)),
        Some(Summary {
            passed: 60,
            failed: 0,
            skipped: 2
        })
    );
    // Output is kept under artifacts-temp/testkit/<lane>/.
    assert_eq!(
        runner.ran[0].1,
        root.path()
            .join("artifacts-temp")
            .join("testkit")
            .join("a")
            .join("lane-a.log")
    );
}

#[test]
fn failures_and_build_errors_exit_nonzero() {
    let root = tempfile::tempdir().unwrap();
    let environment = FakeEnvironment::new(root.path());
    let mut runner = FakeRunner {
        outputs: vec![
            output(
                false,
                "test result: FAILED. 75 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out",
            ),
            output(
                false,
                "error[E0599]: no function or associated item named `x`",
            ),
        ],
        ..FakeRunner::default()
    };
    let (code, text) = run("lane c d", &environment, &mut runner);
    assert_eq!(code, EXIT_FAILED);
    // Lane c's two failed tests, plus every lane d test: its host failed to build and reported
    // no results.
    let lane_d = plan(LaneId::D, &Selection::default()).unwrap().tests.len();
    assert_eq!(
        last_line(&text),
        format!("gpux-testkit: 75 passed, {} failed, 0 skipped", 2 + lane_d)
    );
    assert!(
        text.contains("exited with code 101 before reporting its results"),
        "{text}"
    );
    assert!(
        text.contains("desktop_host::layout-flex: no result was reported"),
        "{text}"
    );

    // A host that stops early fails the tests it never reported.
    let mut runner = FakeRunner {
        outputs: vec![output(
            true,
            "gpux-conformance: PASS platform-open-present ok",
        )],
        ..FakeRunner::default()
    };
    let (code, text) = run("lane d --grep platform", &environment, &mut runner);
    assert_eq!(code, EXIT_FAILED);
    assert!(
        text.contains("desktop_host::platform-close: no result was reported"),
        "{text}"
    );
}

#[test]
fn zero_selected_tests_fail() {
    let root = tempfile::tempdir().unwrap();
    let environment = FakeEnvironment::new(root.path());
    let mut runner = FakeRunner {
        outputs: vec![output(
            true,
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 77 filtered out",
        )],
        ..FakeRunner::default()
    };
    let (code, text) = run("lane c", &environment, &mut runner);
    assert_eq!(code, EXIT_FAILED);
    assert_eq!(
        last_line(&text),
        "gpux-testkit: 0 passed, 0 failed, 0 skipped"
    );

    // A grep that matches nothing selects nothing and runs nothing.
    let mut idle = FakeRunner::default();
    let (code, text) = run("lane a --grep no-such-scene", &environment, &mut idle);
    assert_eq!(code, EXIT_FAILED);
    assert!(text.contains("nothing matches the selection"), "{text}");
    assert!(idle.ran.is_empty());
}

#[test]
fn listing_prints_libtests_exact_selection_without_running_tests() {
    let root = tempfile::tempdir().unwrap();
    let environment = FakeEnvironment::new(root.path());
    let mut runner = FakeRunner {
        outputs: vec![
            output(true, "paint_borders: test\n\n1 test, 0 benchmarks\n"),
            output(
                true,
                "at_1x::paint_borders: test\nat_2x::paint_borders: test\n\n2 tests, 0 benchmarks\n",
            ),
        ],
        ..FakeRunner::default()
    };
    let (code, text) = run(
        "lane a c --scene paint-borders --list",
        &environment,
        &mut runner,
    );
    assert_eq!(code, 0, "{text}");
    // Only libtest listings ran, never the tests.
    assert_eq!(runner.ran.len(), 2);
    assert!(
        runner
            .ran
            .iter()
            .all(|(command, _)| command.contains(" -- --list --exact ")),
        "{:?}",
        runner.ran
    );
    assert!(text.contains("  lane_a::paint_borders"), "{text}");
    assert!(text.contains("  lane_c::at_1x::paint_borders"), "{text}");
    assert!(text.contains("  lane_c::at_2x::paint_borders"), "{text}");
    assert_eq!(
        last_line(&text),
        "gpux-testkit: 0 passed, 0 failed, 0 skipped"
    );

    // Lane d lists its plan: the desktop host selects with the same function.
    let mut idle = FakeRunner::default();
    let (code, text) = run("lane d --scene layout-flex --list", &environment, &mut idle);
    assert_eq!(code, 0, "{text}");
    assert!(idle.ran.is_empty());
    assert!(text.contains("  desktop_host::layout-flex"), "{text}");

    // A listing that fails to build fails the command.
    let mut broken = FakeRunner {
        outputs: vec![output(false, "error[E0425]: cannot find value")],
        ..FakeRunner::default()
    };
    let (code, text) = run("lane c --list", &environment, &mut broken);
    assert_eq!(code, EXIT_FAILED);
    assert!(text.contains("listing `cargo test"), "{text}");
}

#[test]
fn missing_prerequisites_fail_with_actionable_messages() {
    let root = tempfile::tempdir().unwrap();
    let mut environment = FakeEnvironment::new(root.path());
    environment.os = Os::Linux;
    environment.programs = vec!["cargo"];
    let mut runner = FakeRunner::default();
    let (code, text) = run("lane d", &environment, &mut runner);
    assert_eq!(code, EXIT_FAILED);
    assert!(text.contains("xvfb-run -a gpux-testkit lane d"), "{text}");
    assert!(runner.ran.is_empty());

    environment.vars.insert("DISPLAY".into(), ":99".into());
    let (_, text) = run("lane d", &environment, &mut runner);
    assert!(text.contains("needs `xdotool`"), "{text}");

    environment.programs = vec![];
    let (_, text) = run("lane a", &environment, &mut runner);
    assert!(text.contains("`cargo` is not on PATH"), "{text}");
    assert!(runner.ran.is_empty());
}

#[test]
fn unavailable_lanes_and_bad_usage_still_end_with_the_summary() {
    let root = tempfile::tempdir().unwrap();
    let environment = FakeEnvironment::new(root.path());
    let mut runner = FakeRunner::default();
    let (code, text) = run("lane f", &environment, &mut runner);
    assert_eq!(code, EXIT_FAILED);
    assert!(text.contains("lane f is not available yet"), "{text}");
    assert_eq!(
        last_line(&text),
        "gpux-testkit: 0 passed, 1 failed, 0 skipped"
    );

    let (code, text) = run("lane z", &environment, &mut runner);
    assert_eq!(code, EXIT_USAGE);
    assert!(text.contains("unknown lane `z`"), "{text}");
    assert_eq!(
        last_line(&text),
        "gpux-testkit: 0 passed, 0 failed, 0 skipped"
    );

    let outside = FakeEnvironment {
        root: None,
        ..FakeEnvironment::new(root.path())
    };
    let (code, text) = run("lane a", &outside, &mut runner);
    assert_eq!(code, EXIT_FAILED);
    assert!(text.contains("inside a gpux checkout"), "{text}");
}

#[test]
fn the_workspace_root_is_found_from_any_subfolder() {
    let root = tempfile::tempdir().unwrap();
    let conformance = root.path().join("crates").join("gpux_conformance");
    std::fs::create_dir_all(&conformance).unwrap();
    std::fs::write(root.path().join("Cargo.toml"), "[workspace]").unwrap();
    std::fs::write(conformance.join("Cargo.toml"), "[package]").unwrap();
    let deep = conformance.join("src").join("scenes");
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(find_workspace_root(&deep), Some(root.path().to_path_buf()));
    let elsewhere = tempfile::tempdir().unwrap();
    assert_eq!(find_workspace_root(elsewhere.path()), None);
}
