//! A `ParentViewport` `virtual_list` in a scrolling `div` lets the `div`'s
//! layer stay promoted: painted into the layer, the list renders the rows of
//! the region the layer paints and reads no parent offset, and on the frames
//! that composite the layer it answers its metrics and calls its range
//! listener from the parent's offset then. The rows the layer holds that the
//! viewport does not show answer nothing: no hitbox, tab stop, focus,
//! listener or accessibility node, as a frame drawn without layers renders
//! none of them. Every frame equals one drawn from scratch, metrics and
//! ranges included.

use super::{Kind, Page, Step, compare_steps, compare_with_layers_off, open, stats, wheel};
use super::{difference, varied_heights, wheel_by, wheel_history};
use crate::fast::tests::layer_support::{add_window, draw, with_window};
use crate::{
    AnyWindowHandle, Axis, Context, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PlatformInput, Render, Role,
    ScrollDelta, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _,
    TestAppContext, TouchPhase, VirtualListExternalScrollBehavior, VirtualListScrollHandle, Window,
    div, point, px, size,
};
use std::rc::Rc;

const FRAMES: usize = 60;

/// Over [`FRAMES`] wheel frames of 15 px the parent's layer composites and
/// is never demoted, painted again only as the scroll reaches the edge of
/// the region it painted.
#[crate::test]
fn the_parent_layer_stays_promoted(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let window = open(cx, Page::new(Kind::ParentViewport, varied_heights()), false);
    for _ in 0..4 {
        wheel(cx, window, -15.);
    }
    let before = stats(cx, window);
    for _ in 0..FRAMES {
        wheel(cx, window, -15.);
    }
    let work = difference(&stats(cx, window), &before);
    let counter = |name: &str| work.get(name).copied().unwrap_or(0);
    assert_eq!(
        counter("layers_composited") + counter("layers_repainted"),
        FRAMES as u64,
        "every frame is shown from the parent's layer: {work:?}"
    );
    assert_eq!(counter("layer_bypasses.demoted"), 0, "{work:?}");
    // The layer paints the viewport and twice its height on either side:
    // 900 px of scroll reach its edge a few times.
    assert!(
        counter("layers_repainted") <= 8,
        "the layer was painted again more often than the scroll reaches its edge: {work:?}"
    );
    assert!(counter("layers_composited") >= 50, "{work:?}");
}

/// A history of wheel turns, whole and fractional, draws what a window
/// without layers draws, and the list publishes the same metrics and range
/// on every frame, composited ones included.
#[crate::test]
fn metrics_and_ranges_on_composited_frames_are_as_drawn_without(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_with_layers_off(
        cx,
        || Page::new(Kind::ParentViewport, varied_heights()),
        &wheel_history(31, 60),
        "parent viewport",
    );
    assert!(counters["layers_composited"] >= 30, "{counters:?}");
    assert_eq!(
        counters.get("layer_bypasses.demoted").copied().unwrap_or(0),
        0,
        "{counters:?}"
    );
}

/// Rows of fractional heights with a gap and padding at a scale of 1.25.
#[crate::test]
fn fractional_rows_at_scale_1_25_are_as_drawn_without(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let page = || {
        let mut page = Page::new(
            Kind::ParentViewport,
            (0..3_000)
                .map(|row| 12.3 + ((row * 7) % 11) as f32 * 1.94)
                .collect(),
        );
        page.gap = 1.5;
        page.padding = 4.25;
        page
    };
    let steps: Vec<Step> = [-0.37, -12.5, 12.5, -7.3, -33.4, 0.37, -15.]
        .iter()
        .cycle()
        .take(70)
        .map(|dy| Step::Wheel(0., *dy))
        .collect();
    let counters = compare_steps(cx, page, &steps, 1.25, "scale 1.25", true);
    assert!(counters["layers_composited"] >= 30, "{counters:?}");
}

#[derive(Clone, Copy, Debug)]
enum Input {
    Wheel(f32),
    Move(f32),
    Click(f32),
}

fn history(seed: u32, count: usize) -> Vec<Input> {
    const WHEELS: [f32; 6] = [-15., -15., -37.5, 20., -0.5, -90.];
    let mut state: u32 = 0x9e37_79b9 ^ seed.wrapping_mul(0x2545_f491);
    let mut next = move || {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (state >> 16) as usize
    };
    (0..count)
        .map(|_| match next() % 8 {
            0 | 1 => Input::Move((next() % 100) as f32 + 0.5),
            2 => Input::Click((next() % 100) as f32 + 0.25),
            _ => Input::Wheel(WHEELS[next() % WHEELS.len()]),
        })
        .collect()
}

