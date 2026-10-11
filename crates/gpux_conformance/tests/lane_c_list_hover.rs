//! Hover in a list shown from its scroll layer, in lane C: a list whose rows look different
//! under the pointer shows, in every frame, the hover a window without scroll layers shows.
//!
//! Each test opens the same list in two windows of one application, as
//! `lane_c_scroll_layers` does: one with every fast-mode mechanism on, one with all of them
//! off and the device-pixel snapping forced on. Both get the same pointer moves and wheel
//! turns, one frame at a time, and every frame is read back from both and compared byte for
//! byte.
//!
//! ```text
//! cargo test -p gpux-conformance --test lane_c_list_hover
//! ```

use gpux::{
    AnyWindowHandle, BoxShadow, Context, FastMode, FastStats, IntoElement, ListAlignment,
    ListState, Modifiers, ParentElement, Render, ScrollDelta, ScrollWheelEvent, Styled,
    TestAppContext, TestDispatcher, TestWindowOptions, TouchPhase, VisualTestContext, Window, div,
    list, point, prelude::*, px, rgb, rgba, size, uniform_list,
};
use gpux_conformance::test_host::lane_c_renderer;
use image::RgbaImage;

const WINDOW: (f32, f32) = (420., 360.);
const ROWS: usize = 60;

/// A panel around a list of rows that turn amber under the pointer, each holding a shadowed
/// card.
struct Page {
    /// The state of the `list`, when the list is one; a `uniform_list` otherwise.
    list: Option<ListState>,
}

fn row(index: usize) -> gpux::AnyElement {
    div()
        .id(("row", index))
        .hover(|style| style.bg(rgb(0xffd9a0)))
        .h(px(74.))
        .py(px(10.))
        .bg(if index.is_multiple_of(2) {
            rgb(0xe9e4d8)
        } else {
            rgb(0xd5dde6)
        })
        .child(
            div()
                .mx_3()
                .h(px(54.))
                .bg(rgb(0xffffff))
                .shadow(vec![BoxShadow {
                    color: rgba(0x00000080).into(),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(16.),
                    spread_radius: px(0.),
                    inset: false,
                }]),
        )
        .into_any_element()
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = div().size_full().bg(rgb(0x1e2430)).p(px(16.));
        match &self.list {
            Some(state) => panel.child(
                list(state.clone(), |index, _, _| row(index))
                    .size_full()
                    .bg(rgb(0xf4f1ea)),
            ),
            None => panel.child(
                uniform_list(
                    "scroll",
                    ROWS,
                    cx.processor(|_, range: std::ops::Range<usize>, _, _| {
                        range.map(row).collect::<Vec<_>>()
                    }),
                )
                .size_full()
                .bg(rgb(0xf4f1ea)),
            ),
        }
    }
}

/// One of the two windows a test compares.
struct Run {
    window: AnyWindowHandle,
}

fn app() -> TestAppContext {
    let mut cx = TestAppContext::build_with_renderer(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
        lane_c_renderer,
    );
    // A frame is drawn when the test reads it back, one a step.
    cx.set_draw_on_flush(false);
    cx
}

impl Run {
    fn open(cx: &mut TestAppContext, uniform: bool, scale: f32, layered: bool) -> Run {
        let handle = cx.add_window_with_options(
            TestWindowOptions::new(size(px(WINDOW.0), px(WINDOW.1))).scale_factor(scale),
            move |_, _| Page {
                list: (!uniform).then(|| ListState::new(ROWS, ListAlignment::Top, px(0.))),
            },
        );
        let window: AnyWindowHandle = handle.into();
        cx.update_window(window, |_, window, _| {
            if layered {
                window.set_fast_mode(FastMode::ALL);
            } else {
                window.set_fast_mode(FastMode::OFF);
                window.force_layer_snapping(true);
            }
            window.activate_window();
        })
        .expect("the window is open");
        cx.run_until_parked();
        Run { window }
    }

    fn frame(&self, cx: &mut TestAppContext) -> (RgbaImage, FastStats) {
        let image = VisualTestContext::from_window(self.window, cx)
            .capture_screenshot()
            .expect("the window rendered");
        let stats = cx
            .update_window(self.window, |_, window, _| window.fast_frame_stats())
            .expect("the window is open");
        (image, stats)
    }
}

