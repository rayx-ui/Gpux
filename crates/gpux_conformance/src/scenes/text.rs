//! Text: shaping with the bundled fonts, styled runs, decorations, wrapping, truncation, and
//! interactive text.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyElement, AnyView, App, Context, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    Render, StyledText, Window, div, px, rgb, semantic_query::Query,
};

const ALL_LANES: &[Lane] = &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F];

fn shaping() -> AnyElement {
    // Children sized to their content, so their bounds measure the shaped text.
    root()
        .flex()
        .flex_col()
        .items_start()
        .p(px(10.))
        .child(
            div()
                .id("greeting")
                .text_size(px(20.))
                .child("Hello, gpux!"),
        )
        .child(
            div()
                .id("code")
                .font_family("Lilex")
                .text_size(px(20.))
                .child("fn main() {}"),
        )
        .into_any_element()
}

pub const SHAPING: SceneSpec = SceneSpec {
    id: "text-shaping",
    areas: &[Area::TextShaping],
    size: (240., 100.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, shaping),
    script: || {
        vec![
            Step::Expect(Expect::Exists(Query::text("Hello, gpux!"))),
            // Lilex is monospaced: 12 glyphs of 0.6 em at 20 px. The line height rounds to
            // device pixels, half pixels at 2x. The text element ceils its width, and CoreText
            // (macOS windows) measures the line at 144.00002 px, so it can be one pixel wider.
            Step::Expect(Expect::Size(Query::text("fn main() {}"), 144., 32., 1.)),
            // Glyphs paint dark pixels where the text sits and nothing beside it.
            Step::ProbeDiffer((12., 14., 100., 16.), (130., 14., 100., 16.), 40),
            Step::ProbeUniform((150., 10., 80., 20.), WHITE, 0),
            Step::Golden,
        ]
    },
};

fn runs() -> AnyElement {
    let text = "plain bold red";
    root()
        .p(px(10.))
        .text_size(px(20.))
        .child(
            div()
                .id("runs")
                .child(StyledText::new(text).with_highlights([
                    (
                        6..10,
                        HighlightStyle {
                            font_weight: Some(FontWeight::SEMIBOLD),
                            ..Default::default()
                        },
                    ),
                    (
                        11..14,
                        HighlightStyle {
                            color: Some(rgb(RED_HEX).into()),
                            ..Default::default()
                        },
                    ),
                ])),
        )
        .into_any_element()
}

pub const RUNS: SceneSpec = SceneSpec {
    id: "text-runs",
    areas: &[Area::TextRuns],
    size: (240., 60.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, runs),
    script: || {
        vec![
            Step::Expect(Expect::Exists(Query::text("plain bold red"))),
            Step::ProbeDiffer((10., 14., 40., 18.), (180., 14., 40., 18.), 40),
            Step::Golden,
        ]
    },
};

fn decorations() -> AnyElement {
    root()
        .p(px(10.))
        .text_size(px(20.))
        .child(div().id("underlined").underline().child("underlined"))
        .child(div().id("struck").line_through().child("struck"))
        .into_any_element()
}

pub const DECORATIONS: SceneSpec = SceneSpec {
    id: "text-decorations",
    areas: &[Area::TextDecorations],
    size: (200., 80.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, decorations),
    script: || {
        vec![
            Step::Expect(Expect::Exists(Query::text("underlined"))),
            Step::Expect(Expect::Exists(Query::text("struck"))),
            // Both lines paint ink; nothing paints beside them.
            Step::ProbeDiffer((12., 14., 50., 22.), (140., 14., 50., 22.), 40),
            Step::ProbeDiffer((12., 46., 50., 22.), (140., 46., 50., 22.), 40),
            Step::ProbeUniform((130., 10., 60., 64.), WHITE, 0),
            Step::Golden,
        ]
    },
};

fn wrapping() -> AnyElement {
    root()
        .p(px(10.))
        .child(
            div()
                .id("wrapped")
                .w(px(120.))
                .child("The quick brown fox jumps over the lazy dog"),
        )
        .into_any_element()
}

pub const WRAPPING: SceneSpec = SceneSpec {
    id: "text-wrapping",
    areas: &[Area::TextWrapping],
    size: (200., 120.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, wrapping),
    script: || {
        vec![
            // Four 26 px lines of 16 px IBM Plex Sans: "The quick " | "brown fox " |
            // "jumps over the " | "lazy dog", as the bundled text system wraps them at 120 px.
            Step::Expect(Expect::Size(id("wrapped"), 120., 104., 0.01)),
            // Ink inside the wrap width, none past it.
            Step::ProbeDiffer((10., 10., 50., 26.), (140., 10., 50., 26.), 40),
            Step::ProbeUniform((135., 10., 60., 104.), WHITE, 0),
        ]
    },
};

fn truncation() -> AnyElement {
    root()
        .p(px(10.))
        .child(
            div()
                .id("truncated")
                .w(px(100.))
                .whitespace_nowrap()
                .text_ellipsis()
                .child("The quick brown fox jumps over the lazy dog"),
        )
        .into_any_element()
}

pub const TRUNCATION: SceneSpec = SceneSpec {
    id: "text-truncation",
    areas: &[Area::TextTruncation],
    size: (200., 60.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, truncation),
    script: || {
        vec![
            // One 26 px line, cut to the box.
            Step::Expect(Expect::Size(id("truncated"), 100., 26., 0.01)),
            Step::ProbeUniform((115., 10., 60., 20.), WHITE, 0),
        ]
    },
};

/// Interactive text whose word ranges report clicks.
struct Interactive {
    clicked: Option<usize>,
}

impl Render for Interactive {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        root()
            .p(px(10.))
            .child(status(match self.clicked {
                Some(index) => format!("clicked {index}"),
                None => "clicked none".to_string(),
            }))
            .child(div().id("links").child(
                InteractiveText::new("links", StyledText::new("alpha beta gamma")).on_click(
                    vec![0..5, 6..10, 11..16],
                    move |index, _, cx| {
                        entity.update(cx, |this, cx| {
                            this.clicked = Some(index);
                            cx.notify();
                        });
                    },
                ),
            ))
    }
}

fn interactive(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|_| Interactive { clicked: None }).into()
}

pub const INTERACTIVE: SceneSpec = SceneSpec {
    id: "text-interactive",
    areas: &[Area::TextSelection],
    size: (240., 80.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: interactive,
    script: || {
        vec![
            Step::Expect(Expect::Exists(status_is("clicked none"))),
            Step::ProbeDiffer((12., 40., 40., 18.), (180., 40., 40., 18.), 40),
            Step::Capture("before"),
            // "beta" spans about x 55..85 in 16 px IBM Plex Sans.
            Step::ClickAt(70., 49.),
            Step::Expect(Expect::Exists(status_is("clicked 1"))),
            Step::ChangedSince("before", (10., 10., 120., 26.)),
        ]
    },
};
