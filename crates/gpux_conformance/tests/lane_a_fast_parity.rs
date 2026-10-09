//! Fast-mode parity in lane A: every lane-A catalog scene runs twice on `TestPlatform`, once
//! with every fast-mode mechanism on and once with all of them off. Both runs must pass, and
//! the frame the window shows when the scene opens and after every step must describe alike
//! ([`gpux::Window::describe_rendered_frame`]): what fast mode reuses must be what drawing from
//! scratch would draw.
//!
//! `cargo test -p gpux-conformance --test lane_a_fast_parity <scene>` runs one scene.

use gpux::FastMode;
use gpux_conformance::{
    CATALOG, Lane,
    host::run_scene,
    report::Outcome,
    test_host::{RecordedFrame, TestHost},
};

/// Runs scene `id` at scale 1 in `mode` and returns the frames it showed.
fn frames(id: &str, mode: FastMode) -> Vec<RecordedFrame> {
    let scene = gpux_conformance::scene(id).unwrap_or_else(|| panic!("no scene {id}"));
    assert!(scene.runs_in(Lane::A), "{id} is not a lane A scene");
    let mut host = TestHost::behaviour()
        .with_fast_mode(mode)
        .recording_frames();
    let result = run_scene(&mut host, scene, 1.);
    assert_eq!(
        result.outcome,
        Outcome::Pass,
        "fast mode {mode:?}: {result}"
    );
    host.recorded_frames().to_vec()
}

fn run(id: &str) {
    let off = frames(id, FastMode::OFF);
    let on = frames(id, FastMode::ALL);
    assert!(!off.is_empty(), "{id} recorded no frame");
    assert_eq!(
        off.iter().map(|frame| frame.step).collect::<Vec<_>>(),
        on.iter().map(|frame| frame.step).collect::<Vec<_>>(),
        "{id} ran different steps with fast mode on"
    );
    for (off, on) in off.iter().zip(&on) {
        if let Some(difference) = gpux::first_frame_difference(&on.description, &off.description) {
            panic!(
                "{id}, after step {}: fast mode on (actual) differs from off (expected): \
                 {difference}",
                off.step
            );
        }
    }
}

macro_rules! lane_a {
    ($($name:ident => $id:literal,)*) => {
        $(
            #[test]
            fn $name() {
                run($id);
            }
        )*

        const LISTED: &[&str] = &[$($id),*];
    };
}

include!("catalog/lane_a_scenes.rs");
lane_a_scenes!(lane_a);

#[test]
fn every_lane_a_scene_runs_with_fast_mode_on_and_off() {
    let missing = CATALOG
        .iter()
        .filter(|scene| scene.runs_in(Lane::A) && !LISTED.contains(&scene.id))
        .map(|scene| scene.id)
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "lane A scenes without a parity test: {missing:?}"
    );
}
