//! Scrolling: wheel, nested wheel propagation, scrollbar thumb publication, programmatic smooth
//! scroll, and touch pan with fling momentum on the executor clock.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    Context, IntoElement, MouseButton, MouseMoveEvent, Render, ScrollBehavior, ScrollHandle,
    Window, div, point, px, rgb,
};
use std::time::Duration;

const ALL_LANES: &[Lane] = &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F];

/// The scroll distance as a positive number (`0`, never `-0`).
fn offset(handle: &ScrollHandle) -> f32 {
    let offset = -f32::from(handle.offset().y);
    if offset == 0. { 0. } else { offset }
}

/// Stripes so pixel lanes can see content move.
fn stripes(count: usize) -> impl Iterator<Item = gpux::Div> {
    (0..count).map(|index| {
        div()
            .h(px(50.))
            .bg(rgb(if index % 2 == 0 { RED_HEX } else { BLUE_HEX }))
    })
}

struct Wheel {
    scroll: ScrollHandle,
}

impl Render for Wheel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(status(format!("offset {:.0}", offset(&self.scroll))))
            .child(
                div()
                    .id("scroller")
                    .absolute()
                    .top(px(30.))
                    .left(px(10.))
                    .w(px(100.))
                    .h(px(100.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(stripes(20)),
            )
    }
}

pub const WHEEL: SceneSpec = SceneSpec {
    id: "scroll-wheel",
    areas: &[Area::WheelScroll],
    size: (160., 160.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: |_, cx| {
        cx.new(|_| Wheel {
            scroll: ScrollHandle::new(),
        })
        .into()
    },
    script: || {
        vec![
            Step::probe(60., 50., RED),
            Step::Capture("top"),
            Step::Wheel(id("scroller"), 0., -50.),
            Step::Expect(Expect::Exists(status_is("offset 50"))),
            Step::probe(60., 50., BLUE),
            Step::ChangedSince("top", (10., 30., 100., 100.)),
            // Scrolling stops at the end of the content.
            Step::Wheel(id("scroller"), 0., -5000.),
            Step::Expect(Expect::Exists(status_is("offset 900"))),
        ]
    },
};

struct Nested {
    outer: ScrollHandle,
    inner: ScrollHandle,
}

impl Render for Nested {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(status(format!(
                "inner {:.0} outer {:.0}",
                offset(&self.inner),
                offset(&self.outer)
            )))
            .child(
                div()
                    .id("outer")
                    .absolute()
                    .top(px(30.))
                    .left(px(10.))
                    .w(px(160.))
                    .h(px(120.))
                    .overflow_y_scroll()
                    .track_scroll(&self.outer)
                    .child(
                        div()
                            .id("inner")
                            .w(px(120.))
                            .h(px(80.))
                            .overflow_y_scroll()
                            .track_scroll(&self.inner)
                            .children(stripes(3)),
                    )
                    .children(stripes(8)),
            )
    }
}

pub const NESTED: SceneSpec = SceneSpec {
    id: "scroll-nested",
    areas: &[Area::NestedWheel],
    size: (200., 170.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: |_, cx| {
        cx.new(|_| Nested {
            outer: ScrollHandle::new(),
            inner: ScrollHandle::new(),
        })
        .into()
    },
    script: || {
        vec![
            Step::probe(70., 50., RED),
            Step::Capture("start"),
            // The inner scrollport consumes the wheel while it can move...
            Step::Wheel(id("inner"), 0., -40.),
            Step::Expect(Expect::Exists(status_is("inner 40 outer 0"))),
            Step::ChangedSince("start", (10., 30., 120., 80.)),
            // ...stops at its edge (70 of 150 - 80), and only then does the outer one move.
            Step::Wheel(id("inner"), 0., -100.),
            Step::Expect(Expect::Exists(status_is("inner 70 outer 0"))),
            Step::Wheel(id("inner"), 0., -30.),
            Step::Expect(Expect::Exists(status_is("inner 70 outer 30"))),
        ]
    },
};

/// A scroll container with a hand-made scrollbar thumb that publishes offsets as it is dragged.
struct Scrollbar {
    scroll: ScrollHandle,
    drag_origin: Option<(f32, f32)>,
}

impl Render for Scrollbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scroll = self.scroll.clone();
        let entity = cx.entity();
        let owner = cx.entity_id();
        // Content 1000 px tall in a 100 px viewport: the thumb travels 90 px for 900 px.
        let thumb_top = offset(&self.scroll) / 10.;
        root()
            .child(status(format!("offset {:.0}", offset(&self.scroll))))
            .child(
                div()
                    .id("viewport")
                    .absolute()
                    .top(px(30.))
                    .left(px(10.))
                    .w(px(100.))
                    .h(px(100.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(stripes(20)),
            )
            .child(
                div()
                    .id("thumb")
                    .absolute()
                    .top(px(30. + thumb_top))
                    .left(px(115.))
                    .w(px(10.))
                    .h(px(10.))
                    .bg(rgb(GRAY_HEX))
                    .on_mouse_down(MouseButton::Left, {
                        let entity = entity.clone();
                        let scroll = scroll.clone();
                        move |event, _, cx| {
                            scroll.begin_scrollbar_drag();
                            let start = offset(&scroll);
                            entity.update(cx, |this, _| {
                                this.drag_origin = Some((f32::from(event.position.y), start));
                            });
                        }
                    })
                    .on_mouse_up(MouseButton::Left, {
                        let entity = entity.clone();
                        let scroll = scroll.clone();
                        move |_, _, cx| {
                            scroll.end_scrollbar_drag();
                            entity.update(cx, |this, _| this.drag_origin = None);
                        }
                    }),
            )
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                let Some((start_y, start_offset)) = entity.read(cx).drag_origin else {
                    return;
                };
                if !event.dragging() {
                    return;
                }
                let target = start_offset + (f32::from(event.position.y) - start_y) * 10.;
                scroll.publish_scrollbar_offset(point(px(0.), px(-target)), owner, cx);
            })
    }
}

