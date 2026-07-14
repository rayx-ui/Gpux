use gpux::{
    AppContext as _, ClipboardItem, Context, EditableSelection, EditableText, EditableTextEvent,
    EditableTextMode, EditableTextState, Entity, IntoElement, MouseButton, Render, TestApp,
    TestAppContext, Window, div, point, prelude::*, px,
};
use std::{cell::Cell, rc::Rc, time::Duration};

#[gpux::test]
fn editable_text_accepts_neutral_paint_colors(cx: &mut TestAppContext) {
    let state = cx.new(EditableTextState::new);
    let _element = EditableText::new(&state)
        .placeholder_color(gpux::red().into())
        .selection_color(gpux::green().into())
        .caret_color(gpux::blue().into());
}

#[test]
fn editable_text_paint_uses_inherited_text_style_and_injected_colors() {
    let source = include_str!("../src/elements/editable_text/element.rs");

    assert!(source.contains("let style = window.text_style();"));
    assert!(source.contains("font: style.font()"));
    assert!(source.contains("style.color.clone()"));
    assert!(source.contains("placeholder_color"));
    assert!(source.contains("selection_color"));
    assert!(source.contains("caret_color"));
    assert!(!source.contains("hsla("));
    assert!(!source.contains("rgba("));
    assert!(!source.contains("crate::blue()"));
}

struct EditableTextHarness {
    state: Entity<EditableTextState>,
}

#[test]
fn editable_text_pointer_down_focuses_before_platform_input() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| EditableTextHarness {
        state: cx.new(EditableTextState::new),
    });
    let state = window.read(|harness, _| harness.state.clone());

    window.draw();
    let bounds = app.read_entity(&state, |state, _| state.geometry().unwrap().bounds());
    window.simulate_mouse_down(bounds.center(), MouseButton::Left);
    window.simulate_mouse_up(bounds.center(), MouseButton::Left);

    assert!(app.read_entity(&state, |state, _| state.is_focused()));
    assert!(window.update(|_, window, cx| window.dispatch_text_input("focused", cx)));
    assert_eq!(app.read_entity(&state, |state, _| state.text()), "focused");

    window.simulate_keystroke("ctrl-a");
    assert!(window.update(|_, window, cx| window.dispatch_text_input("replaced", cx)));
    assert_eq!(app.read_entity(&state, |state, _| state.text()), "replaced");
}

impl Render for EditableTextHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().p(px(20.)).child(EditableText::new(&self.state))
    }
}

#[test]
fn editable_selection_preserves_direction_while_exposing_a_normalized_range() {
    let forward = EditableSelection::new(2, 7);
    assert_eq!(forward.anchor(), 2);
    assert_eq!(forward.head(), 7);
    assert_eq!(forward.range(), 2..7);
    assert!(!forward.is_reversed());

    let backward = EditableSelection::new(7, 2);
    assert_eq!(backward.range(), 2..7);
    assert!(backward.is_reversed());
}

#[gpux::test]
fn editable_text_moves_and_deletes_by_grapheme(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("a👨‍👩‍👧‍👦e\u{301}中", cx));

    state.update(cx, |state, cx| {
        state.move_to_end(cx);
        state.move_left(false, cx);
        assert_eq!(state.selection().head(), "a👨‍👩‍👧‍👦e\u{301}".len());
        state.delete_backward(cx);
        assert_eq!(state.text(), "a👨‍👩‍👧‍👦中");
        state.delete_backward(cx);
        assert_eq!(state.text(), "a中");
        state.move_left(true, cx);
        assert_eq!(state.selection().range(), 0..1);
        assert!(state.selection().is_reversed());
    });
}

#[gpux::test]
fn editable_text_groups_typing_and_restores_selection_on_undo_redo(cx: &mut TestAppContext) {
    let state = cx.new(EditableTextState::new);

    state.update(cx, |state, cx| {
        state.replace_selection("a", cx);
        state.replace_selection("b", cx);
        state.replace_selection("c", cx);
        assert_eq!(state.text(), "abc");
        assert!(state.undo(cx));
        assert_eq!(state.text(), "");
        assert_eq!(state.selection().range(), 0..0);
        assert!(state.redo(cx));
        assert_eq!(state.text(), "abc");
        assert_eq!(state.selection().range(), 3..3);

        assert!(state.undo(cx));
        state.replace_selection("z", cx);
        assert!(!state.redo(cx));
    });
}