/// Something done to both windows between frames.
#[derive(Clone, Copy, Debug)]
enum Act {
    /// The wheel turned `dy` with the pointer at (`x`, `y`).
    Wheel(f32, f32, f32),
    /// The pointer moved to (`x`, `y`).
    Move(f32, f32),
}

/// Runs `acts` in a layered window and in the reference, comparing every frame, and returns
/// how many frames the layered window composited from its layer.
fn compare(uniform: bool, scale: f32, acts: &[Act]) -> u64 {
    let mut cx = app();
    let layered = Run::open(&mut cx, uniform, scale, true);
    let reference = Run::open(&mut cx, uniform, scale, false);
    let mut composited = 0;
    for (index, act) in acts.iter().enumerate() {
        for run in [&layered, &reference] {
            let mut cx = VisualTestContext::from_window(run.window, &cx);
            match *act {
                Act::Wheel(x, y, dy) => cx.simulate_event(ScrollWheelEvent {
                    position: point(px(x), px(y)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                }),
                Act::Move(x, y) => {
                    cx.simulate_mouse_move(point(px(x), px(y)), None, Modifiers::default())
                }
            }
        }
        let (actual, stats) = layered.frame(&mut cx);
        let (expected, _) = reference.frame(&mut cx);
        composited += stats.layers_composited;
        assert_same(&actual, &expected, index, act, scale, &stats);
    }
    cx.quit();
    composited
}

#[track_caller]
fn assert_same(
    actual: &RgbaImage,
    expected: &RgbaImage,
    index: usize,
    act: &Act,
    scale: f32,
    stats: &FastStats,
) {
    assert_eq!(actual.dimensions(), expected.dimensions());
    let differing: Vec<(u32, u32)> = actual
        .enumerate_pixels()
        .filter(|(x, y, pixel)| *pixel != expected.get_pixel(*x, *y))
        .map(|(x, y, _)| (x, y))
        .collect();
    let Some(&(x, y)) = differing.first() else {
        return;
    };
    let (max_x, max_y) = differing
        .iter()
        .fold((0, 0), |(mx, my), (x, y)| (mx.max(*x), my.max(*y)));
    panic!(
        "at {scale}x, after act {index} ({act:?}): {} pixels differ from the reference, within \
         ({x}, {y})..=({max_x}, {max_y}); first {:?} against {:?}; the layered frame composited \
         {} layers and repainted {}",
        differing.len(),
        actual.get_pixel(x, y),
        expected.get_pixel(x, y),
        stats.layers_composited,
        stats.layers_repainted,
    );
}

/// Sixty moves of the pointer over the list, between small turns of the wheel both ways with
/// the pointer somewhere else: the row the pointer leaves and the row it comes to.
fn moves_between_scrolls() -> Vec<Act> {
    let mut acts = Vec::new();
    for round in 0..60 {
        acts.push(Act::Move(
            60. + (round * 37 % 300) as f32,
            30. + (round * 53 % 300) as f32,
        ));
        let count = 1 + round % 7;
        let dy = if round % 3 == 0 { 2. } else { -3. };
        acts.extend((0..count).map(|_| Act::Wheel(100., 100., dy)));
    }
    acts
}

/// Moves alone, row to row and within a row, and off the list and back.
fn moves_alone() -> Vec<Act> {
    let mut acts = vec![Act::Wheel(200., 180., -40.), Act::Wheel(200., 180., -3.)];
    for round in 0..80 {
        acts.push(match round % 9 {
            8 => Act::Move(5., 5.),
            _ => Act::Move(
                40. + (round * 29 % 340) as f32,
                24. + (round * 71 % 310) as f32,
            ),
        });
    }
    acts
}

#[test]
fn a_uniform_list_shows_each_frames_hover() {
    for scale in [1., 2.] {
        let composited = compare(true, scale, &moves_between_scrolls());
        assert!(composited >= 100, "{composited} frames came from the layer");
        compare(true, scale, &moves_alone());
    }
}

#[test]
fn a_list_shows_each_frames_hover() {
    for scale in [1., 2.] {
        let composited = compare(false, scale, &moves_between_scrolls());
        assert!(composited >= 20, "{composited} frames came from the layer");
        compare(false, scale, &moves_alone());
    }
}
