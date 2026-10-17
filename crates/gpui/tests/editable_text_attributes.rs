//! Stored character formatting in `EditableTextState` (ATTR-1): spans over byte ranges that stay
//! normalized and move with every edit path, the patch/read/state API and `AttributedText`.

use gpux::{
    AppContext as _, AttributeChange, AttributeSpan, AttributeState, AttributedText, ClipboardItem,
    EditableSelection, EditableTextLineMode, EditableTextState, Entity, FontWeight, TestAppContext,
    TextAttributes, TextAttributesPatch, rgb,
};

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

/// The stored spans as `(range, is_bold)` pairs.
fn spans(
    state: &Entity<EditableTextState>,
    cx: &mut TestAppContext,
) -> Vec<(std::ops::Range<usize>, bool)> {
    state.read_with(cx, |state, _| {
        state
            .attribute_spans()
            .iter()
            .map(|span| {
                (
                    span.range.clone(),
                    span.attributes.font_weight == Some(FontWeight::BOLD),
                )
            })
            .collect()
    })
}

/// "hello world" with "llo w" (2..7) bold.
fn bold_middle(cx: &mut TestAppContext) -> Entity<EditableTextState> {
    let state = cx.new(|cx| EditableTextState::with_text("hello world", cx));
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(2..7, &set_bold(), cx))
    });
    state
}

fn caret(state: &Entity<EditableTextState>, offset: usize, cx: &mut TestAppContext) {
    state.update(cx, |state, cx| {
        state.set_selection(offset, offset, cx);
    });
}

#[gpux::test]
fn a_plain_field_stores_no_formatting(cx: &mut TestAppContext) {
    let state = cx.new(EditableTextState::new);
    state.update(cx, |state, cx| {
        state.replace_selection("plain text", cx);
        state.delete_backward(cx);
        assert!(state.attribute_spans().is_empty());
        assert_eq!(state.attributes_at(0), TextAttributes::default());
        assert_eq!(state.format_revision(), 0);
    });
}

#[gpux::test]
fn patches_set_and_clear_fields_and_spans_stay_normalized(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    assert_eq!(spans(&state, cx), [(2..7, true)]);
    state.update(cx, |state, cx| {
        // An overlapping equal patch merges into one span.
        assert!(state.set_attributes(6..9, &set_bold(), cx));
        // Setting a field another span lacks keeps the bold.
        let red = TextAttributesPatch {
            color: AttributeChange::Set(rgb(0xff0000).into()),
            ..Default::default()
        };
        assert!(state.set_attributes(0..4, &red, cx));
        assert_eq!(state.attributes_at(3).font_weight, Some(FontWeight::BOLD));
        assert!(state.attributes_at(3).color.is_some());
        assert!(state.attributes_at(0).font_weight.is_none());
        // Re-applying the same patch changes nothing.
        assert!(!state.set_attributes(0..4, &red, cx));
        let clear = TextAttributesPatch {
            color: AttributeChange::Clear,
            font_weight: AttributeChange::Clear,
            ..Default::default()
        };
        assert!(state.set_attributes(0..4, &clear, cx));
    });
    assert_eq!(spans(&state, cx), [(4..9, true)]);
    state.update(cx, |state, cx| {
        let clear = TextAttributesPatch {
            font_weight: AttributeChange::Clear,
            ..Default::default()
        };
        assert!(state.set_attributes(0..11, &clear, cx));
        assert!(state.attribute_spans().is_empty());
    });
}

#[gpux::test]
fn insertions_inside_a_span_extend_it_and_before_it_shift_it(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    caret(&state, 4, cx);
    state.update(cx, |state, cx| state.replace_selection("XY", cx));
    assert_eq!(spans(&state, cx), [(2..9, true)]);
    caret(&state, 0, cx);
    state.update(cx, |state, cx| state.replace_selection(">>", cx));
    assert_eq!(spans(&state, cx), [(4..11, true)]);
    state.read_with(cx, |state, _| assert_eq!(state.text(), ">>hellXYo world"));
}

