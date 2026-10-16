//! The `on_rendered_range` listener of a layered `virtual_list` runs on
//! every frame the list is drawn, composited frames included, with the
//! mounted, clamped, overscanned range the frame drawn without layers
//! publishes.

use super::{Kind, Page, compare_with_layers_off, varied_heights, wheel_history};
use crate::TestAppContext;

/// The list composites on most frames of a long history; on each, it
/// publishes once, the range a window without layers publishes.
#[crate::test]
fn a_composited_frame_publishes_the_range_drawn_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_with_layers_off(
        cx,
        || Page::new(Kind::Virtual, varied_heights()),
        &wheel_history(1, 80),
        "varied heights",
    );
    assert!(
        counters["layers_composited"] >= 60,
        "the history composites: {counters:?}"
    );
    assert!(
        counters["published_from_layer"] >= 60,
        "the listener ran on the frames shown from the layer: {counters:?}"
    );
}

/// With overscan, padding and a gap the range stays the one drawn without
/// layers, frame after frame.
#[crate::test]
fn overscan_padding_and_gap_publish_the_range_drawn_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_with_layers_off(
        cx,
        || {
            let mut page = Page::new(Kind::Virtual, varied_heights());
            page.gap = 3.;
            page.padding = 6.;
            page.overscan = 2;
            page
        },
        &wheel_history(2, 60),
        "overscan, padding and gap",
    );
    assert!(counters["layers_composited"] >= 40, "{counters:?}");
    assert!(counters["published_from_layer"] >= 40, "{counters:?}");
}

/// Sticky rows keep a list off its layer, and the rows stuck at the top,
/// rendered outside the mounted range, never widen it.
#[crate::test]
fn stuck_rows_never_widen_the_range(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let page = || Page::new(Kind::Sticky, vec![20.; 2_000]);
    {
        let page = page();
        let log = page.log.clone();
        let window = super::open(cx, page, false);
        for _ in 0..8 {
            log.borrow_mut().ranges.clear();
            super::wheel(cx, window, -130.);
            let ranges = log.borrow().ranges.clone();
            assert_eq!(ranges.len(), 1, "one range a frame: {ranges:?}");
            assert!(
                !ranges[0].contains(&0),
                "the stuck row 0 is not part of {ranges:?}"
            );
        }
    }
    compare_with_layers_off(cx, page, &wheel_history(3, 40), "sticky rows");
}
