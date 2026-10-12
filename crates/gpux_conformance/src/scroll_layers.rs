//! Scroll layers in real windows: the pages the browser and mobile hosts scroll, and what their
//! checks read back. The scenes are not in [`crate::CATALOG`]: lane A and C windows have their
//! own scroll-layer proof (`lane_c_scroll_layers`), and a page here only means something where a
//! host turns scroll layers on and reads the screen.
//!
//! A host opens a page twice, with [`FastMode::ALL`] and with [`FastMode::OFF`], scrolls both
//! the same way ([`PAN`], whole device pixels at any scale a device has) and compares what the
//! two show ([`same_pixels`]); the first run's counters ([`Counters`]) say the frames came from
//! tiles.

use crate::scene::{Lane, SceneSpec};
use anyhow::{Result, ensure};
use gpux::prelude::FluentBuilder as _;
use gpux::{
    AnyElement, AnyWindowHandle, AppContext as _, AsyncApp, BoxShadow, Context, Entity,
    IntoElement, ListAlignment, ListState, ParentElement as _, PlatformInput, Render, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, Styled as _, TouchEvent, TouchId, TouchPhase, Window, div,
    list, point, prelude::*, px, rgb, rgba,
};
use image::RgbaImage;

/// The bytes of tiles a window on a phone may keep: 32 MiB, half the desktop's.
pub const MOBILE_TILE_BUDGET: u64 = 32 << 20;

/// The logical size of the pages, in the top-left corner of a host's window.
pub const PAGE: (f32, f32) = (320., 520.);

/// Where the scroll container of a page sits and how large it is, in logical pixels.
pub const SCROLLER: (f32, f32, f32, f32) = (10., 90., 300., 400.);

/// Wheel steps, in logical pixels, that a host applies to a page, a frame each. Every step is a
/// multiple of 8: at any scale factor a device has (a multiple of 1/8) a multiple of 8 logical
/// pixels is whole device pixels, so a window that snaps scroll offsets to device pixels, as
/// one with layers does, and one that does not draw the same pixels.
pub const PAN: [f32; 16] = [
    -40., -40., -24., -40., -56., -40., -16., -40., 24., 40., -40., -40., -72., -40., 8., -40.,
];

/// The logical position where the page's scroll container is panned.
pub fn pan_position() -> (f32, f32) {
    (SCROLLER.0 + SCROLLER.2 / 2., SCROLLER.1 + SCROLLER.3 / 2.)
}

const CARDS: usize = 120;

