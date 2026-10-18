//! A `virtual_list` with sticky rows composites its ordinary rows from a list
//! layer and draws its sticky candidates in view (the active stack, and the
//! incoming rows) above it every frame, in the list's clip: on the top edge
//! and the left one, at levels and in keyed runs, inset, styled while stuck
//! and reporting their state. The layer never holds a sticky candidate, and
//! the sticky rows' hitboxes are hit before the layer's rows.

use super::{
    FrameWork, Kind, Page, Sticky, compare_with_layers_off, difference, mean, open, stats,
    varied_heights, wheel_by, wheel_history,
};
use crate::fast::tests::layer_support::{draw, with_window};
use crate::{
    AnyWindowHandle, AppContext as _, GlobalElementId, MouseButton, MouseDownEvent, MouseUpEvent,
    PlatformInput, TestAppContext, point, px,
};
use std::collections::BTreeSet;

const FRAMES: usize = 60;

/// The rows of `page`'s list that stick under its policy, up to `count`.
fn sticky_rows(sticky: Sticky, count: usize) -> BTreeSet<usize> {
    (0..count)
        .filter(|row| match sticky {
            Sticky::Default => false,
            Sticky::Every25 | Sticky::StackIds | Sticky::Styled => row % 25 == 0,
            Sticky::Dense => row % 4 == 0,
            Sticky::Levels => row % 50 == 0 || row % 50 == 10,
        })
        .collect()
}

/// The rows the layer of the list holds.
fn held_rows(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<usize> {
    with_window(cx, window, |window, _| {
        window
            .fast_layers
            .layers
            .iter()
            .find(|(id, _): &(&GlobalElementId, _)| id.last() == Some(&"list".into()))
            .map(|(_, layer)| layer.rows.held().collect())
            .unwrap_or_default()
    })
}

fn page(kind: Kind, sticky: Sticky) -> Page {
    let mut page = Page::new(kind, varied_heights());
    page.sticky = sticky;
    page
}

/// Turns the wheel by `(dx, dy)` on each of [`FRAMES`] frames after four
/// turns to promote the list, returning each frame's work and checking the
/// layer never holds a sticky candidate.
fn scroll(cx: &mut TestAppContext, page: Page, dx: f32, dy: f32) -> Vec<FrameWork> {
    let sticky = sticky_rows(page.sticky_or_default(), page.extents.len());
    let log = page.log.clone();
    let window = open(cx, page, false);
    for _ in 0..4 {
        wheel_by(cx, window, dx, dy);
    }
    (0..FRAMES)
        .map(|frame| {
            let before = stats(cx, window);
            log.borrow_mut().calls.clear();
            wheel_by(cx, window, dx, dy);
            let after = stats(cx, window);
            let held = held_rows(cx, window);
            assert!(
                held.iter().all(|row| !sticky.contains(row)),
                "frame {frame}: the layer holds a sticky row: {held:?}"
            );
            FrameWork {
                counters: difference(&after, &before),
                closure_rows: log.borrow().rows_rendered(),
                calls: log.borrow().calls.clone(),
            }
        })
        .collect()
}

impl Page {
    fn sticky_or_default(&self) -> Sticky {
        match (self.kind, self.sticky) {
            (Kind::Sticky, Sticky::Default) => Sticky::Every25,
            (_, sticky) => sticky,
        }
    }
}

/// Every scroll frame after promotion composites the ordinary rows from the
/// layer, rendering at most two of them a frame on average; the row closure
/// also renders the sticky rows drawn above it, one to three a frame.
fn composites(cx: &mut TestAppContext, page: Page, dx: f32, dy: f32) {
    let work = scroll(cx, page, dx, dy);
    let mut repainted = 0;
    for (frame, work) in work.iter().enumerate() {
        let composited = work.counter("layers_composited");
        repainted += work.counter("layers_repainted");
        assert_eq!(
            composited + work.counter("layers_repainted"),
            1,
            "frame {frame} is shown from the list's layer: {:?}",
            work.counters
        );
        assert_eq!(
            work.counter("layer_bypasses.ineligible"),
            0,
            "frame {frame}: a list with sticky rows is eligible"
        );
        if composited == 1 {
            assert!(
                work.counter("list_rows_kept") > 0,
                "frame {frame} keeps the rows the layer holds"
            );
        }
        let sticky = work.closure_rows as u64 - work.counter("list_rows_rendered");
        assert!(
            (1..=3).contains(&sticky),
            "frame {frame}: {sticky} sticky rows drawn above the layer: {:?}",
            work.calls
        );
    }
    assert!(
        repainted <= 1,
        "{repainted} of {FRAMES} frames painted afresh"
    );
    let rendered = mean(&work, "list_rows_rendered");
    assert!(
        rendered <= 2.,
        "{rendered} ordinary rows rendered a frame on average, more than 2"
    );
}

#[crate::test]
fn a_list_with_top_sticky_rows_composites_its_ordinary_rows(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    composites(cx, page(Kind::Sticky, Sticky::Every25), 0., -15.);
}

#[crate::test]
fn a_list_with_left_sticky_columns_composites_its_ordinary_columns(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    composites(cx, page(Kind::Horizontal, Sticky::Every25), -15., 0.);
}

/// A history of wheel turns, whole and fractional, draws what a window
/// without layers draws, pushing each stuck row out with the next, and
/// publishes the ranges it publishes, under `sticky` on `kind`.
fn draws_as_without_layers(cx: &mut TestAppContext, kind: Kind, sticky: Sticky, seed: u32) {
    let history: Vec<(f32, f32)> = wheel_history(seed, 40)
        .into_iter()
        .map(|(dx, dy)| match kind {
            Kind::Horizontal => (dy, dx),
            _ => (dx, dy),
        })
        .collect();
    let counters = compare_with_layers_off(
        cx,
        || page(kind, sticky),
        &history,
        &format!("{kind:?}, {sticky:?}"),
    );
    assert!(counters["layers_composited"] >= 20, "{counters:?}");
    assert_eq!(
        counters
            .get("layer_bypasses.ineligible")
            .copied()
            .unwrap_or(0),
        0,
        "{counters:?}"
    );
}

#[crate::test]
fn sticky_rows_on_either_edge_draw_as_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    draws_as_without_layers(cx, Kind::Sticky, Sticky::Every25, 5);
    draws_as_without_layers(cx, Kind::Horizontal, Sticky::Every25, 6);
}

