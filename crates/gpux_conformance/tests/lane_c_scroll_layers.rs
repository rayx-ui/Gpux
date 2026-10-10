//! Scroll layers in lane C: a window that scrolls its content from GPU tiles shows, frame for
//! frame and byte for byte, what a window without scroll layers shows.
//!
//! Each test opens the same view in two windows of one application, on the platform's headless
//! renderer (the software adapter on Linux and Windows unless `GPUX_WGPU_ADAPTER` picks another,
//! Metal on macOS): one with every fast-mode mechanism on, and one with all of them off and the
//! device-pixel snapping the scroll-layers switch brings forced on, which is the frame scroll
//! layers are held to. Both get the same input, one frame at a time, and every frame is read
//! back from both.
//!
//! A test also checks that the layered window did composite tiles, so that a renderer without
//! tiles, which keeps scroll layers off, fails it rather than passing by drawing as before.
//!
//! ```text
//! cargo test -p gpux-conformance --test lane_c_scroll_layers
//! ```

use gpux::{
    AnyWindowHandle, Context, Entity, FastMode, FastStats, IntoElement, Modifiers, ParentElement,
    PathBuilder, Render, ScrollDelta, ScrollWheelEvent, SharedString, Styled, TestAppContext,
    TestDispatcher, TestWindowOptions, TouchPhase, VisualTestContext, Window, canvas, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgb, rgba, size,
};
use gpux_conformance::test_host::lane_c_renderer;
use image::RgbaImage;

const WINDOW: (f32, f32) = (420., 360.);

/// The space between two cards. A shadow whose box has scrolled out of the viewport while its
/// blur still shows is ordered by the edge of the viewport it left (see
/// `gpux::fast::layers::scene::shown_bounds`); a neighbour whose own shadow box is at that edge
/// at the same time is then drawn after it, where a tile, painted with both boxes in view, draws
/// the two in the order they share. Where the two blurs overlap that rounds a pixel one level
/// apart. It takes boxes closer than a blur reaches, three blur radii: these are further.
const CARD_GAP: f32 = 24.;

/// What the scrolled content is made of.
#[derive(Clone, Copy)]
enum Content {
    /// Rows of text on alternating backgrounds, with borders.
    Text,
    /// Rounded cards with shadows, gradients and borders.
    Cards,
    /// Rows that each draw a path, the last thing painted where it lies.
    Paths,
    /// Rows wider than the viewport: the container scrolls on both axes.
    Wide,
}

/// A panel around a scroll container over an opaque background, which a layer's tiles bake.
struct Page {
    content: Content,
    /// Changes what a few rows show, for the tests that change content while it is scrolled.
    tick: usize,
}

