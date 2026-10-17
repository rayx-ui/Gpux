//! Editing: editable text with typing, keys and selection, and IME composition.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyView, App, AttributeChange, AttributeState, Context, DevtoolsSemantics, EditableText,
    EditableTextState, Entity, FontStyle, FontWeight, IntoElement, Render, TextAttributes,
    TextAttributesPatch, UnderlineStyle, Window, div, px, rgb, semantic_query::Query,
};

const ALL_LANES: &[Lane] = &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F];

/// One editable field exposing its text as a textbox value.
struct Field {
    state: Entity<EditableTextState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let value = state.text();
        let selection = state.selection().range();
        let marked = state
            .marked_range()
            .map(|range| format!("{}..{}", range.start, range.end))
            .unwrap_or_else(|| "none".to_string());
        root()
            .p(px(10.))
            .child(status(format!(
                "selection {}..{} marked {marked}",
                selection.start, selection.end
            )))
            .child(
                div().w(px(240.)).child(
                    EditableText::new(&self.state).devtools_semantics(
                        DevtoolsSemantics::new()
                            .role("textbox")
                            .name("Name")
                            .value(value),
                    ),
                ),
            )
    }
}

fn field(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        let state = cx.new(EditableTextState::new);
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Field { state }
    })
    .into()
}

fn textbox() -> Query {
    Query::role("textbox").with_name("Name")
}

fn value_is(value: &str) -> Step {
    Step::Expect(Expect::Exists(textbox().with_value(value)))
}

pub const EDITABLE: SceneSpec = SceneSpec {
    id: "editable-text",
    areas: &[Area::EditableText, Area::TextSelection],
    size: (280., 80.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: field,
    script: || {
        vec![
            Step::Capture("empty"),
            Step::Type(textbox(), "Ada Lovelace".into()),
            value_is("Ada Lovelace"),
            // The typed text paints in the field.
            Step::ChangedSince("empty", (10., 36., 240., 26.)),
            Step::Expect(Expect::Focused(textbox())),
            Step::Keys("backspace backspace".into()),
            value_is("Ada Lovela"),
            // A double click on "Ada" (x 10..38 in 16 px IBM Plex Sans) selects the word;
            // typing replaces it.
            Step::ClickCountAt(20., 49., 2),
            Step::Expect(Expect::Exists(status_is("selection 0..3 marked none"))),
            Step::Type(textbox(), "Eve".into()),
            value_is("Eve Lovela"),
            // A triple click selects the line.
            Step::ClickCountAt(20., 49., 3),
            Step::Expect(Expect::Exists(status_is("selection 0..10 marked none"))),
            // Shift-arrows extend a selection from the caret.
            Step::ClickCount(textbox(), 1),
            Step::Keys("home shift-right shift-right shift-right".into()),
            Step::Expect(Expect::Exists(status_is("selection 0..3 marked none"))),
        ]
    },
};

pub const IME: SceneSpec = SceneSpec {
    id: "editable-ime",
    areas: &[Area::Ime, Area::EditableText],
    size: (280., 80.),
    scales: &[1., 2.],
    // Desktop and mobile windows compose through the platform's input method, which a host
    // cannot drive; the browser's can be driven over CDP.
    lanes: &[Lane::A, Lane::B, Lane::C],
    golden: false,
    build: field,
    script: || {
        vec![
            Step::Capture("empty"),
            Step::Focus(textbox()),
            Step::ImeCompose("に".into(), 1),
            value_is("に"),
            Step::Expect(Expect::Exists(status_is("selection 3..3 marked 0..3"))),
            Step::ImeCompose("にほ".into(), 2),
            value_is("にほ"),
            Step::ImeCommit("日本".into()),
            value_is("日本"),
            // The committed text paints in the field.
            Step::ChangedSince("empty", (10., 36., 240., 26.)),
            Step::Expect(Expect::Exists(status_is("selection 6..6 marked none"))),
            Step::ImeCompose("ご".into(), 1),
            Step::ImeCancel,
            value_is("日本"),
        ]
    },
};

/// One field holding Lilex "H"s (0.6 em each) formatted in place, and buttons that make its
/// first two glyphs bold and the next two italic.
struct RichAttributes {
    state: Entity<EditableTextState>,
}

impl RichAttributes {
    fn new(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| {
            let mut state = EditableTextState::with_text("HHHHHHHHHHHH", cx);
            let mut set = |range: std::ops::Range<usize>, attributes: TextAttributes| {
                state.set_attributes(range, &TextAttributesPatch::setting(&attributes), cx);
            };
            set(
                4..6,
                TextAttributes {
                    font_size: Some(px(28.)),
                    ..Default::default()
                },
            );
            set(
                6..8,
                TextAttributes {
                    color: Some(rgb(RED_HEX).into()),
                    // An underline without a color of its own takes the run's.
                    underline: Some(UnderlineStyle {
                        thickness: px(2.),
                        color: None,
                        wavy: false,
                    }),
                    ..Default::default()
                },
            );
            set(
                8..10,
                TextAttributes {
                    background_color: Some(rgb(BLUE_HEX).into()),
                    ..Default::default()
                },
            );
            set(
                10..11,
                TextAttributes {
                    baseline_shift: Some(px(8.)),
                    ..Default::default()
                },
            );
            set(
                11..12,
                TextAttributes {
                    baseline_shift: Some(px(-6.)),
                    ..Default::default()
                },
            );
            state
        });
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self { state }
    }
}

