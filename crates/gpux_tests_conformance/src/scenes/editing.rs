//! Editing: editable text with typing, keys and selection, and IME composition.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyView, App, AttributeChange, AttributeState, Context, DevtoolsSemantics, EditableText,
    EditableTextLineMode, EditableTextState, Entity, FontStyle, FontWeight, InlineObject,
    IntoElement, MouseButton, ParagraphAlignment, ParagraphFormat, Render, SharedString,
    TextAttributes, TextAttributesPatch, TextListItem, TextListKind, UnderlineStyle, Window, div,
    px, rgb, semantic_query::Query,
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
            // Lane B's `clipboard: html` specs paste formatted text into it.
            state.set_rich_paste(true);
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

/// Six paragraphs of Lilex "H" words, one per paragraph format.
const PARAGRAPH_TEXT: &str =
    "HHHH\nHHHH\nHHH HHH HHH HHH HHH HHH HHH HHH\nHHHH\nHHH HHH HHH HHH HHH HHH HHH HHH\nHHHH";

struct Paragraphs {
    state: Entity<EditableTextState>,
}

impl Paragraphs {
    fn new(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| {
            let mut state = EditableTextState::with_line_mode(
                PARAGRAPH_TEXT,
                EditableTextLineMode::MultiLine,
                cx,
            );
            let starts = std::iter::once(0)
                .chain(
                    PARAGRAPH_TEXT
                        .match_indices('\n')
                        .map(|(index, _)| index + 1),
                )
                .collect::<Vec<_>>();
            let formats = [
                ParagraphFormat {
                    alignment: Some(ParagraphAlignment::Center),
                    ..Default::default()
                },
                ParagraphFormat {
                    alignment: Some(ParagraphAlignment::Right),
                    ..Default::default()
                },
                ParagraphFormat {
                    alignment: Some(ParagraphAlignment::Justify),
                    ..Default::default()
                },
                ParagraphFormat {
                    leading_indent: px(40.),
                    ..Default::default()
                },
                ParagraphFormat {
                    leading_indent: px(40.),
                    first_line_indent: px(-40.),
                    ..Default::default()
                },
                ParagraphFormat {
                    space_before: px(20.),
                    ..Default::default()
                },
            ];
            for (start, format) in starts.into_iter().zip(formats) {
                state.set_paragraph_format(start..start, format, cx);
            }
            state
        });
        Self { state }
    }
}

impl Render for Paragraphs {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root().font_family("Lilex").child(
            div()
                .id("paragraphs")
                .absolute()
                .left(px(10.))
                .top(px(10.))
                .w(px(240.))
                .h(px(230.))
                .line_height(px(24.))
                .child(EditableText::new(&self.state)),
        )
    }
}

/// Paragraph formats in one field, 240 px wide with 24 px rows of 16 px Lilex (0.6 em glyphs;
/// ink from 6.8 to 18 px down each row). Rows (tops from the field's top at y 10): centered
/// "HHHH" (0, x 100.8-139.2), right-aligned "HHHH" (24, x 201.6-240), a justified paragraph
/// (48: six words widened to end at 240; 72: its last row, left), "HHHH" indented 40 (96), a
/// hanging indent (120: first row at 0; 144: at 40), and "HHHH" 20 px below the paragraph before
/// (188).
pub const PARAGRAPHS: SceneSpec = SceneSpec {
    id: "editable-paragraphs",
    areas: &[Area::EditableParagraphs, Area::EditableText],
    size: (260., 250.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C],
    golden: false,
    build: |_, cx| cx.new(Paragraphs::new).into(),
    script: || {
        vec![
            Step::Expect(Expect::Size(id("paragraphs"), 240., 230., 0.5)),
            // Centered: ink from x 110.8, none before.
            Step::ProbeUniform((12., 18., 90., 8.), WHITE, 0),
            Step::ProbeDiffer((112., 18., 30., 8.), (12., 18., 30., 8.), 40),
            // Right-aligned: ink from x 211.6, none before.
            Step::ProbeUniform((12., 42., 190., 8.), WHITE, 0),
            Step::ProbeDiffer((215., 42., 30., 8.), (12., 42., 30., 8.), 40),
            // Justified: the first row's last word reaches the field's edge; unjustified it
            // would end at x 230.8. The last row stays left.
            Step::ProbeDiffer((242., 66., 6., 8.), (242., 90., 6., 8.), 40),
            Step::ProbeUniform((150., 90., 95., 8.), WHITE, 0),
            // Indented 40: nothing before x 50.
            Step::ProbeUniform((12., 114., 36., 8.), WHITE, 0),
            Step::ProbeDiffer((52., 114., 30., 8.), (12., 114., 30., 8.), 40),
            // Hanging indent: the first row starts at the edge, the second at 40.
            Step::ProbeDiffer((12., 138., 30., 8.), (12., 162., 30., 8.), 40),
            Step::ProbeUniform((12., 162., 36., 8.), WHITE, 0),
            // Spacing: an empty 20 px band above the last paragraph.
            Step::ProbeUniform((12., 180., 230., 16.), WHITE, 0),
            Step::ProbeDiffer((12., 206., 30., 8.), (12., 180., 30., 8.), 40),
        ]
    },
};

