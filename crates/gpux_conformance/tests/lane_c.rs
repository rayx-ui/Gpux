//! Lane C: every lane-C catalog scene rendered offscreen through the platform's headless
//! renderer at scale 1 and 2, asserting the scenes' relative pixel probes and, for the curated
//! golden subset, the committed per-backend goldens.
//!
//! It renders on the host's software adapter unless `GPUX_WGPU_ADAPTER` picks another, one scene
//! at a time:
//!
//! ```text
//! cargo test -p gpux-conformance --test lane_c -- --test-threads=1
//! ```
//!
//! That is WARP on Windows and lavapipe on Linux (select it with `VK_DRIVER_FILES`), and Metal
//! on macOS. `GPUX_WGPU_ADAPTER=hardware` renders on a GPU instead (lane G): the probes run and
//! goldens are skipped. `GPUX_PROBES_ONLY=1` skips the goldens on any backend.
//! `GPUX_UPDATE_SNAPSHOTS=1` rewrites the goldens of the active backend; failures write actual,
//! expected and diff images under `artifacts-temp/testkit/c/`. When `GPUX_EXPECT_BACKEND` names a
//! backend (`warp`, `lavapipe`, `metal`, ..., or `hardware` for any GPU), every scene must
//! render on it, so a run cannot silently fall back to another adapter and skip its goldens.

use gpux_conformance::{
    CATALOG, Lane,
    backend::EXPECT_BACKEND_ENV_VAR,
    compare::{PROBES_ONLY_ENV_VAR, switched_on},
    host::run_scene,
    report::Outcome,
    test_host::TestHost,
};

fn run(id: &str, scale: f32) {
    let scene = gpux_conformance::scene(id).unwrap_or_else(|| panic!("no scene {id}"));
    assert!(scene.runs_in(Lane::C), "{id} is not a lane C scene");
    assert!(
        scene.scales.contains(&scale),
        "{id} does not render at {scale}x"
    );
    let mut host = TestHost::pixels();
    if switched_on(std::env::var(PROBES_ONLY_ENV_VAR).ok().as_deref()) {
        host = host.probes_only();
    }
    let result = run_scene(&mut host, scene, scale);
    let label = host
        .backend()
        .map(|backend| backend.label())
        .unwrap_or_else(|| "no renderer".to_string());
    println!("{result} [{label}]");
    assert_eq!(result.outcome, Outcome::Pass, "{result} [{label}]");
    if let Some(expected) = std::env::var(EXPECT_BACKEND_ENV_VAR)
        .ok()
        .filter(|value| !value.is_empty())
    {
        assert!(
            host.backend().is_some_and(|backend| backend.is(&expected)),
            "{id} rendered on {label}, but {EXPECT_BACKEND_ENV_VAR} expects {expected}"
        );
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
fn every_lane_c_scene_has_a_test_at_both_scales() {
    let missing = CATALOG
        .iter()
        .filter(|scene| scene.runs_in(Lane::C) && !LISTED.contains(&scene.id))
        .map(|scene| scene.id)
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "lane C scenes without a test: {missing:?}"
    );
    let single_scale = CATALOG
        .iter()
        .filter(|scene| scene.runs_in(Lane::C) && scene.scales != [1., 2.])
        .map(|scene| scene.id)
        .collect::<Vec<_>>();
    assert!(
        single_scale.is_empty(),
        "lane C renders every scene at 1x and 2x: {single_scale:?}"
    );
    let not_lane_c = LISTED
        .iter()
        .filter(|id| !gpux_conformance::scene(id).is_some_and(|scene| scene.runs_in(Lane::C)))
        .collect::<Vec<_>>();
    assert!(
        not_lane_c.is_empty(),
        "listed but not lane C: {not_lane_c:?}"
    );
}
