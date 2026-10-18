//! List editing defaults (LIST-2): Enter splits an item or ends the list in an empty one,
//! Backspace at an item's start removes its list format before any join, Tab and Shift-Tab
//! change its level; each is one undo step and a public state method, the key table uses them
//! unless the state switches them off, and Tab outside a list propagates.

use gpux::{
    AppContext as _, Context, EditableText, EditableTextLineMode, EditableTextState, Entity,
    IntoElement, KeyDownEvent, ParagraphFormat, Render, TestApp, TestAppWindow, TextListItem,
    TextListKind, Window, div, prelude::*, px,
};
use std::{cell::Cell, rc::Rc};

struct Field {
    state: Entity<EditableTextState>,
    propagated_tabs: Rc<Cell<usize>>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.propagated_tabs.clone();
        div()
            .w(px(300.))
            .h(px(300.))
            .on_key_down(move |event: &KeyDownEvent, _, _| {
                if event.keystroke.key == "tab" {
                    tabs.set(tabs.get() + 1);
                }
            })
            .child(EditableText::new(&self.state))
    }
}

/// A field whose first paragraph is a bullet item, focused, with the caret at `caret`.
fn open(
    text: &str,
    caret: usize,
) -> (
    TestApp,
    TestAppWindow<Field>,
    Entity<EditableTextState>,
    Rc<Cell<usize>>,
) {
    let mut app = TestApp::new();
    let text = text.to_owned();
    let tabs = Rc::new(Cell::new(0));
    let propagated_tabs = tabs.clone();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx.new(|cx| {
            let mut state =
                EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx);
            state.set_paragraph_format(
                0..0,
                ParagraphFormat {
                    list: Some(TextListItem::bullet(0)),
                    ..Default::default()
                },
                cx,
            );
            state
        }),
        propagated_tabs,
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    window.update(|field, window, cx| {
        let handle = field.state.read(cx).focus_handle();
        window.focus(&handle, cx);
    });
    app.update_entity(&state, |state, cx| {
        state.set_selection(caret, caret, cx);
    });
    window.draw();
    (app, window, state, tabs)
}

/// Each paragraph's list item.
fn items(app: &TestApp, state: &Entity<EditableTextState>) -> Vec<Option<TextListItem>> {
    app.read_entity(state, |state, _| {
        let text = state.text();
        std::iter::once(0)
            .chain(text.match_indices('\n').map(|(index, _)| index + 1))
            .map(|start| state.paragraph_format(start).list)
            .collect()
    })
}

fn text(app: &TestApp, state: &Entity<EditableTextState>) -> String {
    app.read_entity(state, |state, _| state.text())
}

#[test]
fn enter_in_an_item_splits_it_into_two_items() {
    let (mut app, mut window, state, _) = open("first item\nafter", 5);
    window.simulate_keystroke("enter");
    assert_eq!(text(&app, &state), "first\n item\nafter");
    assert_eq!(
        items(&app, &state),
        [
            Some(TextListItem::bullet(0)),
            Some(TextListItem::bullet(0)),
            None
        ]
    );
    app.update_entity(&state, |state, cx| assert!(state.undo(cx)));
    assert_eq!(text(&app, &state), "first item\nafter");
    assert_eq!(items(&app, &state), [Some(TextListItem::bullet(0)), None]);
}

#[test]
fn enter_in_an_empty_item_ends_the_list() {
    let (mut app, mut window, state, _) = open("\nafter", 0);
    window.simulate_keystroke("enter");
    assert_eq!(text(&app, &state), "\nafter");
    assert_eq!(items(&app, &state), [None, None]);
    app.update_entity(&state, |state, cx| assert!(state.undo(cx)));
    assert_eq!(items(&app, &state), [Some(TextListItem::bullet(0)), None]);
}

#[test]
fn a_new_checkbox_item_starts_unchecked() {
    let (mut app, mut window, state, _) = open("done", 4);
    app.update_entity(&state, |state, cx| {
        state.update_paragraph_format(
            0..0,
            |format| {
                format.list = Some(TextListItem {
                    kind: TextListKind::Checkbox { checked: true },
                    level: 1,
                })
            },
            cx,
        );
        assert!(state.list_insert_newline(cx));
        assert_eq!(state.text(), "done\n");
    });
    window.draw();
    assert_eq!(
        items(&app, &state),
        [
            Some(TextListItem {
                kind: TextListKind::Checkbox { checked: true },
                level: 1,
            }),
            Some(TextListItem {
                kind: TextListKind::Checkbox { checked: false },
                level: 1,
            }),
        ]
    );
    // One undo step removes the new item and its line break together.
    app.update_entity(&state, |state, cx| {
        assert!(state.undo(cx));
        assert_eq!(state.text(), "done");
    });
}

