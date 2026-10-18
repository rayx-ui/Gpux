//! `uniform_list` keeps its measured item while nothing the measured row
//! read has changed: a scroll-only frame renders only the rows it shows, not
//! the item the list measures its rows by (row 0 here), with scroll layers
//! and with view retention alone. A change to the measured row measures it
//! again, and its frames equal frames drawn from scratch.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::uniform_measured_item
//! ```

use crate::fast::tests::layer_support::{LAYERS, add_window, draw, make_reference, with_window};
use crate::{
    AnyWindowHandle, AppContext as _, Context, Entity, FastMode, IntoElement, ParentElement as _,
    Render, ScrollDelta, ScrollWheelEvent, Styled as _, TestAppContext, TouchPhase, Window, div,
    hsla, point, px,
};
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

const W: f32 = 200.;
const H: f32 = 100.;
const ROWS: usize = 2_000;
/// Turns that take row 0 well out of the list and its overscan.
const WARM_UP: usize = 20;
const TURN: f32 = -15.;

type Calls = Rc<RefCell<Vec<Range<usize>>>>;

struct Page {
    /// How tall every row is; a change measures the item again.
    row: f32,
    /// How much taller the rows are, read by the rows as they render: an
    /// update notifies no view.
    extra: Entity<Extra>,
    /// How much taller the measured row alone is: read by no row the list
    /// shows once scrolled, so only the measured item depends on it.
    measured_extra: Entity<Extra>,
    /// How much taller the rows are, kept outside any entity: a change is
    /// told by a notification of the page alone.
    outside: Rc<Cell<f32>>,
    /// A view beside the list, notified to splice the page around it.
    beside: Entity<Beside>,
    calls: Calls,
}

struct Extra {
    height: f32,
}

struct Beside {
    count: usize,
}

impl Render for Beside {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(20.))
            .h(px(10. + (self.count % 2) as f32 * 5.))
            .bg(crate::black())
    }
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (row, calls, extra) = (self.row, self.calls.clone(), self.extra.clone());
        let (measured_extra, outside) = (self.measured_extra.clone(), self.outside.clone());
        div()
            .size_full()
            .bg(crate::white())
            .child(self.beside.clone())
            .child(
                crate::uniform_list("list", ROWS, move |range, _, cx| {
                    calls.borrow_mut().push(range.clone());
                    let mut row = row + extra.read(cx).height + outside.get();
                    if range.start == 0 {
                        row += measured_extra.read(cx).height;
                    }
                    range
                        .map(|index| {
                            div().w_full().h(px(row)).bg(hsla(
                                (index % 17) as f32 / 17.,
                                0.5,
                                0.5,
                                1.,
                            ))
                        })
                        .collect::<Vec<_>>()
                })
                .w(px(W))
                .h(px(H)),
            )
    }
}

/// Opens a window in `mode`, or, as `reference`, the window a window in
/// `mode` is compared with: drawn from scratch, snapped as scroll layers
/// snap only when `mode` has them.
fn open(cx: &mut TestAppContext, mode: FastMode, reference: bool) -> (AnyWindowHandle, Calls) {
    let calls = Calls::default();
    let window: AnyWindowHandle = add_window(cx, {
        let calls = calls.clone();
        move |_, cx| Page {
            row: 20.,
            extra: cx.new(|_| Extra { height: 0. }),
            measured_extra: cx.new(|_| Extra { height: 0. }),
            outside: Rc::default(),
            beside: cx.new(|_| Beside { count: 0 }),
            calls,
        }
    })
    .into();
    with_window(cx, window, |window, _| {
        if reference && mode.scroll_layers {
            make_reference(window);
        } else if reference {
            window.set_fast_mode(FastMode::OFF);
        } else {
            window.set_fast_mode(mode);
        }
    });
    draw(cx, window);
    draw(cx, window);
    (window, calls)
}

