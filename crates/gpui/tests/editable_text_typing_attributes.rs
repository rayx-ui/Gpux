//! Where inserted text gets its attributes (ATTR-2): the character before, the character after
//! at a paragraph start, the paragraph's last attributes when it is empty, explicit typing
//! attributes until the caret moves or focus changes, IME compositions, and links and tags whose
//! ends are exclusive.

use gpux::{
    AppContext as _, AttributeChange, AttributeState, Context, EditableText, EditableTextLineMode,
    EditableTextState, Entity, FontWeight, IntoElement, Render, SharedString, TestApp,
    TestAppContext, TextAttributes, TextAttributesPatch, Window, div, prelude::*,
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

fn is_bold(attributes: &TextAttributes) -> bool {
    attributes.font_weight == Some(FontWeight::BOLD)
}

fn multiline(text: &str, cx: &mut TestAppContext) -> Entity<EditableTextState> {
    cx.new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx))
}

fn type_at(state: &Entity<EditableTextState>, offset: usize, text: &str, cx: &mut TestAppContext) {
    state.update(cx, |state, cx| {
        state.set_selection(offset, offset, cx);
        assert!(state.replace_selection(text, cx));
    });
}

fn bold_state(
    state: &Entity<EditableTextState>,
    range: std::ops::Range<usize>,
    cx: &mut TestAppContext,
) -> AttributeState {
    state.read_with(cx, |state, _| state.attribute_state(range, is_bold))
}

#[gpux::test]
fn typing_takes_the_attributes_of_the_character_before(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("ab cd", cx));
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(0..2, &set_bold(), cx))
    });
    // At the bold run's end the new text is bold; after the plain space it is plain.
    type_at(&state, 2, "X", cx);
    type_at(&state, 4, "Y", cx);
    state.read_with(cx, |state, _| assert_eq!(state.text(), "abX Ycd"));
    assert_eq!(bold_state(&state, 0..3, cx), AttributeState::On);
    assert_eq!(bold_state(&state, 3..7, cx), AttributeState::Off);
}

#[gpux::test]
fn a_paragraph_start_takes_the_character_after(cx: &mut TestAppContext) {
    let state = multiline("plain\nBOLD", cx);
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(6..10, &set_bold(), cx))
    });
    type_at(&state, 6, "x", cx);
    // Before the first character of the text, too.
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(0..5, &set_bold(), cx))
    });
    type_at(&state, 0, "y", cx);
    state.read_with(cx, |state, _| assert_eq!(state.text(), "yplain\nxBOLD"));
    assert_eq!(bold_state(&state, 0..1, cx), AttributeState::On);
    assert_eq!(bold_state(&state, 7..8, cx), AttributeState::On);
}

#[gpux::test]
fn an_empty_paragraph_takes_its_last_attributes(cx: &mut TestAppContext) {
    let state = multiline("plain\nBOLD", cx);
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(6..10, &set_bold(), cx));
        // Enter at the end of bold text: the new empty paragraph stays bold.
        state.set_selection(10, 10, cx);
        assert!(state.insert_newline(cx));
        assert!(state.replace_selection("n", cx));
        // Deleting all of a bold paragraph leaves its attributes for the next insertion.
        state.set_selection(6, 10, cx);
        assert!(state.delete_backward(cx));
        assert_eq!(state.text(), "plain\n\nn");
        assert!(state.typing_attributes().is_some_and(is_bold));
        assert!(state.replace_selection("again", cx));
        assert_eq!(state.text(), "plain\nagain\nn");
    });
    assert_eq!(bold_state(&state, 6..11, cx), AttributeState::On);
    assert_eq!(bold_state(&state, 12..13, cx), AttributeState::On);
    assert_eq!(bold_state(&state, 0..5, cx), AttributeState::Off);
}

#[gpux::test]
fn explicit_typing_attributes_apply_to_the_next_insertion(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("ab", cx));
    state.update(cx, |state, cx| {
        state.set_selection(1, 1, cx);
        // Bold toggled with no selection applies to what is typed next.
        assert!(state.set_attributes(1..1, &set_bold(), cx));
        assert_eq!(state.attribute_state(1..1, is_bold), AttributeState::On);
        assert!(state.replace_selection("X", cx));
        assert!(state.replace_selection("Y", cx));
        assert_eq!(state.text(), "aXYb");
        assert_eq!(state.attribute_state(1..3, is_bold), AttributeState::On);
        assert_eq!(state.attribute_state(3..4, is_bold), AttributeState::Off);

        // Moving the caret discards them.
        state.set_typing_attributes(bold(), cx);
        state.move_to_end(cx);
        assert!(state.typing_attributes().is_none());
        assert!(state.replace_selection("Z", cx));
        assert_eq!(state.attribute_state(4..5, is_bold), AttributeState::Off);
    });
}

struct Field {
    state: Entity<EditableTextState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .h_full()
            .child(EditableText::new(&self.state))
    }
}

#[test]
fn a_focus_change_discards_typing_attributes() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| Field {
        state: cx.new(EditableTextState::new),
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    window.update(|field, window, cx| {
        let handle = field.state.read(cx).focus_handle();
        window.focus(&handle, cx);
    });
    window.draw();
    app.update_entity(&state, |state, cx| state.set_typing_attributes(bold(), cx));
    window.update(|_, window, cx| window.blur(cx));
    window.draw();
    app.run_until_parked();
    assert!(app.read_entity(&state, |state, _| state.typing_attributes().is_none()));
}

#[gpux::test]
fn a_composition_keeps_its_attributes_through_commit(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("ab", cx));
    state.update(cx, |state, cx| {
        state.set_selection(1, 1, cx);
        state.set_typing_attributes(bold(), cx);
        assert!(state.replace_and_mark(None, "に", None, cx));
        assert!(state.replace_and_mark(None, "にほ", None, cx));
        assert_eq!(state.attribute_state(1..7, is_bold), AttributeState::On);
        // Commit through the platform input path replaces the marked text.
        assert!(state.replace_utf16_range(None, "日本", cx));
        assert_eq!(state.text(), "a日本b");
        assert_eq!(state.attribute_state(1..7, is_bold), AttributeState::On);
        assert_eq!(state.attribute_state(7..8, is_bold), AttributeState::Off);
    });
}

#[gpux::test]
fn links_and_tags_do_not_extend_past_their_ends(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("see docs now", cx));
    let link = TextAttributesPatch {
        link: AttributeChange::Set(SharedString::from("https://gpux.dev")),
        tag: AttributeChange::Set(SharedString::from("ref-1")),
        font_weight: AttributeChange::Set(FontWeight::BOLD),
        ..Default::default()
    };
    state.update(cx, |state, cx| {
        assert!(state.set_attributes(4..8, &link, cx))
    });
    // Typing at the link's end keeps its other formatting but not the link or tag.
    type_at(&state, 8, "!", cx);
    // Typing inside the link extends it.
    type_at(&state, 6, "-", cx);
    state.read_with(cx, |state, _| {
        assert_eq!(state.text(), "see do-cs! now");
        let end = state.attributes_at(9);
        assert!(is_bold(&end));
        assert_eq!((end.link, end.tag), (None, None));
        assert_eq!(
            state.attributes_at(6).link.as_deref(),
            Some("https://gpux.dev")
        );
        assert_eq!(state.attributes_at(6).tag.as_deref(), Some("ref-1"));
        assert!(state.attributes_at(3).link.is_none());
    });
}
