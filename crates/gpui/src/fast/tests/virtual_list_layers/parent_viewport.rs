//! A `ParentViewport` `virtual_list` whose parent `div` paints its content
//! into a scroll layer, or composites it, answers as drawn without layers:
//! the metrics it publishes of the parent's viewport and the range its
//! listener receives come from the viewport shown, not from the region the
//! layer paints, and every frame equals a frame drawn from scratch.

use super::{Kind, Page, Step, compare_steps, varied_heights, wheel_history};
use crate::TestAppContext;

fn page() -> Page {
    Page::new(Kind::ParentViewport, varied_heights())
}

/// Wheel turns over the parent, and the frames between them that draw the
/// page from last frame or notify a view beside the list.
fn script(seed: u32) -> Vec<Step> {
    let mut steps = Vec::new();
    for (index, (dx, dy)) in wheel_history(seed, 48).into_iter().enumerate() {
        steps.push(Step::Wheel(dx, dy));
        if index % 7 == 3 {
            steps.push(Step::NotifyBeside);
        }
    }
    steps
}

#[crate::test]
fn the_metrics_and_range_inside_a_parent_layer_are_the_viewport_s(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_steps(cx, page, &script(21), 1., "parent viewport", true);
    // The list reads the parent's offset, so the parent's layer is painted
    // again on every scrolled frame until it is demoted (making it stay
    // promoted is a follow-up): the frames before are those that test it.
    assert!(
        counters["layers_composited"] + counters["layers_repainted"] >= 5,
        "the parent's content was shown from its layer: {counters:?}"
    );
}

#[crate::test]
fn a_padded_list_in_a_parent_layer_answers_as_drawn_without(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let padded = || {
        let mut page = page();
        page.padding = 6.;
        page.gap = 2.;
        page
    };
    let counters = compare_steps(cx, padded, &script(22), 1., "padded", true);
    assert!(
        counters["layers_composited"] + counters["layers_repainted"] >= 5,
        "{counters:?}"
    );
}

/// Whole frame descriptions over random histories of wheel turns, pointer
/// moves and clicks, in two applications run one after the other.
#[test]
fn whole_frames_inside_a_parent_layer_describe_themselves_as_drawn_without() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for seed in 1..=3 {
        let script = super::oracle::history(seed, 40);
        let page = || {
            let mut page = page();
            page.interactive = true;
            page
        };
        let (layered, shown) = super::oracle::described(page, &script, true);
        let (reference, none) = super::oracle::described(page, &script, false);
        assert_eq!(none, 0, "the reference has no layer");
        assert!(
            shown > 0,
            "seed {seed}: the parent was shown from its layer"
        );
        for (step, (layered, reference)) in layered.iter().zip(&reference).enumerate() {
            if let Some(difference) = crate::first_frame_difference(layered, reference) {
                panic!(
                    "seed {seed}, after step {step} ({:?}): shown from the layer (actual) \
                     differs from drawn without (expected): {difference}",
                    script[step]
                );
            }
        }
    }
}