/// A bullet, a nested bullet, two numbered items and a checkbox item, with a 40 px list step.
struct Lists {
    state: Entity<EditableTextState>,
}

impl Lists {
    fn new(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| {
            let mut state = EditableTextState::with_line_mode(
                "HHHH\nHHHH\nHHHH\nHHHH\nHHHH",
                EditableTextLineMode::MultiLine,
                cx,
            );
            state.set_list_indent_step(px(40.), cx);
            let items = [
                TextListItem::bullet(0),
                TextListItem::bullet(1),
                TextListItem::numbered(0),
                TextListItem::numbered(0),
                TextListItem::checkbox(0),
            ];
            for (paragraph, item) in items.into_iter().enumerate() {
                let start = paragraph * 5;
                state.set_paragraph_format(
                    start..start,
                    ParagraphFormat {
                        list: Some(item),
                        ..Default::default()
                    },
                    cx,
                );
            }
            state
        });
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self { state }
    }
}

impl Render for Lists {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let checked = matches!(
            self.state.read(cx).paragraph_format(20).list,
            Some(TextListItem {
                kind: TextListKind::Checkbox { checked: true },
                ..
            })
        );
        root()
            .font_family("Lilex")
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(10.))
                    .w(px(240.))
                    .h(px(130.))
                    .line_height(px(24.))
                    .child(EditableText::new(&self.state)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(150.))
                    .child(status(format!("checked {checked}"))),
            )
    }
}

/// List items in one field of 24 px rows (baselines 18 px down each row) of 16 px Lilex, with a
/// 40 px list step: item text starts at x 40 (level 0) or 80 (level 1) in the field at (10, 10),
/// and markers end 6.4 px before it. Rows: "\u{2022}", "\u{25e6}" (level 1), "1.", "2." and a 12.8 px
/// checkbox at x 20.8-33.6, y 101.2-114 of the field.
pub const LISTS: SceneSpec = SceneSpec {
    id: "editable-lists",
    areas: &[Area::EditableLists, Area::EditableText],
    size: (260., 180.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C],
    golden: false,
    build: |_, cx| cx.new(Lists::new).into(),
    script: || {
        vec![
            Step::Expect(Expect::Exists(status_is("checked false"))),
            // Item text starts at its level's indent; nothing is painted in front of a marker.
            Step::ProbeUniform((11., 19., 10., 8.), WHITE, 0),
            Step::ProbeDiffer((51., 18., 30., 9.), (11., 18., 30., 9.), 40),
            Step::ProbeUniform((51., 42., 12., 9.), WHITE, 0),
            Step::ProbeDiffer((91., 42., 30., 9.), (51., 42., 30., 9.), 40),
            // Markers: the bullets and both numbers paint before their items.
            Step::ProbeDiffer((34., 19., 9., 8.), (11., 19., 9., 8.), 40),
            Step::ProbeDiffer((74., 43., 9., 8.), (11., 43., 9., 8.), 40),
            Step::ProbeDiffer((25., 66., 18., 9.), (200., 66., 18., 9.), 40),
            Step::ProbeDiffer((25., 90., 18., 9.), (200., 90., 18., 9.), 40),
            // The numbers differ: "1." and "2.".
            Step::ProbeDiffer((25., 66., 18., 9.), (25., 90., 18., 9.), 40),
            // An empty checkbox; a click checks it, filling its middle.
            Step::ProbeUniform((35., 115., 4., 4.), WHITE, 0),
            Step::Capture("unchecked"),
            Step::ClickAt(37.2, 117.6),
            Step::Expect(Expect::Exists(status_is("checked true"))),
            Step::ProbeUniform((35.5, 115.5, 3., 3.), BLACK, 0),
            Step::ChangedSince("unchecked", (31., 111., 12., 12.)),
        ]
    },
};

