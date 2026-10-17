//! A `virtual_list` in `SynchronizedOffset` mode, driven by a scrolling
//! `div` beside it (a table's header and body), composites from a list layer
//! of its own, keyed by the list, on every frame its driver only scrolls:
//! vertical and horizontal. The notification the shared handle sends the
//! list's view on a scroll counts as the scroll's.

use super::{
    FrameWork, Kind, Page, compare_with_layers_off, difference, mean, open, stats, varied_heights,
    wheel_by, wheel_history,
};
use crate::fast::tests::layer_support::{draw, with_window};
use crate::{
    GlobalElementId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, point, px,
};

const FRAMES: usize = 60;

/// Whether a layer is kept under an id ending in the list's own id.
fn list_layer_kept(cx: &mut TestAppContext, window: crate::AnyWindowHandle) -> bool {
    with_window(cx, window, |window, _| {
        window
            .fast_layers
            .layers
            .keys()
            .any(|id: &GlobalElementId| id.last() == Some(&"list".into()))
    })
}

/// Turns the wheel by `(dx, dy)` over the driver on each of [`FRAMES`]
/// frames after four turns to promote the list, and checks each frame is
/// shown from the list's layer.
fn composites(cx: &mut TestAppContext, kind: Kind, dx: f32, dy: f32) {
    let page = Page::new(kind, varied_heights());
    let log = page.log.clone();
    let window = open(cx, page, false);
    for _ in 0..4 {
        wheel_by(cx, window, dx, dy);
    }
    let work: Vec<FrameWork> = (0..FRAMES)
        .map(|_| {
            let before = stats(cx, window);
            log.borrow_mut().calls.clear();
            wheel_by(cx, window, dx, dy);
            FrameWork {
                counters: difference(&stats(cx, window), &before),
                closure_rows: log.borrow().rows_rendered(),
                calls: log.borrow().calls.clone(),
            }
        })
        .collect();
    assert!(
        list_layer_kept(cx, window),
        "{kind:?}: the list has a layer"
    );
    let mut kept_frames = 0;
    for (frame, work) in work.iter().enumerate() {
        assert_eq!(
            work.counter("layer_bypasses.ineligible"),
            0,
            "{kind:?}, frame {frame}: {:?}",
            work.counters
        );
        assert_eq!(
            work.closure_rows as u64,
            work.counter("list_rows_rendered"),
            "{kind:?}, frame {frame}: the row closure ran for {:?}",
            work.calls
        );
        // Only a list's layer keeps rows: the driver's keeps its content.
        if work.counter("list_rows_kept") > 0 {
            kept_frames += 1;
        }
    }
    assert!(
        kept_frames >= FRAMES - 1,
        "{kind:?}: {kept_frames} of {FRAMES} frames composited the list's rows"
    );
    let rendered = mean(&work, "list_rows_rendered");
    assert!(
        rendered <= 2.,
        "{kind:?}: {rendered} rows rendered a frame on average, more than 2"
    );
}

#[crate::test]
fn a_synchronized_list_composites_from_its_own_layer(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    composites(cx, Kind::Synchronized, 0., -15.);
}

#[crate::test]
fn a_horizontal_synchronized_list_composites_from_its_own_layer(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    composites(cx, Kind::SynchronizedHorizontal, -15., 0.);
}

/// Histories of wheel turns over the driver, whole and fractional, draw
/// what a window without layers draws and publish the ranges it publishes.
#[crate::test]
fn synchronized_lists_draw_as_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_with_layers_off(
        cx,
        || Page::new(Kind::Synchronized, varied_heights()),
        &wheel_history(7, 40),
        "synchronized offset",
    );
    assert!(counters["list_rows_kept"] > 0, "{counters:?}");
    let across: Vec<(f32, f32)> = wheel_history(8, 40)
        .into_iter()
        .map(|(_, dy)| (dy, 0.))
        .collect();
    let counters = compare_with_layers_off(
        cx,
        || Page::new(Kind::SynchronizedHorizontal, varied_heights()),
        &across,
        "horizontal synchronized offset",
    );
    assert!(counters["list_rows_kept"] > 0, "{counters:?}");
}

#[derive(Clone, Copy, Debug)]
enum Input {
    Wheel(f32),
    Move(f32),
    Click(f32),
}

/// A random history from `seed`: mostly wheel turns over the driver, with
/// pointer moves and clicks over the list's rows.
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

/// Each frame's description after each input of `script`, drawn in an
/// application of its own, with layers or as the reference without.
fn described(script: &[Input], layered: bool) -> (Vec<Vec<String>>, u64) {
    let mut cx = crate::fast::tests::oracle_support::oracle_app(9);
    let cx = &mut cx;
    let mut page = Page::new(Kind::Synchronized, varied_heights());
    page.interactive = true;
    let window = open(cx, page, !layered);
    with_window(cx, window, |window, _| window.reset_fast_stats());
    // The driver is the left half, the list the right.
    let over_list = |y: f32| point(px(140.), px(y));
    let mut frames = Vec::new();
    for input in script {
        let events = match *input {
            Input::Wheel(dy) => vec![PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(30.), px(30.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            })],
            Input::Move(y) => vec![PlatformInput::MouseMove(MouseMoveEvent {
                position: over_list(y),
                pressed_button: None,
                modifiers: Default::default(),
            })],
            Input::Click(y) => vec![
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: over_list(y),
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: over_list(y),
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
    let kept = with_window(cx, window, |window, _| window.fast_stats().list_rows_kept);
    cx.quit();
    (frames, kept)
}

#[test]
fn synchronized_frames_shown_from_the_layer_describe_themselves_as_drawn_without() {
    if !crate::fast::layers::COMPILED {
        return;
    }
    for seed in 1..=4 {
        let script = history(seed, 50);
        let (layered, kept) = described(&script, true);
        let (reference, none) = described(&script, false);
        assert_eq!(none, 0, "the reference has no layer");
        assert!(kept > 0, "seed {seed}: the list kept rows in its layer");
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

/// A list driven by a sibling that nothing clips to its bounds draws its rows
/// past them, as without layers, while their hitboxes are clipped to the
/// list: a layer, clipping both to its viewport, would tell them apart. It is
/// counted ineligible, keeps no layer and draws what a window without layers
/// draws.
#[crate::test]
fn an_unclipped_synchronized_list_is_ineligible_and_draws_as_without_layers(
    cx: &mut TestAppContext,
) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let unclipped = || {
        let mut page = Page::new(Kind::Synchronized, varied_heights());
        page.clipped = false;
        page
    };
    let counters = compare_with_layers_off(cx, unclipped, &wheel_history(11, 40), "unclipped");
    assert!(counters["layer_bypasses.ineligible"] >= 30, "{counters:?}");
    assert_eq!(counters["list_rows_kept"], 0, "{counters:?}");
    let window = open(cx, unclipped(), false);
    for _ in 0..8 {
        wheel_by(cx, window, 0., -30.);
    }
    assert!(!list_layer_kept(cx, window), "the list has no layer");
}
