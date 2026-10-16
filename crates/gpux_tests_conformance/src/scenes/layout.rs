//! Layout: flex, grid, absolute, overflow clipping, fixed and sticky positioning.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyElement, AnyView, App, Context, IntoElement, Render, ScrollHandle, Window, div, px, rgb,
};

const ALL_LANES: &[Lane] = &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F];

fn flex() -> AnyElement {
    root()
        .flex()
        .flex_row()
        .gap(px(10.))
        .p(px(10.))
        .child(div().id("a").w(px(50.)).h(px(40.)).bg(rgb(RED_HEX)))
        .child(div().id("b").flex_1().h(px(40.)).bg(rgb(GREEN_HEX)))
        .child(div().id("c").w(px(60.)).h(px(40.)).bg(rgb(BLUE_HEX)))
        .into_any_element()
}

pub const FLEX: SceneSpec = SceneSpec {
    id: "layout-flex",
    areas: &[Area::LayoutFlex],
    size: (300., 120.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, flex),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(id("a"), (10., 10., 50., 40.), 0.01)),
            Step::Expect(Expect::Bounds(id("b"), (70., 10., 150., 40.), 0.01)),
            Step::Expect(Expect::Bounds(id("c"), (230., 10., 60., 40.), 0.01)),
            Step::probe(35., 30., RED),
            Step::probe(145., 30., GREEN),
            Step::probe(260., 30., BLUE),
            Step::probe(5., 5., WHITE),
            Step::Golden,
        ]
    },
};

fn grid() -> AnyElement {
    let colors = [RED_HEX, GREEN_HEX, BLUE_HEX, GREEN_HEX, BLUE_HEX, RED_HEX];
    root()
        .grid()
        .grid_cols(3)
        .gap(px(8.))
        .p(px(8.))
        .children(colors.iter().enumerate().map(|(index, color)| {
            div()
                .id(gpux::SharedString::from(format!("cell-{index}")))
                .h(px(40.))
                .bg(rgb(*color))
        }))
        .into_any_element()
}

pub const GRID: SceneSpec = SceneSpec {
    id: "layout-grid",
    areas: &[Area::LayoutGrid],
    size: (242., 120.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, grid),
    script: || {
        // Three columns share 210 px evenly, so every scale lays them out on whole pixels; the
        // two rows stretch to fill the 104 px of height (48 px each).
        vec![
            Step::Expect(Expect::Bounds(id("cell-0"), (8., 8., 70., 40.), 0.01)),
            Step::Expect(Expect::Bounds(id("cell-4"), (86., 64., 70., 40.), 0.01)),
            Step::Expect(Expect::Bounds(id("cell-5"), (164., 64., 70., 40.), 0.01)),
            Step::probe(40., 28., RED),
            Step::probe(120., 84., BLUE),
            Step::probe(200., 84., RED),
        ]
    },
};

fn absolute() -> AnyElement {
    root()
        .child(
            div()
                .id("box")
                .absolute()
                .top(px(20.))
                .left(px(30.))
                .w(px(50.))
                .h(px(40.))
                .bg(rgb(RED_HEX)),
        )
        .child(
            div()
                .id("corner")
                .absolute()
                .right(px(10.))
                .bottom(px(10.))
                .w(px(40.))
                .h(px(30.))
                .bg(rgb(BLUE_HEX)),
        )
        .into_any_element()
}

pub const ABSOLUTE: SceneSpec = SceneSpec {
    id: "layout-absolute",
    areas: &[Area::LayoutAbsolute],
    size: (200., 150.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, absolute),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(id("box"), (30., 20., 50., 40.), 0.01)),
            Step::Expect(Expect::Bounds(id("corner"), (150., 110., 40., 30.), 0.01)),
            Step::probe(55., 40., RED),
            Step::probe(170., 125., BLUE),
            Step::probe(100., 80., WHITE),
        ]
    },
};

fn overflow() -> AnyElement {
    root()
        .child(
            div()
                .id("clip")
                .absolute()
                .top(px(20.))
                .left(px(20.))
                .w(px(100.))
                .h(px(60.))
                .overflow_hidden()
                .bg(rgb(GRAY_HEX))
                .child(div().id("content").w(px(160.)).h(px(100.)).bg(rgb(RED_HEX))),
        )
        .into_any_element()
}

