//! Paragraph formats (PARA-1): one format per paragraph between hard line breaks, set, read and
//! cleared over the paragraphs a range touches, moved with every edit (splits copy, joins keep
//! the first), restored by undo, and carried by `AttributedText`.

use gpux::{
    AppContext as _, AttributedText, ClipboardItem, EditableTextLineMode, EditableTextState,
    Entity, ParagraphAlignment, ParagraphFormat, TestAppContext, px,
};

fn centered() -> ParagraphFormat {
    ParagraphFormat {
        alignment: Some(ParagraphAlignment::Center),
        space_before: px(6.),
        ..Default::default()
    }
}

fn indented() -> ParagraphFormat {
    ParagraphFormat {
        leading_indent: px(20.),
        ..Default::default()
    }
}

fn multiline(text: &str, cx: &mut TestAppContext) -> Entity<EditableTextState> {
    cx.new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx))
}

/// The format of each paragraph, read at its start.
fn formats(state: &Entity<EditableTextState>, cx: &mut TestAppContext) -> Vec<ParagraphFormat> {
    state.read_with(cx, |state, _| {
        let text = state.text();
        std::iter::once(0)
            .chain(text.match_indices('\n').map(|(index, _)| index + 1))
            .map(|start| state.paragraph_format(start))
            .collect()
    })
}

#[gpux::test]
fn formats_are_set_read_and_cleared_per_paragraph(cx: &mut TestAppContext) {
    let state = multiline("one\ntwo\nthree", cx);
    state.update(cx, |state, cx| {
        // A range touching the first two paragraphs formats both.
        assert!(state.set_paragraph_format(2..5, centered(), cx));
        assert!(!state.set_paragraph_format(2..5, centered(), cx));
    });
    assert_eq!(
        formats(&state, cx),
        [centered(), centered(), ParagraphFormat::default()]
    );
    state.update(cx, |state, cx| {
        assert_eq!(state.paragraph_format(6), centered());
        assert!(state.update_paragraph_format(9..9, |format| format.leading_indent = px(20.), cx));
        assert!(state.clear_paragraph_format(0..1, cx));
    });
    assert_eq!(
        formats(&state, cx),
        [ParagraphFormat::default(), centered(), indented()]
    );
}

#[gpux::test]
fn a_single_line_field_has_one_paragraph(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("single line", cx));
    state.update(cx, |state, cx| {
        assert!(state.set_paragraph_format(3..3, indented(), cx));
        assert_eq!(state.paragraph_format(0), indented());
        assert_eq!(state.paragraph_format(11), indented());
    });
}

#[gpux::test]
fn splitting_a_paragraph_gives_both_halves_its_format(cx: &mut TestAppContext) {
    let state = multiline("first\nsecond", cx);
    state.update(cx, |state, cx| {
        assert!(state.set_paragraph_format(0..0, centered(), cx));
        state.set_selection(2, 2, cx);
        assert!(state.insert_newline(cx));
        assert_eq!(state.text(), "fi\nrst\nsecond");
    });
    assert_eq!(
        formats(&state, cx),
        [centered(), centered(), ParagraphFormat::default()]
    );
}

#[gpux::test]
fn joining_paragraphs_keeps_the_first_ones_format(cx: &mut TestAppContext) {
    let state = multiline("first\nsecond\nthird", cx);
    state.update(cx, |state, cx| {
        assert!(state.set_paragraph_format(0..0, centered(), cx));
        assert!(state.set_paragraph_format(6..6, indented(), cx));
        // Backspace at the start of "second" joins it to "first".
        state.set_selection(6, 6, cx);
        assert!(state.delete_backward(cx));
        assert_eq!(state.text(), "firstsecond\nthird");
    });
    assert_eq!(
        formats(&state, cx),
        [centered(), ParagraphFormat::default()]
    );
}

