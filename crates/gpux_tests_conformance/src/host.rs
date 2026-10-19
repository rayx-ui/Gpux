//! Hosts run scene scripts. Every host executes the same steps; a host that cannot perform a
//! step (pixel probes without a renderer, for example) reports it skipped with a reason.

use crate::{
    report::{Outcome, ResultLine},
    scene::{Capability, Lane, SceneSpec, Step, flatten},
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

    /// Runs one step, failing with an explanation when it does not hold. [`Step::When`] never
    /// reaches a host: [`run_scene`] asks [`Host::supports`] and runs or skips its steps.
    fn step(&mut self, scene: &SceneSpec, step: &Step) -> anyhow::Result<StepOutcome>;

    /// Whether the open scene's renderer has `capability`. Hosts without pixels have none.
    fn supports(&mut self, capability: Capability) -> bool;

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
    let total = flatten(&script).len();
    let mut skipped = Vec::new();
    let mut number = 0;
    let result = run_steps(host, scene, &script, &mut number, &mut skipped);
    host.close();
    if let Err(detail) = result {
        return ResultLine::new(Outcome::Fail, id, detail);
    }
    let ran = total - skipped.len();
    let mut detail = format!("{ran} steps");
    if !skipped.is_empty() {
        let count = skipped.len();
        skipped.sort();
        skipped.dedup();
        detail.push_str(&format!(", {count} skipped ({})", skipped.join("; ")));
    }
    ResultLine::new(Outcome::Pass, id, detail)
}

/// Runs `steps` in order, numbering them as the flattened script does. Returns the failure
/// detail of the first step that does not hold.
fn run_steps(
    host: &mut dyn Host,
    scene: &SceneSpec,
    steps: &[Step],
    number: &mut usize,
    skipped: &mut Vec<String>,
) -> Result<(), String> {
    for step in steps {
        if let Step::When(capability, inner) = step {
            if host.supports(*capability) {
                run_steps(host, scene, inner, number, skipped)?;
            } else {
                let count = flatten(inner).len();
                *number += count;
                skipped.extend(std::iter::repeat_n(capability.missing(), count));
            }
            continue;
        }
        *number += 1;
        match host.step(scene, step) {
            Ok(StepOutcome::Done) => {}
            Ok(StepOutcome::Skipped(reason)) => skipped.push(reason),
            Err(error) => {
                return Err(format!("step {} {}: {error:#}", number, describe(step)));
            }
        }
    }
    Ok(())
}

/// A short description of a step for failure details.
pub fn describe(step: &Step) -> String {
    match step {
        Step::Click(query) => format!("click `{query}`"),
        Step::ClickCount(query, count) => format!("click x{count} `{query}`"),
        Step::Hover(query) => format!("hover `{query}`"),
        Step::MoveTo(x, y) => format!("move to ({x}, {y})"),
        Step::ClickAt(x, y) => format!("click at ({x}, {y})"),
        Step::ClickCountAt(x, y, count) => format!("click x{count} at ({x}, {y})"),
        Step::Press(query) => format!("press `{query}`"),
        Step::Release(query) => format!("release `{query}`"),
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
        Step::EvenRing(query, at_1x, at_2x) => {
            format!("ring `{query}` is even within {at_1x} at 1x, {at_2x} at 2x")
        }
        Step::Capture(name) => format!("capture {name:?}"),
        Step::ChangedSince(name, rect) => format!("{rect:?} changed since {name:?}"),
        Step::Golden => "golden".to_string(),
        Step::When(capability, steps) => format!(
            "when the host has {}: {} steps",
            capability.describe(),
            steps.len()
        ),
        Step::GoldenWithin(tolerance) => format!(
            "golden within ±{} and {}% ({})",
            tolerance.channel_tolerance,
            tolerance.max_differing_ratio * 100.,
            tolerance.reason
        ),
    }
}
