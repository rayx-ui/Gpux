//! Lane A: every lane-A catalog scene on `TestPlatform`, with the bundled text system, the
//! deterministic executor clock and semantic locators. No GPU or display is needed.
//!
//! Each scene is its own test, so `cargo test -p gpux-tests-conformance --test lane_a <scene>` runs
//! one scene. Pixel steps are skipped here; lane C checks them.

use gpux_tests_conformance::{
    CATALOG, Lane, host::run_scene, report::Outcome, test_host::TestHost,
};

fn run(id: &str) {
    let scene = gpux_tests_conformance::scene(id).unwrap_or_else(|| panic!("no scene {id}"));
    assert!(scene.runs_in(Lane::A), "{id} is not a lane A scene");
    let mut host = TestHost::behaviour();
    let result = run_scene(&mut host, scene, 1.);
    println!("{result}");
    assert_eq!(result.outcome, Outcome::Pass, "{result}");
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
fn every_lane_a_scene_has_a_test() {
    let missing = CATALOG
        .iter()
        .filter(|scene| scene.runs_in(Lane::A) && !LISTED.contains(&scene.id))
        .map(|scene| scene.id)
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "lane A scenes without a test: {missing:?}"
    );
}