/// Each frame's description after each input of `script` over rows that
/// listen for clicks, drawn in an application of its own, with layers or as
/// the reference without, with the rows clicked and the frames composited.
fn described(script: &[Input], layered: bool) -> (Vec<Vec<String>>, Vec<usize>, u64) {
    let mut cx = crate::fast::tests::oracle_support::oracle_app(11);
    let cx = &mut cx;
    let mut page = Page::new(Kind::ParentViewport, varied_heights());
    page.clicks = true;
    let log = page.log.clone();
    let window = open(cx, page, !layered);
    with_window(cx, window, |window, _| window.reset_fast_stats());
    let at = |y: f32| point(px(40.), px(y));
    let mut frames = Vec::new();
    for input in script {
        let events = match *input {
            Input::Wheel(dy) => vec![PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: at(30.),
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            })],
            Input::Move(y) => vec![PlatformInput::MouseMove(MouseMoveEvent {
                position: at(y),
                pressed_button: None,
                modifiers: Default::default(),
            })],
            Input::Click(y) => vec![
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: at(y),
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: at(y),
                    modifiers: Default::default(),
                    click_count: 1,
                }),
            ],
        };
        for event in events {
            with_window(cx, window, |window, cx| {
                window.dispatch_event(event, cx);
            });
        }
        draw(cx, window);
        frames.push(with_window(cx, window, |window, _| {
            window.describe_rendered_frame()
        }));
    }
    let composited = with_window(cx, window, |window, _| {
        window.fast_stats().layers_composited
    });
    cx.quit();
    let clicked = log.borrow().clicked.clone();
    (frames, clicked, composited)
}

/// Whole frame descriptions over random histories of wheel turns, pointer
/// moves and clicks over rows that listen for clicks: what the rows the layer
/// holds off screen record answers nothing, as without layers, and the same
/// rows are clicked.
#[test]
fn promoted_frames_describe_themselves_as_drawn_without() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let mut composited = 0;
    for seed in 1..=4 {
        let script = history(seed, 50);
        let (layered, clicked, shown) = described(&script, true);
        let (reference, expected, none) = described(&script, false);
        assert_eq!(none, 0, "the reference has no layer");
        assert_eq!(clicked, expected, "seed {seed}: the rows clicked");
        composited += shown;
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
    assert!(composited > 0, "the parent's layer composited");
}

