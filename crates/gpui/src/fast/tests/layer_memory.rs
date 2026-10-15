//! What a list scrolled for long keeps from frame to frame: as much as it
//! shows and holds around that, however far and fast it scrolled. The nodes
//! of rows scrolled out linger only as many as the rows in use claim, a list
//! layer holds the rows of its overscan, and nothing grows from one pass
//! over the list to the next.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::layer_memory::
//! ```

use crate::fast::layout::{LIST_ROW_NODE_LINGER_FRAMES, LIST_ROW_NODE_LINGER_MIN};
use crate::fast::tests::layer_support::{LAYERS, with_window};
use crate::{
    AnyWindowHandle, Context, FastMode, IntoElement, ParentElement as _, Render, ScrollDelta,
    ScrollWheelEvent, Styled as _, TestAppContext, TouchPhase, Window, div, point, px, rgb,
};

/// How tall the list is, in pixels.
const VIEWPORT_HEIGHT: f32 = 200.;
/// How tall a row is, in pixels.
const ROW_HEIGHT: f32 = 20.;
/// How far a frame scrolls: two rows, as a fast wheel or a fling does.
const STEP: f32 = 40.;
/// How many frames a pass over the list takes, before it turns back.
const PASS_FRAMES: usize = 150;

/// A list of 10,000 rows of four text cells over an opaque background.
struct Table;

impl Render for Table {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x1e1f22)).child(
            crate::uniform_list("rows", 10_000, |range, _, _| {
                range
                    .map(|row| {
                        div().h(px(ROW_HEIGHT)).flex().children(
                            (0..4).map(move |column| {
                                div().w(px(90.)).child(format!("{row}.{column}"))
                            }),
                        )
                    })
                    .collect()
            })
            .w(px(400.))
            .h(px(VIEWPORT_HEIGHT)),
        )
    }
}

fn open(cx: &mut TestAppContext, mode: FastMode) -> AnyWindowHandle {
    // What a layer holds is measured at the overscan lists ship with.
    crate::fast::layers::paint::run_at_shipped_list_overscan();
    let window: AnyWindowHandle = cx.add_window(|_, _| Table).into();
    with_window(cx, window, |window, _| window.set_fast_mode(mode));
    cx.set_draw_on_flush(false);
    draw(cx, window, 0.);
    window
}

/// Draws a frame, scrolled by `dy` with the wheel over the list first.
fn draw(cx: &mut TestAppContext, window: AnyWindowHandle, dy: f32) {
    with_window(cx, window, |window, cx| {
        if dy != 0. {
            window.dispatch_event(
                crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: point(px(20.), px(20.)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                    modifiers: Default::default(),
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            );
        }
        window.draw(cx).clear(cx);
    });
}

/// Scrolls `frames` frames, a step each, down if `down`.
fn scroll(cx: &mut TestAppContext, window: AnyWindowHandle, frames: usize, down: bool) {
    let dy = if down { -STEP } else { STEP };
    for _ in 0..frames {
        draw(cx, window, dy);
    }
}

fn kept_nodes(cx: &mut TestAppContext, window: AnyWindowHandle) -> usize {
    with_window(cx, window, |window, _| {
        window.fast_owned_state().kept_layout_nodes
    })
}

/// The nodes in use where the list is: kept once no row scrolled out
/// lingers any more.
fn nodes_in_use(cx: &mut TestAppContext, window: AnyWindowHandle) -> usize {
    for _ in 0..=LIST_ROW_NODE_LINGER_FRAMES {
        draw(cx, window, 0.);
    }
    kept_nodes(cx, window)
}

fn held_rows(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<usize> {
    with_window(cx, window, |window, _| {
        window
            .fast_layers
            .layers
            .values()
            .flat_map(|layer| layer.rows.held())
            .collect()
    })
}

/// A fast scroll passes a hundred rows a second: had every row it passed
/// lingered for [`LIST_ROW_NODE_LINGER_FRAMES`], it would keep hundreds of
/// rows of nodes. Only as many nodes linger as the rows in use claim.
fn assert_lingering_bounded(mode: FastMode, cx: &mut TestAppContext) {
    let window = open(cx, mode);
    scroll(cx, window, PASS_FRAMES, true);
    let scrolling = kept_nodes(cx, window);
    let in_use = nodes_in_use(cx, window);
    let passed_rows = PASS_FRAMES * (STEP / ROW_HEIGHT) as usize;
    assert!(
        in_use * 2 + LIST_ROW_NODE_LINGER_MIN < passed_rows * 9,
        "the scroll passes more rows than the bound keeps: {in_use} nodes in use"
    );
    assert!(
        scrolling <= in_use * 2 + LIST_ROW_NODE_LINGER_MIN,
        "{scrolling} nodes kept while scrolling, {in_use} in use"
    );
}

#[crate::test]
fn lingering_row_nodes_are_bounded_by_the_rows_in_use(cx: &mut TestAppContext) {
    assert_lingering_bounded(
        FastMode {
            scroll_layers: false,
            ..FastMode::ALL
        },
        cx,
    );
}

#[crate::test]
fn lingering_row_nodes_are_bounded_by_the_rows_a_layer_holds(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    assert_lingering_bounded(LAYERS, cx);
}

/// A list layer holds the rows it shows and those of its overscan on each
/// side, wherever the scroll took it.
#[crate::test]
fn a_list_layer_holds_the_rows_of_its_overscan(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let window = open(cx, LAYERS);
    scroll(cx, window, PASS_FRAMES, true);
    let held = held_rows(cx, window);
    assert!(!held.is_empty(), "the list has a layer");
    let shown = (VIEWPORT_HEIGHT / ROW_HEIGHT) as usize;
    let overscan =
        (shown as f32 * crate::fast::layers::paint::LIST_OVERSCAN_VIEWPORTS).ceil() as usize;
    let needed = shown + 1 + 2 * overscan;
    // Rows past the overscan go a batch at a time, a quarter of it.
    assert!(
        held.len() <= needed + needed / 4 + 1,
        "{} rows held, {needed} needed",
        held.len()
    );
    let top = (PASS_FRAMES as f32 * STEP / ROW_HEIGHT) as usize;
    assert!(
        held.iter()
            .all(|row| row + overscan + needed / 4 + 1 >= top
                && *row <= top + shown + overscan + 1),
        "rows held far from the viewport: {held:?}"
    );
}

/// Scrolled back and forth over the list, a window keeps as much after the
/// fourth pass as after the second: what it keeps is bounded by what it
/// shows, and none of it grows with the frames drawn.
#[crate::test]
fn passes_over_a_list_keep_no_more_than_the_first(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let window = open(cx, LAYERS);
    let state = |cx: &mut TestAppContext| {
        let rows = held_rows(cx, window).len();
        with_window(cx, window, |window, _| {
            let owned = window.fast_owned_state();
            (
                owned.kept_layout_nodes,
                owned.layout_engine_nodes,
                owned.layer_tiles,
                rows,
                window.rendered_frame.element_states.len(),
            )
        })
    };
    scroll(cx, window, PASS_FRAMES, true);
    scroll(cx, window, PASS_FRAMES, false);
    let second = state(cx);
    scroll(cx, window, PASS_FRAMES, true);
    scroll(cx, window, PASS_FRAMES, false);
    let fourth = state(cx);
    assert_eq!(second, fourth);
}
