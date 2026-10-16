//! Per-test durations of a libtest run, derived from when each completion line arrived.
//!
//! libtest prints `test <name> ... ok` once a test finishes. In a serial run
//! (`--test-threads=1`) the time between two completion lines is the later test's duration, so no
//! unstable libtest flag (`--report-time`, `--format json`) is needed. The report lands in
//! `artifacts-temp/testkit/<lane>/timings.md`.

use std::fmt::Write as _;
use std::time::Duration;

/// One output line with the time it arrived, measured from the start of its process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedLine {
    /// When the line arrived.
    pub at: Duration,
    /// The line, without its newline.
    pub line: String,
}

/// How many of the slowest tests the report lists.
pub const SLOWEST: usize = 20;

/// One completed test and how long it took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestTiming {
    /// The test target (`lane_c_scroll_layers`), when cargo named it before the test ran.
    pub target: String,
    /// The test's name as libtest printed it.
    pub name: String,
    /// The time between the previous completion (or the start of its binary) and this one.
    pub duration: Duration,
    /// Whether it passed.
    pub passed: bool,
}

impl TestTiming {
    fn full_name(&self) -> String {
        if self.target.is_empty() {
            self.name.clone()
        } else {
            format!("{}::{}", self.target, self.name)
        }
    }
}

/// The completed tests of `lines`, with their durations. Each test binary starts its own clock at
/// its `running N tests` line, so compiling and starting the next binary is not charged to its
/// first test. Ignored tests did not run and are left out.
pub fn durations(lines: &[TimedLine]) -> Vec<TestTiming> {
    let mut timings = Vec::new();
    let mut target = String::new();
    let mut previous = Duration::ZERO;
    for timed in lines {
        let line = timed.line.trim();
        if let Some(rest) = line.strip_prefix("Running ") {
            target = target_of(rest);
        } else if line.starts_with("running ") && line.contains(" test") {
            previous = timed.at;
        } else if let Some(rest) = line.strip_prefix("test ")
            && let Some((name, status)) = rest.rsplit_once(" ... ")
        {
            let passed = match status {
                "ok" => true,
                "FAILED" => false,
                _ => continue,
            };
            timings.push(TestTiming {
                target: target.clone(),
                name: name.to_string(),
                duration: timed.at.saturating_sub(previous),
                passed,
            });
            previous = timed.at;
        }
    }
    timings
}

/// The test target named by cargo's `Running tests/lane_c.rs (target/debug/deps/lane_c-hash)`.
fn target_of(rest: &str) -> String {
    let path = rest.split(" (").next().unwrap_or(rest).trim();
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file.strip_suffix(".rs").unwrap_or(file).to_string()
}

/// One process's timed output, with the label of the process that produced it.
pub struct ProcessTimeline {
    /// The invocation's label.
    pub label: String,
    /// Its lines in arrival order.
    pub lines: Vec<TimedLine>,
}

/// The `timings.md` for a lane run: one block per process, slowest tests first with their share
/// of the block's time, then the slowest test targets. A run in which no test completed has no
/// table; the report says so and why.
pub fn report(lane: &str, processes: &[ProcessTimeline]) -> String {
    let mut text = format!("# Test timings of lane {lane}\n\n");
    let mut any = false;
    for process in processes {
        let timings = durations(&process.lines);
        let _ = writeln!(text, "## {}\n", process.label);
        if timings.is_empty() {
            let _ = writeln!(
                text,
                "No test completed in this process, so there is nothing to time (it was a build \
                 error, a crash, or its selection matched no test).\n"
            );
            continue;
        }
        any = true;
        let total: Duration = timings.iter().map(|timing| timing.duration).sum();
        let _ = writeln!(
            text,
            "{} tests, {} in total. Each duration is the time between consecutive completion \
             lines, so with concurrent tests inside one process it is only a bound.\n",
            timings.len(),
            seconds(total)
        );
        let mut slowest: Vec<&TestTiming> = timings.iter().collect();
        slowest.sort_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.full_name().cmp(&b.full_name()))
        });
        let _ = writeln!(text, "| Test | Duration | Share |\n| --- | ---: | ---: |");
        for timing in slowest.iter().take(SLOWEST) {
            let _ = writeln!(
                text,
                "| `{}`{} | {} | {} |",
                timing.full_name(),
                if timing.passed { "" } else { " (failed)" },
                seconds(timing.duration),
                share(timing.duration, total)
            );
        }
        let _ = writeln!(text, "\n| Group | Tests | Duration | Share |");
        let _ = writeln!(text, "| --- | ---: | ---: | ---: |");
        let mut groups: Vec<(String, usize, Duration)> = Vec::new();
        for timing in &timings {
            let key = if timing.target.is_empty() {
                "(unnamed)".to_string()
            } else {
                timing.target.clone()
            };
            match groups.iter_mut().find(|group| group.0 == key) {
                Some(group) => {
                    group.1 += 1;
                    group.2 += timing.duration;
                }
                None => groups.push((key, 1, timing.duration)),
            }
        }
        groups.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        for (name, count, duration) in groups {
            let _ = writeln!(
                text,
                "| `{name}` | {count} | {} | {} |",
                seconds(duration),
                share(duration, total)
            );
        }
        text.push('\n');
    }
    if !any && processes.is_empty() {
        text.push_str("No process ran, so there is nothing to time.\n");
    }
    text
}

fn seconds(duration: Duration) -> String {
    format!("{:.2} s", duration.as_secs_f64())
}

fn share(part: Duration, total: Duration) -> String {
    if total.is_zero() {
        "0.0%".to_string()
    } else {
        format!("{:.1}%", 100.0 * part.as_secs_f64() / total.as_secs_f64())
    }
}
