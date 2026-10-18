//! Edges on exactly half a device pixel land alike in a frame shown from a
//! scroll layer and one drawn from scratch, with every kind of scroll layer
//! (a scrolling `div`, `uniform_list`, `list`, `virtual_list`) at scale
//! factors 1, 1.25, 1.5 and 2.
//!
//! Each row holds a block inset by an amount that puts its top and bottom
//! edges on half a device pixel, and the wheel turns by whole device pixels,
//! so the scroll moves those edges across the window's origin as the rows
//! leave the top. Every few turns the list's top padding changes by half a
//! device pixel, which moves the rows a layer holds by a tie. A layer keeps
//! what it holds and moves it, so the rounding of a tie must not depend on
//! where the edge was when the layer took it.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::half_pixel_edges
//! ```

use crate::fast::tests::layer_support::{add_window, draw, make_reference, with_window};
use crate::{
    AnyWindowHandle, AppContext as _, Axis, Context, InteractiveElement as _, IntoElement,
    ListAlignment, ListState, ParentElement as _, Render, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _, TestAppContext, TouchPhase,
    Window, div, hsla, point, px, size,
};
use std::rc::Rc;

const W: f32 = 200.;
const H: f32 = 100.;
const ROWS: usize = 400;
/// A whole number of device pixels at every scale factor tested.
const ROW: f32 = 20.;
/// Whole device pixels at every scale factor tested.
const TURN: f32 = -4.;
const TURNS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Div,
    Uniform,
    List,
    Virtual,
}

/// The inset that puts an edge on half a device pixel at `scale_factor`.
fn tie_inset(scale_factor: f32) -> f32 {
    match scale_factor {
        1. => 0.5,
        1.25 => 2.,
        1.5 => 1.,
        2. => 0.25,
        _ => unreachable!("no tie chosen for scale {scale_factor}"),
    }
}

fn row(index: usize, inset: f32) -> impl IntoElement {
    div().w_full().h(px(ROW)).flex_none().child(
        div()
            .mt(px(inset))
            .h(px(ROW - 2. * inset))
            .w_full()
            .bg(hsla((index % 13) as f32 / 13., 0.6, 0.5, 1.)),
    )
}

struct Page {
    kind: Kind,
    inset: f32,
    /// The list's top padding, toggled between none and a tie.
    lead: f32,
    scroll: ScrollHandle,
    list: ListState,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inset = self.inset;
        let lead = px(self.lead);
        let list = match self.kind {
            Kind::Div => div()
                .id("scroller")
                .w(px(W))
                .h(px(H))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .pt(lead)
                .children((0..ROWS).map(|index| row(index, inset)))
                .into_any_element(),
            Kind::Uniform => crate::uniform_list("list", ROWS, move |range, _, _| {
                range.map(|index| row(index, inset)).collect::<Vec<_>>()
            })
            .w(px(W))
            .h(px(H))
            .pt(lead)
            .into_any_element(),
            Kind::List => crate::list(self.list.clone(), move |index, _, _| {
                row(index, inset).into_any_element()
            })
            .w(px(W))
            .h(px(H))
            .pt(lead)
            .into_any_element(),
            Kind::Virtual => crate::virtual_list(
                cx.entity(),
                "list",
                Axis::Vertical,
                Rc::new(vec![size(px(W), px(ROW)); ROWS]),
                move |_, range, _, _| range.map(|index| row(index, inset)).collect::<Vec<_>>(),
            )
            .w(px(W))
            .h(px(H))
            .pt(lead)
            .into_any_element(),
        };
        div().size_full().bg(crate::white()).child(list)
    }
}

fn open(
    cx: &mut TestAppContext,
    kind: Kind,
    scale_factor: f32,
    reference: bool,
) -> AnyWindowHandle {
    let window: AnyWindowHandle = add_window(cx, move |_, _| Page {
        kind,
        inset: tie_inset(scale_factor),
        lead: 0.,
        scroll: ScrollHandle::new(),
        list: ListState::new(ROWS, ListAlignment::Top, px(0.)).measure_all(),
    })
    .into();
    cx.test_window(window)
        .simulate_scale_factor_change(scale_factor);
    if reference {
        with_window(cx, window, |window, _| make_reference(window));
    }
    draw(cx, window);
    draw(cx, window);
    window
}

fn wheel(cx: &mut TestAppContext, window: AnyWindowHandle) {
    let frame = with_window(cx, window, |window, _| window.fast_layers.frame);
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
    if with_window(cx, window, |window, _| window.fast_layers.frame) == frame {
        draw(cx, window);
    }
}

/// Scrolls a layered window and a reference alike, comparing every frame.
fn compare(cx: &mut TestAppContext, kind: Kind) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for scale_factor in [1., 1.25, 1.5, 2.] {
        let layered = open(cx, kind, scale_factor, false);
        let reference = open(cx, kind, scale_factor, true);
        let composited_before = with_window(cx, layered, |window, _| {
            window.fast_stats().layers_composited
        });
        for turn in 0..TURNS {
            if turn % 12 == 7 {
                for window in [layered, reference] {
                    cx.update_window(window, |root, _, cx| {
                        root.downcast::<Page>().unwrap().update(cx, |page, cx| {
                            page.lead = if page.lead == 0. { page.inset } else { 0. };
                            cx.notify();
                        })
                    })
                    .unwrap();
                }
            }
            wheel(cx, layered);
            wheel(cx, reference);
            let actual = with_window(cx, layered, |window, _| window.painted_primitives());
            let expected = with_window(cx, reference, |window, _| window.painted_primitives());
            if let Some(difference) =
                crate::fast::verify::first_frame_difference(&actual, &expected)
            {
                panic!(
                    "{kind:?} at scale {scale_factor}, turn {turn}: a half-pixel edge landed \
                     otherwise than in a frame drawn from scratch: {difference}"
                );
            }
        }
        let composited = with_window(cx, layered, |window, _| {
            window.fast_stats().layers_composited
        }) - composited_before;
        assert!(
            // A padding change repaints a `div`'s or a `list`'s layer, which
            // composites again a few turns later.
            composited >= (TURNS / 4) as u64,
            "{kind:?} at scale {scale_factor}: only {composited} frames were shown from a layer"
        );
    }
}

#[crate::test]
fn a_div_layer_lands_half_pixel_edges_as_drawn_from_scratch(cx: &mut TestAppContext) {
    compare(cx, Kind::Div);
}

#[crate::test]
fn a_uniform_list_layer_lands_half_pixel_edges_as_drawn_from_scratch(cx: &mut TestAppContext) {
    compare(cx, Kind::Uniform);
}

#[crate::test]
fn a_list_layer_lands_half_pixel_edges_as_drawn_from_scratch(cx: &mut TestAppContext) {
    compare(cx, Kind::List);
}

#[crate::test]
fn a_virtual_list_layer_lands_half_pixel_edges_as_drawn_from_scratch(cx: &mut TestAppContext) {
    compare(cx, Kind::Virtual);
}
