//! One alignment offset per visual row (ROW-2): caret, selection, hit testing and IME geometry
//! of a centered or right-aligned field use each row's own offset, the one paint uses, not one
//! offset from the widest row.

use gpux::{
    AppContext as _, Bounds, Context, EditableText, EditableTextLineMode, EditableTextState,
    Entity, EntityInputHandler, IntoElement, Pixels, Render, TestApp, TestAppWindow, TextAlign,
    Window, div, editable_text_selection_quads, point, prelude::*, px,
};

struct Field {
    state: Entity<EditableTextState>,
    align: TextAlign,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let field = div().w(px(300.)).h(px(120.));
        let field = match self.align {
            TextAlign::Left => field,
            TextAlign::Center => field.text_center(),
            TextAlign::Right => field.text_right(),
        };
        field.child(EditableText::new(&self.state))
    }
}

/// A short paragraph over a longer one, so the rows differ in width.
const TEXT: &str = "short\nmuch longer row";

fn open(align: TextAlign) -> (TestApp, TestAppWindow<Field>, Entity<EditableTextState>) {
    let mut app = TestApp::new();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx
            .new(|cx| EditableTextState::with_line_mode(TEXT, EditableTextLineMode::MultiLine, cx)),
        align,
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    (app, window, state)
}

/// The viewport and the width of each logical row.
fn geometry(app: &TestApp, state: &Entity<EditableTextState>) -> (Bounds<Pixels>, Vec<Pixels>) {
    app.read_entity(state, |state, _| {
        let map = state.layout_map();
        let widths = (0..2)
            .map(|row| map.logical_row_layout(row).unwrap().width())
            .collect();
        (state.geometry().unwrap().bounds(), widths)
    })
}

/// Where paint starts each row.
fn row_start(align: TextAlign, bounds: Bounds<Pixels>, width: Pixels) -> Pixels {
    match align {
        TextAlign::Left => bounds.left(),
        TextAlign::Center => bounds.left() + (bounds.size.width - width) / 2.,
        TextAlign::Right => bounds.right() - width,
    }
}

#[track_caller]
fn assert_near(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() < px(0.01),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn ime_and_caret_geometry_use_each_rows_offset() {
    for align in [TextAlign::Center, TextAlign::Right] {
        let (mut app, mut window, state) = open(align);
        let (bounds, widths) = geometry(&app, &state);
        assert!(widths[0] < widths[1]);
        for (row, range) in [(0, 0..1), (1, 6..7)] {
            let ime = window.update(|field, window, cx| {
                field.state.update(cx, |state, cx| {
                    state.bounds_for_range(range.clone(), bounds, window, cx)
                })
            });
            let ime = ime.expect("IME bounds");
            assert_near(ime.left(), row_start(align, bounds, widths[row]));
        }
        app.update_entity(&state, |state, cx| {
            state.set_selection(0, 0, cx);
        });
        window.draw();
        let caret = window.update(|field, window, cx| {
            field.state.update(cx, |state, cx| {
                state.bounds_for_range(0..0, bounds, window, cx)
            })
        });
        assert_near(caret.unwrap().left(), row_start(align, bounds, widths[0]));
    }
}

#[test]
fn hit_testing_uses_each_rows_offset() {
    for align in [TextAlign::Center, TextAlign::Right] {
        let (_app, mut window, state) = open(align);
        let (bounds, widths) = window.update(|_, _, cx| {
            let state = state.read(cx);
            let map = state.layout_map();
            (
                state.geometry().unwrap().bounds(),
                [
                    map.logical_row_layout(0).unwrap().width(),
                    map.logical_row_layout(1).unwrap().width(),
                ],
            )
        });
        // Just inside the first row's first glyph, where paint draws it.
        let first_row_y = bounds.top() + px(2.);
        let x = row_start(align, bounds, widths[0]) + px(1.);
        let index = window.update(|field, window, cx| {
            field.state.update(cx, |state, cx| {
                state.character_index_for_point(point(x, first_row_y), window, cx)
            })
        });
        assert_eq!(index, Some(0));
    }
}

#[test]
fn selection_quads_use_each_rows_offset() {
    for align in [TextAlign::Center, TextAlign::Right] {
        let (app, _window, state) = open(align);
        let (bounds, widths) = geometry(&app, &state);
        let quads = app.read_entity(&state, |state, _| {
            let geometry = state.geometry().unwrap();
            let origin = geometry.bounds().origin
                + geometry.scroll_offset()
                + point(geometry.alignment_offset(), px(0.));
            editable_text_selection_quads(
                state,
                0..TEXT.len(),
                origin,
                geometry.bounds(),
                gpux::black().into(),
            )
        });
        assert_eq!(quads.len(), 2);
        for (row, quad) in quads.iter().enumerate() {
            assert_near(quad.bounds.left(), row_start(align, bounds, widths[row]));
        }
    }
}
