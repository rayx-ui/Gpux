//! `Step::When`: steps that depend on a renderer capability run where the host's renderer has
//! it and are reported skipped, with the reason, where it does not.

use gpux::semantic_query::Query;
use gpux::{AnyView, App, Window};
use gpux_tests_conformance::{
    Area, Lane, SceneSpec, Step,
    host::{Host, StepOutcome, run_scene},
    report::Outcome,
    scene::{Capability, Expect, flatten},
};

/// A host that records the steps it ran and fails steps probing (13, 13).
struct FakeHost {
    effects: bool,
    ran: Vec<String>,
}

impl Host for FakeHost {
    fn lane(&self) -> Lane {
        Lane::C
    }

    fn open(&mut self, _: &SceneSpec, _: f32) -> anyhow::Result<()> {
        Ok(())
    }

    fn step(&mut self, _: &SceneSpec, step: &Step) -> anyhow::Result<StepOutcome> {
        let description = gpux_tests_conformance::host::describe(step);
        self.ran.push(description);
        match step {
            Step::Probe(x, _, _, _) if *x == 13. => anyhow::bail!("pixel is off"),
            _ => Ok(StepOutcome::Done),
        }
    }

    fn supports(&mut self, capability: Capability) -> bool {
        capability == Capability::ElementEffects && self.effects
    }

    fn close(&mut self) {}
}

fn unused_build(_: &mut Window, _: &mut App) -> AnyView {
    unreachable!("the fake host never builds scenes")
}

fn script() -> Vec<Step> {
    vec![
        Step::Expect(Expect::Exists(Query::id("plain"))),
        Step::When(
            Capability::ElementEffects,
            vec![
                Step::probe(1., 1., [0, 0, 0, 255]),
                Step::When(
                    Capability::SnapshotTransitions,
                    vec![Step::probe(2., 2., [0, 0, 0, 255])],
                ),
            ],
        ),
        Step::probe(3., 3., [0, 0, 0, 255]),
    ]
}

fn failing_script() -> Vec<Step> {
    vec![
        Step::probe(1., 1., [0, 0, 0, 255]),
        Step::When(
            Capability::ElementEffects,
            vec![Step::probe(13., 13., [0, 0, 0, 255])],
        ),
    ]
}

const SCENE: SceneSpec = SceneSpec {
    id: "capability-fixture",
    areas: &[Area::ShaderEffects],
    size: (40., 40.),
    scales: &[1.],
    lanes: &[Lane::C],
    golden: false,
    build: unused_build,
    script,
};

#[test]
fn capable_renderers_run_the_gated_steps() {
    let mut host = FakeHost {
        effects: true,
        ran: Vec::new(),
    };
    let result = run_scene(&mut host, &SCENE, 1.);
    assert_eq!(result.outcome, Outcome::Pass, "{result}");
    // The nested snapshot-transition step is skipped; the element-effect probe ran.
    assert_eq!(host.ran.len(), 3, "{:?}", host.ran);
    assert_eq!(
        result.detail,
        "3 steps, 1 skipped (the renderer has no snapshot transitions)"
    );
}

#[test]
fn other_renderers_skip_the_gated_steps_with_the_reason() {
    let mut host = FakeHost {
        effects: false,
        ran: Vec::new(),
    };
    let result = run_scene(&mut host, &SCENE, 1.);
    assert_eq!(result.outcome, Outcome::Pass, "{result}");
    assert_eq!(host.ran.len(), 2, "{:?}", host.ran);
    assert!(
        result.detail.starts_with("2 steps, 2 skipped (")
            && result
                .detail
                .contains("the renderer has no element effects"),
        "{result}"
    );
}

#[test]
fn failures_inside_gated_steps_name_their_flattened_step_number() {
    let scene = SceneSpec {
        script: failing_script,
        ..SCENE
    };
    let mut host = FakeHost {
        effects: true,
        ran: Vec::new(),
    };
    let result = run_scene(&mut host, &scene, 1.);
    assert_eq!(result.outcome, Outcome::Fail);
    assert!(
        result.detail.starts_with("step 2 probe (13, 13)"),
        "{result}"
    );
    assert_eq!(flatten(&failing_script()).len(), 2);
    assert!(Step::When(Capability::ElementEffects, failing_script()).needs_pixels());
}
