//! What a workflow run executes for lane C: `.github/actions/plan/plan.sh` under representative
//! environments, and the wiring of its outputs in `ci.yml`. The script needs `bash` and `jq`
//! (every Unix host has them; on Windows the script tests are reported ignored with the reason,
//! and a Unix host without them fails the tests instead of skipping them).

#![allow(
    clippy::disallowed_methods,
    reason = "the plan script is a synchronous child process; waiting on it is the test"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The script's outputs for `env`, run in `dir`, or `None` when this host cannot run it.
fn plan_in(dir: &Path, env: &[(&str, &str)]) -> Option<Result<Outputs, String>> {
    assert!(
        available("bash") && available("jq"),
        "plan.sh needs `bash` and `jq` on PATH: install jq (for example `brew install jq` or \
         `sudo apt-get install jq`)"
    );
    let output = Command::new("bash")
        .arg(root().join(".github/actions/plan/plan.sh"))
        .current_dir(dir)
        .env_remove("GITHUB_OUTPUT")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    if !output.status.success() {
        return Some(Err(String::from_utf8_lossy(&output.stderr).into_owned()));
    }
    Some(Ok(Outputs(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    )))
}

fn plan(env: &[(&str, &str)]) -> Option<Outputs> {
    plan_in(&root(), env).map(|result| result.expect("plan.sh succeeds"))
}

#[derive(Debug)]
struct Outputs(Vec<(String, String)>);

impl Outputs {
    fn get(&self, key: &str) -> &str {
        &self.0.iter().find(|(name, _)| name == key).unwrap().1
    }

    /// The `backend`s of lane C's matrix, in order.
    fn backends(&self) -> Vec<String> {
        let entries: Vec<serde_like::Entry> = serde_like::parse(self.get("lane_c"));
        entries.into_iter().map(|entry| entry.backend).collect()
    }

    fn lane_c_jobs(&self, backend: &str) -> u32 {
        serde_like::parse(self.get("lane_c"))
            .into_iter()
            .find(|entry| entry.backend == backend)
            .unwrap()
            .jobs
    }
}

/// Just enough of `[{"os":..,"backend":..,"jobs":N}]` for these tests, without a JSON crate.
mod serde_like {
    pub struct Entry {
        pub backend: String,
        pub jobs: u32,
    }

