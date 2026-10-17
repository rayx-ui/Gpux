//! The kind of `virtual_list` without a list layer: a list in its parent's
//! flow (`ParentViewport`) is no scroll container of its own and has no list
//! layer. It draws what it draws without layers.

use super::{Kind, Page, varied_heights, wheel_history};
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
