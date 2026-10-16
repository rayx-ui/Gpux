//! Reading test results out of tool output: libtest summaries, `gpux-conformance` result lines
//! (from stdout, Android logcat or a browser console), and Playwright's list reporter.

use gpux_tests_conformance::report::{Outcome, ResultLine};
use std::collections::BTreeMap;

/// Counted results.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Tests that passed.
    pub passed: usize,
    /// Tests that failed.
    pub failed: usize,
    /// Tests that were skipped or ignored.
    pub skipped: usize,
    /// One line per failure, for the report.
    pub failures: Vec<String>,
}

impl Tally {
    /// Adds `other` to this tally.
    pub fn add(&mut self, other: Tally) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.failures.extend(other.failures);
    }

    /// All counted tests.
    pub fn total(&self) -> usize {
        self.passed + self.failed + self.skipped
    }

    /// A tally of one failure.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            failed: 1,
            failures: vec![message.into()],
            ..Self::default()
        }
    }
}

/// Every `gpux-conformance:` result line in `output`, whatever prefix each line carries.
pub fn result_lines(output: &str) -> Vec<ResultLine> {
    output.lines().filter_map(ResultLine::parse).collect()
}

/// Counts the result lines a conformance host printed. A scene reported more than once (a log
/// that repeats a line) counts once, with its last outcome.
pub fn tally_result_lines(output: &str) -> Tally {
    let mut last: BTreeMap<String, ResultLine> = BTreeMap::new();
    for line in result_lines(output) {
        last.insert(line.scene.clone(), line);
    }
    let mut tally = Tally::default();
    for line in last.into_values() {
        match line.outcome {
            Outcome::Pass => tally.passed += 1,
            Outcome::Skip => tally.skipped += 1,
            Outcome::Fail => {
                tally.failed += 1;
                tally
                    .failures
                    .push(format!("{} {}", line.scene, line.detail));
            }
        }
    }
    tally
}

/// Counts libtest's `test result:` summary lines across every test binary in `output`, and
/// names the failed tests.
pub fn tally_libtest(output: &str) -> Tally {
    let mut tally = Tally::default();
    for line in output.lines() {
        let line = line.trim();
        if let Some(summary) = line.strip_prefix("test result: ") {
            let count = |label: &str| {
                summary
                    .split(';')
                    .filter_map(|part| part.trim().rsplit_once(' '))
                    .find(|(_, name)| *name == label)
                    .and_then(|(number, _)| number.rsplit(' ').next()?.parse::<usize>().ok())
                    .unwrap_or(0)
            };
            tally.passed += count("passed");
            tally.failed += count("failed");
            tally.skipped += count("ignored");
        } else if let Some(name) = line
            .strip_prefix("test ")
            .and_then(|rest| rest.strip_suffix(" ... FAILED"))
        {
            tally.failures.push(name.to_string());
        }
    }
    // A failing conformance scene names its failed step in the panic message.
    let mut details = Vec::new();
    for line in result_lines(output) {
        if line.outcome == Outcome::Fail {
            let detail = format!("{} {}", line.scene, line.detail);
            if !details.contains(&detail) {
                details.push(detail);
            }
        }
    }
    tally.failures.extend(details);
    tally
}

/// The test names libtest's `--list` printed (`name: test` lines), in order.
pub fn libtest_list(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.trim().strip_suffix(": test"))
        .map(str::to_string)
        .collect()
}

/// Counts Playwright list-reporter output. The closing counts (`3 passed`, `1 failed`,
/// `1 skipped`, `1 flaky`) win when present; otherwise each test's final status line counts.
pub fn tally_playwright_list(output: &str) -> Tally {
    let mut tally = Tally::default();
    let mut found_counts = false;
    let mut in_failed_list = false;
    let mut failures = Vec::new();
    for raw in output.lines() {
        let line = raw.trim();
        if let Some((number, label)) = line.split_once(' ')
            && let Ok(number) = number.parse::<usize>()
        {
            let label = label.split(" (").next().unwrap_or(label).trim();
            let slot = match label {
                "passed" | "flaky" => Some(&mut tally.passed),
                "failed" | "interrupted" => Some(&mut tally.failed),
                "skipped" | "did not run" => Some(&mut tally.skipped),
                _ => None,
            };
            if let Some(slot) = slot {
                *slot += number;
                found_counts = true;
                in_failed_list = label == "failed";
                continue;
            }
        }
        if in_failed_list && line.starts_with('[') {
            failures.push(line.to_string());
        }
    }
    if found_counts {
        tally.failures = failures;
        return tally;
    }
    // No closing counts (an interrupted run): use each test's last status line.
    let mut statuses: BTreeMap<String, (char, String)> = BTreeMap::new();
    for raw in output.lines() {
        let line = raw.trim_start();
        let (status, rest) = if let Some(rest) = line.strip_prefix("✓") {
            ('p', rest)
        } else if let Some(rest) = line.strip_prefix("ok ") {
            ('p', rest)
        } else if let Some(rest) = line.strip_prefix("✘").or_else(|| line.strip_prefix("×")) {
            ('f', rest)
        } else if let Some(rest) = line.strip_prefix("x ") {
            ('f', rest)
        } else if let Some(rest) = line.strip_prefix("- ") {
            ('s', rest)
        } else {
            continue;
        };
        let rest = rest.trim_start();
        // `<n> [project] › file:line › title (duration)`, retries add ` (retry #1)`.
        let Some((_, title)) = rest.split_once(' ') else {
            continue;
        };
        if !title.starts_with('[') {
            continue;
        }
        let key = title.split(" (").next().unwrap_or(title).trim().to_string();
        statuses.insert(key.clone(), (status, key));
    }
    for (status, title) in statuses.into_values() {
        match status {
            'p' => tally.passed += 1,
            'f' => {
                tally.failed += 1;
                tally.failures.push(title);
            }
            _ => tally.skipped += 1,
        }
    }
    tally
}
