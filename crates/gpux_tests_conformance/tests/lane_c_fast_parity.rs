//! Fast-mode parity in lane C: every lane-C catalog scene renders twice through the platform's
//! headless renderer (the software adapter on Linux and Windows unless `GPUX_WGPU_ADAPTER` picks
//! another, Metal on macOS), once with every fast-mode mechanism on and once with all of them
//! off. Both runs must pass, and the pixels the window shows when the scene opens and after every
//! step must be byte-identical.
//!
//! The scroll-layers switch also snaps scroll offsets, glyph origins and paths to device pixels.
//! A scene whose pixels that snapping moves is listed in [`SNAPPED`] with the reason, and is
//! compared with a layers-off run that snaps alike; no scene needs it yet.
//!
//! ```text
//! cargo test -p gpux-tests-conformance --test lane_c_fast_parity -- --test-threads=1
//! ```

use gpux::FastMode;
use gpux_tests_conformance::{
    CATALOG, Lane,
    host::run_scene,
    report::Outcome,
    test_host::{RecordedFrame, TestHost},
};

/// Scenes whose pixels scroll-layer snapping moves, with why. They compare the layers-on run
/// with a layers-off run that snaps alike.
const SNAPPED: &[(&str, &str)] = &[];

/// Renders scene `id` at `scale` in `mode` and returns the frames it showed.
fn frames(id: &str, scale: f32, mode: FastMode) -> Vec<RecordedFrame> {
    let scene = gpux_tests_conformance::scene(id).unwrap_or_else(|| panic!("no scene {id}"));
    assert!(scene.runs_in(Lane::C), "{id} is not a lane C scene");
    // Parity compares the two runs with each other, so goldens are not needed.
    let mut host = TestHost::pixels()
        .probes_only()
        .with_fast_mode(mode)
        .recording_frames();
    let result = run_scene(&mut host, scene, scale);
    assert_eq!(
        result.outcome,
        Outcome::Pass,
        "fast mode {mode:?} at {scale}x: {result}"
    );
    host.recorded_frames().to_vec()
}

fn run(id: &str, scale: f32) {
    assert!(
        !SNAPPED.iter().any(|(snapped, _)| *snapped == id),
        "{id} needs the snapped layers-off reference, which lands with scroll layers"
    );
    let off = frames(id, scale, FastMode::OFF);
    let on = frames(id, scale, FastMode::ALL);
    assert_eq!(
        off.iter().map(|frame| frame.step).collect::<Vec<_>>(),
        on.iter().map(|frame| frame.step).collect::<Vec<_>>(),
        "{id} ran different steps with fast mode on"
    );
    for (off, on) in off.iter().zip(&on) {
        let (Some(off_pixels), Some(on_pixels)) = (&off.pixels, &on.pixels) else {
            panic!("{id} rendered no pixels after step {}", off.step);
        };
        assert_eq!(
            off_pixels.dimensions(),
            on_pixels.dimensions(),
            "{id} at {scale}x, after step {}: the frames differ in size",
            off.step
        );
        if let Some(index) = off_pixels
            .as_raw()
            .iter()
            .zip(on_pixels.as_raw())
            .position(|(off, on)| off != on)
        {
            let pixel = index / 4;
            let width = off_pixels.width() as usize;
            panic!(
                "{id} at {scale}x, after step {}: fast mode on differs from off at pixel \
                 ({}, {}): {:?} against {:?}",
                off.step,
                pixel % width,
                pixel / width,
                on_pixels.get_pixel((pixel % width) as u32, (pixel / width) as u32),
                off_pixels.get_pixel((pixel % width) as u32, (pixel / width) as u32),
            );
        }
    }
}

macro_rules! lane_c {
    ($($name:ident => $id:literal,)*) => {
        /// Every scene at scale 1.
        mod at_1x {
            $(
                #[test]
                fn $name() {
                    super::run($id, 1.);
                }
            )*
        }

        /// Every scene at scale 2.
        mod at_2x {
            $(
                #[test]
                fn $name() {
                    super::run($id, 2.);
                }
            )*
        }

        const LISTED: &[&str] = &[$($id),*];
    };
}

include!("catalog/lane_c_scenes.rs");
lane_c_scenes!(lane_c);

#[test]
fn every_lane_c_scene_runs_with_fast_mode_on_and_off() {
    let missing = CATALOG
        .iter()
        .filter(|scene| scene.runs_in(Lane::C) && !LISTED.contains(&scene.id))
        .map(|scene| scene.id)
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "lane C scenes without a parity test: {missing:?}"
    );
    for (id, reason) in SNAPPED {
        assert!(LISTED.contains(id), "{id} is snapped but not listed");
        assert!(!reason.is_empty(), "{id} is snapped without a reason");
    }
}
