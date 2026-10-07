//! Hosts run scene scripts. Every host executes the same steps; a host that cannot perform a
//! step (pixel probes without a renderer, for example) reports it skipped with a reason.

use crate::{
    report::{Outcome, ResultLine},
    scene::{Lane, SceneSpec, Step},
};

/// What a host did with one step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// The step ran and held.
    Done,
    /// The host cannot run the step; the reason says why.
    Skipped(String),
}

/// Runs scene scripts on one platform.
pub trait Host {
    /// The lane this host proves.
    fn lane(&self) -> Lane;

    /// Opens `scene` at `scale` in a fresh window.
    fn open(&mut self, scene: &SceneSpec, scale: f32) -> anyhow::Result<()>;

    /// Runs one step, failing with an explanation when it does not hold.
    fn step(&mut self, scene: &SceneSpec, step: &Step) -> anyhow::Result<StepOutcome>;

    /// Closes the scene's window.
    fn close(&mut self);
}

/// Runs `scene` at `scale` on `host` and returns its result line.
pub fn run_scene(host: &mut dyn Host, scene: &SceneSpec, scale: f32) -> ResultLine {
    let id = scene.result_id(scale);
    if !scene.runs_in(host.lane()) {
        return ResultLine::new(
            Outcome::Skip,
            id,
            format!("not a lane {} scene", host.lane().as_str()),
        );
    }
    if let Err(error) = host.open(scene, scale) {
        return ResultLine::new(Outcome::Fail, id, format!("could not open: {error:#}"));
    }
    let script = (scene.script)();
    let mut skipped = Vec::new();
    for (index, step) in script.iter().enumerate() {
        match host.step(scene, step) {
            Ok(StepOutcome::Done) => {}
            Ok(StepOutcome::Skipped(reason)) => skipped.push(reason),
            Err(error) => {
                host.close();
                return ResultLine::new(
                    Outcome::Fail,
                    id,
                    format!("step {} {}: {error:#}", index + 1, describe(step)),
                );
            }
        }
    }
    host.close();
    let ran = script.len() - skipped.len();
    let mut detail = format!("{ran} steps");
    if !skipped.is_empty() {
        skipped.sort();
        skipped.dedup();
        detail.push_str(&format!(
            ", {} skipped ({})",
            script.len() - ran,
            skipped.join("; ")
        ));
    }
    ResultLine::new(Outcome::Pass, id, detail)
}

/// A short description of a step for failure details.
pub fn describe(step: &Step) -> String {
    match step {
        Step::Click(query) => format!("click `{query}`"),
        Step::ClickCount(query, count) => format!("click x{count} `{query}`"),
        Step::Hover(query) => format!("hover `{query}`"),
        Step::MoveTo(x, y) => format!("move to ({x}, {y})"),
        Step::ClickAt(x, y) => format!("click at ({x}, {y})"),
        Step::Drag(from, to) => format!("drag `{from}` to `{to}`"),
        Step::Wheel(query, dx, dy) => format!("wheel ({dx}, {dy}) over `{query}`"),
        Step::TouchPan(query, dx, dy, steps) => {
            format!("touch pan ({dx}, {dy}) in {steps} frames from `{query}`")
        }
        Step::TouchTap(query) => format!("tap `{query}`"),
        Step::Type(query, text) => format!("type {text:?} into `{query}`"),
        Step::Keys(keys) => format!("press {keys:?}"),
        Step::KeyUp(key) => format!("release {key:?}"),
        Step::ImeCompose(text, caret) => format!("compose {text:?} (caret {caret})"),
        Step::ImeCommit(text) => format!("commit {text:?}"),
        Step::ImeCancel => "cancel composition".to_string(),
        Step::Focus(query) => format!("focus `{query}`"),
        Step::Advance(duration) => format!("advance {duration:?}"),
        Step::Expect(expect) => format!("expect {expect:?}"),
        Step::Probe(x, y, color, _) => format!("probe ({x}, {y}) is {color:?}"),
        Step::ProbeUniform(rect, color, _) => format!("probe {rect:?} is uniformly {color:?}"),
        Step::ProbeDiffer(a, b, _) => format!("probe {a:?} differs from {b:?}"),
        Step::Capture(name) => format!("capture {name:?}"),
        Step::ChangedSince(name, rect) => format!("{rect:?} changed since {name:?}"),
        Step::Golden => "golden".to_string(),
    }
}
