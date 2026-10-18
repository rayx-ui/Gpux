//! List items (LIST-1): a paragraph's list item moves its leading indent one step per level,
//! paints a marker before its first row (bullets and numbers by level, consecutive numbering,
//! checkboxes that toggle on a click), and its marker never becomes text.

use gpux::{
    AppContext as _, AttributeChange, Context, EditableText, EditableTextLineMode,
    EditableTextState, Entity, EntityInputHandler, IntoElement, MouseButton, ParagraphFormat,
    Pixels, Render, SharedString, TestApp, TestAppWindow, TextAttributesPatch, TextDisplayPoint,
    TextListItem, TextListKind, Window, div, prelude::*, px,
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

/// Makes each paragraph (by index) the given list item, or no item.
fn lists(
    app: &mut TestApp,
    window: &mut TestAppWindow<Field>,
    state: &Entity<EditableTextState>,
    items: &[Option<TextListItem>],
) {
    app.update_entity(state, |state, cx| {
        let text = state.text();
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(index, _)| index + 1))
            .collect::<Vec<_>>();
        for (start, item) in starts.into_iter().zip(items) {
            state.set_paragraph_format(
                start..start,
                ParagraphFormat {
                    list: item.clone(),
                    ..Default::default()
                },
                cx,
            );
        }
    });
    window.draw();
}

fn markers(app: &TestApp, state: &Entity<EditableTextState>) -> Vec<String> {
    app.read_entity(state, |state, _| {
        state
            .layout_map()
            .list_markers()
            .iter()
            .map(|marker| match marker.checkbox {
                Some(checked) => format!("[{}]", if checked { "x" } else { " " }),
                None => marker.text.to_string(),
            })
            .collect()
    })
}

fn row_start(app: &TestApp, state: &Entity<EditableTextState>, row: usize) -> Pixels {
    app.read_entity(state, |state, _| {
        state
            .layout_map()
            .position_for_display(TextDisplayPoint::new(row, 0))
            .unwrap()
            .x
    })
}

#[track_caller]
fn assert_near(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() < px(0.01),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn each_level_moves_the_leading_indent_one_step() {
    let (mut app, mut window, state) = open("a\nb\nc");
    lists(
        &mut app,
        &mut window,
        &state,
        &[
            Some(TextListItem::bullet(0)),
            Some(TextListItem::bullet(1)),
            None,
        ],
    );
    assert_near(row_start(&app, &state, 0), px(24.));
    assert_near(row_start(&app, &state, 1), px(48.));
    assert_near(row_start(&app, &state, 2), px(0.));
    app.update_entity(&state, |state, cx| state.set_list_indent_step(px(30.), cx));
    window.draw();
    assert_near(row_start(&app, &state, 1), px(60.));
}

#[test]
fn bullets_and_numbers_change_by_level() {
    let (mut app, mut window, state) = open("a\nb\nc\nd\ne\nf");
    lists(
        &mut app,
        &mut window,
        &state,
        &[
            Some(TextListItem::bullet(0)),
            Some(TextListItem::bullet(1)),
            Some(TextListItem::bullet(2)),
            Some(TextListItem::numbered(0)),
            Some(TextListItem::numbered(1)),
            Some(TextListItem::numbered(2)),
        ],
    );
    assert_eq!(
        markers(&app, &state),
        ["\u{2022}", "\u{25e6}", "\u{25aa}", "1.", "a.", "i."]
    );
}

#[test]
fn numbering_counts_consecutive_items_at_a_level() {
    let (mut app, mut window, state) = open("a\nb\nc\nd\ne\nf\ng\nh");
    let start_at_4 = TextListItem {
        kind: TextListKind::Numbered { start: 4 },
        level: 0,
    };
    lists(
        &mut app,
        &mut window,
        &state,
        &[
            Some(TextListItem::numbered(0)),
            // Deeper items do not break the count at level 0.
            Some(TextListItem::numbered(1)),
            Some(TextListItem::numbered(1)),
            Some(TextListItem::numbered(0)),
            // Another kind at the level restarts it.
            Some(TextListItem::bullet(0)),
            Some(TextListItem::numbered(0)),
            // So does a paragraph that is not a list item.
            None,
            Some(start_at_4),
        ],
    );
    assert_eq!(
        markers(&app, &state),
        ["1.", "a.", "b.", "2.", "\u{2022}", "1.", "4."]
    );
}

#[test]
fn custom_markers_and_marker_sizes_follow_the_first_character() {
    let (mut app, mut window, state) = open("big\nplain");
    lists(
        &mut app,
        &mut window,
        &state,
        &[
            Some(TextListItem {
                kind: TextListKind::Custom(SharedString::from("->")),
                level: 0,
            }),
            Some(TextListItem::bullet(0)),
        ],
    );
    app.update_entity(&state, |state, cx| {
        let patch = TextAttributesPatch {
            font_size: AttributeChange::Set(px(30.)),
            ..Default::default()
        };
        assert!(state.set_attributes(0..3, &patch, cx));
    });
    window.draw();
    assert_eq!(markers(&app, &state), ["->", "\u{2022}"]);
    app.read_entity(&state, |state, _| {
        let markers = state.layout_map().list_markers();
        assert_eq!(markers[0].font_size, px(30.));
        assert!(markers[1].font_size < px(30.));
        assert_eq!(markers[1].paragraph_start, 4);
    });
}

#[test]
fn a_checkbox_sits_before_its_item_on_the_baseline() {
    let (mut app, mut window, state) = open("todo");
    lists(
        &mut app,
        &mut window,
        &state,
        &[Some(TextListItem::checkbox(0))],
    );
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let marker = &map.list_markers()[0];
        let bounds = map.list_checkbox_bounds(marker).unwrap();
        let row = map.visual_row(0).unwrap();
        assert!(bounds.right() < map.position_for_index(0).unwrap().x);
        assert!(bounds.left() >= px(0.));
        assert_near(bounds.bottom(), row.top + row.baseline);
    });
}

