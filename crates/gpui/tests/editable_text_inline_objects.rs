//! Inline objects (OBJ-1): one U+FFFC character carrying an object attribute that reserves the
//! object's width and height in its row, never splits across rows, edits as one character,
//! appears as U+FFFC in the text and IME, copies as its text fallback, and travels in
//! `AttributedText`.

use gpux::{
    AppContext as _, AttributeChange, Context, EditableText, EditableTextLineMode,
    EditableTextState, Entity, EntityInputHandler, FontWeight, InlineObject, IntoElement,
    OBJECT_REPLACEMENT_CHARACTER, Pixels, Render, SharedString, TestApp, TestAppWindow,
    TextAttributesPatch, TextDisplayPoint, Window, div, prelude::*, px,
};

struct Field {
    state: Entity<EditableTextState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(200.))
            .h(px(300.))
            .child(EditableText::new(&self.state))
    }
}

fn open(text: &str) -> (TestApp, TestAppWindow<Field>, Entity<EditableTextState>) {
    let mut app = TestApp::new();
    let text = text.to_owned();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx
            .new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx)),
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    (app, window, state)
}

fn chip(width: f32, height: f32) -> InlineObject {
    InlineObject {
        key: SharedString::from("chip"),
        width: px(width),
        height: px(height),
        baseline_offset: px(4.),
        text_fallback: SharedString::from("[chip]"),
        html_fallback: None,
    }
}

const OBJECT: &str = "\u{FFFC}";

fn insert(
    app: &mut TestApp,
    window: &mut TestAppWindow<Field>,
    state: &Entity<EditableTextState>,
    offset: usize,
    object: InlineObject,
) {
    app.update_entity(state, |state, cx| {
        assert!(state.insert_inline_object(offset..offset, object, cx));
    });
    window.draw();
}

#[track_caller]
fn assert_near(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() < px(0.01),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn an_object_is_one_replacement_character_with_its_attribute() {
    let (mut app, mut window, state) = open("ab");
    insert(&mut app, &mut window, &state, 1, chip(40., 20.));
    app.update_entity(&state, |state, cx| {
        assert_eq!(state.text(), format!("a{OBJECT}b"));
        assert_eq!(state.inline_objects(), [(1, chip(40., 20.))]);
        assert_eq!(OBJECT_REPLACEMENT_CHARACTER.len_utf8(), OBJECT.len());
        // Inserting is one undo step.
        assert!(state.undo(cx));
        assert_eq!(state.text(), "ab");
        assert!(state.inline_objects().is_empty());
    });
}

#[test]
fn an_object_reserves_exactly_its_width_and_height() {
    let (mut app, mut window, state) = open("ab");
    let line_height = app.read_entity(&state, |state, _| state.layout_map().line_height());
    insert(
        &mut app,
        &mut window,
        &state,
        1,
        chip(40., f32::from(line_height) * 2.),
    );
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let before = map.position_for_index(1).unwrap().x;
        let after = map.position_for_index(1 + OBJECT.len()).unwrap().x;
        assert_near(after - before, px(40.));
        // The row is as tall as the object, which stands 4 px below its baseline.
        let row = map.visual_row(0).unwrap();
        assert!(row.height >= line_height * 2.);
        let (_, _, bounds) = &map.inline_object_bounds()[0];
        assert_near(bounds.left(), before);
        assert_near(bounds.size.width, px(40.));
        assert_near(bounds.bottom(), row.top + row.baseline + px(4.));
        assert!(bounds.top() >= row.top - px(0.01));
    });
}

#[test]
fn an_object_never_splits_and_a_row_may_break_before_it() {
    // About 9.6 px glyphs: "aaaa aaaa" fills most of a 200 px row with the object after it.
    let (mut app, mut window, state) = open("aaaa aaaa aaaa ");
    insert(&mut app, &mut window, &state, 15, chip(120., 10.));
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        assert_eq!(map.visual_line_count(), 2);
        // The object starts the second row, whole.
        assert_eq!(map.visual_row_range(1).map(|range| range.start), Some(15));
        let (_, _, bounds) = &map.inline_object_bounds()[0];
        let row = map.visual_row(1).unwrap();
        assert!(bounds.top() >= row.top - px(0.01));
        assert_near(
            bounds.left(),
            map.position_for_display(TextDisplayPoint::new(1, 0))
                .unwrap()
                .x,
        );
    });
}

#[test]
fn caret_selection_and_deletion_treat_an_object_as_one_character() {
    let (mut app, mut window, state) = open("ab");
    insert(&mut app, &mut window, &state, 1, chip(30., 10.));
    app.update_entity(&state, |state, cx| {
        state.set_selection(1, 1, cx);
        state.move_right(false, cx);
        assert_eq!(state.selection().head(), 1 + OBJECT.len());
        state.move_left(true, cx);
        assert_eq!(state.selection().range(), 1..1 + OBJECT.len());
        state.set_selection(1 + OBJECT.len(), 1 + OBJECT.len(), cx);
        assert!(state.delete_backward(cx));
        assert_eq!(state.text(), "ab");
        assert!(state.inline_objects().is_empty());
        assert!(state.undo(cx));
        assert_eq!(state.inline_objects(), [(1, chip(30., 10.))]);
    });
}

