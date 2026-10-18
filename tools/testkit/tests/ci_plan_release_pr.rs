//! What `.github/actions/plan/plan.sh` runs for a release pull request: one that changes nothing
//! but own version values in `Cargo.toml` files and `Cargo.lock`, plus `CHANGELOG.md`, runs only
//! `check` and `package` on ubuntu; anything else beside it runs every job. Each case builds a
//! small workspace in a temporary git repository. The script needs `bash`, `jq` and `git` (every
//! Unix host has them; on Windows the tests are reported ignored with the reason).

#![allow(
    clippy::disallowed_methods,
    reason = "the plan script and git are synchronous child processes; waiting on them is the test"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

const EVERY_JOB: &str =
    r#"["check","test","lane-a","lane-c","lane-d","package","lane-b","lane-e","lane-f"]"#;
const EVERY_OS: &str = r#"["ubuntu-24.04","windows-2025","macos-15"]"#;

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

const ROOT_MANIFEST: &str = r#"[workspace]
members = ["crates/a", "crates/b", "vendor/fork"]

[workspace.package]
version = "1.0.0"
edition = "2024"

[workspace.dependencies]
a = { package = "x-a", path = "crates/a", version = "1.0.0" }
b = { package = "x-b", path = "crates/b", version = "1.0.0", default-features = false }
fork = { package = "x-fork", path = "vendor/fork", version = "=0.1.0-gpux.1" }
serde = { version = "1.0.200", features = ["derive"] }
"#;

const MEMBER_MANIFEST: &str = r#"[package]
name = "x-b"
version.workspace = true
edition.workspace = true

[dependencies]
a = { package = "x-a", path = "../a", version = "1.0.0", default-features = false }
serde.workspace = true
"#;

const LOCKFILE: &str = r#"version = 4

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaaa"

[[package]]
name = "x-a"
version = "1.0.0"

[[package]]
name = "x-b"
version = "1.0.0"
dependencies = [
 "serde",
 "x-a",
]

[[package]]
name = "x-fork"
version = "0.1.0-gpux.1"
"#;

const CHANGELOG: &str = "# Changelog\n\n## [1.0.0] - 2026-10-18\n\nFirst release.\n";

