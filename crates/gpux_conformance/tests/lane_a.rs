//! Lane A: every lane-A catalog scene on `TestPlatform`, with the bundled text system, the
//! deterministic executor clock and semantic locators. No GPU or display is needed.
//!
//! Each scene is its own test, so `cargo test -p gpux-conformance --test lane_a <scene>` runs
//! one scene. Pixel steps are skipped here; lane C checks them.

use gpux_conformance::{CATALOG, Lane, host::run_scene, report::Outcome, test_host::TestHost};

fn run(id: &str) {
    let scene = gpux_conformance::scene(id).unwrap_or_else(|| panic!("no scene {id}"));
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

lane_a! {
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