    pub fn parse(json: &str) -> Vec<Entry> {
        json.trim_matches(['[', ']'])
            .split("},{")
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                let field = |name: &str| {
                    let start = entry.find(&format!("\"{name}\":")).unwrap() + name.len() + 3;
                    entry[start..]
                        .split([',', '}'])
                        .next()
                        .unwrap()
                        .trim_matches('"')
                        .to_string()
                };
                Entry {
                    backend: field("backend"),
                    jobs: field("jobs").parse().unwrap(),
                }
            })
            .collect()
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_pull_request_without_labels_runs_lane_c_on_lavapipe_and_metal_sampled() {
    let Some(plan) = plan(&[("EVENT_NAME", "pull_request"), ("PR_LABELS", "[]")]) else {
        return;
    };
    assert_eq!(plan.backends(), ["lavapipe", "metal"]);
    assert_eq!(plan.get("full"), "false");
    assert!(
        plan.get("skipped").contains("lane C skipped on warp"),
        "{}",
        plan.get("skipped")
    );
    assert!(plan.get("skipped").contains("ci-os:windows"));
    // Every other job still runs on every platform.
    assert!(plan.get("os").contains("windows-2025"));
    assert!(plan.get("jobs").contains("lane-d") && plan.get("jobs").contains("lane-c"));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn the_windows_label_and_ci_all_add_warp() {
    let Some(windows) = plan(&[
        ("EVENT_NAME", "pull_request"),
        ("PR_LABELS", r#"["ci-os:windows"]"#),
    ]) else {
        return;
    };
    assert_eq!(windows.backends(), ["warp"]);
    assert_eq!(windows.get("skipped"), "");
    assert_eq!(windows.get("os"), r#"["windows-2025"]"#);
    let all = plan(&[
        ("EVENT_NAME", "pull_request"),
        ("PR_LABELS", r#"["ci:all"]"#),
    ])
    .unwrap();
    assert_eq!(all.backends(), ["lavapipe", "warp", "metal"]);
    assert_eq!(all.get("skipped"), "");
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_ci_lane_label_alone_does_not_add_warp_and_os_labels_still_restrict() {
    let Some(lane) = plan(&[
        ("EVENT_NAME", "pull_request"),
        ("PR_LABELS", r#"["ci:lane-c"]"#),
    ]) else {
        return;
    };
    assert_eq!(lane.get("jobs"), r#"["lane-c"]"#);
    assert_eq!(lane.backends(), ["lavapipe", "metal"]);
    let ubuntu = plan(&[
        ("EVENT_NAME", "pull_request"),
        ("PR_LABELS", r#"["ci-os:ubuntu","ci-os:macos"]"#),
    ])
    .unwrap();
    assert_eq!(ubuntu.get("os"), r#"["ubuntu-24.04","macos-15"]"#);
    assert_eq!(ubuntu.backends(), ["lavapipe", "metal"]);
    assert_eq!(ubuntu.get("skipped"), "");
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_manual_run_honours_its_platforms_and_its_full_input() {
    let Some(default) = plan(&[("EVENT_NAME", "workflow_dispatch")]) else {
        return;
    };
    assert_eq!(default.backends(), ["lavapipe", "warp", "metal"]);
    assert_eq!(default.get("full"), "false");
    let picked = plan(&[
        ("EVENT_NAME", "workflow_dispatch"),
        ("DISPATCH_PLATFORMS", "windows, macos"),
        ("DISPATCH_FULL", "true"),
        ("DISPATCH_JOBS", "lane-c"),
    ])
    .unwrap();
    assert_eq!(picked.backends(), ["warp", "metal"]);
    assert_eq!(picked.get("full"), "true");
    assert_eq!(picked.get("jobs"), r#"["lane-c"]"#);
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn the_nightly_schedule_runs_every_platform_in_full_mode() {
    let Some(nightly) = plan(&[("EVENT_NAME", "schedule")]) else {
        return;
    };
    assert_eq!(nightly.backends(), ["lavapipe", "warp", "metal"]);
    assert_eq!(nightly.get("full"), "true");
    assert_eq!(nightly.get("skipped"), "");
    assert_eq!(
        nightly.get("os"),
        r#"["ubuntu-24.04","windows-2025","macos-15"]"#
    );
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn concurrency_per_platform_comes_from_one_place_and_follows_the_measurements() {
    let Some(nightly) = plan(&[("EVENT_NAME", "schedule")]) else {
        return;
    };
    // Three concurrent processes ran stably on WARP and were faster; two did not pay on lavapipe.
    assert_eq!(nightly.lane_c_jobs("warp"), 3);
    assert!(nightly.lane_c_jobs("lavapipe") == 1 && nightly.lane_c_jobs("metal") == 1);
    // A manual run can try another level on every platform.
    let more = plan(&[
        ("EVENT_NAME", "workflow_dispatch"),
        ("DISPATCH_LANE_C_JOBS", "3"),
    ])
    .unwrap();
    assert!(
        ["lavapipe", "warp", "metal"]
            .iter()
            .all(|backend| more.lane_c_jobs(backend) == 3)
    );
    let script = std::fs::read_to_string(root().join(".github/actions/plan/plan.sh")).unwrap();
    assert!(script.contains("lane_c_jobs()"));
    assert!(script.contains("LANE_C_WARP_ON_PULL_REQUEST=false"));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_documentation_only_pull_request_runs_only_check() {
    let dir = tempfile::tempdir().unwrap();
    assert!(available("git"), "this test builds a repository with `git`");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("lib.rs"), "fn main() {}").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "base"]);
    git(&["tag", "base"]);
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/a.md"), "words").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "docs"]);
    let docs = plan_in(
        dir.path(),
        &[
            ("EVENT_NAME", "pull_request"),
            ("PR_LABELS", "[]"),
            ("BASE_SHA", "base"),
            ("HEAD_SHA", "HEAD"),
        ],
    );
    let Some(docs) = docs else {
        return;
    };
    let docs = docs.unwrap();
    assert_eq!(docs.get("jobs"), r#"["check"]"#);
    // Lane C is not selected, so nothing is reported as left out of it.
    assert_eq!(docs.get("skipped"), "");
    std::fs::write(dir.path().join("lib.rs"), "fn main() { 1; }").unwrap();
    git(&["commit", "-qam", "code"]);
    let code = plan_in(
        dir.path(),
        &[
            ("EVENT_NAME", "pull_request"),
            ("PR_LABELS", "[]"),
            ("BASE_SHA", "base"),
            ("HEAD_SHA", "HEAD"),
        ],
    )
    .unwrap()
    .unwrap();
    assert!(code.get("jobs").contains("lane-c"));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn an_unknown_platform_is_refused() {
    let Some(result) = plan_in(
        &root(),
        &[
            ("EVENT_NAME", "workflow_dispatch"),
            ("DISPATCH_PLATFORMS", "plan9"),
        ],
    ) else {
        return;
    };
    assert!(result.unwrap_err().contains("unknown platform 'plan9'"));
}

fn workflow() -> String {
    std::fs::read_to_string(root().join(".github/workflows/ci.yml")).unwrap()
}

/// The lines of the top-level key `key` of `text` (`on:`, `jobs:`), and the jobs inside `jobs:`.
fn block<'a>(text: &'a str, header: &str, indent: usize) -> Vec<&'a str> {
    let mut lines = text.lines().skip_while(|line| *line != header);
    lines.next();
    lines
        .take_while(|line| {
            line.trim().is_empty() || line.len() - line.trim_start().len() > indent - 1
        })
        .collect()
}

#[test]
fn the_workflow_has_the_nightly_trigger_and_the_full_input_and_no_push_trigger() {
    let text = workflow();
    let triggers = block(&text, "on:", 1);
    let keys: Vec<&str> = triggers
        .iter()
        .filter(|line| line.starts_with("  ") && !line.starts_with("   "))
        .filter(|line| !line.trim_start().starts_with('#'))
        .map(|line| line.trim().trim_end_matches(':'))
        .collect();
    assert_eq!(keys, ["pull_request", "schedule", "workflow_dispatch"]);
    assert!(
        triggers
            .iter()
            .any(|line| line.trim_start().starts_with("- cron:"))
    );
    // Hosted CI never runs on pushes to main (decision of 2026-10-09).
    assert!(!keys.contains(&"push"));
    let full: Vec<&str> = triggers
        .iter()
        .skip_while(|line| line.trim() != "full:")
        .take(4)
        .copied()
        .collect();
    assert!(
        full.iter().any(|line| line.trim() == "type: boolean")
            && full.iter().any(|line| line.trim() == "default: false"),
        "{full:?}"
    );
}

#[test]
fn lane_cs_step_passes_the_plans_mode_and_jobs_and_keeps_its_names() {
    let text = workflow();
    let lane_c = block(&text, "  lane-c:", 4);
    let joined = lane_c.join("\n");
    assert!(
        joined.contains("name: lane C (${{ matrix.backend }})"),
        "{joined}"
    );
    assert!(joined.contains("include: ${{ fromJSON(needs.plan.outputs.lane_c) }}"));
    assert!(joined.contains("- name: Lane C\n"));
    assert!(joined.contains("FULL: ${{ needs.plan.outputs.full }}"));
    assert!(joined.contains("JOBS: ${{ matrix.jobs }}"));
    assert!(joined.contains("--full") && joined.contains("--jobs \"$JOBS\""));
    assert!(joined.contains("cargo run -p gpux-testkit -- lane c"));
    // The plan job hands the outputs on, and the other lane-c step names stay.
    let plan = block(&text, "  plan:", 4).join("\n");
    assert!(plan.contains("full: ${{ steps.plan.outputs.full }}"));
    assert!(plan.contains("skipped: ${{ steps.plan.outputs.skipped }}"));
    assert!(plan.contains("full: ${{ inputs.full }}"));
    assert!(joined.contains("- name: Upload failure artifacts"));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_plan_that_does_not_select_lane_c_reports_nothing_left_out_of_it() {
    let lane_a = plan(&[
        ("EVENT_NAME", "pull_request"),
        ("PR_LABELS", r#"["ci:lane-a"]"#),
    ])
    .unwrap();
    assert_eq!(lane_a.get("jobs"), r#"["lane-a"]"#);
    assert_eq!(lane_a.get("skipped"), "");
}

/// The `run:` script of the step `name` in the `lane-c` job, as the workflow writes it.
fn lane_c_step_script(name: &str) -> String {
    let text = workflow();
    let lane_c = block(&text, "  lane-c:", 4);
    let step = lane_c
        .iter()
        .position(|line| line.trim() == format!("- name: {name}"))
        .unwrap();
    let run = lane_c[step..]
        .iter()
        .position(|line| line.trim() == "run: |")
        .unwrap()
        + step;
    let indent = |line: &str| line.len() - line.trim_start().len();
    let base = indent(lane_c[run]);
    lane_c[run + 1..]
        .iter()
        .take_while(|line| line.trim().is_empty() || indent(line) > base)
        .map(|line| line.trim_start_matches(' ').to_string() + "\n")
        .collect::<String>()
        .replace("\n\n", "\n")
}

/// What the Lane C step's script runs `cargo` with, given the plan's `full`, the selector and the
/// platform's `--jobs`. Runs the workflow's own script under `bash -e` with a recording `cargo`.
fn lane_c_command(full: &str, grep: &str, jobs: &str) -> String {
    let script = lane_c_step_script("Lane C");
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let cargo = bin.join("cargo");
    std::fs::write(&cargo, "#!/bin/sh\necho \"$@\" > \"$RECORD\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let record = dir.path().join("argv");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = Command::new("bash")
        .args(["-e", "-o", "pipefail", "-c", &script])
        .env("PATH", path)
        .env("RECORD", &record)
        .env("FULL", full)
        .env("GREP", grep)
        .env("JOBS", jobs)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "the step's script failed for {full:?} {grep:?} {jobs:?}"
    );
    std::fs::read_to_string(record).unwrap().trim().to_string()
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the step's script runs under bash; the Unix runners run this test"
)]
fn lane_cs_step_runs_the_testkit_with_the_plans_mode_and_jobs() {
    assert_eq!(
        lane_c_command("false", "", "2"),
        "run -p gpux-testkit -- lane c --jobs 2"
    );
    assert_eq!(
        lane_c_command("", "", "1"),
        "run -p gpux-testkit -- lane c --jobs 1"
    );
    assert_eq!(
        lane_c_command("true", "", "2"),
        "run -p gpux-testkit -- lane c --full --jobs 2"
    );
    // A selection runs as one process, and says which tests: `--jobs` only applies to the lane.
    assert_eq!(
        lane_c_command("false", "layout", "2"),
        "run -p gpux-testkit -- lane c --grep layout"
    );
    assert_eq!(
        lane_c_command("true", "layout", "2"),
        "run -p gpux-testkit -- lane c --full --grep layout"
    );
}

#[test]
fn the_nightly_run_has_its_own_concurrency_group() {
    let text = workflow();
    let group = text
        .lines()
        .find(|line| line.trim_start().starts_with("group:"))
        .unwrap();
    assert!(
        group.contains("github.event_name == 'schedule' && '-nightly'"),
        "{group}"
    );
    // A pull request's superseded run is cancelled; scheduled and manual runs always finish.
    assert!(text.contains("cancel-in-progress: ${{ github.event_name == 'pull_request' }}"));
}