/// A workspace in a temporary git repository whose first commit is tagged `base`.
struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn new() -> Self {
        assert!(
            available("bash") && available("jq") && available("git"),
            "plan.sh needs `bash`, `jq` and `git` on PATH: install jq (for example `brew install \
             jq` or `sudo apt-get install jq`)"
        );
        let repo = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        repo.git(&["init", "-q"]);
        repo.write("Cargo.toml", ROOT_MANIFEST);
        repo.write(
            "crates/a/Cargo.toml",
            &MEMBER_MANIFEST.replace("x-b", "x-a"),
        );
        repo.write("crates/a/src/lib.rs", "pub fn a() {}\n");
        repo.write("crates/b/Cargo.toml", MEMBER_MANIFEST);
        repo.write("crates/b/src/lib.rs", "pub fn b() {}\n");
        repo.write(
            "vendor/fork/Cargo.toml",
            "[package]\nname = \"x-fork\"\nversion = \"0.1.0-gpux.1\"\n",
        );
        repo.write("vendor/fork/src/lib.rs", "pub fn fork() {}\n");
        repo.write("Cargo.lock", LOCKFILE);
        repo.write("CHANGELOG.md", CHANGELOG);
        repo.write(".github/workflows/ci.yml", "name: ci\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "base"]);
        repo.git(&["tag", "base"]);
        repo
    }

    fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(self.dir.path())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn path(&self, file: &str) -> PathBuf {
        self.dir.path().join(file)
    }

    fn write(&self, file: &str, text: &str) {
        let path = self.path(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn edit(&self, file: &str, from: &str, to: &str) {
        let text = std::fs::read_to_string(self.path(file)).unwrap();
        assert!(text.contains(from), "{file} contains {from:?}");
        self.write(file, &text.replacen(from, to, 1));
    }

    /// What release-plz's release pull request changes: the workspace version, every own
    /// requirement, the own lockfile entries and a changelog section.
    fn bump_release(&self) {
        self.edit(
            "Cargo.toml",
            "[workspace.package]\nversion = \"1.0.0\"",
            "[workspace.package]\nversion = \"1.0.1\"",
        );
        self.edit(
            "Cargo.toml",
            r#"path = "crates/a", version = "1.0.0""#,
            r#"path = "crates/a", version = "1.0.1""#,
        );
        self.edit(
            "Cargo.toml",
            r#"path = "crates/b", version = "1.0.0""#,
            r#"path = "crates/b", version = "1.0.1""#,
        );
        self.edit(
            "crates/b/Cargo.toml",
            r#"path = "../a", version = "1.0.0""#,
            r#"path = "../a", version = "1.0.1""#,
        );
        self.edit(
            "Cargo.lock",
            "name = \"x-a\"\nversion = \"1.0.0\"",
            "name = \"x-a\"\nversion = \"1.0.1\"",
        );
        self.edit(
            "Cargo.lock",
            "name = \"x-b\"\nversion = \"1.0.0\"",
            "name = \"x-b\"\nversion = \"1.0.1\"",
        );
        self.edit(
            "CHANGELOG.md",
            "## [1.0.0]",
            "## [1.0.1] - 2026-10-19\n\n### Fixes\n- a fix\n\n## [1.0.0]",
        );
    }

    /// Commits the working tree and runs the plan for a pull request from `base` with `labels`.
    fn plan(&self, labels: &str) -> Outputs {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "change"]);
        let output = Command::new("bash")
            .arg(root().join(".github/actions/plan/plan.sh"))
            .current_dir(self.dir.path())
            .env_remove("GITHUB_OUTPUT")
            .envs([
                ("EVENT_NAME", "pull_request"),
                ("PR_LABELS", labels),
                ("BASE_SHA", "base"),
                ("HEAD_SHA", "HEAD"),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "plan.sh: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Outputs(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .filter_map(|line| line.split_once('='))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        )
    }
}

#[derive(Debug)]
struct Outputs(Vec<(String, String)>);

impl Outputs {
    fn get(&self, key: &str) -> &str {
        &self.0.iter().find(|(name, _)| name == key).unwrap().1
    }

    fn assert_everything(&self, case: &str) {
        assert_eq!(self.get("jobs"), EVERY_JOB, "{case}");
        assert_eq!(self.get("os"), EVERY_OS, "{case}");
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_release_pull_request_runs_check_and_package_on_ubuntu() {
    let repo = Repo::new();
    repo.bump_release();
    let plan = repo.plan("[]");
    assert_eq!(plan.get("jobs"), r#"["check","package"]"#);
    assert_eq!(plan.get("os"), r#"["ubuntu-24.04"]"#);
    assert_eq!(plan.get("skipped"), "");
    for lane in [
        "\"test\"", "lane-a", "lane-b", "lane-c", "lane-d", "lane-e", "lane-f",
    ] {
        assert!(!plan.get("jobs").contains(lane), "{lane}");
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_registry_bump_in_the_lockfile_runs_every_job() {
    let repo = Repo::new();
    repo.bump_release();
    repo.edit(
        "Cargo.lock",
        "version = \"1.0.200\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aaaa\"",
        "version = \"1.0.201\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"bbbb\"",
    );
    repo.plan("[]").assert_everything("registry bump");
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_manifest_change_beside_the_versions_runs_every_job() {
    let feature = Repo::new();
    feature.bump_release();
    feature.edit(
        "crates/b/Cargo.toml",
        "serde.workspace = true\n",
        "serde.workspace = true\n\n[features]\nextra = []\n",
    );
    feature.plan("[]").assert_everything("new feature");

    let dependency = Repo::new();
    dependency.bump_release();
    dependency.edit(
        "crates/b/Cargo.toml",
        "serde.workspace = true\n",
        "serde.workspace = true\nfork.workspace = true\n",
    );
    dependency.plan("[]").assert_everything("new dependency");

    let added = Repo::new();
    added.bump_release();
    added.write("crates/c/Cargo.toml", MEMBER_MANIFEST);
    added.plan("[]").assert_everything("added manifest");
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_crates_io_requirement_change_runs_every_job() {
    let repo = Repo::new();
    repo.bump_release();
    repo.edit(
        "Cargo.toml",
        r#"serde = { version = "1.0.200""#,
        r#"serde = { version = "1.0.201""#,
    );
    repo.plan("[]").assert_everything("crates.io requirement");
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_fork_requirement_or_a_vendored_file_runs_every_job() {
    let requirement = Repo::new();
    requirement.bump_release();
    requirement.edit(
        "Cargo.toml",
        r#"path = "vendor/fork", version = "=0.1.0-gpux.1""#,
        r#"path = "vendor/fork", version = "=0.1.0-gpux.2""#,
    );
    requirement.plan("[]").assert_everything("fork requirement");

    let vendored = Repo::new();
    vendored.bump_release();
    vendored.edit(
        "vendor/fork/Cargo.toml",
        "version = \"0.1.0-gpux.1\"",
        "version = \"0.1.0-gpux.2\"",
    );
    vendored.plan("[]").assert_everything("vendored manifest");
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn version_bumps_beside_any_other_file_run_every_job() {
    let source = Repo::new();
    source.bump_release();
    source.edit("crates/a/src/lib.rs", "pub fn a() {}", "pub fn a() { 1; }");
    source.plan("[]").assert_everything("source file");

    let workflow = Repo::new();
    workflow.bump_release();
    workflow.edit(".github/workflows/ci.yml", "name: ci", "name: changed");
    workflow.plan("[]").assert_everything("workflow");
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn labels_still_choose_for_a_release_pull_request() {
    let lane = Repo::new();
    lane.bump_release();
    assert_eq!(lane.plan(r#"["ci:lane-a"]"#).get("jobs"), r#"["lane-a"]"#);

    let os = Repo::new();
    os.bump_release();
    let plan = os.plan(r#"["ci-os:macos"]"#);
    assert_eq!(plan.get("jobs"), r#"["check","package"]"#);
    assert_eq!(plan.get("os"), r#"["macos-15"]"#);
}

#[test]
#[cfg_attr(
    windows,
    ignore = "plan.sh needs bash and jq; the Unix runners run this test"
)]
fn a_changelog_only_change_runs_only_check() {
    let repo = Repo::new();
    repo.edit("CHANGELOG.md", "First release.", "First release, reworded.");
    assert_eq!(repo.plan("[]").get("jobs"), r#"["check"]"#);
}
