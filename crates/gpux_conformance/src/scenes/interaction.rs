//! Interaction: drag and drop, tooltips, anchored overlays, and unclipped deferred overlays.

use super::common::*;
use crate::scene::{Area, Capability, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyElement, Context, Empty, IntoElement, Render, Window, anchored, deferred, div, point, px,
    rgb, semantic_query::Query,
};
use std::time::Duration;

#[derive(Clone)]
struct Card(&'static str);

struct Preview;

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct DragDrop {
    dropped: Vec<&'static str>,
}

impl Render for DragDrop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(status(format!("dropped {}", self.dropped.join(","))))
            .children(
                ["ace", "king"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| {
                        div()
                            .id(name)
                            .absolute()
                            .top(px(30.))
                            .left(px(10. + index as f32 * 50.))
                            .w(px(40.))
                            .h(px(40.))
                            .bg(rgb(RED_HEX))
                            .on_drag(Card(name), |_, _, _, cx| cx.new(|_| Preview))
                    }),
            )
            .child(
                div()
                    .id("pile")
                    .absolute()
                    .top(px(30.))
                    .left(px(160.))
                    .w(px(60.))
                    .h(px(60.))
                    .bg(rgb(GRAY_HEX))
                    .drag_over::<Card>(|style, _, _, _| style.bg(rgb(GREEN_HEX)))
                    .on_drop(cx.listener(|this, card: &Card, _, cx| {
                        this.dropped.push(card.0);
                        cx.notify();
                    })),
            )
    }
}

pub const DRAG_DROP: SceneSpec = SceneSpec {
    id: "drag-drop",
    areas: &[Area::DragDrop],
    size: (240., 110.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D],
    golden: false,
    build: |_, cx| {
        cx.new(|_| DragDrop {
            dropped: Vec::new(),
        })
        .into()
    },
    script: || {
        vec![
            Step::probe(30., 50., RED),
            Step::probe(190., 60., GRAY),
            Step::Capture("start"),
            Step::Drag(id("ace"), id("pile")),
            Step::Expect(Expect::Exists(status_is("dropped ace"))),
            Step::ChangedSince("start", (0., 0., 160., 26.)),
            Step::Drag(id("king"), id("pile")),
            Step::Expect(Expect::Exists(status_is("dropped ace,king"))),
            // A drag released outside the pile drops nothing.
            Step::Drag(id("ace"), status_is("dropped ace,king")),
            Step::Expect(Expect::Exists(status_is("dropped ace,king"))),
        ]
    },
};

struct Tip;

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("tooltip")
            .p(px(4.))
            .bg(rgb(GRAY_HEX))
            .child("Saves the file")
    }
}

fn tooltip() -> AnyElement {
    root()
        .child(
            div()
                .id("save")
                .absolute()
                .top(px(20.))
                .left(px(20.))
                .w(px(60.))
                .h(px(24.))
                .bg(rgb(GREEN_HEX))
                .tooltip(|_, cx| cx.new(|_| Tip).into()),
        )
        .into_any_element()
}

pub const TOOLTIP: SceneSpec = SceneSpec {
    id: "tooltip",
    areas: &[Area::Tooltips],
    size: (240., 120.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D],
    golden: false,
    build: |_, cx| static_view(cx, tooltip),
    script: || {
        vec![
            Step::probe(50., 32., GREEN),
            // Captured before the hover: on the real clock a slow capture can outlast the show
            // delay. The button ends above the compared region and has no hover style.
            Step::Capture("hidden"),
            Step::Hover(id("save")),
            // Only the executor clock can hold the show delay back for this check.
            Step::When(
                Capability::ExecutorClock,
                vec![Step::Expect(Expect::Missing(Query::text("Saves the file")))],
            ),
            // The tooltip appears after its show delay on the executor clock.
            Step::Advance(Duration::from_millis(600)),
            Step::Expect(Expect::Exists(Query::text("Saves the file"))),
            Step::ChangedSince("hidden", (0., 44., 240., 76.)),
            Step::MoveTo(200., 110.),
            Step::Advance(Duration::from_millis(600)),
            Step::Expect(Expect::Missing(Query::text("Saves the file"))),
        ]
    },
};

struct Popover {
    open: bool,
}

impl Render for Popover {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(
                div()
                    .id("toggle")
                    .absolute()
                    .top(px(20.))
                    .left(px(20.))
                    .w(px(60.))
                    .h(px(24.))
                    .bg(rgb(GREEN_HEX))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open = !this.open;
                        cx.notify();
                    })),
            )
            .when(self.open, |this| {
                this.child(deferred(
                    anchored()
                        .position(point(px(20.), px(48.)))
                        .child(div().id("popover").w(px(100.)).h(px(50.)).bg(rgb(BLUE_HEX))),
                ))
            })
    }
}

pub const ANCHORED: SceneSpec = SceneSpec {
    id: "overlay-anchored",
    areas: &[Area::AnchoredOverlay],
    size: (200., 150.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D],
    golden: false,
    build: |_, cx| cx.new(|_| Popover { open: false }).into(),
    script: || {
        vec![
            Step::Expect(Expect::Missing(id("popover"))),
            Step::Click(id("toggle")),
            Step::Expect(Expect::Bounds(id("popover"), (20., 48., 100., 50.), 0.01)),
            Step::probe(70., 70., BLUE),
            Step::Click(id("toggle")),
            Step::Expect(Expect::Missing(id("popover"))),
            Step::probe(70., 70., WHITE),
        ]
    },
};

fn deferred_overlays() -> AnyElement {
    root()
        .child(
            div()
                .id("dialog")
                .absolute()
                .top(px(20.))
                .left(px(20.))
                .w(px(100.))
                .h(px(50.))
                .overflow_hidden()
                .bg(rgb(GRAY_HEX))
                // A clipped deferred child stays inside the dialog...
                .child(deferred(
                    div()
                        .id("clipped")
                        .absolute()
                        .top(px(30.))
                        .left(px(10.))
                        .w(px(40.))
                        .h(px(60.))
                        .bg(rgb(RED_HEX)),
                ))
                // ...while an unclipped one escapes it, like a calendar in a dialog.
                .child(
                    deferred(
                        div()
                            .id("unclipped")
                            .absolute()
                            .top(px(30.))
                            .left(px(60.))
                            .w(px(40.))
                            .h(px(60.))
                            .bg(rgb(BLUE_HEX)),
                    )
                    .unclipped(),
                ),
        )
        .into_any_element()
}

pub const DEFERRED: SceneSpec = SceneSpec {
    id: "overlay-deferred",
    areas: &[Area::DeferredOverlay, Area::Clipping],
    size: (200., 140.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D],
    golden: true,
    build: |_, cx| static_view(cx, deferred_overlays),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(id("clipped"), (30., 50., 40., 60.), 0.01)),
            Step::Expect(Expect::Bounds(id("unclipped"), (80., 50., 40., 60.), 0.01)),
            Step::probe(50., 60., RED),
            Step::probe(50., 100., WHITE),
            Step::probe(100., 60., BLUE),
            Step::probe(100., 100., BLUE),
            Step::Golden,
        ]
    },
};