/// The list items `window`'s accessibility tree holds, in tree order, with
/// their bounds: the rows a frame tells assistive technology of.
fn told_items(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<String> {
    with_window(cx, window, |window, _| {
        let tree = window.a11y_tree().expect("the window is inspected");
        let nodes: collections::FxHashMap<_, _> =
            tree.nodes.iter().map(|(id, node)| (*id, node)).collect();
        let mut items = Vec::new();
        let mut pending = vec![tree.tree.as_ref().expect("a full tree").root];
        while let Some(id) = pending.pop() {
            let Some(node) = nodes.get(&id) else {
                continue;
            };
            if node.role() == accesskit::Role::ListItem {
                items.push(format!("{:?} {:?}", node.label(), node.bounds()));
            }
            pending.extend(node.children().iter().rev().copied());
        }
        items
    })
}

/// Turns the wheel by each of `steps` over both windows and checks, on every
/// frame, that the layered one tells assistive technology of the rows the
/// reference does, where it does, and draws what it draws. Returns the
/// frames composited.
fn compare_told(
    cx: &mut TestAppContext,
    layered: AnyWindowHandle,
    reference: AnyWindowHandle,
    steps: &[f32],
    label: &str,
) -> u64 {
    let _inspections: Vec<_> = [layered, reference]
        .into_iter()
        .map(|window| {
            let inspection = with_window(cx, window, |window, _| window.begin_a11y_inspection());
            // The first inspected frame is drawn from scratch; the next as usual.
            draw(cx, window);
            draw(cx, window);
            inspection
        })
        .collect();
    let before = stats(cx, layered);
    for (step, dy) in steps.iter().enumerate() {
        wheel_by(cx, layered, 0., *dy);
        wheel_by(cx, reference, 0., *dy);
        let (shown, expected) = (told_items(cx, layered), told_items(cx, reference));
        assert!(
            !expected.is_empty(),
            "{label}, step {step}: the tree holds rows"
        );
        assert_eq!(
            shown, expected,
            "{label}, step {step} ({dy}): the rows told from the layer (left) differ from \
             those told without it (right)"
        );
        let primitives = |cx: &mut TestAppContext, window| {
            with_window(cx, window, |window, _| window.painted_primitives())
        };
        assert_eq!(
            primitives(cx, layered),
            primitives(cx, reference),
            "{label}, step {step}: drawn"
        );
    }
    let work = difference(&stats(cx, layered), &before);
    work.get("layers_composited").copied().unwrap_or(0)
}

/// With accessibility on, the frames that composite the parent's layer tell
/// of the rows the viewport shows now, not those it showed when the layer
/// painted: the rows the layer holds off screen are hidden from the tree,
/// and those a scroll brings in are shown, where they lie now.
#[crate::test]
fn composited_frames_tell_the_rows_the_viewport_shows(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let page = || {
        let mut page = Page::new(Kind::ParentViewport, varied_heights());
        page.labelled = true;
        page
    };
    let layered = open(cx, page(), false);
    let reference = open(cx, page(), true);
    let steps: Vec<f32> = [-15., -15., -37.5, 20., -0.5, -90., -15., 45., -15.]
        .iter()
        .cycle()
        .take(40)
        .copied()
        .collect();
    let composited = compare_told(cx, layered, reference, &steps, "parent viewport");
    assert!(
        composited >= 20,
        "the parent's layer composited {composited} frames"
    );
}

const OUTER_ROWS: usize = 400;
const INNER_ROWS: usize = 3;
const INNER_ROW: f32 = 18.;
const OUTER_LABEL: f32 = 12.;
const OUTER_END: f32 = 6.;

/// A scrolling `div` holding a `ParentViewport` list whose rows each hold a
/// `ParentViewport` list of their own in the same `div`'s flow, every row a
/// named list item.
struct Nested {
    parent: ScrollHandle,
    outer: VirtualListScrollHandle,
    inner: Vec<VirtualListScrollHandle>,
}

impl Nested {
    fn new() -> Self {
        let parent = ScrollHandle::new();
        Nested {
            outer: VirtualListScrollHandle::from(parent.clone()),
            inner: (0..OUTER_ROWS)
                .map(|_| VirtualListScrollHandle::from(parent.clone()))
                .collect(),
            parent,
        }
    }
}

impl Render for Nested {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let outer_height = OUTER_LABEL + INNER_ROWS as f32 * INNER_ROW + OUTER_END;
        let sizes = Rc::new(vec![size(px(super::W), px(outer_height)); OUTER_ROWS]);
        let list = crate::virtual_list(
            cx.entity(),
            "outer",
            Axis::Vertical,
            sizes,
            move |this: &mut Nested, range, _, cx| {
                range
                    .map(|row| {
                        let inner_sizes =
                            Rc::new(vec![size(px(super::W), px(INNER_ROW)); INNER_ROWS]);
                        let inner = crate::virtual_list(
                            cx.entity(),
                            ("inner", row),
                            Axis::Vertical,
                            inner_sizes,
                            move |_, range, _, _| {
                                range
                                    .map(|item| {
                                        div()
                                            .id(("inner item", row * INNER_ROWS + item))
                                            .role(Role::ListItem)
                                            .aria_label(format!("inner {row}.{item}"))
                                            .h(px(INNER_ROW))
                                            .w_full()
                                            .bg(super::row_color(row * INNER_ROWS + item, false))
                                    })
                                    .collect::<Vec<_>>()
                            },
                        )
                        .track_external_scroll(
                            &this.inner[row],
                            VirtualListExternalScrollBehavior::ParentViewport,
                        )
                        .w_full();
                        // A container of no role, whose items are its children: none of
                        // them is nested in a node of the row.
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .h(px(outer_height))
                            .child(
                                div()
                                    .id(("outer item", row))
                                    .role(Role::ListItem)
                                    .aria_label(format!("outer {row}"))
                                    .h(px(OUTER_LABEL))
                                    .w_full()
                                    .bg(crate::black()),
                            )
                            .child(inner)
                            // Told after the inner list, as hidden as the outer row.
                            .child(
                                div()
                                    .id(("outer end", row))
                                    .role(Role::ListItem)
                                    .aria_label(format!("outer {row} end"))
                                    .h(px(OUTER_END))
                                    .w_full(),
                            )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_external_scroll(
            &self.outer,
            VirtualListExternalScrollBehavior::ParentViewport,
        )
        .w_full();
        div().size_full().bg(crate::white()).child(
            div()
                .id("parent")
                .flex()
                .flex_col()
                .w(px(super::W))
                .h(px(super::H))
                .overflow_y_scroll()
                .track_scroll(&self.parent)
                .child(div().flex_none().h(px(30.)).w_full())
                .child(list),
        )
    }
}

fn open_nested(cx: &mut TestAppContext, reference: bool) -> AnyWindowHandle {
    let window: AnyWindowHandle = add_window(cx, |_, _| Nested::new()).into();
    if reference {
        with_window(cx, window, |window, _| {
            crate::fast::tests::layer_support::make_reference(window)
        });
    }
    draw(cx, window);
    draw(cx, window);
    window
}

/// A promoted list nested in a row of another in the same `div` tells, on
/// the frames that composite the `div`'s layer, of the rows of both that the
/// viewport shows, as without a layer: a row of the inner list shown while
/// the outer row holding it is not stays hidden, and the outer row's nodes
/// after the inner list (its end) keep the outer row's hidden-ness.
#[crate::test]
fn nested_promoted_lists_tell_the_rows_the_viewport_shows(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let layered = open_nested(cx, false);
    let reference = open_nested(cx, true);
    let steps: Vec<f32> = [-15., -21., 9., -40., -3.5, -66.]
        .iter()
        .cycle()
        .take(36)
        .copied()
        .collect();
    let composited = compare_told(cx, layered, reference, &steps, "nested");
    assert!(
        composited >= 15,
        "the parent's layer composited {composited} frames"
    );
}
