//! Frames shown from a horizontal `virtual_list`'s layer equal frames drawn
//! from scratch: the primitives they draw, tiles expanded, at scale factors
//! 1 and 1.25 with fractional column widths, a gap and padding; and whole
//! frame descriptions over random histories of horizontal wheel turns,
//! pointer moves, clicks and column width changes, in two applications run
//! one after the other.

use super::{Kind, Page, Step, compare_steps, open};
use crate::fast::tests::layer_support::{draw, with_window};
use crate::{
    AppContext as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, point, px,
};
use std::rc::Rc;

/// Columns of fractional widths, 12.3 to 31.7 px.
fn fractional_widths() -> Vec<f32> {
    (0..3_000)
        .map(|column| 12.3 + ((column * 7) % 11) as f32 * 1.94)
        .collect()
}

fn fractional_page() -> Page {
    let mut page = Page::new(Kind::Horizontal, fractional_widths());
    page.gap = 1.5;
    page.padding = 4.25;
    page
}

/// Wheel turns across the list from `seed`, whole, fractional and half a
/// device pixel at 1.25.
fn fractional_wheels(seed: u32, count: usize) -> Vec<Step> {
    const DELTAS: [f32; 7] = [-0.37, 0.37, -12.5, 12.5, -1.0, -7.3, -33.4];
    let mut state: u32 = 0x2545_f491 ^ seed.wrapping_mul(0x9e37_79b9);
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            Step::Wheel(DELTAS[((state >> 16) as usize) % DELTAS.len()], 0.)
        })
        .collect()
}

#[crate::test]
fn horizontal_quads_equal_those_drawn_without_layers_at_scale_1(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_steps(
        cx,
        fractional_page,
        &fractional_wheels(21, 80),
        1.,
        "scale 1",
        true,
    );
    assert!(counters["layers_composited"] >= 40, "{counters:?}");
}

#[crate::test]
fn horizontal_quads_equal_those_drawn_without_layers_at_scale_1_25(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_steps(
        cx,
        fractional_page,
        &fractional_wheels(22, 80),
        1.25,
        "scale 1.25",
        true,
    );
    assert!(counters["layers_composited"] >= 40, "{counters:?}");
}

#[derive(Clone, Copy, Debug)]
enum Input {
    Wheel(f32),
    Move(f32),
    Click(f32),
    /// Two columns grow or shrink.
    Resize,
}

/// A random history from `seed`: mostly wheel turns across the list, with
/// pointer moves and clicks over the columns and column width changes.
fn history(seed: u32, count: usize) -> Vec<Input> {
    const WHEELS: [f32; 6] = [-15., -15., -37.5, 20., -0.5, -90.];
    let mut state: u32 = 0x9e37_79b9 ^ seed.wrapping_mul(0x2545_f491);
    let mut next = move || {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (state >> 16) as usize
    };
    (0..count)
        .map(|_| match next() % 10 {
            0 | 1 => Input::Move((next() % 200) as f32 + 0.5),
            2 => Input::Click((next() % 200) as f32 + 0.25),
            3 => Input::Resize,
            _ => Input::Wheel(WHEELS[next() % WHEELS.len()]),
        })
        .collect()
}

/// Each frame's description after each input of `script`, drawn in an
/// application of its own, with layers or as the reference without.
fn described(script: &[Input], layered: bool) -> (Vec<Vec<String>>, u64) {
    let mut cx = crate::fast::tests::oracle_support::oracle_app(5);
    let cx = &mut cx;
    let mut page = Page::new(Kind::Horizontal, super::varied_heights());
    page.interactive = true;
    page.gap = 2.;
    let window = open(cx, page, !layered);
    with_window(cx, window, |window, _| window.reset_fast_stats());
    let at = |x: f32| point(px(x), px(40.));
    let mut frames = Vec::new();
    let mut resizes = 0;
    for input in script {
        let events = match *input {
            Input::Wheel(dx) => vec![PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: at(30.),
                delta: ScrollDelta::Pixels(point(px(dx), px(0.))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            })],
            Input::Move(x) => vec![PlatformInput::MouseMove(MouseMoveEvent {
                position: at(x),
                pressed_button: None,
                modifiers: Default::default(),
            })],
            Input::Click(x) => vec![
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: at(x),
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: at(x),
                    modifiers: Default::default(),
                    click_count: 1,
                }),
            ],
            Input::Resize => {
                resizes += 1;
                let grow = if resizes % 2 == 1 { 6. } else { -6. };
                cx.update_window(window, |root, _, cx| {
                    root.downcast::<Page>().unwrap().update(cx, |page, cx| {
                        let mut extents = (*page.extents).clone();
                        extents[2] += grow;
                        extents[30] += grow;
                        page.extents = Rc::new(extents);
                        cx.notify();
                    })
                })
                .unwrap();
                Vec::new()
            }
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
fn horizontal_frames_shown_from_the_layer_describe_themselves_as_drawn_without() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for seed in 1..=4 {
        let script = history(seed, 50);
        let (layered, shown) = described(&script, true);
        let (reference, none) = described(&script, false);
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