fn apply_attributes(
    state: &Entity<EditableTextState>,
    range: std::ops::Range<usize>,
    patch: TextAttributesPatch,
    cx: &mut App,
) {
    state.update(cx, |state, cx| {
        state.set_attributes(range, &patch, cx);
    });
}

impl Render for RichAttributes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let on = |state: AttributeState| match state {
            AttributeState::On => "on",
            AttributeState::Off => "off",
            AttributeState::Mixed => "mixed",
        };
        let bold = on(state.attribute_state(0..2, |attributes| {
            attributes.font_weight == Some(FontWeight::BOLD)
        }));
        let italic = on(state.attribute_state(2..4, |attributes| {
            attributes.font_style == Some(FontStyle::Italic)
        }));
        let field = self.state.clone();
        let button = |id: &'static str, x: f32| {
            div()
                .id(id)
                .absolute()
                .left(px(x))
                .top(px(80.))
                .w(px(60.))
                .h(px(20.))
                .bg(rgb(GRAY_HEX))
        };
        root()
            .font_family("Lilex")
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(10.))
                    .w(px(280.))
                    .line_height(px(24.))
                    .child(EditableText::new(&self.state)),
            )
            .child(button("make-bold", 10.).on_click({
                let field = field.clone();
                move |_, _, cx| {
                    apply_attributes(
                        &field,
                        0..2,
                        TextAttributesPatch {
                            font_weight: AttributeChange::Set(FontWeight::BOLD),
                            ..Default::default()
                        },
                        cx,
                    )
                }
            }))
            .child(button("make-italic", 80.).on_click(move |_, _, cx| {
                apply_attributes(
                    &field,
                    2..4,
                    TextAttributesPatch {
                        font_style: AttributeChange::Set(FontStyle::Italic),
                        ..Default::default()
                    },
                    cx,
                )
            }))
            .child(
                div()
                    .absolute()
                    .left(px(150.))
                    .top(px(80.))
                    .child(status(format!("bold {bold} italic {italic}"))),
            )
    }
}

/// Stored formatting in one field with a 24 px line height. The 28 px run makes its row 42 px
/// tall (24 x 28 / 16); the row's ascent comes from that run (28.7) and its descent from the
/// subscript (10.4), so the baseline sits at y 40.15 and the underline about 6.4 px below it.
/// The runs span x 10-29.2 (bold target), 29.2-48.4 (italic target), 48.4-82
/// (28 px), 82-101.2 (red, underlined), 101.2-120.4 (blue background), 120.4-130 (superscript,
/// raised 8) and 130-139.6 (subscript, lowered 6).
pub const RICH_ATTRIBUTES: SceneSpec = SceneSpec {
    id: "editable-rich-attributes",
    areas: &[Area::EditableRichAttributes, Area::EditableText],
    size: (300., 110.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C],
    golden: false,
    build: |_, cx| cx.new(RichAttributes::new).into(),
    script: || {
        vec![
            Step::Expect(Expect::Exists(status_is("bold off italic off"))),
            // The 28 px run's ink reaches above the 16 px runs'.
            Step::ProbeDiffer((52., 23., 26., 4.), (200., 23., 26., 4.), 40),
            Step::ProbeUniform((11., 23., 36., 4.), WHITE, 0),
            // The red run's underline takes its color.
            Step::ProbeUniform((84., 47.2, 15., 1.5), RED, 0),
            // The background covers the whole 42 px row, above and below the glyphs.
            Step::ProbeUniform((103., 12., 15., 3.), BLUE, 0),
            Step::ProbeUniform((103., 49., 15., 2.), BLUE, 0),
            // The superscript sits above the baseline runs' x-height; the subscript reaches
            // below the baseline.
            Step::ProbeDiffer((122., 23., 6., 7.), (200., 23., 6., 7.), 40),
            Step::ProbeUniform((121., 33.5, 8., 5.), WHITE, 0),
            Step::ProbeDiffer((131., 42., 7., 3.), (200., 42., 7., 3.), 40),
            Step::Capture("regular"),
            Step::Click(id("make-bold")),
            Step::Expect(Expect::Exists(status_is("bold on italic off"))),
            Step::ChangedSince("regular", (10., 29., 19., 11.)),
            Step::Capture("bold"),
            Step::Click(id("make-italic")),
            Step::Expect(Expect::Exists(status_is("bold on italic on"))),
            Step::ChangedSince("bold", (29., 29., 19., 11.)),
        ]
    },
};
