//! Rendering stored attributes (ATTR-4): shaping attributes reshape only the rows they touch,
//! paint attributes never reshape, owner style runs still apply over stored paint fields, and a
//! plain field lays out and shapes exactly as before.

use gpux::{
    AppContext as _, AttributeChange, Context, EditableText, EditableTextLineMode,
    EditableTextState, EditableTextStyleRun, EditableTextStyleRuns, Entity, HighlightStyle,
    IntoElement, Pixels, Render, TestApp, TestAppWindow, TextAttributesPatch, TextLayoutCacheStats,
    Window, div, editable_text_decoration_runs, prelude::*, px, rgb,
};

struct Field {
    state: Entity<EditableTextState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(400.))
            .h(px(200.))
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

fn stats(app: &TestApp, state: &Entity<EditableTextState>) -> TextLayoutCacheStats {
    app.read_entity(state, |state, _| state.layout_map().cache_stats())
}

fn width(app: &TestApp, state: &Entity<EditableTextState>) -> Pixels {
    app.read_entity(state, |state, _| state.layout_map().size().width)
}

fn patch(
    app: &mut TestApp,
    state: &Entity<EditableTextState>,
    range: std::ops::Range<usize>,
    patch: TextAttributesPatch,
) {
    app.update_entity(state, |state, cx| {
        assert!(state.set_attributes(range, &patch, cx))
    });
}

const ROWS: &str = "first row\nsecond row\nthird row";

#[test]
fn shaping_attributes_reshape_only_the_rows_they_touch() {
    let (mut app, mut window, state) = open(ROWS);
    let plain_width = width(&app, &state);
    let before = stats(&app, &state);
    patch(
        &mut app,
        &state,
        0..5,
        TextAttributesPatch {
            font_size: AttributeChange::Set(px(40.)),
            ..Default::default()
        },
    );
    window.draw();
    let after = stats(&app, &state);
    assert_eq!(after.shaped_rows - before.shaped_rows, 1);
    assert!(width(&app, &state) > plain_width);
    // The row shapes its runs at their sizes.
    let sizes = app.read_entity(&state, |state, _| {
        state
            .layout_map()
            .logical_row_layout(0)
            .unwrap()
            .runs()
            .iter()
            .map(|run| run.font_size)
            .collect::<Vec<_>>()
    });
    assert_eq!(sizes[0], px(40.));
    assert!(sizes[1..].iter().all(|size| *size < px(40.)));
}

#[test]
fn paint_attributes_never_reshape() {
    let (mut app, mut window, state) = open(ROWS);
    let before = stats(&app, &state);
    patch(
        &mut app,
        &state,
        2..14,
        TextAttributesPatch {
            color: AttributeChange::Set(rgb(0xff0000).into()),
            background_color: AttributeChange::Set(rgb(0x0000ff).into()),
            ..Default::default()
        },
    );
    window.draw();
    // Splitting a shaping span by paint alone does not reshape either.
    patch(
        &mut app,
        &state,
        0..9,
        TextAttributesPatch {
            font_weight: AttributeChange::Set(gpux::FontWeight::BOLD),
            ..Default::default()
        },
    );
    window.draw();
    let bolded = stats(&app, &state);
    assert_eq!(bolded.shaped_rows - before.shaped_rows, 1);
    patch(
        &mut app,
        &state,
        4..6,
        TextAttributesPatch {
            color: AttributeChange::Set(rgb(0x00ff00).into()),
            ..Default::default()
        },
    );
    window.draw();
    assert_eq!(stats(&app, &state).shaped_rows, bolded.shaped_rows);
}

#[test]
fn stored_paint_attributes_become_decoration_runs_under_owner_runs() {
    let (mut app, mut window, state) = open("abcdef");
    patch(
        &mut app,
        &state,
        0..4,
        TextAttributesPatch {
            color: AttributeChange::Set(rgb(0xff0000).into()),
            background_color: AttributeChange::Set(rgb(0x0000ff).into()),
            ..Default::default()
        },
    );
    app.update_entity(&state, |state, cx| {
        let revision = state.revision();
        state
            .set_style_runs(
                EditableTextStyleRuns::new(
                    revision,
                    [EditableTextStyleRun::new(
                        2..6,
                        HighlightStyle {
                            background_color: Some(rgb(0x00ff00).into()),
                            ..Default::default()
                        },
                    )],
                ),
                cx,
            )
            .unwrap();
    });
    window.draw();
    let runs = app.read_entity(&state, |state, _| {
        editable_text_decoration_runs(state, 0, rgb(0x000000).into())
    });
    let summary = runs
        .iter()
        .map(|run| (run.len, run.color.clone(), run.background_color.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        [
            (2, rgb(0xff0000).into(), Some(rgb(0x0000ff).into())),
            // The owner's background wins; the stored color stays.
            (2, rgb(0xff0000).into(), Some(rgb(0x00ff00).into())),
            (2, rgb(0x000000).into(), Some(rgb(0x00ff00).into())),
        ]
    );
}

#[test]
fn typing_in_formatted_text_reshapes_only_the_edited_row() {
    let (mut app, mut window, state) = open(ROWS);
    patch(
        &mut app,
        &state,
        0..ROWS.len(),
        TextAttributesPatch {
            font_size: AttributeChange::Set(px(20.)),
            ..Default::default()
        },
    );
    window.draw();
    let before = stats(&app, &state);
    app.update_entity(&state, |state, cx| {
        state.set_selection(12, 12, cx);
        assert!(state.replace_selection("X", cx));
    });
    window.draw();
    let after = stats(&app, &state);
    assert_eq!(after.shaped_rows - before.shaped_rows, 1);
    assert_eq!(after.cache_hits - before.cache_hits, 2);
}

#[test]
fn a_plain_field_shapes_as_before() {
    let (mut app, mut window, state) = open(ROWS);
    let before = stats(&app, &state);
    // A same-length edit takes the single-row splice: one row shaped, the rest kept.
    app.update_entity(&state, |state, cx| {
        state.set_selection(0, 1, cx);
        assert!(state.replace_selection("F", cx));
    });
    window.draw();
    let after = stats(&app, &state);
    assert_eq!(after.shaped_rows - before.shaped_rows, 1);
    assert_eq!(after.cache_hits - before.cache_hits, 2);
    // An insertion reshapes its row and reuses the rest.
    app.update_entity(&state, |state, cx| {
        state.set_selection(12, 12, cx);
        assert!(state.replace_selection("X", cx));
    });
    window.draw();
    let inserted = stats(&app, &state);
    assert_eq!(inserted.shaped_rows - after.shaped_rows, 1);
    assert_eq!(inserted.cache_hits - after.cache_hits, 2);
    assert!(app.read_entity(&state, |state, _| state.attribute_spans().is_empty()));
}