pub const SCROLLBAR: SceneSpec = SceneSpec {
    id: "scroll-scrollbar",
    areas: &[Area::Scrollbars],
    size: (160., 160.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D],
    golden: false,
    build: |_, cx| {
        cx.new(|_| Scrollbar {
            scroll: ScrollHandle::new(),
            drag_origin: None,
        })
        .into()
    },
    script: || {
        vec![
            Step::probe(120., 35., GRAY),
            Step::Capture("start"),
            // Drag the thumb from its center to the viewport's, 45 px down: the published
            // offset (10 per thumb pixel) follows in the same turn.
            Step::Drag(id("thumb"), id("viewport")),
            Step::Expect(Expect::Exists(status_is("offset 450"))),
            Step::Expect(Expect::Bounds(id("thumb"), (115., 75., 10., 10.), 0.01)),
            Step::probe(120., 80., GRAY),
            Step::ChangedSince("start", (10., 30., 100., 100.)),
        ]
    },
};

/// A button that scrolls its list smoothly to the bottom.
struct Programmatic {
    scroll: ScrollHandle,
}

impl Render for Programmatic {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let scroll = self.scroll.clone();
        root()
            .child(status(format!("offset {:.0}", offset(&self.scroll))))
            .child(
                div()
                    .id("jump")
                    .absolute()
                    .top(px(0.))
                    .right(px(0.))
                    .w(px(40.))
                    .h(px(20.))
                    .bg(rgb(GREEN_HEX))
                    .on_click(move |_, window, _| {
                        scroll.scroll_to(
                            point(px(0.), px(-900.)),
                            ScrollBehavior::Smooth(Duration::from_millis(300)),
                        );
                        window.refresh();
                    }),
            )
            .child(
                div()
                    .id("scroller")
                    .absolute()
                    .top(px(30.))
                    .left(px(10.))
                    .w(px(100.))
                    .h(px(100.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(stripes(20)),
            )
    }
}

pub const PROGRAMMATIC: SceneSpec = SceneSpec {
    id: "scroll-programmatic",
    areas: &[Area::ProgrammaticScroll],
    size: (160., 160.),
    scales: &[1., 2.],
    // Exact eased offsets need the executor clock; lane D runs on the real clock.
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::E, Lane::F],
    golden: false,
    build: |_, cx| {
        cx.new(|_| Programmatic {
            scroll: ScrollHandle::new(),
        })
        .into()
    },
    script: || {
        vec![
            Step::probe(140., 10., GREEN),
            Step::Capture("start"),
            Step::Click(id("jump")),
            Step::Expect(Expect::Exists(status_is("offset 0"))),
            // Cubic ease-out: halfway through 300 ms it has covered 87.5% of 900 px.
            Step::Advance(Duration::from_millis(150)),
            Step::Expect(Expect::Exists(status_is("offset 788"))),
            // Mid-scroll the stripes have moved (at 900 they line up with the start again).
            Step::ChangedSince("start", (10., 30., 100., 100.)),
            Step::Advance(Duration::from_millis(150)),
            Step::Expect(Expect::Exists(status_is("offset 900"))),
        ]
    },
};

struct Touch {
    scroll: ScrollHandle,
}

impl Render for Touch {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(status(format!("offset {:.0}", offset(&self.scroll))))
            .child(
                div()
                    .id("scroller")
                    .absolute()
                    .top(px(30.))
                    .left(px(10.))
                    .w(px(100.))
                    .h(px(100.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(stripes(100)),
            )
    }
}

pub const TOUCH: SceneSpec = SceneSpec {
    id: "scroll-touch",
    areas: &[Area::TouchPan, Area::TouchFling],
    size: (160., 160.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::E, Lane::F],
    golden: false,
    build: |_, cx| {
        cx.new(|_| Touch {
            scroll: ScrollHandle::new(),
        })
        .into()
    },
    script: || {
        let frame = Duration::from_millis(16);
        let mut steps = vec![
            Step::probe(60., 50., RED),
            Step::Capture("start"),
            // A fast 80 px pan: the pan follows the finger, then the fling carries on.
            Step::TouchPan(id("scroller"), 0., -80., 4),
            Step::Expect(Expect::Exists(status_is("offset 80"))),
        ];
        steps.extend((0..200).map(|_| Step::Advance(frame)));
        steps.push(Step::Expect(Expect::Exists(status_is("offset 699"))));
        steps.push(Step::ChangedSince("start", (10., 30., 100., 100.)));
        steps
    },
};
