//! Frames shown from a `virtual_list`'s layer equal frames drawn from
//! scratch: the primitives they draw, tiles expanded, at scale factors 1,
//! 1.25 and 2 with fractional row sizes, a gap and padding (on half a device
//! pixel at 2); and whole frame
//! descriptions over random histories of wheel turns, pointer moves and
//! clicks, in two applications run one after the other at scale 1.

use super::{Kind, Page, Step, compare_steps, open};
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

fn fractional_page() -> Page {
    let mut page = Page::new(Kind::Virtual, fractional_heights());
    page.gap = 1.5;
    page.padding = 4.25;
    page
}

/// Wheel turns from `seed`, whole, fractional and half a device pixel at
/// 1.25.
fn fractional_wheels(seed: u32, count: usize) -> Vec<Step> {
    const DELTAS: [f32; 7] = [-0.37, 0.37, -12.5, 12.5, -1.0, -7.3, -33.4];
    let mut state: u32 = 0x2545_f491 ^ seed.wrapping_mul(0x9e37_79b9);
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            Step::Wheel(0., DELTAS[((state >> 16) as usize) % DELTAS.len()])
        })
        .collect()
}

#[crate::test]
fn quads_equal_those_drawn_without_layers_at_scale_1(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_steps(
        cx,
        fractional_page,
        &fractional_wheels(11, 80),
        1.,
        "scale 1",
        true,
    );
    assert!(counters["layers_composited"] >= 40, "{counters:?}");
}

#[crate::test]
fn quads_equal_those_drawn_without_layers_at_scale_1_25(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_steps(
        cx,
        fractional_page,
        &fractional_wheels(12, 80),
        1.25,
        "scale 1.25",
        true,
    );
    assert!(counters["layers_composited"] >= 40, "{counters:?}");
}

#[crate::test]
fn quads_equal_those_drawn_without_layers_at_scale_2(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_steps(
        cx,
        fractional_page,
        &fractional_wheels(13, 80),
        2.,
        "scale 2",
        true,
    );
    assert!(counters["layers_composited"] >= 40, "{counters:?}");
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Input {
    Wheel(f32),
    Move(f32),
    Click(f32),
}

/// A random history from `seed`: mostly wheel turns, with pointer moves and
/// clicks over the rows.
pub(super) fn history(seed: u32, count: usize) -> Vec<Input> {
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

/// Each frame's description after each input of `script`, drawn in an
/// application of its own, with layers or as the reference without.
pub(super) fn described(
    page: impl FnOnce() -> Page,
    script: &[Input],
    layered: bool,
) -> (Vec<Vec<String>>, u64) {
    let mut cx = crate::fast::tests::oracle_support::oracle_app(3);
    let cx = &mut cx;
    let window = open(cx, page(), !layered);
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
    (frames, shown)
}

#[test]
fn whole_frames_shown_from_the_layer_describe_themselves_as_drawn_without() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for seed in 1..=4 {
        let script = history(seed, 50);
        let page = || {
            let mut page = Page::new(Kind::Virtual, super::varied_heights());
            page.interactive = true;
            page.gap = 2.;
            page
        };
        let (layered, shown) = described(page, &script, true);
        let (reference, none) = described(page, &script, false);
        assert_eq!(none, 0, "the reference has no layer");
        assert!(shown > 0, "seed {seed}: the list was shown from its layer");
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
