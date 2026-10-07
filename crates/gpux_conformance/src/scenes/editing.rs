//! Editing: editable text with typing, keys and selection, and IME composition.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyView, App, Context, DevtoolsSemantics, EditableText, EditableTextState, Entity, IntoElement,
    Render, Window, div, px, semantic_query::Query,
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
            // A double click selects a word; typing replaces it.
            Step::ClickCount(textbox(), 1),
            Step::Keys("home".into()),
            Step::Keys("shift-right shift-right shift-right".into()),
            Step::Expect(Expect::Exists(status_is("selection 0..3 marked none"))),
            Step::Type(textbox(), "Eve".into()),
            value_is("Eve Lovela"),
        ]
    },
};

pub const IME: SceneSpec = SceneSpec {
    id: "editable-ime",
    areas: &[Area::Ime, Area::EditableText],
    size: (280., 80.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
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
