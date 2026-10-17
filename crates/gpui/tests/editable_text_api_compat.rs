//! RayX-facing compatibility (API-1): the public items RayX uses keep compiling with the same
//! meaning after rich editable text. Editable-text constructors and methods, irrefutable
//! `EditableTextEvent::Changed` destructuring, `TextLayoutMap` and geometry accessors,
//! `TextRun { .., ..Default::default() }`, `ClipboardString` literals and exhaustive `TextAlign`
//! matches.

use gpux::{
    AppContext as _, AttributeChange, ClipboardEntry, ClipboardItem, ClipboardString, Context,
    EditableText, EditableTextEvent, EditableTextLineMode, EditableTextMode, EditableTextState,
    EditableTextStyleRun, EditableTextStyleRuns, Entity, FontWeight, HighlightStyle, IntoElement,
    Render, TestApp, TestAppWindow, TextAlign, TextAttributesPatch, TextRun, Window, black, div,
    font, prelude::*, px,
};
use std::{cell::RefCell, rc::Rc};

struct Field {
    state: Entity<EditableTextState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(240.))
            .h(px(120.))
            .child(EditableText::new(&self.state))
    }
}

fn open(
    build: impl FnOnce(&mut Context<EditableTextState>) -> EditableTextState + 'static,
) -> (TestApp, TestAppWindow<Field>, Entity<EditableTextState>) {
    let mut app = TestApp::new();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx.new(build),
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    (app, window, state)
}

#[test]
fn constructors_and_editing_methods_keep_their_meaning() {
    let (mut app, _window, state) = open(|cx| EditableTextState::with_text("abc", cx));
    app.update_entity(&state, |state, cx| {
        assert_eq!(state.line_mode(), EditableTextLineMode::SingleLine);
        assert_eq!(state.text(), "abc");
        state.set_placeholder("Name", cx);
        state.move_to_end(cx);
        state.move_left(false, cx);
        assert_eq!(state.selection().head(), 2);
        state.move_right(true, cx);
        assert_eq!(state.selection().range(), 2..3);
        assert!(state.replace_selection("X", cx));
        assert_eq!(state.text(), "abX");
        assert_eq!(state.snapshot().len(), 3);
        assert!(state.undo(cx));
        assert_eq!(state.text(), "abc");
        // A plain field stores no formatting.
        assert!(state.attributed_text().spans.is_empty());
        assert!(state.attributed_text().paragraphs.is_empty());
    });
    let (app, _window, state) = open(|cx| {
        EditableTextState::with_line_mode("one\ntwo", EditableTextLineMode::MultiLine, cx)
    });
    app.read_entity(&state, |state, _| {
        assert_eq!(state.line_mode(), EditableTextLineMode::MultiLine);
        assert_eq!(state.text(), "one\ntwo");
    });
}

#[test]
fn selection_and_mode_setters_keep_their_meaning() {
    let (mut app, _window, state) = open(|cx| EditableTextState::with_text("abcd", cx));
    app.update_entity(&state, |state, cx| {
        assert!(state.set_selection(1, 3, cx));
        assert_eq!(state.selection().range(), 1..3);
        assert_eq!(state.selection().head(), 3);
        state.set_mode(EditableTextMode::ReadOnly, cx);
        assert_eq!(state.mode(), EditableTextMode::ReadOnly);
        assert!(!state.replace_selection("x", cx));
        assert_eq!(state.text(), "abcd");
        state.set_mode(EditableTextMode::Editable, cx);
        assert!(state.replace_selection("x", cx));
        assert_eq!(state.text(), "axd");
    });
}

