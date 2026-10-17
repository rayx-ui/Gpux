//! Text: shaping with the bundled fonts, styled runs, decorations, wrapping, truncation, and
//! interactive text.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyElement, AnyView, App, Context, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    Render, RunMetrics, StrikethroughStyle, StyledText, TextRun, Window, div, font, px, rgb,
    semantic_query::Query,
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

/// A Lilex run (0.6 em per glyph) of `len` bytes with its own size, spacing and shift.
fn metric_run(len: usize, size: f32, letter_spacing: f32, shift: f32) -> TextRun {
    TextRun {
        len,
        font: font("Lilex"),
        color: rgb(BLACK_HEX).into(),
        metrics: RunMetrics {
            font_size: Some(px(size)),
            letter_spacing: (letter_spacing != 0.).then(|| px(letter_spacing)),
            baseline_shift: px(shift),
        },
        ..Default::default()
    }
}

/// "HH" at 12, 28 and 16 px (the 16 px pair on a blue background), a superscript and a
/// subscript "H", and "HHH" spaced 6 px apart (`spacing` 0 draws the same line unspaced).
fn metric_line(spacing: f32) -> StyledText {
    let mut runs = vec![
        metric_run(2, 12., 0., 0.),
        metric_run(2, 28., 0., 0.),
        metric_run(2, 16., 0., 0.),
        metric_run(1, 16., 0., 8.),
        metric_run(1, 16., 0., -6.),
        metric_run(3, 16., spacing, 0.),
    ];
    runs[2].background_color = Some(rgb(BLUE_HEX).into());
    StyledText::new("HHHHHHHHHHH").with_runs(runs)
}

/// Four spaces at 12 px, then four at 28 px, both struck through in red 3 px thick.
fn struck_line() -> StyledText {
    let struck = |size| TextRun {
        strikethrough: Some(StrikethroughStyle {
            thickness: px(3.),
            color: Some(rgb(RED_HEX).into()),
        }),
        ..metric_run(4, size, 0., 0.)
    };
    StyledText::new("        ").with_runs(vec![struck(12.), struck(28.)])
}

fn run_metrics() -> AnyElement {
    let line = |id: &'static str, top: f32, text: StyledText| {
        div()
            .id(id)
            .absolute()
            .left(px(10.))
            .top(px(top))
            .line_height(px(48.))
            .child(text)
    };
    root()
        .child(line("rich", 10., metric_line(6.)))
        .child(line("unspaced", 70., metric_line(0.)))
        .child(line("struck", 130., struck_line()))
        .into_any_element()
}

/// Per-run font size, letter spacing and baseline shift in one line. Lilex glyphs advance 0.6 em
/// and its ascent and descent are 1.025 and 0.275 em, so in the 48 px row of the top line (ascent
/// 28.7 from the 28 px run, descent 10.4 from the subscript) the baseline sits at y 43.15. The runs
/// span x 10-24.4 (12 px), 24.4-58 (28 px), 58-77.2 (16 px), 77.2-86.8 (superscript, raised 8),
/// 86.8-96.4 (subscript, lowered 6) and 96.4-137.2 ("HHH" spaced 6 px; 125.2 unspaced). The struck
/// line's baseline is at y 164.5: the 12 px run's strikethrough at 160.2, the 28 px run's at 154.5.
pub const RUN_METRICS: SceneSpec = SceneSpec {
    id: "rich-text-runs",
    areas: &[Area::TextRunMetrics],
    size: (220., 190.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::E, Lane::F],
    golden: false,
    build: |_, cx| static_view(cx, run_metrics),
    script: || {
        vec![
            // The text element ceils its width; CoreText may measure a hair wider.
            Step::Expect(Expect::Size(id("rich"), 128., 48., 1.)),
            Step::Expect(Expect::Size(id("unspaced"), 116., 48., 1.)),
            // The 28 px run's ink reaches above the 12 px run's.
            Step::ProbeUniform((11., 26., 12., 6.), WHITE, 0),
            Step::ProbeDiffer((28., 26., 24., 6.), (150., 26., 24., 6.), 40),
            // The unshifted runs stand on one baseline: ink just above it, none below.
            Step::ProbeDiffer((12., 39., 10., 3.), (150., 39., 10., 3.), 40),
            Step::ProbeDiffer((30., 39., 10., 3.), (150., 39., 10., 3.), 40),
            Step::ProbeDiffer((60., 39., 10., 3.), (150., 39., 10., 3.), 40),
            Step::ProbeUniform((10., 45., 46., 4.), WHITE, 0),
            // The background covers the whole row above and below the 16 px glyphs.
            Step::ProbeUniform((60., 11., 15., 3.), BLUE, 0),
            Step::ProbeUniform((60., 54., 15., 3.), BLUE, 0),
            // The superscript's ink sits above the x-height of the baseline runs.
            Step::ProbeDiffer((79., 26., 6., 7.), (150., 26., 6., 7.), 40),
            Step::ProbeUniform((78., 37., 8., 5.), WHITE, 0),
            // The subscript's ink reaches below the baseline.
            Step::ProbeDiffer((88., 45., 7., 3.), (150., 45., 7., 3.), 40),
            // The spaced run's last glyph lies past where the unspaced line ends.
            Step::ProbeDiffer((129., 34., 6., 8.), (150., 34., 6., 8.), 40),
            Step::ProbeUniform((127., 94., 10., 8.), WHITE, 0),
            // Each run's strikethrough crosses its own middle.
            Step::ProbeUniform((14., 160.5, 20., 2.), RED, 0),
            Step::ProbeUniform((14., 152., 20., 4.), WHITE, 0),
            Step::ProbeUniform((44., 155., 50., 2.), RED, 0),
            Step::ProbeUniform((44., 159.5, 50., 4.), WHITE, 0),
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