impl Page {
    fn row(&self, index: usize) -> gpux::AnyElement {
        let marked = self.tick > 0 && index % 7 == self.tick % 7;
        match self.content {
            Content::Text | Content::Wide => div()
                .h(px(27.))
                .when(matches!(self.content, Content::Wide), |row| {
                    row.w(px(900.)).flex_none()
                })
                .px_3()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(rgb(0xc9c2b4))
                .bg(if marked {
                    rgb(0xffd9a0)
                } else if index.is_multiple_of(2) {
                    rgb(0xf4f1ea)
                } else {
                    rgb(0xe9e4d8)
                })
                .text_color(rgb(0x1e2430))
                .text_size(px(13.))
                .child(SharedString::from(format!(
                    "Row {index:03} · sphinx of black quartz, judge my vow · {}",
                    self.tick
                )))
                .into_any_element(),
            // Their shadows are a blur's reach apart (see `CARD_GAP`).
            Content::Cards => div()
                .mx_3()
                .mt(px(CARD_GAP))
                .h(px(74.))
                .rounded(px(10.))
                .border_1()
                .border_color(rgba(0x1e243080))
                .shadow_md()
                .bg(linear_gradient(
                    (index * 37 % 360) as f32,
                    [
                        linear_color_stop(if marked { rgb(0x5a2a2a) } else { rgb(0x2a3a5a) }, 0.),
                        linear_color_stop(rgb(0x1b2233), 1.),
                    ],
                ))
                .p_3()
                .text_color(rgb(0xf4f1ea))
                .text_size(px(15.))
                .child(SharedString::from(format!("Card {index:02}")))
                .into_any_element(),
            Content::Paths => div()
                .h(px(48.))
                .border_b_1()
                .border_color(rgb(0xc9c2b4))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let left = bounds.origin.x + px(16. + (index % 5) as f32 * 9.5);
                            let top = bounds.origin.y + px(8.);
                            let mut builder = PathBuilder::fill();
                            builder.move_to(point(left, top + px(30.)));
                            builder.line_to(point(left + px(60.), top));
                            builder.curve_to(
                                point(left + px(180.), top + px(30.)),
                                point(left + px(120.), top - px(4.)),
                            );
                            builder.close();
                            if let Ok(path) = builder.build() {
                                window.paint_path(
                                    path,
                                    if marked { rgb(0xb5432f) } else { rgb(0x2f6fb5) },
                                );
                            }
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
        }
    }
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let rows = match self.content {
            Content::Cards => 40,
            _ => 120,
        };
        div().size_full().bg(rgb(0x1e2430)).p_4().child(
            div()
                .id("scroll")
                .size_full()
                .bg(rgb(0xf4f1ea))
                .when(matches!(self.content, Content::Wide), |container| {
                    container.overflow_scroll()
                })
                .when(!matches!(self.content, Content::Wide), |container| {
                    container.overflow_y_scroll()
                })
                .children((0..rows).map(|index| self.row(index))),
        )
    }
}

/// One of the two windows a test compares.
#[derive(Clone)]
struct Run {
    window: AnyWindowHandle,
    page: Entity<Page>,
}

/// The application both windows of a test live in, as two windows of a program do.
fn app() -> TestAppContext {
    let mut cx = TestAppContext::build_with_renderer(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
        lane_c_renderer,
    );
    // A frame is drawn when the test reads it back, one a step, so that the renderer is handed
    // every frame.
    cx.set_draw_on_flush(false);
    cx
}

impl Run {
    /// Opens `content` at `scale`: with every mechanism on when `layered`, as the reference
    /// scroll layers are held to otherwise.
    fn open(cx: &mut TestAppContext, content: Content, scale: f32, layered: bool) -> Run {
        let handle = cx.add_window_with_options(
            TestWindowOptions::new(size(px(WINDOW.0), px(WINDOW.1))).scale_factor(scale),
            move |_, _| Page { content, tick: 0 },
        );
        let page = handle.root(cx).expect("the window's page");
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
        Run { window, page }
    }