#[test]
fn backspace_at_an_items_start_removes_its_list_format_before_joining() {
    let (mut app, mut window, state, _) = open("above\nitem", 6);
    app.update_entity(&state, |state, cx| {
        state.set_paragraph_format(
            6..6,
            ParagraphFormat {
                list: Some(TextListItem::numbered(0)),
                ..Default::default()
            },
            cx,
        );
        state.set_selection(6, 6, cx);
    });
    window.simulate_keystroke("backspace");
    assert_eq!(text(&app, &state), "above\nitem");
    assert_eq!(items(&app, &state), [Some(TextListItem::bullet(0)), None]);
    window.simulate_keystroke("backspace");
    assert_eq!(text(&app, &state), "aboveitem");
}

#[test]
fn tab_and_shift_tab_change_the_level() {
    let (app, mut window, state, tabs) = open("item", 2);
    window.simulate_keystroke("tab");
    assert_eq!(items(&app, &state), [Some(TextListItem::bullet(1))]);
    window.simulate_keystroke("shift-tab");
    window.simulate_keystroke("shift-tab");
    // Shift-Tab at level 0 removes the list format.
    assert_eq!(items(&app, &state), [None]);
    assert_eq!(tabs.get(), 0);
    // Inside a list item, Tab never reached the text.
    assert_eq!(text(&app, &state), "item");
    // Outside a list Tab propagates, and reaches text input, as before.
    window.simulate_keystroke("tab");
    assert_eq!(tabs.get(), 1);
    assert_eq!(text(&app, &state), "it\tem");
}

#[test]
fn each_default_is_one_undo_step() {
    let (mut app, _window, state, _) = open("item", 4);
    app.update_entity(&state, |state, cx| {
        assert!(state.indent_list_item(cx));
        assert!(state.indent_list_item(cx));
        assert!(state.outdent_list_item(cx));
        assert_eq!(
            state.paragraph_format(0).list,
            Some(TextListItem::bullet(1))
        );
        assert!(state.undo(cx));
        assert_eq!(
            state.paragraph_format(0).list,
            Some(TextListItem::bullet(2))
        );
        assert!(state.undo(cx));
        assert!(state.undo(cx));
        assert_eq!(
            state.paragraph_format(0).list,
            Some(TextListItem::bullet(0))
        );
        // Outside a list the methods do nothing.
        state.set_paragraph_format(0..0, ParagraphFormat::default(), cx);
        assert!(!state.indent_list_item(cx));
        assert!(!state.list_insert_newline(cx));
        assert!(!state.list_delete_backward(cx));
    });
}

#[test]
fn owners_can_switch_the_defaults_off() {
    let (mut app, mut window, state, tabs) = open("item", 4);
    app.update_entity(&state, |state, _| state.set_list_editing(false));
    window.simulate_keystroke("enter");
    window.simulate_keystroke("tab");
    assert_eq!(text(&app, &state), "item\n\t");
    // The plain newline splits the paragraph, and both halves keep the item.
    assert_eq!(
        items(&app, &state),
        [Some(TextListItem::bullet(0)), Some(TextListItem::bullet(0))]
    );
    assert_eq!(tabs.get(), 1);
    app.update_entity(&state, |state, _| assert!(!state.list_editing()));
}

/// Found by review: Enter at the start of a checked item leaves the new, empty item unchecked
/// and the item's text checked.
#[test]
fn enter_at_the_start_of_a_checked_item_keeps_its_state() {
    let (mut app, _window, state, _) = open("done", 0);
    let checked = |checked| {
        Some(TextListItem {
            kind: TextListKind::Checkbox { checked },
            level: 0,
        })
    };
    app.update_entity(&state, |state, cx| {
        state.update_paragraph_format(0..0, |format| format.list = checked(true), cx);
        state.set_selection(0, 0, cx);
        assert!(state.list_insert_newline(cx));
        assert_eq!(state.text(), "\ndone");
    });
    assert_eq!(items(&app, &state), [checked(false), checked(true)]);
    app.update_entity(&state, |state, cx| {
        assert!(state.undo(cx));
        assert_eq!(state.text(), "done");
        assert_eq!(state.paragraph_format(0).list, checked(true));
    });
}