#[gpux::test]
fn deleting_across_paragraphs_keeps_the_first_ones_format(cx: &mut TestAppContext) {
    let state = multiline("aa\nbb\ncc\ndd", cx);
    state.update(cx, |state, cx| {
        assert!(state.set_paragraph_format(3..3, indented(), cx));
        assert!(state.set_paragraph_format(6..6, centered(), cx));
        assert!(state.set_paragraph_format(9..9, centered(), cx));
        state.set_selection(4, 7, cx);
        assert!(state.delete_backward(cx));
        assert_eq!(state.text(), "aa\nbc\ndd");
    });
    assert_eq!(
        formats(&state, cx),
        [ParagraphFormat::default(), indented(), centered()]
    );
}

#[gpux::test]
fn pasted_lines_take_the_format_of_the_paragraph_they_land_in(cx: &mut TestAppContext) {
    let state = multiline("head\ntail", cx);
    state.update(cx, |state, cx| {
        assert!(state.set_paragraph_format(0..0, centered(), cx));
        state.set_selection(2, 2, cx);
    });
    cx.write_to_clipboard(ClipboardItem::new_string("x\ny\nz".into()));
    state.update(cx, |state, cx| {
        assert!(state.paste(cx));
        assert_eq!(state.text(), "hex\ny\nzad\ntail");
    });
    assert_eq!(
        formats(&state, cx),
        [
            centered(),
            centered(),
            centered(),
            ParagraphFormat::default()
        ]
    );
}

#[gpux::test]
fn undo_restores_paragraph_formats(cx: &mut TestAppContext) {
    let state = multiline("one\ntwo", cx);
    state.update(cx, |state, cx| {
        let revision = state.revision();
        assert!(state.set_paragraph_format(4..4, indented(), cx));
        // A format change is formatting-only.
        assert_eq!(state.revision(), revision);
        assert_eq!(state.format_revision(), 1);
        state.set_selection(3, 4, cx);
        assert!(state.delete_backward(cx));
        assert_eq!(state.text(), "onetwo");
        assert_eq!(state.paragraph_format(0), ParagraphFormat::default());
        assert!(state.undo(cx));
        assert_eq!(state.text(), "one\ntwo");
        assert_eq!(state.paragraph_format(4), indented());
        assert!(state.undo(cx));
        assert_eq!(state.paragraph_format(4), ParagraphFormat::default());
        assert!(state.redo(cx));
        assert_eq!(state.paragraph_format(4), indented());
    });
}

#[gpux::test]
fn attributed_text_carries_paragraph_formats(cx: &mut TestAppContext) {
    let state = multiline("one\ntwo\nthree", cx);
    state.update(cx, |state, cx| {
        assert!(state.set_paragraph_format(4..4, centered(), cx));
    });
    let snapshot = state.read_with(cx, |state, _| state.attributed_text());
    assert_eq!(
        snapshot.paragraphs,
        [
            ParagraphFormat::default(),
            centered(),
            ParagraphFormat::default()
        ]
    );
    let copy = multiline("", cx);
    copy.update(cx, |copy, cx| {
        assert!(copy.set_attributed_text(snapshot.clone(), cx));
        assert_eq!(copy.attributed_text(), snapshot);
    });
    // A single-line field flattens the breaks and keeps the first paragraph's format.
    let single = cx.new(EditableTextState::new);
    single.update(cx, |single, cx| {
        assert!(single.set_attributed_text(
            AttributedText {
                text: "a\nb".into(),
                paragraphs: vec![indented(), centered()],
                ..Default::default()
            },
            cx,
        ));
        assert_eq!(single.text(), "a b");
        assert_eq!(single.attributed_text().paragraphs, [indented()]);
    });
}

/// Found by review: a range ending where a paragraph starts does not reach into it.
#[gpux::test]
fn a_range_ending_at_a_paragraph_start_leaves_that_paragraph(cx: &mut TestAppContext) {
    let state = multiline("one\ntwo", cx);
    state.update(cx, |state, cx| {
        assert!(state.set_paragraph_format(0..4, centered(), cx));
    });
    assert_eq!(
        formats(&state, cx),
        [centered(), ParagraphFormat::default()]
    );
}
