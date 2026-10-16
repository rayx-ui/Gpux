//! The repository layout of gpux's test tooling: Rust crates in `crates/` (the never-shipped
//! verification crates named `gpux_tests_*`), everything that drives or hosts the tests in
//! `tools/`. The guard proves the layout and scans the tracked live files for references to the
//! old locations and package name; historical records keep the paths they were written with.

#![allow(
    clippy::disallowed_methods,
    reason = "`git ls-files` is a synchronous child process; waiting on it is the test"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The folders the layout defines.
const TARGET_FOLDERS: [&str; 7] = [
    "crates/gpux_tests_benchmark",
    "crates/gpux_tests_conformance",
    "tools/testkit",
    "tools/perf",
    "tools/browser",
    "tools/android",
    "tools/ios",
];

/// The folders the layout replaced. Older checkouts can keep git-ignored build output in them,
/// so the guard requires only that git tracks nothing there.
const OLD_FOLDERS: [&str; 4] = [
    "tooling",
    "tools-node",
    "benchmark",
    "crates/gpux_conformance",
];

/// Tracked files that keep the paths they were written with: historical records, recorded tool
/// output, the vendored fork, the lock file and this guard.
const HISTORICAL: [&str; 9] = [
    ".agentx/tasks/",
    ".agentx/experiments/",
    "docs/changelog/",
    "docs/history/",
    "docs/measurements/",
    "ToFix/",
    "crates/gpux_tests_benchmark/results/",
    "tools/testkit/tests/fixtures/",
    "vendor/",
];

fn is_scanned(path: &str) -> bool {
    let benchmark_record = path
        .strip_prefix("crates/gpux_tests_benchmark/")
        .is_some_and(|rest| rest == "RESULTS.md" || rest.starts_with("LOOP-"));
    !HISTORICAL.iter().any(|prefix| path.starts_with(prefix))
        && !benchmark_record
        && path != "Cargo.lock"
        && path != "tools/testkit/tests/repository_layout.rs"
}

/// A stale reference: what it names and where it occurs.
#[derive(Debug, PartialEq, Eq)]
struct Stale {
    file: String,
    line: usize,
    pattern: &'static str,
}

impl std::fmt::Display for Stale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.pattern)
    }
}

/// `true` when the occurrence of a path at byte `at` of `line` starts a path of its own: not
/// glued to a preceding letter, digit, `_` or `/`, except a relative link's `../`.
fn starts_a_path(line: &str, at: usize) -> bool {
    let before = &line[..at];
    if before.ends_with("../") {
        return true;
    }
    !before
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/')
}

/// The byte offsets of `needle` in `line`.
fn occurrences<'a>(line: &'a str, needle: &'a str) -> impl Iterator<Item = usize> + 'a {
    line.match_indices(needle).map(|(at, _)| at)
}

/// The stale patterns in one line, each named once.
fn stale_patterns(line: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    // The old roots, as paths of their own: RayX's memory domain `knowledge/tooling/` and
    // `crates/gpux_tests_benchmark/` never match.
    if occurrences(line, "tooling/").any(|at| starts_a_path(line, at)) {
        found.push("tooling/");
    }
    if occurrences(line, "benchmark/").any(|at| starts_a_path(line, at)) {
        found.push("benchmark/");
    }
    // RayX keeps its own `tools-node` folder; a reference to it is not a gpux path.
    if occurrences(line, "tools-node").any(|at| !line[..at].ends_with("RayX/")) {
        found.push("tools-node");
    }
    if line.contains("crates/gpux_conformance") {
        found.push("crates/gpux_conformance");
    } else if line.contains("gpux_conformance") {
        found.push("gpux_conformance");
    }
    // The package name in a cargo position; the runtime protocol identifiers that merely
    // contain it (`gpux-conformance:` result lines, the Simulator name) stay legal.
    if ["-p ", "--exclude ", "package = \"", "name = \""]
        .iter()
        .any(|prefix| line.contains(&format!("{prefix}gpux-conformance")))
    {
        found.push("gpux-conformance (package)");
    }
    found
}

/// The stale references in `text`, reported against `file`.
fn scan(file: &str, text: &str) -> Vec<Stale> {
    text.lines()
        .enumerate()
        .flat_map(|(index, line)| {
            stale_patterns(line).into_iter().map(move |pattern| Stale {
                file: file.to_string(),
                line: index + 1,
                pattern,
            })
        })
        .collect()
}

fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The files git tracks under `paths` (the whole checkout when empty). Fails outside a git
/// checkout instead of passing on an empty list.
fn tracked(root: &Path, paths: &[&str]) -> Vec<String> {
    let output = Command::new("git")
        .arg("ls-files")
        .arg("--")
        .args(paths)
        .current_dir(root)
        .output()
        .unwrap_or_else(|error| panic!("the layout guard needs git on PATH: {error}"));
    assert!(
        output.status.success(),
        "the layout guard needs a git checkout of gpux at {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The stale references in the tracked live files of the checkout at `root`.
fn scan_checkout(root: &Path) -> Vec<Stale> {
    let files = tracked(root, &[]);
    assert!(
        !files.is_empty(),
        "git tracks no files at {}",
        root.display()
    );
    files
        .iter()
        .filter(|file| is_scanned(file))
        .filter_map(|file| {
            // Binary files (goldens, fonts, images) hold no references.
            let text = std::fs::read_to_string(root.join(file)).ok()?;
            Some(scan(file, &text))
        })
        .flatten()
        .collect()
}

fn report(stale: &[Stale]) -> String {
    stale
        .iter()
        .map(Stale::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_target_folders_exist() {
    let root = checkout();
    for folder in TARGET_FOLDERS {
        assert!(root.join(folder).is_dir(), "{folder} is missing");
        assert!(
            !tracked(&root, &[folder]).is_empty(),
            "git tracks nothing in {folder}"
        );
    }
}

#[test]
fn the_old_folders_hold_no_tracked_files() {
    let root = checkout();
    for folder in OLD_FOLDERS {
        assert_eq!(
            tracked(&root, &[folder]),
            Vec::<String>::new(),
            "{folder} still has tracked files"
        );
    }
}

#[test]
fn the_root_workspace_lists_the_moved_crates_but_not_the_benchmark() {
    let manifest = std::fs::read_to_string(checkout().join("Cargo.toml")).unwrap();
    let members = &manifest[manifest.find("members = [").unwrap()..];
    let members = &members[..members.find(']').unwrap()];
    for member in [
        "\"crates/gpux_tests_conformance\"",
        "\"tools/testkit\"",
        "\"tools/perf\"",
    ] {
        assert!(members.contains(member), "{member} is not a member");
    }
    assert!(!members.contains("gpux_tests_benchmark"), "{members}");
    assert!(manifest.contains("path = \"tools/perf\""));
    assert!(manifest.contains(
        "gpux_tests_conformance = { package = \"gpux-tests-conformance\", path = \"crates/gpux_tests_conformance\""
    ));
    // The benchmark stays a workspace of its own.
    let benchmark =
        std::fs::read_to_string(checkout().join("crates/gpux_tests_benchmark/Cargo.toml")).unwrap();
    // Compared by line: Windows checks the manifest out with CRLF line endings.
    assert!(
        benchmark
            .lines()
            .any(|line| line.trim_end() == "[workspace]"),
        "{benchmark}"
    );
    assert!(benchmark.contains("name = \"gpux-tests-benchmark\""));
}

#[test]
fn no_live_file_names_an_old_location() {
    let stale = scan_checkout(&checkout());
    assert!(
        stale.is_empty(),
        "stale references to the old layout:\n{}",
        report(&stale)
    );
}

#[test]
fn historical_records_and_fixtures_are_not_scanned() {
    for path in [
        ".agentx/tasks/gpux-tools-layout/design.json",
        ".agentx/experiments/macos/loop.json",
        "docs/changelog/2026-10.md",
        "docs/history/x.md",
        "docs/measurements/x.md",
        "ToFix/x.md",
        "crates/gpux_tests_benchmark/RESULTS.md",
        "crates/gpux_tests_benchmark/LOOP-2026-10-16.md",
        "crates/gpux_tests_benchmark/results/run/report.md",
        "tools/testkit/tests/fixtures/libtest_lane_a.txt",
        "vendor/wgpu/wgpu/Cargo.toml",
        "Cargo.lock",
    ] {
        assert!(!is_scanned(path), "{path}");
    }
    for path in [
        "README.md",
        "docs/testing.md",
        ".agentx/memory/knowledge/repository/validation/state.md",
        ".github/workflows/lane-b.yml",
        "crates/gpux_tests_benchmark/README.md",
        "tools/testkit/src/web.rs",
    ] {
        assert!(is_scanned(path), "{path}");
    }
}

/// Writes `line` into a temporary file, scans it as the guard scans tracked files, and returns
/// the patterns it found.
fn seeded(line: &str) -> Vec<&'static str> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("seeded.md");
    std::fs::write(&file, format!("first line\n{line}\nlast line\n")).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    let stale = scan("seeded.md", &text);
    assert!(stale.iter().all(|found| found.line == 2), "{stale:?}");
    stale.into_iter().map(|found| found.pattern).collect()
}

#[test]
fn seeded_old_tool_roots_are_found() {
    assert_eq!(seeded("cargo run in `tooling/testkit`"), ["tooling/"]);
    assert_eq!(seeded("[runner](../../tooling/testkit)"), ["tooling/"]);
    assert_eq!(seeded("cd tools-node && pnpm install"), ["tools-node"]);
    // RayX's own folders are not gpux paths.
    assert!(seeded("see knowledge/tooling/validation/state.md").is_empty());
    assert!(seeded("cd ~/rayx/RayX/tools-node").is_empty());
    assert!(seeded("`tools/testkit` and `tools/browser`").is_empty());
}

#[test]
fn seeded_old_benchmark_paths_are_found() {
    assert_eq!(
        seeded("python3 benchmark/run.py --target macos"),
        ["benchmark/"]
    );
    assert_eq!(seeded("(`benchmark/`)"), ["benchmark/"]);
    assert_eq!(seeded("[results](../benchmark/results)"), ["benchmark/"]);
    assert!(seeded("python3 crates/gpux_tests_benchmark/run.py").is_empty());
}

#[test]
fn seeded_old_conformance_names_are_found() {
    assert_eq!(
        seeded("lives in crates/gpux_conformance/src/scenes"),
        ["crates/gpux_conformance"]
    );
    assert_eq!(
        seeded("use gpux_conformance::report;"),
        ["gpux_conformance"]
    );
    assert_eq!(
        seeded("cargo test -p gpux-conformance --test lane_a"),
        ["gpux-conformance (package)"]
    );
    assert_eq!(
        seeded("--exclude gpux-conformance"),
        ["gpux-conformance (package)"]
    );
    assert_eq!(
        seeded("x = { package = \"gpux-conformance\", path = \"x\" }"),
        ["gpux-conformance (package)"]
    );
    assert_eq!(
        seeded("name = \"gpux-conformance\""),
        ["gpux-conformance (package)"]
    );
    // The runtime protocol identifiers and the new names stay legal.
    assert!(seeded("gpux-conformance: pass layout-flex").is_empty());
    assert!(seeded("gpux-conformance-host: background").is_empty());
    assert!(seeded("the Simulator `gpux-conformance`").is_empty());
    assert!(seeded("use gpux_tests_conformance::report; -p gpux-tests-conformance").is_empty());
}

#[test]
fn failures_name_file_line_and_pattern() {
    let stale = scan("docs/x.md", "ok\nsee tools-node/specs\n");
    assert_eq!(report(&stale), "docs/x.md:2: tools-node");
}

#[test]
fn the_guard_fails_outside_a_git_checkout() {
    let outside = tempfile::tempdir().unwrap();
    let result = std::panic::catch_unwind(|| scan_checkout(outside.path()));
    let message = result.expect_err("an empty folder must not pass");
    let message = message
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(message.contains("git checkout"), "{message}");
}

#[test]
fn lane_b_merges_the_shard_reports_it_downloaded() {
    // The report job downloads the shards relative to the checkout and merges them from the
    // Playwright project, so the merge path must climb out of `tools/browser`.
    let workflow =
        std::fs::read_to_string(checkout().join(".github/workflows/lane-b.yml")).unwrap();
    let lines: Vec<&str> = workflow.lines().map(str::trim).collect();
    let download = lines
        .iter()
        .position(|line| *line == "pattern: lane-b-blob-report-*")
        .expect("the shard download");
    let downloaded = lines[download..]
        .iter()
        .find_map(|line| line.strip_prefix("path: "))
        .expect("the download path");
    let merged = lines
        .iter()
        .find_map(|line| line.strip_prefix("run: pnpm exec playwright merge-reports "))
        .and_then(|args| args.split_whitespace().last())
        .expect("the merge command");
    let mut from_project = PathBuf::from("tools/browser");
    for part in merged.split('/') {
        match part {
            ".." => assert!(from_project.pop(), "{merged} leaves the checkout"),
            part => from_project.push(part),
        }
    }
    assert_eq!(from_project, PathBuf::from(downloaded), "{merged}");
}
