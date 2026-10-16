//! A scroll container whose layer is expected to composite does not request the layout of its
//! children, which its layer kept, and requests it in prepaint when the layer is painted after
//! all.

use super::layer_support::{ROWS, page, promote, scroll, with_window};
use crate::fast::layers::policy::Decision;
use crate::{AnyWindowHandle, TestAppContext};

fn claimed(cx: &mut TestAppContext, window: AnyWindowHandle) -> u64 {
    with_window(cx, window, |window, _| window.fast_stats().nodes_claimed)
}

fn reset(cx: &mut TestAppContext, window: AnyWindowHandle) {
    with_window(cx, window, |window, _| window.reset_fast_stats());
}

/// Frames that only scroll a promoted container composite its layer without claiming a node for
/// each of its rows; a frame that changes what the container holds claims them again.
#[crate::test]
fn composited_frames_claim_no_node_for_the_rows(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let handle = page(cx, false);
    let window: AnyWindowHandle = handle.into();
    promote(cx, window);
    // The frame that painted the layer asked for the rows' layout.
    assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    reset(cx, window);
    for _ in 0..3 {
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }
    let kept = claimed(cx, window);
    assert!(
        kept < ROWS as u64,
        "three composited frames claimed {kept} nodes for {ROWS} rows"
    );

    // The page notified for its own sake: what the container holds is built again.
    reset(cx, window);
    handle.update(cx, |_, _, cx| cx.notify()).unwrap();
    crate::fast::tests::layer_support::draw(cx, window);
    let rebuilt = claimed(cx, window);
    assert!(
        rebuilt >= ROWS as u64,
        "a frame after a notification claimed {rebuilt} nodes for {ROWS} rows"
    );
}

/// A frame that paints the layer again though it was expected to composite it asks for the
/// layout it was spared, and the frames after it keep to the layout the layer kept.
#[crate::test]
fn a_repainted_layer_keeps_the_layout_of_its_content(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let handle = page(cx, false);
    let window: AnyWindowHandle = handle.into();
    promote(cx, window);
    // Scrolled past what the layer painted, the container paints it again.
    let mut repainted = false;
    for _ in 0..40 {
        if scroll(cx, window, -20.) == Some(Decision::Repaint) {
            repainted = true;
            break;
        }
    }
    assert!(repainted, "the scroll never left the painted region");
    reset(cx, window);
    for _ in 0..2 {
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }
    let kept = claimed(cx, window);
    assert!(
        kept < ROWS as u64,
        "two composited frames after a repaint claimed {kept} nodes for {ROWS} rows"
    );
}