/// Lilex "HH" around a green chip that counts its clicks and a 40 px blue box.
struct InlineObjects {
    state: Entity<EditableTextState>,
    clicks: Entity<usize>,
}

impl InlineObjects {
    fn new(cx: &mut Context<Self>) -> Self {
        let object =
            |key: &'static str, width: f32, height: f32, baseline_offset: f32| InlineObject {
                key: SharedString::from(key),
                width: px(width),
                height: px(height),
                baseline_offset: px(baseline_offset),
                text_fallback: SharedString::from(key),
                html_fallback: None,
            };
        let state = cx.new(|cx| {
            let mut state =
                EditableTextState::with_line_mode("HHHHHH", EditableTextLineMode::MultiLine, cx);
            state.insert_inline_object(4..4, object("box", 40., 40., 0.), cx);
            state.insert_inline_object(2..2, object("chip", 60., 18., 4.), cx);
            state.set_selection(0, 0, cx);
            state
        });
        let clicks = cx.new(|_| 0);
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        cx.observe(&clicks, |_, _, cx| cx.notify()).detach();
        Self { state, clicks }
    }
}

impl Render for InlineObjects {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = *self.clicks.read(cx);
        let caret = self.state.read(cx).selection().head();
        let counter = self.clicks.clone();
        root()
            .font_family("Lilex")
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(10.))
                    .w(px(280.))
                    .h(px(60.))
                    .line_height(px(24.))
                    .child(EditableText::new(&self.state).inline_object_renderer(
                        move |object, _, _| {
                            let counter = counter.clone();
                            let color = if object.key.as_ref() == "chip" {
                                GREEN_HEX
                            } else {
                                BLUE_HEX
                            };
                            div()
                                .id(object.key.clone())
                                .size_full()
                                .bg(rgb(color))
                                .when(object.key.as_ref() == "chip", |chip| {
                                    chip.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        counter.update(cx, |clicks, cx| {
                                            *clicks += 1;
                                            cx.notify();
                                        });
                                        cx.stop_propagation();
                                    })
                                })
                                .into_any_element()
                        },
                    )),
            )
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(80.))
                    .child(status(format!("clicks {clicks} caret {caret}"))),
            )
    }
}

/// Inline objects in one field of 16 px Lilex at (10, 10): "HH" (x 0-19.2), a 60 x 18 green chip
/// standing 4 px below the baseline (x 19.2-79.2), "HH", a 40 x 40 blue box on the baseline
/// (x 98.4-138.4) and "HH". The box makes the row 44.4 px tall (ascent 40, Lilex descent 4.4),
/// with the baseline at y 40 of the field.
pub const INLINE_OBJECTS: SceneSpec = SceneSpec {
    id: "editable-inline-objects",
    areas: &[Area::EditableInlineObjects, Area::EditableText],
    size: (300., 110.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C],
    golden: false,
    build: |_, cx| cx.new(InlineObjects::new).into(),
    script: || {
        vec![
            Step::Expect(Expect::Exists(status_is("clicks 0 caret 0"))),
            // The chip fills its box (y 36-54); the box fills its own (y 10-50).
            Step::ProbeUniform((32., 38., 54., 14.), GREEN, 0),
            Step::ProbeUniform((111., 13., 34., 34.), BLUE, 0),
            // Text after the chip sits on the shared baseline (y 50).
            Step::ProbeDiffer((90., 40., 16., 8.), (220., 40., 16., 8.), 40),
            // The chip takes the click; the caret does not move.
            Step::ClickAt(59., 45.),
            Step::Expect(Expect::Exists(status_is("clicks 1 caret 0"))),
        ]
    },
};
