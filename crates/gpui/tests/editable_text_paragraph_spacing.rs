//! Paragraph indents, spacing, line height and letter spacing (PARA-3): the first row starts at
//! the leading plus first-line indent (a negative one hangs, clamped at the field's edge), other
//! rows at the leading indent, every row wraps before the trailing indent; spacing adds space
//! outside the rows, the multiplier scales them, and the paragraph's letter spacing applies to
//! runs that set none.

use gpux::{
    AppContext as _, AttributeChange, Context, EditableText, EditableTextLineMode,
    EditableTextState, Entity, IntoElement, ParagraphFormat, Pixels, Render, TestApp,
    TestAppWindow, TextAttributesPatch, TextDisplayPoint, Window, div, prelude::*, px,
};

const WIDTH: f32 = 160.;
/// Words of three glyphs.
const WORDS: &str = "aaa bbb ccc ddd eee fff ggg hhh";

struct Field {
    state: Entity<EditableTextState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(WIDTH))
            .h(px(400.))
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

fn format(
    app: &mut TestApp,
    window: &mut TestAppWindow<Field>,
    state: &Entity<EditableTextState>,
    offset: usize,
    format: ParagraphFormat,
) {
    app.update_entity(state, |state, cx| {
        assert!(state.set_paragraph_format(offset..offset, format, cx));
    });
    window.draw();
}

/// Where each visual row starts and how wide it is.
fn rows(app: &TestApp, state: &Entity<EditableTextState>) -> Vec<(Pixels, Pixels)> {
    app.read_entity(state, |state, _| {
        let map = state.layout_map();
        (0..map.visual_line_count())
            .map(|row| {
                let visual = map.visual_row(row).unwrap();
                (
                    map.position_for_display(TextDisplayPoint::new(row, 0))
                        .unwrap()
                        .x,
                    visual.width,
                )
            })
            .collect()
    })
}

fn line_height(app: &TestApp, state: &Entity<EditableTextState>) -> Pixels {
    app.read_entity(state, |state, _| state.layout_map().line_height())
}

#[track_caller]
fn assert_near(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() < px(0.01),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn rows_start_at_the_leading_indent_and_wrap_before_the_trailing_one() {
    let (mut app, mut window, state) = open(WORDS);
    let plain_rows = rows(&app, &state).len();
    format(
        &mut app,
        &mut window,
        &state,
        0,
        ParagraphFormat {
            leading_indent: px(30.),
            trailing_indent: px(20.),
            ..Default::default()
        },
    );
    let rows = rows(&app, &state);
    assert!(
        rows.len() > plain_rows,
        "{} rows, {plain_rows} unindented",
        rows.len()
    );
    for (x, _) in &rows {
        assert_near(*x, px(30.));
    }
    // Each row's text, without its trailing space, ends before the trailing indent.
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        for row in 0..map.visual_line_count() {
            let range = map.visual_row_range(row).unwrap();
            let end = range.start + WORDS[range].trim_end().len();
            let end_x = map.position_for_index(end).unwrap().x;
            assert!(
                end_x <= px(WIDTH - 20.) + px(0.01),
                "row {row} ends at {end_x:?}"
            );
        }
    });
}

#[test]
fn the_first_row_adds_the_first_line_indent() {
    let (mut app, mut window, state) = open(WORDS);
    format(
        &mut app,
        &mut window,
        &state,
        0,
        ParagraphFormat {
            leading_indent: px(10.),
            first_line_indent: px(25.),
            ..Default::default()
        },
    );
    let rows = rows(&app, &state);
    assert_near(rows[0].0, px(35.));
    assert!(
        rows[1..]
            .iter()
            .all(|(x, _)| (*x - px(10.)).abs() < px(0.01))
    );
}

#[test]
fn a_hanging_indent_is_clamped_at_the_fields_edge() {
    let (mut app, mut window, state) = open(WORDS);
    format(
        &mut app,
        &mut window,
        &state,
        0,
        ParagraphFormat {
            leading_indent: px(20.),
            first_line_indent: px(-50.),
            ..Default::default()
        },
    );
    let rows = rows(&app, &state);
    assert_near(rows[0].0, px(0.));
    assert!(
        rows[1..]
            .iter()
            .all(|(x, _)| (*x - px(20.)).abs() < px(0.01))
    );
}

