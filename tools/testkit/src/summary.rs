//! The summary line every command ends with, and the exit code that goes with it.

use crate::results::Tally;
use std::fmt;

/// The prefix of the summary line.
pub const PREFIX: &str = "gpux-testkit:";

/// Exit code for a run with a failure or nothing selected.
pub const EXIT_FAILED: i32 = 1;
/// Exit code for a command line that cannot run.
pub const EXIT_USAGE: i32 = 2;

/// The counts a command reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Tests that passed.
    pub passed: usize,
    /// Tests that failed, including lanes that could not run.
    pub failed: usize,
    /// Tests that were skipped.
    pub skipped: usize,
}

impl Summary {
    /// The counts of `tally`.
    pub fn of(tally: &Tally) -> Self {
        Self {
            passed: tally.passed,
            failed: tally.failed,
            skipped: tally.skipped,
        }
    }

    /// 0 when something ran and nothing failed, [`EXIT_FAILED`] otherwise. `listed` counts tests
    /// a `--list` command selected without running them.
    pub fn exit_code(&self, listed: usize) -> i32 {
        let selected = self.passed + self.failed + self.skipped + listed;
        if self.failed > 0 || selected == 0 {
            EXIT_FAILED
        } else {
            0
        }
    }

    /// Parses a summary line, as CI and checks read it.
    pub fn parse(line: &str) -> Option<Self> {
        let rest = line.trim().strip_prefix(PREFIX)?.trim();
        let mut counts = rest.split(", ").map(|part| {
            let (number, label) = part.split_once(' ')?;
            Some((number.parse::<usize>().ok()?, label))
        });
        let (passed, "passed") = counts.next()?? else {
            return None;
        };
        let (failed, "failed") = counts.next()?? else {
            return None;
        };
        let (skipped, "skipped") = counts.next()?? else {
            return None;
        };
        Some(Self {
            passed,
            failed,
            skipped,
        })
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{PREFIX} {} passed, {} failed, {} skipped",
            self.passed, self.failed, self.skipped
        )
    }
}