/// Owner style runs (RayX's code and rich-text editors) stay bound to the buffer revision: runs
/// for another revision are refused, and an edit drops installed runs while stored attributes
/// move with the text.
#[test]
fn owner_style_runs_stay_bound_to_the_revision() {
    let (mut app, _window, state) = open(|cx| EditableTextState::with_text("abcd", cx));
    app.update_entity(&state, |state, cx| {
        let revision = state.revision();
        let bold = HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let runs =
            EditableTextStyleRuns::new(revision, [EditableTextStyleRun::new(0..2, bold.clone())]);
        assert!(state.set_style_runs(runs.clone(), cx).is_ok());
        assert_eq!(state.style_runs().len(), 1);
        assert_eq!(state.style_runs().revision(), revision);
        let stale =
            EditableTextStyleRuns::new(revision + 1, [EditableTextStyleRun::new(0..2, bold)]);
        assert!(state.set_style_runs(stale, cx).is_err());
        assert_eq!(state.style_runs(), &runs);

        assert!(state.set_attributes(
            2..4,
            &TextAttributesPatch {
                font_weight: AttributeChange::Set(FontWeight::BOLD),
                ..Default::default()
            },
            cx
        ));
        state.set_selection(0, 0, cx);
        assert!(state.replace_selection("x", cx));
        assert_eq!(state.style_runs().len(), 0);
        assert_eq!(state.style_runs().revision(), state.revision());
        assert_eq!(state.attributes_at(3).font_weight, Some(FontWeight::BOLD));
    });
}

#[test]
fn the_changed_event_destructures_irrefutably() {
    let (mut app, _window, state) = open(|cx| EditableTextState::with_text("", cx));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let observed = seen.clone();
    let _subscription = app.update(|cx| {
        cx.subscribe(&state, move |_, event: &EditableTextEvent, _| {
            let EditableTextEvent::Changed {
                revision,
                composition_active,
            } = *event;
            observed.borrow_mut().push((revision, composition_active));
        })
    });
    app.update_entity(&state, |state, cx| {
        assert!(state.replace_selection("a", cx));
    });
    let revision = app.read_entity(&state, |state, _| state.revision());
    assert_eq!(*seen.borrow(), [(revision, false)]);
}

#[test]
fn layout_map_and_geometry_accessors_answer_as_before() {
    let (app, _window, state) = open(|cx| {
        EditableTextState::with_line_mode("one\ntwo", EditableTextLineMode::MultiLine, cx)
    });
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        assert_eq!(map.revision(), Some(state.revision()));
        assert!(map.line_height() > px(0.));
        assert_eq!(map.visual_line_count(), 2);
        let geometry = state.geometry().expect("laid out");
        assert!(geometry.bounds().size.width > px(0.));
        assert_eq!(geometry.revision(), state.revision());
        assert_eq!(geometry.scroll_offset(), gpux::point(px(0.), px(0.)));
        assert_eq!(geometry.alignment_offset(), px(0.));
        // Uniform rows: the second row starts one line height down.
        let second = map.position_for_index(4).unwrap();
        assert_eq!(second.y, map.line_height());
        assert_eq!(map.visual_row_range(0), Some(0..3));
        assert_eq!(map.visual_row_range(1), Some(4..7));
    });
}

#[test]
fn text_runs_with_defaults_shape_as_before() {
    let (_app, mut window, _state) = open(|cx| EditableTextState::with_text("", cx));
    window.update(|_, window, _| {
        let run = TextRun {
            len: 2,
            font: font("Lilex"),
            color: black().into(),
            ..Default::default()
        };
        let plain = TextRun {
            len: 2,
            font: font("Lilex"),
            color: black().into(),
            background_color: None,
            underline: None,
            strikethrough: None,
            ..Default::default()
        };
        assert_eq!(run, plain);
        let line = window
            .text_system()
            .shape_line("HH".into(), px(16.), &[run], None);
        let twice = window
            .text_system()
            .shape_line("HH".into(), px(16.), &[plain], None);
        assert!(line.width() > px(0.));
        assert_eq!(line.width(), twice.width());
    });
}

#[test]
fn clipboard_string_literals_read_back_as_text() {
    let literal = ClipboardString {
        text: "- fallback".into(),
        metadata: Some("{not-json".into()),
    };
    let item = ClipboardItem {
        entries: vec![ClipboardEntry::String(literal)],
    };
    assert_eq!(item.text().as_deref(), Some("- fallback"));
    assert_eq!(item.metadata().map(String::as_str), Some("{not-json"));
    assert_eq!(item.html(), None);
}

#[test]
fn text_align_matches_stay_exhaustive() {
    fn offset(align: TextAlign) -> i32 {
        match align {
            TextAlign::Left => 0,
            TextAlign::Center => 1,
            TextAlign::Right => 2,
        }
    }
    assert_eq!(
        [TextAlign::Left, TextAlign::Center, TextAlign::Right].map(offset),
        [0, 1, 2]
    );
    assert_eq!(TextAlign::default(), TextAlign::Left);
}
