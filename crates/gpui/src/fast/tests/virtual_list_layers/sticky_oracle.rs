//! Frames of a `virtual_list` with sticky rows shown from its layer equal
//! frames drawn from scratch at every offset of a scroll history: the
//! primitives they draw, tiles expanded, at scale factors 1 and 1.25 with
//! fractional rows, a gap and padding; and whole frame descriptions over
//! random histories of wheel turns, pointer moves and clicks with nested
//! levels, keyed runs and rows styled while stuck, in two applications run
//! one after the other. The rows' sticky state changes are reported in the
//! same order and number as without layers.

use super::{Kind, Page, Sticky, StickyEvents, open, open_at, stats, wheel_by};
use crate::fast::tests::layer_support::{draw, with_window};
use crate::{
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, ScrollDelta,
    ScrollWheelEvent, TestAppContext, TouchPhase, point, px,
};

/// Rows of fractional heights, 12.3 to 31.7 px.
fn fractional_heights() -> Vec<f32> {
    (0..3_000)
        .map(|row| 12.3 + ((row * 7) % 11) as f32 * 1.94)
        .collect()
}

fn fractional_page(sticky: Sticky) -> Page {
    let mut page = Page::new(Kind::Sticky, fractional_heights());
    page.sticky = sticky;
    page.gap = 1.5;
    page.padding = 4.25;
    page
}

/// Wheel turns from `seed`, whole, fractional and half a device pixel at
/// 1.25, mostly down.
fn fractional_wheels(seed: u32, count: usize) -> Vec<f32> {
    const DELTAS: [f32; 7] = [-0.37, 0.37, -12.5, 12.5, -1.0, -7.3, -33.4];
    let mut state: u32 = 0x2545_f491 ^ seed.wrapping_mul(0x9e37_79b9);
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            DELTAS[((state >> 16) as usize) % DELTAS.len()]
        })
        .collect()
}

/// Turns the wheel by each of `deltas` in a window with layers and in the
/// reference, at `scale_factor`, and checks after each that they drew the
/// same primitives and reported the same sticky state changes.
fn compare(cx: &mut TestAppContext, sticky: Sticky, deltas: &[f32], scale_factor: f32) {
    let (layered_page, reference_page) = (fractional_page(sticky), fractional_page(sticky));
    let (layered_events, reference_events): (StickyEvents, StickyEvents) = (
        layered_page.sticky_events.clone(),
        reference_page.sticky_events.clone(),
    );
    let layered = open_at(cx, layered_page, false, scale_factor);
    let reference = open_at(cx, reference_page, true, scale_factor);
    let before = stats(cx, layered).layers_composited;
    for (step, dy) in deltas.iter().copied().enumerate() {
        wheel_by(cx, layered, 0., dy);
        wheel_by(cx, reference, 0., dy);
        let actual = with_window(cx, layered, |window, _| window.painted_primitives());
        let expected = with_window(cx, reference, |window, _| window.painted_primitives());
        if let Some(difference) = crate::first_frame_difference(&actual, &expected) {
            panic!(
                "{sticky:?} at scale {scale_factor}, step {step} ({dy}): the frame differs \
                 from one drawn without layers: {difference}"
            );
        }
        assert_eq!(
            *layered_events.borrow(),
            *reference_events.borrow(),
            "{sticky:?} at scale {scale_factor}, step {step}: the sticky state changes"
        );
    }
    let composited = stats(cx, layered).layers_composited - before;
    assert!(
        composited >= deltas.len() as u64 / 2,
        "{sticky:?} at scale {scale_factor}: {composited} frames composited"
    );
}

#[crate::test]
fn sticky_quads_equal_those_drawn_without_layers_at_scale_1(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    compare(cx, Sticky::Styled, &fractional_wheels(31, 80), 1.);
    compare(cx, Sticky::Levels, &fractional_wheels(32, 80), 1.);
}

#[crate::test]
fn sticky_quads_equal_those_drawn_without_layers_at_scale_1_25(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    compare(cx, Sticky::Styled, &fractional_wheels(33, 80), 1.25);
    compare(cx, Sticky::StackIds, &fractional_wheels(34, 80), 1.25);
}

#[derive(Clone, Copy, Debug)]
enum Input {
    Wheel(f32),
    Move(f32),
    Click(f32),
}

/// A random history from `seed`: mostly wheel turns, with pointer moves and
/// clicks over the rows.
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

/// Each frame's description after each input of `script`, and the sticky
/// state changes reported, drawn in an application of its own under
/// `sticky`, with layers or as the reference without.
fn described(
    sticky: Sticky,
    script: &[Input],
    layered: bool,
) -> (Vec<Vec<String>>, Vec<(usize, String)>, u64) {
    let mut cx = crate::fast::tests::oracle_support::oracle_app(7);
    let cx = &mut cx;
    let mut page = Page::new(Kind::Sticky, super::varied_heights());
    page.sticky = sticky;
    page.interactive = true;
    page.gap = 2.;
    let events = page.sticky_events.clone();
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
    let shown = with_window(cx, window, |window, _| {
        let stats = window.fast_stats();
        stats.layers_composited + stats.layers_repainted
    });
    cx.quit();
    let events = events.borrow().clone();
    (frames, events, shown)
}

#[test]
fn sticky_frames_shown_from_the_layer_describe_themselves_as_drawn_without() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for (seed, sticky) in [
        (1, Sticky::Levels),
        (2, Sticky::StackIds),
        (3, Sticky::Styled),
        (4, Sticky::Levels),
    ] {
        let script = history(seed, 50);
        let (layered, layered_events, shown) = described(sticky, &script, true);
        let (reference, reference_events, none) = described(sticky, &script, false);
        assert_eq!(none, 0, "the reference has no layer");
        assert!(shown > 0, "seed {seed}: the list was shown from its layer");
        for (step, (layered, reference)) in layered.iter().zip(&reference).enumerate() {
            if let Some(difference) = crate::first_frame_difference(layered, reference) {
                panic!(
                    "seed {seed}, {sticky:?}, after step {step} ({:?}): shown from the layer \
                     (actual) differs from drawn without (expected): {difference}",
                    script[step]
                );
            }
        }
        assert_eq!(
            layered_events, reference_events,
            "seed {seed}, {sticky:?}: the sticky state changes"
        );
        if sticky == Sticky::Styled {
            assert!(
                !layered_events.is_empty(),
                "seed {seed}: the rows reported state changes"
            );
        }
    }
}