/// A card in a scroll container: alternating row colors, a border, rounded corners and, with
/// `shadow`, a soft shadow, so tiles hold what a page of cards holds. The mobile pages go
/// without the shadow: an emulator draws every frame in software, and a blur over a whole
/// screen of them is what a check of lifecycles and counters does not need (lane C scrolls
/// shadows).
pub fn card(index: usize, shadow: bool) -> AnyElement {
    div()
        .id(("card", index))
        .h(px(64.))
        .py(px(8.))
        .bg(rgb(if index.is_multiple_of(2) {
            0xe9e4d8
        } else {
            0xd5dde6
        }))
        .child(
            div()
                .mx_3()
                .h(px(48.))
                .px_2()
                .bg(rgb(0xffffff))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(0x555555))
                .when(shadow, |card| {
                    card.shadow(vec![BoxShadow {
                        color: rgba(0x00000066).into(),
                        offset: point(px(0.), px(2.)),
                        blur_radius: px(6.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                })
                .child(format!("Row {index}")),
        )
        .into_any_element()
}

/// [`card`] with its shadow, as the browser pages show it.
pub fn card_row(index: usize) -> AnyElement {
    card(index, true)
}

/// The scroll positions of a page, in a view of their own: a view that reads an offset is built
/// again with every scroll, and the scroll containers it held would be built with it.
pub struct ScrollStatus {
    scroll: ScrollHandle,
    list: Option<ListState>,
}

impl ScrollStatus {
    /// A status line for a `div` scrolled by `scroll`, and a `list` when there is one.
    pub fn new(scroll: ScrollHandle, list: Option<ListState>) -> Self {
        Self { scroll, list }
    }
}

impl Render for ScrollStatus {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let offset = -f32::from(self.scroll.offset().y);
        let offset = if offset == 0. { 0. } else { offset };
        crate::scenes::common::status(match &self.list {
            Some(list) => format!("div {offset:.0} list {}", list.logical_scroll_top().item_ix),
            None => format!("div {offset:.0}"),
        })
    }
}

/// A phone-sized page holding one scroll container: a scrolling `div` of cards, or a `list`.
struct Page {
    scroll: ScrollHandle,
    list: Option<ListState>,
    status: Entity<ScrollStatus>,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (x, y, width, height) = SCROLLER;
        let container = div()
            .id("scroller")
            .absolute()
            .top(px(y))
            .left(px(x))
            .w(px(width))
            .h(px(height))
            .bg(rgb(0xf4f1ea));
        crate::scenes::common::root()
            .child(self.status.clone())
            .child(match &self.list {
                Some(list) => container
                    .child(list_of_cards(list.clone()))
                    .into_any_element(),
                None => container
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children((0..CARDS).map(|index| card(index, false)))
                    .into_any_element(),
            })
    }
}

fn list_of_cards(state: ListState) -> impl IntoElement {
    list(state, |index, _, _| card(index, false)).size_full()
}

fn build(with_list: bool, cx: &mut gpux::App) -> gpux::AnyView {
    let scroll = ScrollHandle::new();
    let list = with_list.then(|| ListState::new(CARDS, ListAlignment::Top, px(64.)).measure_all());
    let status = cx.new(|_| ScrollStatus::new(scroll.clone(), list.clone()));
    cx.new(|_| Page {
        scroll,
        list,
        status,
    })
    .into()
}

/// A scrolling `div` of cards.
pub static DIV: SceneSpec = SceneSpec {
    id: "scroll-layers-div",
    areas: &[],
    size: PAGE,
    scales: &[1.],
    lanes: &[Lane::E, Lane::F],
    golden: false,
    build: |_, cx| build(false, cx),
    script: Vec::new,
};

/// A `list` of cards.
pub static LIST: SceneSpec = SceneSpec {
    id: "scroll-layers-list",
    areas: &[],
    size: PAGE,
    scales: &[1.],
    lanes: &[Lane::E, Lane::F],
    golden: false,
    build: |_, cx| build(true, cx),
    script: Vec::new,
};

/// The counters a scroll-layers check reads from a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// Scroll-container frames drawn from tiles.
    pub composited: u64,
    /// Scroll-container frames whose content was painted into its layer.
    pub repainted: u64,
    /// Tiles the core asked the renderer to rasterize.
    pub tiles_rasterized: u64,
    /// Tiles composited into frames.
    pub tiles_composited: u64,
    /// What the renderer holds, has rasterized and has released, when it says.
    pub renderer: Option<Renderer>,
    /// Frames the window drew.
    pub frames: u64,
    /// The bytes of tiles a window may keep ([`gpux::LAYER_TILE_BUDGET_BYTES`]).
    pub budget: u64,
    /// Scroll-container frames drawn on the ordinary path, by reason (inactive, not promoted,
    /// demoted, ineligible, nested, moved), for reports.
    pub bypasses: [u64; 6],
}

/// What a renderer did with tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Renderer {
    /// Tiles held now.
    pub held: u64,
    /// Tiles rasterized since the renderer was created.
    pub rasterized: u64,
    /// Times the tiles were released.
    pub released: u64,
}

impl std::fmt::Display for Counters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} frames composited, {} repainted, {} tiles rasterized, {} composited of {} frames; bypassed {:?}; budget {} MiB",
            self.composited,
            self.repainted,
            self.tiles_rasterized,
            self.tiles_composited,
            self.frames,
            self.bypasses,
            self.budget >> 20
        )?;
        if let Some(renderer) = self.renderer {
            write!(
                f,
                "; the renderer holds {} tiles, rasterized {} and released {} times",
                renderer.held, renderer.rasterized, renderer.released
            )?;
        }
        Ok(())
    }
}

/// The window's counters since it opened.
pub fn counters(cx: &mut AsyncApp, window: AnyWindowHandle) -> Result<Counters> {
    window.update(cx, |_, window, _| {
        let stats = window.fast_stats();
        let renderer = window.layer_tile_counters().map(|tiles| Renderer {
            held: tiles.held,
            rasterized: tiles.rasterized,
            released: tiles.released,
        });
        Counters {
            composited: stats.layers_composited,
            repainted: stats.layers_repainted,
            tiles_rasterized: stats.tiles_rasterized,
            tiles_composited: stats.tiles_composited,
            renderer,
            frames: stats.frames,
            budget: gpux::LAYER_TILE_BUDGET_BYTES,
            bypasses: stats.layer_bypasses,
        }
    })
}