#[test]
fn a_click_on_a_checkbox_toggles_it_as_one_undo_step() {
    let (mut app, mut window, state) = open("todo\nnext");
    lists(
        &mut app,
        &mut window,
        &state,
        &[
            Some(TextListItem::checkbox(0)),
            Some(TextListItem::checkbox(0)),
        ],
    );
    let center = app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let bounds = map.list_checkbox_bounds(&map.list_markers()[1]).unwrap();
        let geometry = state.geometry().unwrap();
        geometry.bounds().origin
            + geometry.scroll_offset()
            + gpux::point(geometry.alignment_offset(), px(0.))
            + bounds.center()
    });
    window.simulate_click(center, MouseButton::Left);
    window.draw();
    assert_eq!(markers(&app, &state), ["[ ]", "[x]"]);
    // The caret did not move to the click.
    assert_eq!(
        app.read_entity(&state, |state, _| state.selection().head()),
        9
    );
    app.update_entity(&state, |state, cx| assert!(state.undo(cx)));
    window.draw();
    assert_eq!(markers(&app, &state), ["[ ]", "[ ]"]);
}

#[test]
fn markers_are_not_text() {
    let (mut app, mut window, state) = open("one\ntwo");
    lists(
        &mut app,
        &mut window,
        &state,
        &[
            Some(TextListItem::numbered(0)),
            Some(TextListItem::bullet(0)),
        ],
    );
    assert_eq!(app.read_entity(&state, |state, _| state.text()), "one\ntwo");
    let ime = window.update(|field, window, cx| {
        field.state.update(cx, |state, cx| {
            let mut actual = None;
            state.text_for_range(0..7, &mut actual, window, cx)
        })
    });
    assert_eq!(ime.as_deref(), Some("one\ntwo"));
    app.update_entity(&state, |state, cx| {
        state.select_all(cx);
        assert!(state.copy(cx));
    });
    let copied = app.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("one\ntwo"));
    // The caret at an item's start sits at its text, after the marker.
    assert_near(row_start(&app, &state, 0), px(24.));
}

#[test]
fn list_formats_move_and_undo_with_their_paragraphs() {
    let (mut app, mut window, state) = open("one\ntwo");
    lists(
        &mut app,
        &mut window,
        &state,
        &[Some(TextListItem::numbered(0)), None],
    );
    app.update_entity(&state, |state, cx| {
        state.set_selection(3, 3, cx);
        assert!(state.insert_newline(cx));
        assert!(state.replace_selection("x", cx));
    });
    window.draw();
    assert_eq!(markers(&app, &state), ["1.", "2."]);
    // Undo the typing, then the split.
    app.update_entity(&state, |state, cx| {
        assert!(state.undo(cx));
        assert!(state.undo(cx));
        assert_eq!(state.text(), "one\ntwo");
    });
    window.draw();
    assert_eq!(markers(&app, &state), ["1."]);
}

/// Found by review: a marker wider than the space before its item stays inside the field.
#[test]
fn wide_markers_stay_inside_the_field() {
    let text = (1..=10)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let (mut app, mut window, state) = open(&text);
    lists(
        &mut app,
        &mut window,
        &state,
        &vec![Some(TextListItem::numbered(0)); 10],
    );
    assert_eq!(markers(&app, &state)[9], "10.");
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let marker = &map.list_markers()[9];
        // "10." is three glyphs, wider than the 24 px step less its gap.
        let x = map.list_marker_x(marker, px(30.)).unwrap();
        assert_eq!(x, px(0.));
        let narrow = map.list_marker_x(marker, px(4.)).unwrap();
        assert_near(narrow, px(24.) - marker.font_size * 0.4 - px(4.));
    });
}
