//! Rich copy, cut and paste (CLIP-3): copy writes HTML beside the text only when the selection
//! is formatted, with each inline object's HTML fallback or escaped text fallback; paste inserts
//! the HTML's formatting only with rich paste on; every paste, keyboard or platform, is its own
//! undo step; single-line fields flatten paragraphs.

use gpux::{
    AppContext as _, AttributeChange, ClipboardItem, Context, EditableText, EditableTextLineMode,
    EditableTextState, Entity, EntityInputHandler, FontWeight, InlineObject, IntoElement,
    ParagraphFormat, Render, SharedString, TestApp, TestAppWindow, TextAttributesPatch,
    TextListItem, Window, div, prelude::*, px,
};

struct Field {
    state: Entity<EditableTextState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(300.))
            .h(px(300.))
            .child(EditableText::new(&self.state))
    }
}

fn open(
    text: &str,
    line_mode: EditableTextLineMode,
) -> (TestApp, TestAppWindow<Field>, Entity<EditableTextState>) {
    let mut app = TestApp::new();
    let text = text.to_owned();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx.new(|cx| EditableTextState::with_line_mode(text, line_mode, cx)),
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    (app, window, state)
}

fn bold() -> TextAttributesPatch {
    TextAttributesPatch {
        font_weight: AttributeChange::Set(FontWeight::BOLD),
        ..Default::default()
    }
}

fn clipboard(app: &mut TestApp) -> ClipboardItem {
    app.update(|cx| cx.read_from_clipboard())
        .expect("the clipboard holds an item")
}

fn write_clipboard(app: &mut TestApp, item: ClipboardItem) {
    app.update(|cx| cx.write_to_clipboard(item));
}

fn object(key: &str, text_fallback: &str, html_fallback: Option<&str>) -> InlineObject {
    InlineObject {
        key: SharedString::from(key.to_owned()),
        width: px(20.),
        height: px(10.),
        baseline_offset: px(2.),
        text_fallback: SharedString::from(text_fallback.to_owned()),
        html_fallback: html_fallback.map(|html| SharedString::from(html.to_owned())),
    }
}

#[test]
fn plain_copy_writes_no_html() {
    let (mut app, _window, state) = open("plain words", EditableTextLineMode::MultiLine);
    app.update_entity(&state, |state, cx| {
        state.select_all(cx);
        assert!(state.copy(cx));
    });
    let item = clipboard(&mut app);
    assert_eq!(item.text().as_deref(), Some("plain words"));
    assert_eq!(item.html(), None);
}

#[test]
fn formatted_copy_and_cut_write_html_beside_the_text() {
    let (mut app, _window, state) = open("plain bold", EditableTextLineMode::MultiLine);
    app.update_entity(&state, |state, cx| {
        assert!(state.set_attributes(6..10, &bold(), cx));
        state.select_all(cx);
        assert!(state.copy(cx));
    });
    let item = clipboard(&mut app);
    assert_eq!(item.text().as_deref(), Some("plain bold"));
    let html = item.html().expect("formatted text copies HTML");
    assert!(html.contains("<b>bold</b>"), "{html}");

    // Only the plain part selected: plain text only.
    app.update_entity(&state, |state, cx| {
        state.set_selection(0, 5, cx);
        assert!(state.cut(cx));
        assert_eq!(state.text(), " bold");
    });
    let item = clipboard(&mut app);
    assert_eq!(item.text().as_deref(), Some("plain"));
    assert_eq!(item.html(), None);
}

#[test]
fn a_paragraph_format_alone_makes_copy_rich() {
    let (mut app, _window, state) = open("item", EditableTextLineMode::MultiLine);
    app.update_entity(&state, |state, cx| {
        assert!(state.set_paragraph_format(
            0..0,
            ParagraphFormat {
                list: Some(TextListItem::bullet(0)),
                ..Default::default()
            },
            cx,
        ));
        state.select_all(cx);
        assert!(state.copy(cx));
    });
    let html = clipboard(&mut app).html().map(str::to_owned).unwrap();
    assert!(html.contains("<ul") && html.contains("<li"), "{html}");
}

#[test]
fn objects_copy_their_html_fallback_or_their_escaped_text_fallback() {
    let (mut app, _window, state) = open("ab", EditableTextLineMode::MultiLine);
    app.update_entity(&state, |state, cx| {
        assert!(state.insert_inline_object(
            1..1,
            object("trusted", "[img]", Some("<img alt=\"chart\">")),
            cx
        ));
        let end = state.text().len();
        assert!(state.insert_inline_object(end..end, object("chip", "<chip & co>", None), cx));
        state.select_all(cx);
        assert!(state.copy(cx));
    });
    let item = clipboard(&mut app);
    assert_eq!(item.text().as_deref(), Some("a[img]b<chip & co>"));
    let html = item.html().expect("objects make copy rich");
    assert!(html.contains("<img alt=\"chart\">"), "{html}");
    assert!(html.contains("&lt;chip &amp; co&gt;"), "{html}");
    assert!(!html.contains("<chip"), "{html}");
}

#[test]
fn without_rich_paste_html_pastes_as_plain_text() {
    let (mut app, _window, state) = open("", EditableTextLineMode::MultiLine);
    write_clipboard(
        &mut app,
        ClipboardItem::new_string_with_html("bold".into(), "<b>bold</b>".into()),
    );
    app.update_entity(&state, |state, cx| {
        assert!(!state.rich_paste());
        assert!(state.paste(cx));
        assert_eq!(state.text(), "bold");
        assert_eq!(state.attributes_at(0).font_weight, None);
    });
}

