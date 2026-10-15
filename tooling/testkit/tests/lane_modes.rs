//! The two modes of lane C's scroll-layer suites as the testkit selects and reports them:
//! `--full` and `GPUX_LANE_C_FULL` reach the test process, and a run says which mode it is in
//! before it starts and again beside its summary.

use gpux_testkit::{
    Environment, Os,
    cli::{Command, Selection, parse},
    execute,
    lanes::Invocation,
    local_gpu::hardware_plans,
    run::{RunOutput, Runner},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const SAMPLED: &str =
    "lane c: sampled scroll-layer suites (GPUX_LANE_C_FULL=1 or --full runs every frame)";
const FULL: &str = "lane c: full scroll-layer suites";

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

#[derive(Default)]
struct Recorder {
    envs: Vec<Vec<(String, String)>>,
}

impl Runner for Recorder {
    fn run(
        &mut self,
        invocation: &Invocation,
        _program: &Path,
        _workdir: &Path,
        _log: &Path,
    ) -> anyhow::Result<RunOutput> {
        self.envs.push(invocation.env.clone());
        Ok(RunOutput {
            success: true,
            code: Some(0),
            output: "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
                .into(),
        })
    }
}

/// Runs `line` with `vars` set and returns the invocations' environments and the output.
fn run(line: &str, vars: &[(&str, &str)]) -> (Vec<Vec<(String, String)>>, Vec<String>) {
    let root = tempfile::tempdir().unwrap();
    let host = Host {
        vars: vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
        root: root.path().to_path_buf(),
    };
    let mut runner = Recorder::default();
    let mut out = Vec::new();
    let code = execute(
        line.split_whitespace().map(str::to_string),
        &host,
        &mut runner,
        &mut out,
    );
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{text}");
    (runner.envs, text.lines().map(str::to_string).collect())
}

fn sets_full(env: &[(String, String)]) -> bool {
    env.contains(&("GPUX_LANE_C_FULL".to_string(), "1".to_string()))
}

#[test]
fn a_default_run_is_sampled_says_so_before_it_starts_and_beside_the_summary() {
    let (envs, lines) = run("lane c", &[]);
    assert!(envs.iter().all(|env| !sets_full(env)), "{envs:?}");
    let first = lines.iter().position(|line| line == SAMPLED).unwrap();
    let command = lines
        .iter()
        .position(|line| line.starts_with("lane c: cargo test"))
        .unwrap();
    assert!(
        first < command,
        "the mode comes before the run starts: {lines:?}"
    );
    let summary = lines.len() - 1;
    assert!(
        lines[summary].starts_with("gpux-testkit: 3 passed"),
        "{lines:?}"
    );
    assert_eq!(lines[summary - 1], SAMPLED, "{lines:?}");
}

#[test]
fn full_on_the_command_line_reaches_the_test_process() {
    let (envs, lines) = run("lane c --full", &[]);
    assert!(envs.iter().all(|env| sets_full(env)), "{envs:?}");
    assert_eq!(
        lines.iter().filter(|line| *line == FULL).count(),
        2,
        "{lines:?}"
    );
    assert!(!lines.iter().any(|line| line == SAMPLED), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("lane c: GPUX_LANE_C_FULL=1 cargo test")),
        "the command line shows the variable: {lines:?}"
    );
}

#[test]
fn the_environment_variable_selects_full_unless_it_is_empty_or_zero() {
    let (envs, lines) = run("lane c", &[("GPUX_LANE_C_FULL", "1")]);
    assert!(envs.iter().all(|env| sets_full(env)), "{envs:?}");
    assert!(lines.iter().any(|line| line == FULL), "{lines:?}");
    for value in ["", "0"] {
        let (envs, lines) = run("lane c", &[("GPUX_LANE_C_FULL", value)]);
        assert!(
            envs.iter().all(|env| !sets_full(env)),
            "{value:?}: {envs:?}"
        );
        assert!(
            lines.iter().any(|line| line == SAMPLED),
            "{value:?}: {lines:?}"
        );
    }
}

#[test]
fn a_narrowed_run_does_not_run_the_suites_and_does_not_report_a_mode() {
    let (_, lines) = run("lane c --grep layout", &[]);
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("scroll-layer suites")),
        "{lines:?}"
    );
}

#[test]
fn the_full_flag_is_parsed_and_lane_g_runs_lane_c_in_full() {
    let Command::Lane(args) =
        parse("lane c --full".split_whitespace().map(str::to_string)).unwrap()
    else {
        panic!("a lane command");
    };
    assert!(args.full);
    let Command::Lane(args) = parse("lane c".split_whitespace().map(str::to_string)).unwrap()
    else {
        panic!("a lane command");
    };
    assert!(!args.full);
    let plans = hardware_plans(&Selection::default(), Os::MacOs).unwrap();
    let lane_c = plans
        .iter()
        .find(|plan| plan.invocations[0].label.contains("lane-c"))
        .unwrap();
    assert!(
        lane_c
            .invocations
            .iter()
            .all(|invocation| sets_full(&invocation.env))
    );
}
