//! gpux's own conformance suite: one catalog of deterministic scenes and interaction scripts
//! that every platform host runs, plus the image comparison and result protocol around it.
//!
//! - [`scene`] defines scenes, lanes, coverage areas and script steps; [`scenes::CATALOG`]
//!   holds every scene.
//! - [`host`] runs scripts; `test_host` (feature `test-host`, native only) runs them on the
//!   test platform for lane A (behaviour) and lane C (offscreen pixels).
//! - [`probe`] and [`compare`] assert pixels: relative probes everywhere, per-backend goldens
//!   for a curated subset.
//! - [`report`] is the `gpux-conformance: PASS|FAIL|SKIP <scene> <detail>` line protocol that
//!   `gpux-testkit` parses from stdout, logcat and browser consoles.
//!
//! Scenes use only the bundled fonts, no network, and the executor clock, so every host sees the
//! same layout and timing.

#[cfg(feature = "app-host")]
pub mod app_host;
pub mod backend;
pub mod compare;
pub mod devtools;
pub mod evaluate;
pub mod host;
pub mod platform_checks;
pub mod probe;
pub mod report;
pub mod scene;
pub mod scenes;
#[cfg(all(feature = "app-host", feature = "diagnostics"))]
pub mod scroll_layers;
#[cfg(all(feature = "test-host", not(target_family = "wasm")))]
pub mod test_host;

pub use scene::{Area, Expect, Lane, SceneSpec, Step};
pub use scenes::CATALOG;

/// The scene with this id.
pub fn scene(id: &str) -> Option<&'static SceneSpec> {
    CATALOG.iter().find(|scene| scene.id == id)
}

/// Whether the test `id` (a scene or platform check id) is in a selection. `scene` names one
/// id exactly; `grep` keeps ids that contain it, as written (`layout-flex`) or as the snake_case
/// test name (`layout_flex`); both apply when both are given. `gpux-testkit` plans lanes with it
/// and every host selects with it, so a listing and its run agree.
pub fn selects(id: &str, scene: Option<&str>, grep: Option<&str>) -> bool {
    scene.is_none_or(|scene| scene == id)
        && grep.is_none_or(|pattern| id.contains(pattern) || id.replace('-', "_").contains(pattern))
}

/// The scenes `lane` runs, in catalog order.
pub fn lane_scenes(lane: Lane) -> impl Iterator<Item = &'static SceneSpec> {
    CATALOG.iter().filter(move |scene| scene.runs_in(lane))
}