fn wheel(cx: &mut TestAppContext, window: AnyWindowHandle) {
    let frames = with_window(cx, window, |window, _| window.fast_stats().frames);
    with_window(cx, window, |window, cx| {
        window.dispatch_event(
            crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(20.), px(20.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(TURN))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    });
    if with_window(cx, window, |window, _| window.fast_stats().frames) == frames {
        draw(cx, window);
    }
}

fn rows_rendered(cx: &mut TestAppContext, window: AnyWindowHandle) -> u64 {
    with_window(cx, window, |window, _| {
        window.fast_stats().list_rows_rendered
    })
}

/// Scrolls `mode`'s window past row 0, then checks every scroll-only frame
/// renders only the rows it shows; with `layered`, exactly the rows its
/// layer rendered.
fn scroll_only(cx: &mut TestAppContext, mode: FastMode, layered: bool) {
    let (window, calls) = open(cx, mode, false);
    for _ in 0..WARM_UP {
        wheel(cx, window);
    }
    for frame in 0..40 {
        calls.borrow_mut().clear();
        let before = rows_rendered(cx, window);
        wheel(cx, window);
        let frame_calls = calls.borrow().clone();
        assert!(
            frame_calls.iter().all(|range| !range.contains(&0)),
            "frame {frame}: the measured item was measured again on a scroll-only frame: the \
             row closure ran for {frame_calls:?}"
        );
        if layered {
            let rendered = rows_rendered(cx, window) - before;
            let ran: usize = frame_calls.iter().map(|range| range.len()).sum();
            assert_eq!(
                ran as u64, rendered,
                "frame {frame}: the measured item was measured again: the row closure ran for \
                 {frame_calls:?}, the layer rendered {rendered} rows"
            );
        }
    }
    if layered {
        let composited = with_window(cx, window, |window, _| {
            window.fast_stats().layers_composited
        });
        assert!(
            composited >= 30,
            "shown from the list's layer: {composited}"
        );
    }
}

#[crate::test]
fn scroll_only_frames_with_layers_render_only_the_layers_rows(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    scroll_only(cx, LAYERS, true);
}

#[crate::test]
fn scroll_only_frames_with_splice_render_only_the_shown_rows(cx: &mut TestAppContext) {
    scroll_only(
        cx,
        FastMode {
            scroll_layers: false,
            ..FastMode::ALL
        },
        false,
    );
}

#[crate::test]
fn scroll_only_frames_with_view_retention_alone_render_only_the_shown_rows(
    cx: &mut TestAppContext,
) {
    scroll_only(
        cx,
        FastMode {
            splice: false,
            scroll_layers: false,
            ..FastMode::ALL
        },
        false,
    );
}

/// A change to the rows' height measures the item again on the frame it
/// changes, and every frame equals one drawn from scratch.
#[crate::test]
fn a_changed_measured_row_is_measured_again(cx: &mut TestAppContext) {
    for (name, mode) in [
        ("layers", LAYERS),
        (
            "view retention",
            FastMode {
                splice: false,
                scroll_layers: false,
                ..FastMode::ALL
            },
        ),
    ] {
        let (window, calls) = open(cx, mode, false);
        let (reference, _) = open(cx, mode, true);
        for step in 0..40 {
            calls.borrow_mut().clear();
            let change = step % 10 == 9;
            if change {
                for window in [window, reference] {
                    cx.update_window(window, |root, _, cx| {
                        root.downcast::<Page>().unwrap().update(cx, |page, cx| {
                            page.row = if page.row == 20. { 24. } else { 20. };
                            cx.notify();
                        })
                    })
                    .unwrap();
                }
            }
            wheel(cx, window);
            wheel(cx, reference);
            if change {
                assert!(
                    calls.borrow().iter().any(|range| range.contains(&0)),
                    "{name}, step {step}: a changed row was not measured again: {:?}",
                    calls.borrow()
                );
            }
            let actual = with_window(cx, window, |window, _| window.painted_primitives());
            let expected = with_window(cx, reference, |window, _| window.painted_primitives());
            if let Some(difference) =
                crate::fast::verify::first_frame_difference(&actual, &expected)
            {
                panic!(
                    "{name}, step {step}: the frame differs from one drawn from scratch: {difference}"
                );
            }
        }
    }
}

/// A change of the scale factor measures the item again, without a layer:
/// the rows' style sizes round to other device pixels.
#[crate::test]
fn a_scale_factor_change_measures_the_item_again(cx: &mut TestAppContext) {
    let mode = FastMode {
        splice: false,
        scroll_layers: false,
        ..FastMode::ALL
    };
    let (window, calls) = open(cx, mode, false);
    let (reference, _) = open(cx, mode, true);
    for step in 0..12 {
        calls.borrow_mut().clear();
        let change = step == 5;
        if change {
            for window in [window, reference] {
                cx.test_window(window).simulate_scale_factor_change(1.25);
            }
        }
        wheel(cx, window);
        wheel(cx, reference);
        if change {
            assert!(
                calls.borrow().iter().any(|range| range.contains(&0)),
                "step {step}: the item was not measured again at the new scale: {:?}",
                calls.borrow()
            );
        }
        let actual = with_window(cx, window, |window, _| window.painted_primitives());
        let expected = with_window(cx, reference, |window, _| window.painted_primitives());
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!("step {step}: the frame differs from one drawn from scratch: {difference}");
        }
    }
}