#[test]
fn spacing_adds_space_outside_the_rows() {
    let (mut app, mut window, state) = open("one\ntwo\nthree");
    let line_height = line_height(&app, &state);
    format(
        &mut app,
        &mut window,
        &state,
        4,
        ParagraphFormat {
            space_before: px(10.),
            space_after: px(14.),
            ..Default::default()
        },
    );
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        assert_eq!(map.visual_row(0).unwrap().top, px(0.));
        let second = map.visual_row(1).unwrap();
        assert_near(second.top, line_height + px(10.));
        assert_eq!(second.height, line_height);
        assert_near(map.visual_row(2).unwrap().top, line_height * 2. + px(24.));
        assert_near(map.size().height, line_height * 3. + px(24.));
        assert_near(map.position_for_index(5).unwrap().y, line_height + px(10.));
    });
}

#[test]
fn the_line_height_multiplier_scales_the_paragraphs_rows() {
    let (mut app, mut window, state) = open("one\ntwo\nthree");
    let line_height = line_height(&app, &state);
    format(
        &mut app,
        &mut window,
        &state,
        4,
        ParagraphFormat {
            line_height: Some(1.5),
            ..Default::default()
        },
    );
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        assert_near(map.visual_row(1).unwrap().height, line_height * 1.5);
        assert_near(map.visual_row(2).unwrap().top, line_height * 2.5);
        assert_eq!(map.visual_row(0).unwrap().height, line_height);
    });
}

#[test]
fn the_paragraphs_letter_spacing_applies_to_runs_that_set_none() {
    let (mut app, mut window, state) = open("abcd\nabcd");
    let plain = app.read_entity(&state, |state, _| {
        state.layout_map().visual_row(0).unwrap().width
    });
    format(
        &mut app,
        &mut window,
        &state,
        0,
        ParagraphFormat {
            letter_spacing: Some(px(3.)),
            ..Default::default()
        },
    );
    app.update_entity(&state, |state, cx| {
        // A run with its own spacing keeps it.
        let patch = TextAttributesPatch {
            letter_spacing: AttributeChange::Set(px(0.)),
            ..Default::default()
        };
        assert!(state.set_attributes(2..4, &patch, cx));
    });
    window.draw();
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        // Two spaced clusters ("a", "b") and none after "c" and "d".
        assert_near(map.visual_row(0).unwrap().width, plain + px(6.));
        // The other paragraph is untouched.
        assert_near(map.visual_row(1).unwrap().width, plain);
    });
}

/// Found by review: content reaches past each row's own indent, so a non-wrapping field can
/// reveal the end of a first-line-indented row, and an overflowing aligned row starts at its
/// indent.
#[test]
fn content_width_includes_each_rows_indent() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| Field {
        state: cx.new(|cx| EditableTextState::with_text(WORDS, cx)),
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    let plain = app.read_entity(&state, |state, _| state.content_size().width);
    format(
        &mut app,
        &mut window,
        &state,
        0,
        ParagraphFormat {
            leading_indent: px(10.),
            first_line_indent: px(30.),
            alignment: Some(gpux::ParagraphAlignment::Right),
            ..Default::default()
        },
    );
    app.update_entity(&state, |state, cx| state.move_to_end(cx));
    window.draw();
    app.read_entity(&state, |state, _| {
        assert_near(state.content_size().width, plain + px(40.));
        // The overflowing right-aligned row starts at its indent.
        let start = state
            .layout_map()
            .position_for_display(TextDisplayPoint::new(0, 0));
        assert_near(start.unwrap().x, px(40.));
        // The caret at the end is revealed inside the viewport.
        let caret = state.layout_map().position_for_index(WORDS.len()).unwrap();
        let viewport = state.geometry().unwrap().bounds();
        let visible = caret.x + state.scroll_offset().x;
        assert!(
            visible <= viewport.size.width && visible >= px(0.),
            "{visible:?}"
        );
    });
}