#[test]
fn attribute_changes_keep_objects_and_typing_never_takes_them() {
    let (mut app, mut window, state) = open("ab");
    insert(&mut app, &mut window, &state, 1, chip(30., 10.));
    app.update_entity(&state, |state, cx| {
        let bold = TextAttributesPatch {
            font_weight: AttributeChange::Set(FontWeight::BOLD),
            ..Default::default()
        };
        assert!(state.set_attributes(0..state.text().len(), &bold, cx));
        assert_eq!(state.inline_objects(), [(1, chip(30., 10.))]);
        assert_eq!(state.attributes_at(1).font_weight, Some(FontWeight::BOLD));
        // Typing right after the object takes its bold, not the object.
        let after = 1 + OBJECT.len();
        state.set_selection(after, after, cx);
        assert!(state.replace_selection("x", cx));
        assert_eq!(
            state.attributes_at(after).font_weight,
            Some(FontWeight::BOLD)
        );
        assert!(state.attributes_at(after).object.is_none());
        // Replacing the object with text drops it too.
        state.set_selection(1, after, cx);
        assert!(state.replace_selection("y", cx));
        assert!(state.inline_objects().is_empty());
    });
}

#[test]
fn text_and_ime_contain_the_replacement_character() {
    let (mut app, mut window, state) = open("ab");
    insert(&mut app, &mut window, &state, 1, chip(30., 10.));
    let ime = window.update(|field, window, cx| {
        field.state.update(cx, |state, cx| {
            let mut actual = None;
            state.text_for_range(0..3, &mut actual, window, cx)
        })
    });
    assert_eq!(ime, Some(format!("a{OBJECT}b")));
    assert_eq!(
        app.read_entity(&state, |state, _| state.text()),
        format!("a{OBJECT}b")
    );
}

#[test]
fn plain_text_copy_writes_each_objects_text_fallback() {
    let (mut app, mut window, state) = open("ab");
    insert(&mut app, &mut window, &state, 1, chip(30., 10.));
    app.update_entity(&state, |state, cx| {
        state.select_all(cx);
        assert!(state.copy(cx));
    });
    let copied = app.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("a[chip]b"));
}

#[test]
fn attributed_text_carries_objects() {
    let (mut app, mut window, state) = open("ab");
    insert(&mut app, &mut window, &state, 1, chip(30., 10.));
    let snapshot = app.read_entity(&state, |state, _| state.attributed_text());
    assert_eq!(
        snapshot
            .objects()
            .map(|(offset, object)| (offset, object.clone()))
            .collect::<Vec<_>>(),
        [(1, chip(30., 10.))]
    );
    assert_eq!(snapshot.plain_text(), "a[chip]b");
    app.update_entity(&state, |state, cx| {
        state.reset(cx);
        assert!(state.set_attributed_text(snapshot.clone(), cx));
        assert_eq!(state.inline_objects(), [(1, chip(30., 10.))]);
    });
}

/// Found by review: a row may break after an object, not only before it.
#[test]
fn a_row_may_break_after_an_object() {
    let (mut app, mut window, state) = open(&format!("aa {}", "b".repeat(16)));
    insert(&mut app, &mut window, &state, 3, chip(40., 10.));
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let after = 3 + OBJECT.len();
        assert_eq!(map.visual_row_range(0), Some(0..after));
        assert_eq!(
            map.visual_row_range(1).map(|range| range.start),
            Some(after)
        );
        assert_eq!(map.visual_line_count(), 2);
    });
}

/// Found by review: deleting an object that empties its paragraph leaves no object for typing.
#[test]
fn typing_after_deleting_a_lone_object_takes_no_object() {
    let (mut app, mut window, state) = open("x\n");
    insert(&mut app, &mut window, &state, 2, chip(30., 10.));
    app.update_entity(&state, |state, cx| {
        let end = state.text().len();
        state.set_selection(end, end, cx);
        assert!(state.delete_backward(cx));
        assert_eq!(state.text(), "x\n");
        assert!(state.replace_selection("y", cx));
        assert!(state.attributes_at(2).object.is_none());
        assert!(state.inline_objects().is_empty());
    });
}

/// Found by review: a U+FFFC without an object attribute is ordinary text, which paints and
/// reserves no object box.
#[test]
fn a_plain_replacement_character_is_not_an_object() {
    let (mut app, mut window, state) = open("ab");
    app.update_entity(&state, |state, cx| {
        state.set_selection(1, 1, cx);
        assert!(state.replace_selection(OBJECT, cx));
    });
    window.draw();
    app.read_entity(&state, |state, _| {
        assert!(state.inline_objects().is_empty());
        assert!(state.layout_map().inline_object_bounds().is_empty());
    });
}