    fn wheel(&self, cx: &TestAppContext, dx: f32, dy: f32) {
        VisualTestContext::from_window(self.window, cx).simulate_event(ScrollWheelEvent {
            position: point(px(WINDOW.0 / 2.), px(WINDOW.1 / 2.)),
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
    }

    fn tick(&self, cx: &mut TestAppContext) {
        self.page.update(cx, |page, cx| {
            page.tick += 1;
            cx.notify();
        });
    }

    /// Draws the frame the last step asked for and reads it back, with its counters.
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

/// A step of a test's script.
#[derive(Clone, Copy)]
enum Step {
    Wheel(f32, f32),
    /// Changes what some rows show.
    Tick,
}

/// What the layered window did over a script.
#[derive(Default)]
struct Totals {
    composited: u64,
    repainted: u64,
    tiles_composited: u64,
    tiles_rasterized: u64,
}

/// Runs `script` in a layered window and in the reference, comparing every frame, and returns
/// what the layered window's layers did.
fn compare(content: Content, scale: f32, script: &[Step]) -> Totals {
    let mut cx = app();
    let layered = Run::open(&mut cx, content, scale, true);
    let reference = Run::open(&mut cx, content, scale, false);
    let mut totals = Totals::default();
    let (first, _) = layered.frame(&mut cx);
    let (expected, _) = reference.frame(&mut cx);
    assert_same(&first, &expected, 0, scale);
    for (index, step) in script.iter().enumerate() {
        for run in [&layered, &reference] {
            match *step {
                Step::Wheel(dx, dy) => run.wheel(&cx, dx, dy),
                Step::Tick => run.tick(&mut cx),
            }
        }
        let (actual, stats) = layered.frame(&mut cx);
        let (expected, reference_stats) = reference.frame(&mut cx);
        assert_eq!(
            reference_stats.layers_composited + reference_stats.layers_repainted,
            0,
            "the reference has no layers"
        );
        assert_same(&actual, &expected, index + 1, scale);
        totals.composited += stats.layers_composited;
        totals.repainted += stats.layers_repainted;
        totals.tiles_composited += stats.tiles_composited;
        totals.tiles_rasterized += stats.tiles_rasterized;
    }
    cx.quit();
    totals
}

#[track_caller]
fn assert_same(actual: &RgbaImage, expected: &RgbaImage, step: usize, scale: f32) {
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
        "at {scale}x, after step {step}: {} pixels differ from the reference, within \
         ({x}, {y})..=({max_x}, {max_y}); first {:?} against {:?}",
        differing.len(),
        actual.get_pixel(x, y),
        expected.get_pixel(x, y),
    );
}

/// Wheel steps down the content: small ones, an odd one, a fractional one, a long one, and back.
fn scrolls() -> Vec<Step> {
    [
        -20., -20., -20., -37., -91.5, -13., -240., 55., -7.25, -600., -31., 410., -3., -3., -3.,
    ]
    .into_iter()
    .map(|dy| Step::Wheel(0., dy))
    .collect()
}

#[track_caller]
fn assert_composited(totals: &Totals, at_least: u64) {
    assert!(
        totals.composited >= at_least && totals.tiles_composited >= at_least,
        "the layered window composited {} frames ({} tiles) from its layer, repainted {} and \
         rasterized {} tiles: fewer than {at_least} frames came from tiles",
        totals.composited,
        totals.tiles_composited,
        totals.repainted,
        totals.tiles_rasterized,
    );
}

#[test]
fn text_rows_scroll_from_tiles() {
    let totals = compare(Content::Text, 1., &scrolls());
    assert_composited(&totals, 8);
}

#[test]
fn text_rows_scroll_from_tiles_at_2x() {
    let totals = compare(Content::Text, 2., &scrolls());
    assert_composited(&totals, 8);
}

/// At a fractional scale factor a logical pixel is not a whole number of device pixels: the
/// snapping the reference shares keeps every scroll on device pixels.
#[test]
fn text_rows_scroll_from_tiles_at_a_fractional_scale() {
    let totals = compare(Content::Text, 1.5, &scrolls());
    assert_composited(&totals, 8);
}

#[test]
fn cards_with_shadows_and_gradients_scroll_from_tiles() {
    for scale in [1., 2.] {
        let totals = compare(Content::Cards, scale, &scrolls());
        assert_composited(&totals, 8);
    }
}

/// A layer's paths are drawn into the frame over its tiles.
#[test]
fn paths_scroll_over_the_tiles() {
    for scale in [1., 2.] {
        let totals = compare(Content::Paths, scale, &scrolls());
        assert_composited(&totals, 8);
    }
}

#[test]
fn content_scrolls_on_both_axes_from_tiles() {
    let script: Vec<Step> = [
        (0., -20.),
        (0., -20.),
        (0., -20.),
        (-33., 0.),
        (-90.5, -41.),
        (-200., 0.),
        (17., -350.),
        (160., 120.),
        (-5., -5.),
        (-5., -5.),
    ]
    .into_iter()
    .map(|(dx, dy)| Step::Wheel(dx, dy))
    .collect();
    let totals = compare(Content::Wide, 1., &script);
    assert_composited(&totals, 5);
}

/// Content that changes while it is scrolled is painted again, and the tiles the change touched
/// are rasterized again; the frames after it come from tiles again.
#[test]
fn a_change_while_scrolled_shows_and_scrolling_goes_on_from_tiles() {
    let mut script = scrolls();
    script.insert(6, Step::Tick);
    script.extend([-20., -20., -20., -20., -20., -20.].map(|dy| Step::Wheel(0., dy)));
    for content in [Content::Text, Content::Cards] {
        let totals = compare(content, 1., &script);
        assert_composited(&totals, 10);
    }
}