/// Waits for the window's next frame, or `limit` when none is coming. Unlike
/// [`crate::app_host::wait_frames`] it does not refresh the window, which draws the frame from
/// scratch, with no layer: it waits for the frame whatever drew it last (a scroll, a promotion).
pub async fn next_frame(cx: &mut AsyncApp, window: AnyWindowHandle, limit: std::time::Duration) {
    let drawn = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = drawn.clone();
    if window
        .update(cx, |_, window, _| {
            window.on_next_frame(move |_, _| flag.set(true))
        })
        .is_err()
    {
        return;
    }
    let executor = cx.background_executor().clone();
    let start = executor.now();
    while !drawn.get() && executor.now().duration_since(start) < limit {
        executor.timer(std::time::Duration::from_millis(4)).await;
    }
}

/// Sets the window's fast mode: every mechanism on, or all off.
pub fn set_layers(cx: &mut AsyncApp, window: AnyWindowHandle, on: bool) -> Result<()> {
    window.update(cx, |_, window, _| {
        window.set_fast_mode(if on {
            gpux::FastMode::ALL
        } else {
            gpux::FastMode::OFF
        })
    })
}

/// Turns the wheel by `dy` logical pixels (negative scrolls down) at the logical `position`.
pub fn wheel(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    position: (f32, f32),
    dy: f32,
) -> Result<()> {
    window.update(cx, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(position.0), px(position.1)),
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    })
}

/// A touch that goes down at the logical `from`, moves by `dy` over `steps` frames of 16 ms and
/// lifts: a fast pan leaves fling momentum, which scrolls on over the frames after it.
pub async fn touch_pan(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    from: (f32, f32),
    dy: f32,
    steps: u32,
) -> Result<()> {
    const FRAME: std::time::Duration = std::time::Duration::from_millis(16);
    let touch = |cx: &mut AsyncApp, phase: TouchPhase, y: f32| {
        window.update(cx, |_, window, cx| {
            window.dispatch_event(
                PlatformInput::Touch(TouchEvent {
                    id: TouchId(1),
                    phase,
                    position: point(px(from.0), px(y)),
                    predicted_position: None,
                    force: None,
                    timestamp: None,
                }),
                cx,
            );
        })
    };
    touch(cx, TouchPhase::Started, from.1)?;
    for step in 1..=steps.max(1) {
        cx.background_executor().timer(FRAME).await;
        touch(
            cx,
            TouchPhase::Moved,
            from.1 + dy * step as f32 / steps.max(1) as f32,
        )?;
    }
    cx.background_executor().timer(FRAME).await;
    touch(cx, TouchPhase::Ended, from.1 + dy)
}

/// The physical pixels of the page's scroll container in a screen capture of a window at the
/// screen's origin rendered at `scale`: the part of the screen no system bar draws over.
pub fn scroller_pixels(scale: f32) -> (u32, u32, u32, u32) {
    let (x, y, width, height) = SCROLLER;
    (
        (x * scale).round() as u32,
        (y * scale).round() as u32,
        (width * scale).round() as u32,
        (height * scale).round() as u32,
    )
}

/// Whether the rectangle `(x, y, width, height)` of two frames is the same, to the byte.
pub fn same_pixels(
    first: &RgbaImage,
    second: &RgbaImage,
    (x, y, width, height): (u32, u32, u32, u32),
) -> Result<()> {
    ensure!(
        first.dimensions() == second.dimensions(),
        "the frames are {:?} and {:?} pixels",
        first.dimensions(),
        second.dimensions()
    );
    let (frame_width, frame_height) = first.dimensions();
    ensure!(
        x + width <= frame_width && y + height <= frame_height,
        "the scroller lies outside a {frame_width}x{frame_height} frame"
    );
    let mut differing = 0usize;
    let mut first_at = None;
    for row in y..y + height {
        for column in x..x + width {
            if first.get_pixel(column, row) != second.get_pixel(column, row) {
                differing += 1;
                first_at.get_or_insert((column, row));
            }
        }
    }
    if let Some((column, row)) = first_at {
        anyhow::bail!(
            "{differing} of {} pixels differ, the first at ({column}, {row}): {:?} with layers, {:?} without",
            width * height,
            first.get_pixel(column, row).0,
            second.get_pixel(column, row).0,
        );
    }
    Ok(())
}