#[test]
fn rich_paste_inserts_the_htmls_formatting() {
    let (mut app, _window, state) = open("x", EditableTextLineMode::MultiLine);
    write_clipboard(
        &mut app,
        ClipboardItem::new_string_with_html(
            "bold\none".into(),
            "<p><b>bold</b></p><ul><li>one</li></ul>".into(),
        ),
    );
    app.update_entity(&state, |state, cx| {
        state.set_rich_paste(true);
        state.set_selection(0, 1, cx);
        assert!(state.paste(cx));
        assert_eq!(state.text(), "bold\none");
        assert_eq!(state.attributes_at(0).font_weight, Some(FontWeight::BOLD));
        assert_eq!(state.attributes_at(5).font_weight, None);
        assert_eq!(
            state.paragraph_format(5).list,
            Some(TextListItem::bullet(0))
        );
        assert_eq!(state.selection().head(), state.text().len());
        // One undo step restores the replaced text.
        assert!(state.undo(cx));
        assert_eq!(state.text(), "x");
        assert_eq!(state.paragraph_format(0), ParagraphFormat::default());
    });
}

#[test]
fn rich_paste_falls_back_to_text_when_the_html_does_not_parse() {
    let (mut app, _window, state) = open("", EditableTextLineMode::MultiLine);
    let huge = format!("<b>{}</b>", "x".repeat(4 * 1024 * 1024 + 1));
    write_clipboard(
        &mut app,
        ClipboardItem::new_string_with_html("fallback".into(), huge),
    );
    app.update_entity(&state, |state, cx| {
        state.set_rich_paste(true);
        assert!(state.paste(cx));
        assert_eq!(state.text(), "fallback");
        assert_eq!(state.attributes_at(0).font_weight, None);
    });
}

#[test]
fn a_keyboard_paste_is_its_own_undo_step_after_typing() {
    let (mut app, _window, state) = open("", EditableTextLineMode::MultiLine);
    write_clipboard(&mut app, ClipboardItem::new_string("cd".into()));
    app.update_entity(&state, |state, cx| {
        assert!(state.replace_selection("a", cx));
        assert!(state.replace_selection("b", cx));
        assert!(state.paste(cx));
        assert!(state.replace_selection("e", cx));
        assert_eq!(state.text(), "abcde");
        assert!(state.undo(cx));
        assert_eq!(state.text(), "abcd");
        assert!(state.undo(cx));
        assert_eq!(state.text(), "ab");
    });
}

#[test]
fn a_platform_paste_is_its_own_undo_step_and_takes_rich_paste() {
    let (_app, mut window, _state) = open("", EditableTextLineMode::MultiLine);
    window.update(|field, window, cx| {
        field.state.update(cx, |state, cx| {
            state.set_rich_paste(true);
            assert!(state.replace_selection("a", cx));
            EntityInputHandler::paste(
                state,
                ClipboardItem::new_string_with_html("b".into(), "<b>b</b>".into()),
                window,
                cx,
            );
            assert_eq!(state.text(), "ab");
            assert_eq!(state.attributes_at(1).font_weight, Some(FontWeight::BOLD));
            assert!(state.undo(cx));
            assert_eq!(state.text(), "a");
        });
    });
}

#[test]
fn a_platform_paste_of_plain_text_is_its_own_undo_step() {
    let (_app, mut window, _state) = open("", EditableTextLineMode::MultiLine);
    window.update(|field, window, cx| {
        field.state.update(cx, |state, cx| {
            assert!(state.replace_selection("a", cx));
            EntityInputHandler::paste(state, ClipboardItem::new_string("b".into()), window, cx);
            assert_eq!(state.text(), "ab");
            assert!(state.undo(cx));
            assert_eq!(state.text(), "a");
        });
    });
}

#[test]
fn single_line_fields_flatten_pasted_paragraphs() {
    let (mut app, _window, state) = open("", EditableTextLineMode::SingleLine);
    write_clipboard(
        &mut app,
        ClipboardItem::new_string_with_html(
            "one\ntwo".into(),
            "<ul><li><b>one</b></li><li>two</li></ul>".into(),
        ),
    );
    app.update_entity(&state, |state, cx| {
        state.set_rich_paste(true);
        assert!(state.paste(cx));
        assert_eq!(state.text(), "one two");
        assert_eq!(state.attributes_at(0).font_weight, Some(FontWeight::BOLD));
        assert_eq!(state.paragraph_format(0), ParagraphFormat::default());
    });
}

#[test]
fn rich_copy_then_rich_paste_keeps_the_formatting() {
    let (mut app, _window, state) = open("plain bold", EditableTextLineMode::MultiLine);
    app.update_entity(&state, |state, cx| {
        assert!(state.set_attributes(6..10, &bold(), cx));
        state.set_selection(6, 10, cx);
        assert!(state.copy(cx));
        state.set_rich_paste(true);
        state.set_selection(0, 0, cx);
        assert!(state.paste(cx));
        assert_eq!(state.text(), "boldplain bold");
        assert_eq!(state.attributes_at(0).font_weight, Some(FontWeight::BOLD));
        assert_eq!(state.attributes_at(4).font_weight, None);
    });
}

/// Found by review: HTML that holds no text (only an image without alt text, or metadata)
/// pastes the plain text instead of deleting the selection.
#[test]
fn rich_paste_of_html_without_text_pastes_the_plain_text() {
    let (mut app, _window, state) = open("keep", EditableTextLineMode::MultiLine);
    write_clipboard(
        &mut app,
        ClipboardItem::new_string_with_html(
            "plain".into(),
            "<meta charset=utf-8><img src=x>".into(),
        ),
    );
    app.update_entity(&state, |state, cx| {
        state.set_rich_paste(true);
        state.select_all(cx);
        assert!(state.paste(cx));
        assert_eq!(state.text(), "plain");
    });
}
