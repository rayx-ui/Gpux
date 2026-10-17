//! Paragraph alignment (PARA-2): left, center, right and justify per paragraph, unset meaning
//! the element's alignment. Justified rows widen their spaces to the trailing edge except the
//! paragraph's last row and single-word rows, and caret, selection, hit testing and IME geometry
//! follow the justified positions.

use gpux::{
    AppContext as _, Bounds, Context, EditableText, EditableTextLineMode, EditableTextState,
    Entity, EntityInputHandler, IntoElement, ParagraphAlignment, ParagraphFormat, Pixels, Render,
    TestApp, TestAppWindow, TextAlign, TextDisplayPoint, Window, div,
    editable_text_selection_quads, point, prelude::*, px,
};

const WIDTH: f32 = 120.;

struct Field {
    state: Entity<EditableTextState>,
    align: TextAlign,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let field = div().w(px(WIDTH)).h(px(300.));
        let field = match self.align {
            TextAlign::Right => field.text_right(),
            _ => field,
        };
        field.child(EditableText::new(&self.state))
    }
}

fn open(
    text: &str,
    align: TextAlign,
) -> (TestApp, TestAppWindow<Field>, Entity<EditableTextState>) {
    let mut app = TestApp::new();
    let text = text.to_owned();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx
            .new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx)),
        align,
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    (app, window, state)
}

fn align(
    app: &mut TestApp,
    window: &mut TestAppWindow<Field>,
    state: &Entity<EditableTextState>,
    range: std::ops::Range<usize>,
    alignment: Option<ParagraphAlignment>,
) {
    app.update_entity(state, |state, cx| {
        let format = ParagraphFormat {
            alignment,
            ..Default::default()
        };
        assert!(state.set_paragraph_format(range, format, cx));
    });
    window.draw();
}

/// The x of `offset` in field coordinates (the field's own alignment offset included).
fn x_at(app: &TestApp, state: &Entity<EditableTextState>, offset: usize) -> Pixels {
    app.read_entity(state, |state, _| {
        let geometry = state.geometry().unwrap();
        state.layout_map().position_for_index(offset).unwrap().x + geometry.alignment_offset()
    })
}

/// The width of one glyph of the test text system at the field's size.
fn glyph_width(app: &TestApp, state: &Entity<EditableTextState>) -> Pixels {
    app.read_entity(state, |state, _| {
        state
            .layout_map()
            .logical_row_layout(0)
            .unwrap()
            .unwrapped_layout
            .x_for_index(1)
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
fn center_and_right_paragraphs_align_their_rows() {
    let (mut app, mut window, state) = open("ab\ncd\nef", TextAlign::Left);
    align(
        &mut app,
        &mut window,
        &state,
        0..0,
        Some(ParagraphAlignment::Center),
    );
    align(
        &mut app,
        &mut window,
        &state,
        3..3,
        Some(ParagraphAlignment::Right),
    );
    let glyph = glyph_width(&app, &state);
    let width = glyph * 2.;
    assert_near(x_at(&app, &state, 0), (px(WIDTH) - width) / 2.);
    assert_near(x_at(&app, &state, 3), px(WIDTH) - width);
    // An unset paragraph uses the element's (left) alignment.
    assert_near(x_at(&app, &state, 6), px(0.));
}

#[test]
fn unset_paragraphs_follow_the_element_and_set_ones_override_it() {
    let (mut app, mut window, state) = open("ab\ncd", TextAlign::Right);
    let width = glyph_width(&app, &state) * 2.;
    assert_near(x_at(&app, &state, 0), px(WIDTH) - width);
    align(
        &mut app,
        &mut window,
        &state,
        3..3,
        Some(ParagraphAlignment::Left),
    );
    assert_near(x_at(&app, &state, 3), px(0.));
    assert_near(x_at(&app, &state, 0), px(WIDTH) - width);
}

/// Words of three glyphs wrap two to a row at this width.
const WORDS: &str = "aaa bbb ccc ddd eee";

#[test]
fn justified_rows_reach_the_trailing_edge_except_the_last() {
    let (mut app, mut window, state) = open(WORDS, TextAlign::Left);
    align(
        &mut app,
        &mut window,
        &state,
        0..0,
        Some(ParagraphAlignment::Justify),
    );
    let glyph = glyph_width(&app, &state);
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        assert!(map.visual_line_count() >= 2);
        // Each non-last row's trimmed text ends at the field's edge.
        for row in 0..map.visual_line_count() - 1 {
            let range = map.visual_row_range(row).unwrap();
            let trimmed_end = range.start + WORDS[range.clone()].trim_end().len();
            let end = map.position_for_index(trimmed_end).unwrap().x;
            assert_near(end, px(WIDTH));
            assert!(map.visual_row(row).unwrap().justify > px(0.));
        }
        // The last row stays left-aligned and unjustified.
        let last = map.visual_line_count() - 1;
        let range = map.visual_row_range(last).unwrap();
        assert_eq!(map.visual_row(last).unwrap().justify, px(0.));
        // (An offset at a wrap boundary reports the end of the row before it.)
        assert_eq!(
            map.position_for_display(TextDisplayPoint::new(last, 0))
                .unwrap()
                .x,
            px(0.)
        );
        assert_near(map.position_for_index(range.start + 1).unwrap().x, glyph);
        assert!(range.end > range.start);
    });
}

#[test]
fn a_single_word_row_stays_left_aligned() {
    let (mut app, mut window, state) = open("abcdefghijklmnopq rs", TextAlign::Left);
    align(
        &mut app,
        &mut window,
        &state,
        0..0,
        Some(ParagraphAlignment::Justify),
    );
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        assert!(map.visual_line_count() >= 2);
        let first = map.visual_row(0).unwrap();
        assert_eq!(first.justify, px(0.));
        assert_eq!(
            map.position_for_index(1).unwrap().x,
            map.logical_row_layout(0)
                .unwrap()
                .unwrapped_layout
                .x_for_index(1)
        );
    });
}

#[test]
fn caret_selection_hit_testing_and_ime_follow_justified_positions() {
    let (mut app, mut window, state) = open(WORDS, TextAlign::Left);
    align(
        &mut app,
        &mut window,
        &state,
        0..0,
        Some(ParagraphAlignment::Justify),
    );
    // "bbb" starts after the first row's one widened space.
    let bbb = x_at(&app, &state, 4);
    let glyph = glyph_width(&app, &state);
    assert!(bbb > glyph * 4., "{bbb:?}");
    let (bounds, index, quads) = app.read_entity(&state, |state, _| {
        let geometry = state.geometry().unwrap();
        let map = state.layout_map();
        let index = map.index_for_position(point(bbb + px(1.), px(2.)));
        let quads = editable_text_selection_quads(
            state,
            4..7,
            geometry.bounds().origin,
            geometry.bounds(),
            gpux::black().into(),
        );
        assert_eq!(
            map.position_for_display(TextDisplayPoint::new(0, 4))
                .map(|p| p.x),
            Some(bbb)
        );
        (geometry.bounds(), index, quads)
    });
    assert_eq!(index, Some(4));
    assert_near(quads[0].bounds.left() - bounds.left(), bbb);
    let ime: Option<Bounds<Pixels>> = window.update(|field, window, cx| {
        field.state.update(cx, |state, cx| {
            state.bounds_for_range(4..5, bounds, window, cx)
        })
    });
    assert_near(ime.unwrap().left() - bounds.left(), bbb);
}
