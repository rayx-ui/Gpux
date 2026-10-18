//! Formatting in undo history (ATTR-3): attribute changes are their own undo steps, typing
//! still coalesces, undo and redo restore exact spans, and formatting-only changes bump a format
//! revision and report a format event instead of `EditableTextEvent`.

use gpux::{
    AppContext as _, AttributeChange, AttributeSpan, EditableTextEvent, EditableTextFormatEvent,
    EditableTextState, Entity, FontWeight, TestAppContext, TextAttributes, TextAttributesPatch,
};
use std::{cell::RefCell, rc::Rc};

fn bold() -> TextAttributes {
    TextAttributes {
        font_weight: Some(FontWeight::BOLD),
        ..Default::default()
    }
}

fn set_bold() -> TextAttributesPatch {
    TextAttributesPatch {
        font_weight: AttributeChange::Set(FontWeight::BOLD),
        ..Default::default()
    }
}

fn spans(state: &Entity<EditableTextState>, cx: &mut TestAppContext) -> Vec<AttributeSpan> {
    state.read_with(cx, |state, _| state.attribute_spans().to_vec())
}

#[gpux::test]
fn an_attribute_change_is_its_own_undo_step(cx: &mut TestAppContext) {
    let state = cx.new(EditableTextState::new);
    state.update(cx, |state, cx| {
        state.replace_selection("a", cx);
        state.replace_selection("b", cx);
        assert!(state.set_attributes(0..2, &set_bold(), cx));
        state.set_selection(2, 2, cx);
        state.replace_selection("c", cx);
        assert_eq!(state.text(), "abc");

        assert!(state.undo(cx));
        assert_eq!(state.text(), "ab");
        assert_eq!(state.attribute_spans(), [AttributeSpan::new(0..2, bold())]);
        assert!(state.undo(cx));
        assert_eq!(state.text(), "ab");
        assert!(state.attribute_spans().is_empty());
        assert!(state.undo(cx));
        assert_eq!(state.text(), "");
        assert!(!state.undo(cx));

        assert!(state.redo(cx));
        assert!(state.redo(cx));
        assert_eq!(state.attribute_spans(), [AttributeSpan::new(0..2, bold())]);
        assert!(state.redo(cx));
        assert_eq!(state.text(), "abc");
        assert_eq!(state.attribute_spans(), [AttributeSpan::new(0..3, bold())]);
    });
}

#[gpux::test]
fn typing_under_typing_attributes_still_coalesces(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("x", cx));
    state.update(cx, |state, cx| {
        state.set_selection(1, 1, cx);
        assert!(state.set_attributes(1..1, &set_bold(), cx));
        for ch in ["a", "b", "c"] {
            state.replace_selection(ch, cx);
        }
        assert_eq!(state.attribute_spans(), [AttributeSpan::new(1..4, bold())]);
        assert!(state.undo(cx));
        assert_eq!(state.text(), "x");
        assert!(state.attribute_spans().is_empty());
    });
}

#[gpux::test]
fn undoing_an_edit_restores_the_exact_spans(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("one two three", cx));
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(2..5, &set_bold(), cx));
        assert!(state.set_attributes(8..11, &set_bold(), cx));
    });
    let before = spans(&state, cx);
    state.update(cx, |state, cx| {
        state.set_selection(3, 10, cx);
        assert!(state.delete_backward(cx));
        assert_eq!(state.attribute_spans(), [AttributeSpan::new(2..4, bold())]);
        assert!(state.undo(cx));
        assert_eq!(state.text(), "one two three");
    });
    assert_eq!(spans(&state, cx), before);
    state.update(cx, |state, cx| {
        assert!(state.redo(cx));
        assert_eq!(state.attribute_spans(), [AttributeSpan::new(2..4, bold())]);
    });
}

#[gpux::test]
fn formatting_only_changes_bump_the_format_revision_and_report_their_own_event(
    cx: &mut TestAppContext,
) {
    let state = cx.new(|cx| EditableTextState::with_text("abc", cx));
    let events = Rc::new(RefCell::new(Vec::new()));
    let _subscriptions = cx.update(|cx| {
        let text_events = events.clone();
        let format_events = events.clone();
        [
            cx.subscribe(&state, move |_, event: &EditableTextEvent, _| {
                // `EditableTextEvent` keeps its shape: one variant, destructured irrefutably.
                let EditableTextEvent::Changed { revision, .. } = *event;
                text_events.borrow_mut().push(format!("text {revision}"));
            }),
            cx.subscribe(&state, move |_, event: &EditableTextFormatEvent, _| {
                let EditableTextFormatEvent::Changed { format_revision } = *event;
                format_events
                    .borrow_mut()
                    .push(format!("format {format_revision}"));
            }),
        ]
    });
    let revision = state.read_with(cx, |state, _| state.revision());
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(0..2, &set_bold(), cx));
        assert_eq!(state.revision(), revision);
        assert_eq!(state.format_revision(), 1);
        // Undoing a formatting-only step is formatting-only too.
        assert!(state.undo(cx));
        assert_eq!(state.revision(), revision);
        assert_eq!(state.format_revision(), 2);
        state.replace_selection("d", cx);
        assert_eq!(state.format_revision(), 2);
    });
    cx.run_until_parked();
    let revision_after_typing = state.read_with(cx, |state, _| state.revision());
    assert_eq!(
        *events.borrow(),
        [
            "format 1",
            "format 2",
            &format!("text {revision_after_typing}")
        ]
    );
}

#[gpux::test]
fn toggling_typing_attributes_records_no_undo_step(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("ab", cx));
    state.update(cx, |state, cx| {
        state.set_selection(2, 2, cx);
        assert!(state.set_attributes(2..2, &set_bold(), cx));
        assert_eq!(state.format_revision(), 0);
        // Nothing to undo: the field has no history yet.
        assert!(!state.undo(cx));
        assert_eq!(state.text(), "ab");
    });
}

#[gpux::test]
fn undo_restores_a_deleted_formatted_paragraph_and_its_neighbours(cx: &mut TestAppContext) {
    let state = cx.new(|cx| {
        EditableTextState::with_line_mode(
            "keep\nBOLD gone\ntail",
            gpux::EditableTextLineMode::MultiLine,
            cx,
        )
    });
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(5..9, &set_bold(), cx));
        assert!(state.set_attributes(15..19, &set_bold(), cx));
        state.set_selection(4, 14, cx);
        assert!(state.delete_backward(cx));
        assert_eq!(state.text(), "keep\ntail");
        assert_eq!(state.attribute_spans(), [AttributeSpan::new(5..9, bold())]);
        assert!(state.undo(cx));
        assert_eq!(state.text(), "keep\nBOLD gone\ntail");
        assert_eq!(
            state.attribute_spans(),
            [
                AttributeSpan::new(5..9, bold()),
                AttributeSpan::new(15..19, bold())
            ]
        );
    });
}
