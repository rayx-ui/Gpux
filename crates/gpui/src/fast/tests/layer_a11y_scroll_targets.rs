//! A scrolling `div` whose layer composites tells assistive technology where it scrolls to: the
//! scroll target of the `div` itself, which its layer's records begin with, stands where the
//! `div` is and holds the offset the frame scrolled to, while the nodes of its content move with
//! the content. Every composited frame describes itself, accessibility tree and action targets
//! included, as one drawn without layers does.

use crate::fast::tests::layer_support::{add_window, draw, make_reference, with_window};
use crate::{
    AnyWindowHandle, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    PlatformInput, Render, Role, ScrollDelta, ScrollWheelEvent, StatefulInteractiveElement as _,
    Styled as _, TouchPhase, Window, div, hsla, point, px,
};

/// A scrolling `div` of named rows, holding a scrolling `div` of its own among them.
struct Rows;

impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let row = |row: usize| {
            div()
                .id(("row", row))
                .role(Role::ListItem)
                .aria_label(format!("row {row}"))
                .h(px(20.))
                .w_full()
                .bg(hsla(row as f32 / 200., 0.5, 0.5, 1.))
        };
        // An opaque panel: what the layer's tiles are cleared with.
        div()
            .id("rows")
            .bg(crate::white())
            .w(px(200.))
            .h(px(100.))
            .overflow_y_scroll()
            .children((0..60usize).map(row))
            // A container nested in the content, which moves with it.
            .child(
                div()
                    .id("nested")
                    .h(px(40.))
                    .w_full()
                    .overflow_y_scroll()
                    .children((60..70usize).map(row)),
            )
            .children((70..200usize).map(row))
    }
}

/// Each frame's description over wheel turns, in an application of its own.
fn described(reference: bool) -> (Vec<Vec<String>>, u64) {
    let mut cx = crate::fast::tests::oracle_support::oracle_app(5);
    let cx = &mut cx;
    let window: AnyWindowHandle = add_window(cx, |_, _| Rows).into();
    if reference {
        with_window(cx, window, |window, _| make_reference(window));
    }
    let _inspection = with_window(cx, window, |window, _| window.begin_a11y_inspection());
    draw(cx, window);
    draw(cx, window);
    let mut frames = Vec::new();
    for step in 0..24 {
        let dy = if step % 9 == 8 { 35. } else { -15. };
        with_window(cx, window, |window, cx| {
            window.dispatch_event(
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: point(px(190.), px(50.)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                    modifiers: Default::default(),
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            );
        });
        draw(cx, window);
        frames.push(with_window(cx, window, |window, _| {
            window.describe_rendered_frame()
        }));
    }
    let composited = with_window(cx, window, |window, _| {
        window.fast_stats().layers_composited
    });
    cx.quit();
    (frames, composited)
}

#[test]
fn a_composited_scroll_container_tells_where_it_scrolls_to() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let (frames, composited) = described(false);
    let (expected, _) = described(true);
    assert!(composited >= 15, "the layer composited {composited} frames");
    for (step, (shown, expected)) in frames.iter().zip(&expected).enumerate() {
        if let Some(difference) = crate::first_frame_difference(shown, expected) {
            panic!(
                "step {step}: shown from the layer (actual) differs from drawn without \
                 (expected): {difference}"
            );
        }
    }
}
