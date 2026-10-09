//! Typing into a focused editable text field in a form of labelled fields.

use crate::{
    AnyView, App, Context, EditableText, EditableTextState, Entity, FastStats, IntoElement, Render,
    SharedString, Window, div, hsla, prelude::*, px,
};

use super::{Fixture, Mode, Scenario, at_least_per_frame, drawn_per_frame, expect_frames};

const FIELDS: usize = 12;
const TYPED: &str = "the quick brown fox jumps over the lazy dog ";

pub(super) fn scenarios() -> Vec<Scenario> {
    vec![Scenario {
        name: "form-typing",
        description: "a form of 12 labelled editable fields; a character is typed into the \
                      focused one per frame",
        build: |window, cx| Box::new(Typing::new(window, cx)),
        warm_up_frames: 3,
    }]
}

pub(crate) struct Form {
    fields: Vec<Entity<EditableTextState>>,
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .children(self.fields.iter().enumerate().map(|(index, field)| {
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .w(px(100.))
                            .child(SharedString::from(format!("Field {index}"))),
                    )
                    .child(
                        div()
                            .w(px(320.))
                            .bg(hsla(0., 0., 0.15, 1.))
                            .child(EditableText::new(field)),
                    )
            }))
    }
}

struct Typing {
    form: Entity<Form>,
    focused: Entity<EditableTextState>,
}

impl Typing {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        let fields: Vec<Entity<EditableTextState>> = (0..FIELDS)
            .map(|index| cx.new(|cx| EditableTextState::with_text(format!("value {index}"), cx)))
            .collect();
        let focused = fields[0].clone();
        let form = cx.new(|cx| {
            for field in &fields {
                cx.observe(field, |_, _, cx| cx.notify()).detach();
            }
            Form { fields }
        });
        let handle = focused.read(cx).focus_handle();
        window.focus(&handle, cx);
        Self { form, focused }
    }
}

impl Fixture for Typing {
    fn root(&self) -> AnyView {
        self.form.clone().into()
    }

    fn step(&mut self, frame: usize, _: &mut Window, cx: &mut App) {
        self.focused.update(cx, |state, cx| {
            // Keep the text bounded: clear it every line's worth of typing.
            if frame.is_multiple_of(TYPED.len()) {
                state.select_all(cx);
                state.replace_selection("", cx);
            }
            let at = frame % TYPED.len();
            state.replace_selection(&TYPED[at..at + 1], cx);
        });
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        at_least_per_frame(stats, "views_built", 2)?;
        // A background and the glyphs of every field's label.
        drawn_per_frame(stats, (FIELDS * 6) as u64)
    }
}