#[gpux::test]
fn editable_text_clipboard_and_modes_preserve_single_line_policy(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("copy me", cx));

    state.update(cx, |state, cx| {
        state.set_selection(0, 4, cx);
        assert!(state.copy(cx));
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("copy".into())
        );

        state.set_mode(EditableTextMode::ReadOnly, cx);
        assert!(!state.cut(cx));
        assert_eq!(state.text(), "copy me");
        assert_eq!(state.selection().range(), 0..4);

        state.set_mode(EditableTextMode::Editable, cx);
        cx.write_to_clipboard(ClipboardItem::new_string("one\r\ntwo\nthree".into()));
        assert!(state.paste(cx));
        assert_eq!(state.text(), "one two three me");

        state.set_mode(EditableTextMode::Disabled, cx);
        state.select_all(cx);
        assert!(!state.cut(cx));
        state.replace_selection("blocked", cx);
        assert_eq!(state.text(), "one two three me");
    });
}

#[gpux::test]
fn editable_text_marked_ranges_use_relative_utf16_selection(cx: &mut TestAppContext) {
    let state = cx.new(|cx| EditableTextState::with_text("A中Z", cx));

    state.update(cx, |state, cx| {
        state.set_selection(1, 4, cx);
        assert!(state.replace_and_mark(None, "🎉x", Some(2..3), cx));
        assert_eq!(state.text(), "A🎉xZ");
        assert_eq!(state.marked_range(), Some(1..6));
        assert_eq!(state.selection().range(), 5..6);

        let before = state.snapshot();
        assert!(!state.replace_utf16_range(Some(2..3), "invalid", cx));
        assert_eq!(state.snapshot(), before);
        assert_eq!(state.marked_range(), Some(1..6));
    });
}

#[test]
fn editable_text_element_installs_input_geometry_focus_blink_and_change_events() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| EditableTextHarness {
        state: cx.new(EditableTextState::new),
    });
    let state = window.read(|harness, _| harness.state.clone());
    let changes = Rc::new(Cell::new(0));
    let observed_changes = changes.clone();
    let _subscription = app.update(|cx| {
        cx.subscribe(&state, move |_, event: &EditableTextEvent, _| {
            assert!(matches!(event, EditableTextEvent::Changed { .. }));
            observed_changes.set(observed_changes.get() + 1);
        })
    });

    window.draw();
    window.update(|harness, window, cx| {
        let handle = harness.state.read(cx).focus_handle();
        window.focus(&handle, cx);
    });
    window.draw();

    assert!(window.update(|_, window, cx| window.dispatch_text_input("A🎉", cx)));
    window.draw();
    assert_eq!(
        window.update(|_, window, cx| window.text_input_value(cx)),
        Some("A🎉".into())
    );
    assert_eq!(changes.get(), 1);
    assert!(app.read_entity(&state, |state, _| state.geometry().is_some()));
    assert!(app.read_entity(&state, |state, _| state.is_focused()));
    assert!(app.read_entity(&state, |state, _| state.caret_visible()));

    let geometry_bounds = app.read_entity(&state, |state, _| state.geometry().unwrap().bounds());
    window.simulate_click(
        point(
            geometry_bounds.right() + px(20.),
            geometry_bounds.top() + px(1.),
        ),
        MouseButton::Left,
    );
    assert_eq!(
        app.read_entity(&state, |state, _| state.selection().head()),
        "A🎉".len()
    );

    app.advance_clock(Duration::from_millis(500));
    app.run_until_parked();
    assert!(!app.read_entity(&state, |state, _| state.caret_visible()));

    app.update_entity(&state, |state, cx| {
        state.set_mode(EditableTextMode::ReadOnly, cx);
        state.select_all(cx);
        assert!(state.copy(cx));
    });
    window.draw();
    assert!(app.read_entity(&state, |state, _| state.is_focused()));
    assert!(!window.update(|_, window, cx| window.dispatch_text_input("blocked", cx)));
    assert_eq!(app.read_entity(&state, |state, _| state.text()), "A🎉");

    app.update_entity(&state, |state, cx| {
        state.set_mode(EditableTextMode::Disabled, cx);
    });
    window.draw();
    assert!(!app.read_entity(&state, |state, _| state.is_focused()));
    assert!(!window.update(|_, window, cx| window.dispatch_text_input("blocked", cx)));
    assert_eq!(app.read_entity(&state, |state, _| state.text()), "A🎉");
}