#[crate::test]
fn sticky_levels_and_keyed_runs_draw_as_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    draws_as_without_layers(cx, Kind::Sticky, Sticky::Levels, 7);
    draws_as_without_layers(cx, Kind::Sticky, Sticky::StackIds, 8);
    draws_as_without_layers(cx, Kind::Horizontal, Sticky::Levels, 9);
}

/// Inset rows styled while stuck draw as without layers, and report the same
/// state changes in the same order.
#[crate::test]
fn inset_styled_sticky_rows_report_their_state_as_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let history = wheel_history(10, 40);
    let layered_page = page(Kind::Sticky, Sticky::Styled);
    let layered_events = layered_page.sticky_events.clone();
    let reference_page = page(Kind::Sticky, Sticky::Styled);
    let reference_events = reference_page.sticky_events.clone();
    let layered = open(cx, layered_page, false);
    let reference = open(cx, reference_page, true);
    let mut composited = 0;
    for (step, (dx, dy)) in history.iter().copied().enumerate() {
        let before = stats(cx, layered).layers_composited;
        wheel_by(cx, layered, dx, dy);
        wheel_by(cx, reference, dx, dy);
        composited += stats(cx, layered).layers_composited - before;
        let actual = with_window(cx, layered, |window, _| window.painted_primitives());
        let expected = with_window(cx, reference, |window, _| window.painted_primitives());
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!("step {step}: the frame differs from one drawn without layers: {difference}");
        }
        assert_eq!(
            *layered_events.borrow(),
            *reference_events.borrow(),
            "step {step}: the sticky state changes reported"
        );
    }
    assert!(composited >= 20, "composited {composited} frames");
    assert!(
        !layered_events.borrow().is_empty(),
        "the rows reported state changes"
    );
}

