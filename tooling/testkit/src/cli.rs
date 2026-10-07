//! Command-line parsing.

use crate::lanes::LaneId;
use std::fmt;

/// The usage text printed by `--help` and after usage errors.
pub const USAGE: &str = "\
usage: gpux-testkit lane <lane>... [--list] [--scene <id>] [--grep <pattern>]

lanes:
  a  behaviour on the test platform (no GPU or display)
  b  a real browser (wired in a later sprint)
  c  offscreen pixels through the headless renderer
  d  real desktop windows
  e  Android (wired in a later sprint)
  f  iOS (wired in a later sprint)
  g  the local real GPU (wired in a later sprint)

options:
  --list            print the exact test selection without running it
  --scene <id>      select one conformance scene
  --grep <pattern>  select the tests whose names contain the pattern";

/// A parsed command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Prints the usage.
    Help,
    /// Runs or lists lanes.
    Lane(LaneArgs),
}

/// The arguments of `lane`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneArgs {
    /// The lanes, in the order given, without repeats.
    pub lanes: Vec<LaneId>,
    /// Prints the selection instead of running it.
    pub list: bool,
    /// The selection within each lane.
    pub selection: Selection,
}

/// Which tests of a lane to run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// One conformance scene id.
    pub scene: Option<String>,
    /// A substring of test or scene names.
    pub grep: Option<String>,
}

impl Selection {
    /// Whether nothing narrows the lane.
    pub fn is_everything(&self) -> bool {
        self.scene.is_none() && self.grep.is_none()
    }
}

/// A command line that cannot run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageError(pub String);

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

/// Parses the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None | Some("help" | "--help" | "-h") => Ok(Command::Help),
        Some("lane") => parse_lane(args).map(Command::Lane),
        Some(other) => Err(UsageError(format!(
            "unknown command `{other}`; the command is `lane`"
        ))),
    }
}

fn parse_lane(mut args: impl Iterator<Item = String>) -> Result<LaneArgs, UsageError> {
    let mut lanes = Vec::new();
    let mut list = false;
    let mut selection = Selection::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--scene" | "--grep" => {
                let value = args
                    .next()
                    .filter(|value| !value.starts_with("--") && !value.is_empty())
                    .ok_or_else(|| UsageError(format!("`{arg}` needs a value")))?;
                let slot = if arg == "--scene" {
                    &mut selection.scene
                } else {
                    &mut selection.grep
                };
                if slot.replace(value).is_some() {
                    return Err(UsageError(format!("`{arg}` is given twice")));
                }
            }
            flag if flag.starts_with('-') => {
                return Err(UsageError(format!("unknown option `{flag}`")));
            }
            lane => {
                let lane = LaneId::parse(lane)?;
                if !lanes.contains(&lane) {
                    lanes.push(lane);
                }
            }
        }
    }
    if lanes.is_empty() {
        return Err(UsageError(
            "name at least one lane, for example `gpux-testkit lane a c`".into(),
        ));
    }
    Ok(LaneArgs {
        lanes,
        list,
        selection,
    })
}
