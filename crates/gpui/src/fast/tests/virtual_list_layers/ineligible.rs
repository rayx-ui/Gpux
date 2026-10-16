//! The kinds of `virtual_list` without a list layer: a horizontal list and
//! one with sticky rows are counted ineligible; a list scrolled by another
//! element (`ParentViewport`, `SynchronizedOffset`) is no scroll container
//! of its own and has no list layer. Each draws what it draws without
//! layers.

use super::{Kind, Page, compare_with_layers_off, varied_heights, wheel_history};
use crate::{GlobalElementId, TestAppContext};

/// Whether a layer is kept under an id ending in the list's own id.
fn list_layer_kept(cx: &mut TestAppContext, window: crate::AnyWindowHandle) -> bool {
    crate::fast::tests::layer_support::with_window(cx, window, |window, _| {
        window
            .fast_layers
            .layers
            .keys()
            .any(|id: &GlobalElementId| id.last() == Some(&"list".into()))
    })
}

#[crate::test]
fn a_horizontal_list_is_counted_ineligible(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let history: Vec<(f32, f32)> = wheel_history(4, 40)
        .into_iter()
        .map(|(_, dy)| (dy, 0.))
        .collect();
    let counters = compare_with_layers_off(
        cx,
        || Page::new(Kind::Horizontal, varied_heights()),
        &history,
        "horizontal",
    );
    assert!(counters["layer_bypasses.ineligible"] >= 30, "{counters:?}");
    assert_eq!(counters["layers_composited"], 0);
    assert_eq!(counters["layers_repainted"], 0);
}

#[crate::test]
fn a_list_with_sticky_rows_is_counted_ineligible(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_with_layers_off(
        cx,
        || Page::new(Kind::Sticky, varied_heights()),
        &wheel_history(5, 40),
        "sticky rows",
    );
    assert!(counters["layer_bypasses.ineligible"] >= 30, "{counters:?}");
    assert_eq!(counters["layers_composited"], 0);
    assert_eq!(counters["layers_repainted"], 0);
}

#[crate::test]
fn a_parent_viewport_list_has_no_list_layer(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    // The range a list in its parent's flow publishes inside the parent's
    // layer is `parent_viewport`'s to compare (VL-5).
    super::compare(
        cx,
        || Page::new(Kind::ParentViewport, varied_heights()),
        &wheel_history(6, 40),
        "parent viewport",
        false,
    );
    let window = super::open(cx, Page::new(Kind::ParentViewport, varied_heights()), false);
    for _ in 0..6 {
        super::wheel(cx, window, -30.);
    }
    assert!(!list_layer_kept(cx, window), "the list has no layer");
}

#[crate::test]
fn a_synchronized_offset_list_has_no_list_layer(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    compare_with_layers_off(
        cx,
        || Page::new(Kind::Synchronized, varied_heights()),
        &wheel_history(7, 40),
        "synchronized offset",
    );
    let window = super::open(cx, Page::new(Kind::Synchronized, varied_heights()), false);
    for _ in 0..6 {
        super::wheel(cx, window, -30.);
    }
    assert!(!list_layer_kept(cx, window), "the list has no layer");
}
