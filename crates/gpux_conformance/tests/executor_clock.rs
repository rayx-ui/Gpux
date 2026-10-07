//! Steps that need the executor clock (`Capability::ExecutorClock`) run on the test platform,
//! where time moves only through `Step::Advance`; hosts on the real clock report them skipped
//! with the reason. The tooltip scene checks that its tooltip is still hidden right after the
//! hover only there, because on the real clock a busy machine can outlast the show delay.

use gpux_conformance::{
    Expect, Step, host::run_scene, report::Outcome, scene::Capability, test_host::TestHost,
};

#[test]
fn the_test_platform_holds_the_show_delay_back() {
    let scene = gpux_conformance::scene("tooltip").unwrap();
    let mut host = TestHost::behaviour();
    let result = run_scene(&mut host, scene, 1.);
    assert_eq!(result.outcome, Outcome::Pass, "{result}");
    // Lane A skips the pixel steps, but runs the executor-clock check.
    assert!(!result.detail.contains("real clock"), "{result}");
}

#[test]
fn real_clock_hosts_name_why_they_skip() {
    assert_eq!(
        Capability::ExecutorClock.missing(),
        "the real clock cannot hold a delay back"
    );
    // The renderer capabilities keep their reasons.
    assert_eq!(
        Capability::ElementEffects.missing(),
        "the renderer has no element effects"
    );
    assert_eq!(
        Capability::SnapshotTransitions.missing(),
        "the renderer has no snapshot transitions"
    );
}

#[test]
fn the_tooltip_checks_its_show_delay_only_on_the_executor_clock() {
    let scene = gpux_conformance::scene("tooltip").unwrap();
    let script = (scene.script)();
    let gated = script.iter().find_map(|step| match step {
        Step::When(Capability::ExecutorClock, inner) => Some(inner),
        _ => None,
    });
    let gated = gated.expect("the tooltip scene gates a step on the executor clock");
    assert!(
        matches!(gated.as_slice(), [Step::Expect(Expect::Missing(_))]),
        "{gated:?}"
    );
    // The capture the tooltip is compared with comes before the hover, so a slow capture
    // cannot catch the tooltip.
    let capture = script
        .iter()
        .position(|step| matches!(step, Step::Capture("hidden")))
        .unwrap();
    let hover = script
        .iter()
        .position(|step| matches!(step, Step::Hover(_)))
        .unwrap();
    assert!(capture < hover);
}
