//! Changes a `virtual_list`'s layer must survive, each leaving every frame
//! equal to one drawn from scratch: its rows' sizes, count and gap changing
//! between frames, the view holding it notified, a programmatic scroll to an
//! item. Rows are placed by the list's own origins, so a row the layer holds
//! lands where the new layout puts it.

use super::{Kind, Page, Step, compare_steps, varied_heights};
use crate::{ScrollStrategy, TestAppContext};
use std::rc::Rc;

fn page() -> Page {
    Page::new(Kind::Virtual, varied_heights())
}

/// Wheel turns enough to promote the list and composite it a while.
fn turns(count: usize, dy: f32) -> Vec<Step> {
    vec![Step::Wheel(0., dy); count]
}

fn script(change: Step) -> Vec<Step> {
    let mut steps = turns(8, -20.);
    steps.push(change);
    steps.extend(turns(8, -15.));
    steps.push(change);
    steps.extend(turns(6, 10.));
    steps
}

#[crate::test]
fn a_row_size_change_moves_the_rows_the_layer_holds(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let grow: fn(&mut Page) = |page| {
        let mut extents = (*page.extents).clone();
        // A row above those shown and one the layer holds below them. Whole
        // pixels: an edge on half a device pixel rounds otherwise once moved
        // across the window's origin, with any layer (see `docs/fast-mode.md`).
        extents[3] += 7.;
        extents[12] += 11.;
        page.extents = Rc::new(extents);
    };
    compare_steps(cx, page, &script(Step::Edit(grow)), 1., "sizes", true);
}

#[crate::test]
fn a_count_and_gap_change_is_drawn_as_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let shrink: fn(&mut Page) = |page| {
        let mut extents = (*page.extents).clone();
        extents.truncate(extents.len() - 3);
        extents.insert(0, 33.);
        page.extents = Rc::new(extents);
    };
    let gap: fn(&mut Page) = |page| page.gap = if page.gap == 0. { 4. } else { 0. };
    compare_steps(cx, page, &script(Step::Edit(shrink)), 1., "count", true);
    compare_steps(cx, page, &script(Step::Edit(gap)), 1., "gap", true);
}

#[crate::test]
fn a_notified_view_and_new_rows_are_drawn_as_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let notify: fn(&mut Page) = |_| {};
    let tint: fn(&mut Page) = |page| page.tint = !page.tint;
    compare_steps(cx, page, &script(Step::Edit(notify)), 1., "notified", true);
    compare_steps(cx, page, &script(Step::Edit(tint)), 1., "tinted", true);
}

#[crate::test]
fn a_programmatic_scroll_to_an_item_is_drawn_as_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for strategy in [
        ScrollStrategy::Top,
        ScrollStrategy::Center,
        ScrollStrategy::Bottom,
        ScrollStrategy::Nearest,
    ] {
        compare_steps(
            cx,
            page,
            &script(Step::ScrollTo(700, strategy)),
            1.,
            &format!("scroll to item, {strategy:?}"),
            true,
        );
    }
}

/// A frame that draws the page from last frame, a view beside the list
/// being notified alone, and the scroll after it.
#[crate::test]
fn a_frame_drawn_from_last_frame_and_the_scroll_after_it(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    compare_steps(
        cx,
        page,
        &script(Step::NotifyBeside),
        1.,
        "a view beside notified",
        true,
    );
}