#[gpux::test]
fn deletions_shrink_or_remove_spans(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    // Delete "lo" inside the span.
    state.update(cx, |state, cx| {
        state.set_selection(3, 5, cx);
        state.delete_backward(cx);
    });
    assert_eq!(spans(&state, cx), [(2..5, true)]);
    // Delete across the span's start: "he" + "l".
    state.update(cx, |state, cx| {
        state.set_selection(1, 3, cx);
        state.delete_forward(cx);
    });
    assert_eq!(spans(&state, cx), [(1..3, true)]);
    // Delete the whole rest of the span.
    state.update(cx, |state, cx| {
        state.set_selection(0, 4, cx);
        state.delete_backward(cx);
    });
    assert!(spans(&state, cx).is_empty());
}

#[gpux::test]
fn word_deletes_remap_spans(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    caret(&state, 11, cx);
    state.update(cx, |state, cx| {
        state.delete_word_backward(cx);
    });
    state.read_with(cx, |state, _| assert_eq!(state.text(), "hello "));
    assert_eq!(spans(&state, cx), [(2..6, true)]);
    caret(&state, 3, cx);
    state.update(cx, |state, cx| {
        state.delete_word_forward(cx);
        assert_eq!(state.text(), "hel");
    });
    assert_eq!(spans(&state, cx), [(2..3, true)]);
}

#[gpux::test]
fn replaced_text_takes_the_first_replaced_characters_attributes(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    state.update(cx, |state, cx| {
        state.set_selection(1, 4, cx);
        state.replace_selection("EEE", cx);
    });
    // "h" + "EEE" (plain, like the replaced "e") + "o w" bold.
    assert_eq!(spans(&state, cx), [(4..7, true)]);
    state.update(cx, |state, cx| {
        state.set_selection(5, 9, cx);
        state.replace_selection("Z", cx);
    });
    // " wor" began bold, so its replacement is bold too.
    state.read_with(cx, |state, _| assert_eq!(state.text(), "hEEEoZld"));
    assert_eq!(spans(&state, cx), [(4..6, true)]);
}

#[gpux::test]
fn platform_input_and_ime_remap_spans(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    state.update(cx, |state, cx| {
        // UTF-16 replacement of "wo" (6..8) by "W": "w" was bold, so "W" is.
        assert!(state.replace_utf16_range(Some(6..8), "W", cx));
        assert_eq!(state.text(), "hello Wrld");
    });
    assert_eq!(spans(&state, cx), [(2..7, true)]);
    caret(&state, 4, cx);
    state.update(cx, |state, cx| {
        assert!(state.replace_and_mark(None, "に", None, cx));
        assert!(state.replace_and_mark(None, "にほ", None, cx));
        state.unmark(cx);
        assert_eq!(state.text(), "hellにほo Wrld");
    });
    // The composition sits inside the span and takes its attributes.
    assert_eq!(spans(&state, cx), [(2..13, true)]);
}

#[gpux::test]
fn paste_cut_and_newline_remap_spans(cx: &mut TestAppContext) {
    let state = cx.new(|cx| {
        EditableTextState::with_line_mode("hello world", EditableTextLineMode::MultiLine, cx)
    });
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(2..7, &set_bold(), cx));
        state.set_selection(0, 2, cx);
        assert!(state.cut(cx));
    });
    assert_eq!(spans(&state, cx), [(0..5, true)]);
    cx.write_to_clipboard(ClipboardItem::new_string("AB".into()));
    caret(&state, 3, cx);
    state.update(cx, |state, cx| {
        assert!(state.paste(cx));
        assert!(state.insert_newline(cx));
        assert_eq!(state.text(), "lloAB\n world");
    });
    // Pasted text and the break take the bold of the character before them.
    assert_eq!(spans(&state, cx), [(0..8, true)]);
}

#[gpux::test]
fn rewrite_current_edit_keeps_formatting_outside_the_rewrite(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    caret(&state, 11, cx);
    state.update(cx, |state, cx| {
        state.replace_selection("!", cx);
        // An owner normalizing the edit replaces only "world!" with "WORLD", which takes the
        // bold of the "w" it starts with; "hello " keeps its formatting.
        assert!(state.rewrite_current_edit("hello WORLD", EditableSelection::caret(11), cx));
    });
    assert_eq!(spans(&state, cx), [(2..11, true)]);
    state.update(cx, |state, cx| {
        assert!(state.undo(cx));
        assert_eq!(state.text(), "hello world");
    });
    assert_eq!(spans(&state, cx), [(2..7, true)]);
}

#[gpux::test]
fn reset_clears_formatting(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    state.update(cx, |state, cx| {
        state.reset(cx);
        assert!(state.attribute_spans().is_empty());
        state.replace_selection("new", cx);
        assert!(state.attribute_spans().is_empty());
    });
}

