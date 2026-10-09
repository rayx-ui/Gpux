//! The result-line protocol hosts print and `gpux-testkit` parses.
//!
//! Every scene run ends in one line, `gpux-conformance: PASS|FAIL|SKIP <scene> <detail>`. The
//! marker may appear anywhere in a line, so the same parser reads stdout, Android logcat and
//! browser console output, whatever prefix those add.

use std::fmt;

/// The marker that starts a result.
pub const MARKER: &str = "gpux-conformance:";

/// How a scene run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// Every step passed.
    Pass,
    /// A step failed.
    Fail,
    /// The host could not run the scene; the detail says why.
    Skip,
}

impl Outcome {
    /// The protocol word: `PASS`, `FAIL` or `SKIP`.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Pass => "PASS",
            Outcome::Fail => "FAIL",
            Outcome::Skip => "SKIP",
        }
    }
}

/// One scene result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultLine {
    /// How the run ended.
    pub outcome: Outcome,
    /// The scene id, with the scale when a scene runs at several (`borders@2x`).
    pub scene: String,
    /// A one-line explanation; empty for a plain pass.
    pub detail: String,
}

impl ResultLine {
    /// A result for `scene`.
    pub fn new(outcome: Outcome, scene: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            outcome,
            scene: scene.into(),
            detail: detail.into(),
        }
    }

    /// Parses the result in `line`, ignoring any text before the marker. Returns `None` when
    /// the line holds no result.
    pub fn parse(line: &str) -> Option<Self> {
        let rest = line[line.find(MARKER)? + MARKER.len()..].trim_start();
        let (outcome, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let outcome = match outcome {
            "PASS" => Outcome::Pass,
            "FAIL" => Outcome::Fail,
            "SKIP" => Outcome::Skip,
            _ => return None,
        };
        let rest = rest.trim_start();
        let (scene, detail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if scene.is_empty() {
            return None;
        }
        Some(Self::new(outcome, scene, detail.trim()))
    }
}

impl fmt::Display for ResultLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{MARKER} {} {}", self.outcome.as_str(), self.scene)?;
        if !self.detail.is_empty() {
            // Results are one line each.
            write!(f, " {}", self.detail.replace(['\r', '\n'], " "))?;
        }
        Ok(())
    }
}