/// A click on a stuck row reaches the stuck row, not the ordinary row the
/// layer holds beneath it: the frame it is dispatched against was
/// composited, and the rows clicked are those clicked without layers.
#[crate::test]
fn a_stuck_row_is_hit_before_the_rows_beneath_it(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let clickable = || {
        let mut page = page(Kind::Sticky, Sticky::Every25);
        page.clicks = true;
        page
    };
    let (layered_page, reference_page) = (clickable(), clickable());
    let (layered_log, reference_log) = (layered_page.log.clone(), reference_page.log.clone());
    let layered = open(cx, layered_page, false);
    let reference = open(cx, reference_page, true);
    let mut composited_clicks = 0;
    for step in 0..40 {
        let before = stats(cx, layered).layers_composited;
        wheel_by(cx, layered, 0., -13.);
        wheel_by(cx, reference, 0., -13.);
        let composited = stats(cx, layered).layers_composited > before;
        if step % 4 == 3 {
            let earlier = layered_log.borrow().clicked.len();
            let position = point(px(40.), px(3.));
            for window in [layered, reference] {
                with_window(cx, window, |window, cx| {
                    for event in [
                        PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: Default::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: Default::default(),
                            click_count: 1,
                        }),
                    ] {
                        window.dispatch_event(event, cx);
                    }
                });
                draw(cx, window);
            }
            composited_clicks += usize::from(composited);
            let clicked = layered_log.borrow().clicked.clone();
            assert_eq!(
                clicked,
                reference_log.borrow().clicked,
                "step {step}: the rows clicked"
            );
            // A click reaches every row under the pointer, the topmost
            // first: the stuck row, then the ordinary row beneath it.
            assert!(
                clicked.get(earlier).is_some_and(|row| row % 25 == 0),
                "step {step}: the click reached the stuck row first: {:?}",
                &clicked[earlier..]
            );
        }
        let actual = with_window(cx, layered, |window, _| window.painted_primitives());
        let expected = with_window(cx, reference, |window, _| window.painted_primitives());
        if let Some(difference) = crate::first_frame_difference(&actual, &expected) {
            panic!("step {step}: the frame differs from one drawn without layers: {difference}");
        }
    }
    assert!(
        composited_clicks >= 5,
        "{composited_clicks} clicks were dispatched against composited frames"
    );
}

/// With candidates throughout the layer's reach, dense or at two levels, on
/// either edge, no frame of a run down, back up and through a repaint holds
/// a candidate in the layer or draws a row a frame without layers does not:
/// a candidate is drawn above the layer only while the list shows it.
#[crate::test]
fn no_candidate_in_the_layers_reach_is_held(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for (kind, sticky) in [
        (Kind::Sticky, Sticky::Dense),
        (Kind::Sticky, Sticky::Levels),
        (Kind::Horizontal, Sticky::Dense),
    ] {
        let candidates = sticky_rows(sticky, 10_000);
        let layered = open(cx, page(kind, sticky), false);
        let reference = open(cx, page(kind, sticky), true);
        let turn = |delta: f32| match kind {
            Kind::Horizontal => (delta, 0.),
            _ => (0., delta),
        };
        let mut steps: Vec<Option<f32>> = vec![Some(-15.); 30];
        steps.push(None);
        steps.extend(vec![Some(13.); 20]);
        steps.push(None);
        steps.extend(vec![Some(-7.5); 10]);
        let before = stats(cx, layered).layers_composited;
        for (step, delta) in steps.into_iter().enumerate() {
            for window in [layered, reference] {
                match delta {
                    Some(delta) => {
                        let (dx, dy) = turn(delta);
                        wheel_by(cx, window, dx, dy);
                    }
                    // The page changes: the rows are painted afresh.
                    None => {
                        cx.update_window(window, |root, _, cx| {
                            root.downcast::<Page>().unwrap().update(cx, |page, cx| {
                                page.tint = !page.tint;
                                cx.notify();
                            })
                        })
                        .unwrap();
                        draw(cx, window);
                    }
                }
            }
            let held = held_rows(cx, layered);
            assert!(
                held.iter().all(|row| !candidates.contains(row)),
                "{kind:?}, {sticky:?}, step {step}: the layer holds a candidate: {held:?}"
            );
            let actual = with_window(cx, layered, |window, _| window.painted_primitives());
            let expected = with_window(cx, reference, |window, _| window.painted_primitives());
            if let Some(difference) = crate::first_frame_difference(&actual, &expected) {
                panic!(
                    "{kind:?}, {sticky:?}, step {step}: the frame differs from one drawn \
                     without layers: {difference}"
                );
            }
        }
        let composited = stats(cx, layered).layers_composited - before;
        assert!(
            composited >= 30,
            "{kind:?}, {sticky:?}: {composited} frames composited"
        );
    }
}