#[gpux::test]
fn attribute_state_reports_on_off_and_mixed(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    state.read_with(cx, |state, _| {
        let is_bold =
            |attributes: &TextAttributes| attributes.font_weight == Some(FontWeight::BOLD);
        assert_eq!(state.attribute_state(2..7, is_bold), AttributeState::On);
        assert_eq!(state.attribute_state(3..5, is_bold), AttributeState::On);
        assert_eq!(state.attribute_state(0..2, is_bold), AttributeState::Off);
        assert_eq!(state.attribute_state(0..5, is_bold), AttributeState::Mixed);
        assert_eq!(state.attribute_state(6..11, is_bold), AttributeState::Mixed);
    });
}

#[gpux::test]
fn attributed_text_round_trips_and_replaces_everything(cx: &mut TestAppContext) {
    let state = bold_middle(cx);
    let snapshot = state.read_with(cx, |state, _| state.attributed_text());
    assert_eq!(snapshot.text, "hello world");
    assert_eq!(snapshot.spans, [AttributeSpan::new(2..7, bold())]);

    let other = cx.new(EditableTextState::new);
    other.update(cx, |other, cx| {
        assert!(other.set_attributed_text(snapshot.clone(), cx));
        assert_eq!(other.attributed_text(), snapshot);
        // Overlapping input spans are normalized; the later one wins.
        assert!(other.set_attributed_text(
            AttributedText {
                text: "abcdef".into(),
                spans: vec![
                    AttributeSpan::new(0..4, bold()),
                    AttributeSpan::new(2..6, TextAttributes::default()),
                    AttributeSpan::new(5..9, bold()),
                ],
                ..Default::default()
            },
            cx,
        ));
        assert_eq!(
            other.attribute_spans(),
            [
                AttributeSpan::new(0..2, bold()),
                AttributeSpan::new(5..6, bold())
            ]
        );
        // Replacing everything is one undo step.
        assert!(other.undo(cx));
        assert_eq!(other.attributed_text(), snapshot);
    });
}

/// Found by review: spans follow their characters through line-break normalization and never
/// split a character.
#[gpux::test]
fn attributed_text_spans_follow_normalized_line_breaks(cx: &mut TestAppContext) {
    let multi =
        cx.new(|cx| EditableTextState::with_line_mode("", EditableTextLineMode::MultiLine, cx));
    multi.update(cx, |state, cx| {
        assert!(state.set_attributed_text(
            AttributedText {
                text: "a\r\nbc\rd".into(),
                spans: vec![
                    AttributeSpan::new(3..5, bold()),
                    AttributeSpan::new(6..7, bold())
                ],
                ..Default::default()
            },
            cx,
        ));
        assert_eq!(state.text(), "a\nbc\nd");
        assert_eq!(
            state.attribute_spans(),
            [
                AttributeSpan::new(2..4, bold()),
                AttributeSpan::new(5..6, bold())
            ]
        );
    });
    let single = cx.new(EditableTextState::new);
    single.update(cx, |state, cx| {
        assert!(state.set_attributed_text(
            AttributedText {
                text: "x\r\ny\nz".into(),
                spans: vec![
                    AttributeSpan::new(3..4, bold()),
                    AttributeSpan::new(5..6, bold())
                ],
                ..Default::default()
            },
            cx,
        ));
        assert_eq!(state.text(), "x y z");
        assert_eq!(
            state.attribute_spans(),
            [
                AttributeSpan::new(2..3, bold()),
                AttributeSpan::new(4..5, bold())
            ]
        );
        // A boundary inside "é" (bytes 1..3) snaps to the character's start.
        assert!(state.set_attributed_text(
            AttributedText {
                text: "aéb".into(),
                spans: vec![AttributeSpan::new(2..4, bold())],
                ..Default::default()
            },
            cx,
        ));
        assert_eq!(state.attribute_spans(), [AttributeSpan::new(1..4, bold())]);
    });
}

/// Found by review: resetting a field whose first character was formatted leaves it plain.
#[gpux::test]
fn reset_leaves_no_typing_attributes(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("bold", cx));
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(0..4, &set_bold(), cx));
        state.reset(cx);
        assert!(state.typing_attributes().is_none());
        state.replace_selection("plain", cx);
        assert!(state.attribute_spans().is_empty());
    });
}
