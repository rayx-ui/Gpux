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
//! goldens are skipped. `GPUX_UPDATE_SNAPSHOTS=1` rewrites the goldens of
//! the active backend; failures write actual, expected and diff images under
//! `artifacts-temp/testkit/c/`. When `GPUX_EXPECT_BACKEND` names a backend (`warp`,
//! `lavapipe`, `metal`, ...), every scene must render on it, so CI cannot silently fall back
//! to another adapter and skip its goldens.

use gpux_conformance::{CATALOG, Lane, host::run_scene, report::Outcome, test_host::TestHost};

/// The environment variable naming the backend every scene must render on.
const EXPECT_BACKEND_ENV_VAR: &str = "GPUX_EXPECT_BACKEND";

fn run(id: &str, scale: f32) {
    let scene = gpux_conformance::scene(id).unwrap_or_else(|| panic!("no scene {id}"));
    assert!(scene.runs_in(Lane::C), "{id} is not a lane C scene");
    assert!(
        scene.scales.contains(&scale),
        "{id} does not render at {scale}x"
    );
    let mut host = TestHost::pixels();
    let result = run_scene(&mut host, scene, scale);
    let backend = host
        .backend()
        .map(|backend| backend.label())
        .unwrap_or_else(|| "no renderer".to_string());
    println!("{result} [{backend}]");
    assert_eq!(result.outcome, Outcome::Pass, "{result} [{backend}]");
    if let Some(expected) = std::env::var(EXPECT_BACKEND_ENV_VAR)
        .ok()
        .filter(|value| !value.is_empty())
    {
        assert_eq!(
            backend, expected,
            "{id} rendered on {backend}, but {EXPECT_BACKEND_ENV_VAR} expects {expected}"
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

lane_c! {
    layout_flex => "layout-flex",
    layout_grid => "layout-grid",
    layout_absolute => "layout-absolute",
    layout_overflow => "layout-overflow",
    layout_fixed_sticky => "layout-fixed-sticky",
    text_shaping => "text-shaping",
    text_runs => "text-runs",
    text_decorations => "text-decorations",
    text_wrapping => "text-wrapping",
    text_truncation => "text-truncation",
    text_interactive => "text-interactive",
    editable_text => "editable-text",
    editable_ime => "editable-ime",
    list_uniform => "list-uniform",
    list_variable => "list-variable",
    scroll_wheel => "scroll-wheel",
    scroll_nested => "scroll-nested",
    scroll_scrollbar => "scroll-scrollbar",
    scroll_programmatic => "scroll-programmatic",
    scroll_touch => "scroll-touch",
    focus_tab_order => "focus-tab-order",
    actions_keymaps => "actions-keymaps",
    hover_active => "hover-active",
    drag_drop => "drag-drop",
    tooltip => "tooltip",
    overlay_anchored => "overlay-anchored",
    overlay_deferred => "overlay-deferred",
    media_image => "media-image",
    media_svg => "media-svg",
    paint_canvas_path => "paint-canvas-path",
    paint_borders => "paint-borders",
    paint_shadows_gradients => "paint-shadows-gradients",
    paint_opacity_clipping => "paint-opacity-clipping",
    color_srgb_compositing => "color-srgb-compositing",
    effects_shader => "effects-shader",
    effects_animated_frame => "effects-animated-frame",
    effects_transition_frame => "effects-transition-frame",
    semantics_accessibility => "semantics-accessibility",
}

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
