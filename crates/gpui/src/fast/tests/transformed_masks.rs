//! A transformed element's clip mask painted into a layer's region (`layers::paint::transformed_clip`):
//! the same mask as painted without one.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::transformed_masks::
//! ```

use crate::{
    AnyWindowHandle, InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollDelta,
    ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _, TestAppContext, TouchPhase,
    div,
    fast::tests::layer_support::{add_window, make_reference, with_window},
    point, px, rgb,
};

/// A scrolling `div` of cards, each turned, moved or scaled, with a border and rounded corners.
struct Page;

impl Render for Page {
    fn render(&mut self, _: &mut crate::Window, _: &mut crate::Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x1e2430)).p(px(16.)).child(
            div()
                .id("scroll")
                .size_full()
                .bg(rgb(0xf4f1ea))
                .overflow_y_scroll()
                .children((0..40).map(|index| {
                    let card = div()
                        .mx_3()
                        .h(px(44.))
                        .bg(rgb(0xffffff))
                        .rounded(px(6.))
                        .border_2()
                        .border_color(rgb(0x555555));
                    let card = match index % 3 {
                        0 => card.rotate(0.03),
                        1 => card.translate(point(px(3.), px(5.))),
                        _ => card.scale(1.05),
                    };
                    div()
                        .h(px(60.))
                        .py(px(8.))
                        .bg(if index % 2 == 0 {
                            rgb(0xe9e4d8)
                        } else {
                            rgb(0xd5dde6)
                        })
                        .child(card)
                })),
        )
    }
}

fn open(cx: &mut TestAppContext, layered: bool) -> AnyWindowHandle {
    let window: AnyWindowHandle = add_window(cx, |_, _| Page).into();
    cx.test_window(window)
        .simulate_resize(crate::size(px(420.), px(360.)));
    cx.test_window(window).simulate_scale_factor_change(2.);
    with_window(cx, window, |window, _| {
        if layered {
            window.set_fast_mode(crate::FastMode::ALL)
        } else {
            make_reference(window)
        }
    });
    with_window(cx, window, |window, cx| window.draw(cx).clear(cx));
    with_window(cx, window, |window, cx| window.draw(cx).clear(cx));
    window
}

/// What a frame draws: its primitives, with their bounds, masks and orders, and its layers.
fn drawn(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<String> {
    with_window(cx, window, |window, _| {
        crate::fast::verify::describe_frame(window)
            .into_iter()
            .filter(|line| {
                line.starts_with("primitive")
                    || line.starts_with("scene")
                    || line.starts_with("layer")
            })
            .collect()
    })
}

fn scroll(cx: &mut TestAppContext, window: AnyWindowHandle, dy: f32) {
    with_window(cx, window, |window, cx| {
        window.dispatch_event(
            crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(210.), px(180.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    });
    cx.run_until_parked();
    with_window(cx, window, |window, cx| {
        if window.is_dirty() {
            window.draw(cx).clear(cx);
        }
    });
}

/// The window with every mechanism and the reference draw the same primitives with the same
/// masks, step by step down and up a list of transformed bordered cards: including the frames
/// in which the container is tried again for a layer and found to hold what no tile can.
#[crate::test]
fn a_scrolling_div_of_transformed_bordered_cards_draws_what_its_reference_draws(
    cx: &mut TestAppContext,
) {
    let (layered, reference) = (open(cx, true), open(cx, false));
    let script: Vec<f32> = (0..4)
        .map(|_| -20.)
        .chain((0..120).map(|_| -1.))
        .chain((0..60).map(|_| 1.))
        .chain([-300., -211.])
        .chain((0..100).map(|_| -1.))
        .collect();
    for (step, dy) in script.iter().enumerate() {
        for window in [layered, reference] {
            scroll(cx, window, *dy);
        }
        let (a, b) = (drawn(cx, layered), drawn(cx, reference));
        assert_eq!(a, b, "step {step}");
    }
}