/// An entity the measured row read, updated without notifying any view,
/// measures the item again on the next frame, without a layer and with one.
#[crate::test]
fn an_update_the_measured_row_read_measures_it_again(cx: &mut TestAppContext) {
    for (name, mode) in [
        ("layers", LAYERS),
        (
            "splice",
            FastMode {
                scroll_layers: false,
                ..FastMode::ALL
            },
        ),
    ] {
        let (window, calls) = open(cx, mode, false);
        let (reference, _) = open(cx, mode, true);
        for step in 0..30 {
            calls.borrow_mut().clear();
            let change = step % 10 == 9;
            if change {
                for window in [window, reference] {
                    cx.update_window(window, |root, _, cx| {
                        let extra = root.downcast::<Page>().unwrap().read(cx).extra.clone();
                        extra.update(cx, |extra, _| {
                            extra.height = if extra.height == 0. { 4. } else { 0. };
                        });
                    })
                    .unwrap();
                }
            }
            wheel(cx, window);
            wheel(cx, reference);
            if change {
                assert!(
                    calls.borrow().iter().any(|range| range.contains(&0)),
                    "{name}, step {step}: the updated row was not measured again: {:?}",
                    calls.borrow()
                );
            }
            let actual = with_window(cx, window, |window, _| window.painted_primitives());
            let expected = with_window(cx, reference, |window, _| window.painted_primitives());
            if let Some(difference) =
                crate::fast::verify::first_frame_difference(&actual, &expected)
            {
                panic!(
                    "{name}, step {step}: the frame differs from one drawn from scratch: {difference}"
                );
            }
        }
    }
}

/// Without layers, `mode`'s window and its reference scrolled past row 0,
/// then changed by `change` and drawn without a scroll: the item is measured
/// again and the frame equals one drawn from scratch, `label` naming the
/// change.
fn changed_without_scroll(
    cx: &mut TestAppContext,
    mode: FastMode,
    label: &str,
    change: fn(&mut TestAppContext, AnyWindowHandle),
) {
    let (window, calls) = open(cx, mode, false);
    let (reference, _) = open(cx, mode, true);
    for _ in 0..WARM_UP {
        wheel(cx, window);
        wheel(cx, reference);
    }
    for round in 0..3 {
        calls.borrow_mut().clear();
        for window in [window, reference] {
            change(cx, window);
            draw(cx, window);
        }
        assert!(
            calls.borrow().iter().any(|range| range.contains(&0)),
            "{label}, round {round}: the item was not measured again: {:?}",
            calls.borrow()
        );
        let actual = with_window(cx, window, |window, _| window.painted_primitives());
        let expected = with_window(cx, reference, |window, _| window.painted_primitives());
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!(
                "{label}, round {round}: the frame differs from one drawn from scratch: {difference}"
            );
        }
        wheel(cx, window);
        wheel(cx, reference);
    }
}

fn page(cx: &mut TestAppContext, window: AnyWindowHandle) -> Entity<Page> {
    cx.update_window(window, |root, _, _| root.downcast::<Page>().unwrap())
        .unwrap()
}

/// An entity only the measured row read, updated without a notification,
/// measures the item again: what measuring read is the view's own, whether
/// the view holding the list is reused or a splice copies it around a view
/// beside the list that is notified.
#[crate::test]
fn an_update_only_the_measured_row_read_measures_it_again(cx: &mut TestAppContext) {
    let change: fn(&mut TestAppContext, AnyWindowHandle) = |cx, window| {
        let page = page(cx, window);
        let (measured_extra, beside) = page.read_with(cx, |page, _| {
            (page.measured_extra.clone(), page.beside.clone())
        });
        measured_extra.update(cx, |extra, _| {
            extra.height = if extra.height == 0. { 6. } else { 0. };
        });
        beside.update(cx, |beside, cx| {
            beside.count += 1;
            cx.notify();
        });
    };
    let view_retention = FastMode {
        splice: false,
        scroll_layers: false,
        ..FastMode::ALL
    };
    let splice = FastMode {
        scroll_layers: false,
        ..FastMode::ALL
    };
    changed_without_scroll(cx, view_retention, "view retention", change);
    changed_without_scroll(cx, splice, "splice", change);
    // Nothing notified: the view holding the list is not reused.
    let alone: fn(&mut TestAppContext, AnyWindowHandle) = |cx, window| {
        let page = page(cx, window);
        let measured_extra = page.read_with(cx, |page, _| page.measured_extra.clone());
        measured_extra.update(cx, |extra, _| {
            extra.height = if extra.height == 0. { 6. } else { 0. };
        });
    };
    changed_without_scroll(cx, view_retention, "view retention, alone", alone);
    changed_without_scroll(cx, splice, "splice, alone", alone);
}

/// A notification of the view holding the list, for what shapes the rows
/// outside any entity, measures the item again.
#[crate::test]
fn a_notification_of_the_lists_view_measures_it_again(cx: &mut TestAppContext) {
    let change: fn(&mut TestAppContext, AnyWindowHandle) = |cx, window| {
        let page = page(cx, window);
        let outside = page.read_with(cx, |page, _| page.outside.clone());
        outside.set(if outside.get() == 0. { 4. } else { 0. });
        cx.update(|cx| cx.notify(page.entity_id()));
    };
    for mode in [
        FastMode {
            splice: false,
            scroll_layers: false,
            ..FastMode::ALL
        },
        FastMode {
            scroll_layers: false,
            ..FastMode::ALL
        },
    ] {
        changed_without_scroll(cx, mode, "notified", change);
    }
}