pub const OVERFLOW: SceneSpec = SceneSpec {
    id: "layout-overflow",
    areas: &[Area::LayoutOverflow, Area::Clipping],
    size: (200., 150.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, overflow),
    script: || {
        vec![
            // Layout keeps the child's own size; only painting is clipped.
            Step::Expect(Expect::Bounds(id("content"), (20., 20., 160., 100.), 0.01)),
            Step::probe(60., 50., RED),
            Step::probe(119., 79., RED),
            Step::probe(121., 50., WHITE),
            Step::probe(60., 81., WHITE),
        ]
    },
};

/// A scroll container holding a fixed badge, a sticky header handoff, and ordinary rows.
struct Positioned {
    scroll: ScrollHandle,
}

impl Render for Positioned {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let offset = -f32::from(self.scroll.offset().y) + 0.;
        root().child(status(format!("offset {offset:.0}"))).child(
            div()
                .id("scroller")
                .absolute()
                .top(px(30.))
                .left(px(20.))
                .w(px(200.))
                .h(px(150.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children(["first", "second"].into_iter().map(|section| {
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id(gpux::SharedString::from(format!("header-{section}")))
                                .sticky()
                                .top(px(0.))
                                .h(px(30.))
                                .bg(rgb(GREEN_HEX)),
                        )
                        .children((0..5).map(move |row| {
                            div()
                                .id(gpux::SharedString::from(format!("{section}-{row}")))
                                .h(px(40.))
                                .bg(rgb(GRAY_HEX))
                        }))
                }))
                // Fixed and sticky divs share the positioned paint lane, where later elements paint above
                // earlier ones: last, the badge paints above the sticky headers.
                .child(
                    div()
                        .id("pinned")
                        .fixed()
                        .top(px(10.))
                        .right(px(10.))
                        .w(px(40.))
                        .h(px(20.))
                        .bg(rgb(BLUE_HEX)),
                ),
        )
    }
}

fn positioned(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|_| Positioned {
        scroll: ScrollHandle::new(),
    })
    .into()
}

pub const FIXED_STICKY: SceneSpec = SceneSpec {
    id: "layout-fixed-sticky",
    areas: &[Area::LayoutFixed, Area::LayoutSticky, Area::StickyStacking],
    size: (240., 200.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: positioned,
    script: || {
        let scroller = id("scroller");
        vec![
            // The fixed badge resolves against the scrollport, its containing content mask.
            Step::Expect(Expect::Bounds(id("pinned"), (170., 40., 40., 20.), 0.01)),
            Step::Expect(Expect::Bounds(
                id("header-first"),
                (20., 30., 200., 30.),
                0.01,
            )),
            Step::Wheel(scroller.clone(), 0., -100.),
            Step::Expect(Expect::Exists(status_is("offset 100"))),
            // The badge stays put and the first header sticks to the top.
            Step::Expect(Expect::Bounds(id("pinned"), (170., 40., 40., 20.), 0.01)),
            Step::Expect(Expect::Bounds(
                id("header-first"),
                (20., 30., 200., 30.),
                0.01,
            )),
            Step::Expect(Expect::Bounds(id("first-3"), (20., 80., 200., 40.), 0.01)),
            // The second header (at content y 230) pushes the first out as it arrives.
            Step::Wheel(scroller.clone(), 0., -115.),
            Step::Expect(Expect::Exists(status_is("offset 215"))),
            Step::Expect(Expect::Bounds(
                id("header-second"),
                (20., 45., 200., 30.),
                0.01,
            )),
            Step::Expect(Expect::Bounds(
                id("header-first"),
                (20., 15., 200., 30.),
                0.01,
            )),
            Step::Wheel(scroller, 0., -50.),
            Step::Expect(Expect::Bounds(
                id("header-second"),
                (20., 30., 200., 30.),
                0.01,
            )),
            Step::probe(190., 50., BLUE),
            Step::probe(30., 40., GREEN),
        ]
    },
};
