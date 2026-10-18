//! Rows of different heights (ROW-1): each visual row has its own top, height and baseline, and
//! content height, hit testing, IME bounds, the caret, selection, vertical and page movement,
//! caret reveal and the single-line element height all use them. Uniform rows keep today's
//! geometry exactly.

use gpux::{
    AppContext as _, AttributeChange, Bounds, Context, EditableText, EditableTextLineMode,
    EditableTextState, Entity, EntityInputHandler, IntoElement, Pixels, Render, TestApp,
    TestAppWindow, TextAttributesPatch, TextDisplayPoint, Window, div,
    editable_text_selection_quads, point, prelude::*, px,
};

struct Field {
    state: Entity<EditableTextState>,
    height: Pixels,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(400.))
            .h(self.height)
            .child(EditableText::new(&self.state))
    }
}

const TEXT: &str = "row0\nBIG1\nrow2\nrow3";

fn open_with(
    text: &str,
    mode: EditableTextLineMode,
    height: Pixels,
) -> (TestApp, TestAppWindow<Field>, Entity<EditableTextState>) {
    let mut app = TestApp::new();
    let text = text.to_owned();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx.new(|cx| EditableTextState::with_line_mode(text, mode, cx)),
        height,
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    (app, window, state)
}

/// The base size and line height of the field.
fn base(app: &TestApp, state: &Entity<EditableTextState>) -> (Pixels, Pixels) {
    app.read_entity(state, |state, _| {
        let map = state.layout_map();
        (
            map.logical_row_layout(0)
                .unwrap()
                .unwrapped_layout
                .font_size,
            map.line_height(),
        )
    })
}

/// The field with "BIG1" at twice the base size.
fn open_tall() -> (
    TestApp,
    TestAppWindow<Field>,
    Entity<EditableTextState>,
    Pixels,
) {
    let (mut app, mut window, state) = open_with(TEXT, EditableTextLineMode::MultiLine, px(400.));
    let (font_size, line_height) = base(&app, &state);
    app.update_entity(&state, |state, cx| {
        let patch = TextAttributesPatch {
            font_size: AttributeChange::Set(font_size * 2.),
            ..Default::default()
        };
        assert!(state.set_attributes(5..9, &patch, cx));
    });
    window.draw();
    (app, window, state, line_height)
}

#[track_caller]
fn assert_near(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() < px(0.01),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn uniform_rows_keep_todays_geometry() {
    let (app, _window, state) = open_with(TEXT, EditableTextLineMode::MultiLine, px(400.));
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let line_height = map.line_height();
        for row in 0..4 {
            let visual = map.visual_row(row).unwrap();
            assert_eq!(visual.top, line_height * row);
            assert_eq!(visual.height, line_height);
            assert_eq!(visual.x_offset, px(0.));
            assert_eq!(
                map.position_for_index(row * 5),
                Some(point(px(0.), line_height * row))
            );
        }
        assert_eq!(map.size().height, line_height * 4);
    });
}

#[test]
fn a_row_is_as_tall_as_its_tallest_run_and_later_rows_move_down() {
    let (app, _window, state, line_height) = open_tall();
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let tall = map.visual_row(1).unwrap();
        assert_eq!(tall.top, line_height);
        assert_near(tall.height, line_height * 2.);
        let after = map.visual_row(2).unwrap();
        assert_near(after.top, line_height * 3.);
        assert_eq!(after.height, line_height);
        assert_near(map.size().height, line_height * 5.);
    });
}

#[test]
fn every_run_in_a_row_shares_its_baseline() {
    let (mut app, mut window, state) = open_with("ab", EditableTextLineMode::MultiLine, px(400.));
    let (font_size, line_height) = base(&app, &state);
    app.update_entity(&state, |state, cx| {
        let patch = TextAttributesPatch {
            font_size: AttributeChange::Set(font_size * 3.),
            ..Default::default()
        };
        assert!(state.set_attributes(1..2, &patch, cx));
    });
    window.draw();
    app.read_entity(&state, |state, _| {
        let row = state.layout_map().visual_row(0).unwrap();
        assert_near(row.height, line_height * 3.);
        // The baseline clears the large run's ascent and leaves room for its descent.
        let large = font_size * 3.;
        assert!(row.baseline >= large * 0.8, "{row:?}");
        assert!(
            row.height - row.baseline >= large * 0.2 - px(0.01),
            "{row:?}"
        );
        // Both glyphs sit on it: neither run is shifted.
        let layout = state.layout_map().logical_row_layout(0).unwrap();
        assert!(
            layout
                .runs()
                .iter()
                .all(|run| run.glyphs.iter().all(|glyph| glyph.position.y == px(0.)))
        );
    });
}

#[test]
fn positions_and_display_points_use_row_tops() {
    let (app, _window, state, line_height) = open_tall();
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        assert_eq!(map.position_for_index(5).map(|p| p.y), Some(line_height));
        assert_near(map.position_for_index(10).unwrap().y, line_height * 3.);
        assert_near(map.position_for_index(15).unwrap().y, line_height * 4.);
        assert_eq!(map.buffer_to_display(11), Some(TextDisplayPoint::new(2, 1)));
        assert_eq!(map.display_to_buffer(TextDisplayPoint::new(3, 2)), Some(17));
        assert_eq!(map.visual_row_range(1), Some(5..9));
        assert_near(
            map.position_for_display(TextDisplayPoint::new(2, 0))
                .unwrap()
                .y,
            line_height * 3.,
        );
    });
}

#[test]
fn hit_testing_finds_the_row_under_the_point() {
    let (app, _window, state, line_height) = open_tall();
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        // Anywhere in the tall row is that row, including its lower half.
        assert_eq!(
            map.index_for_position(point(px(0.), line_height * 2.5)),
            Some(5)
        );
        assert_eq!(
            map.index_for_position(point(px(0.), line_height * 1.1)),
            Some(5)
        );
        assert_eq!(
            map.index_for_position(point(px(0.), line_height * 3.2)),
            Some(10)
        );
        assert_eq!(map.visual_row_at_y(line_height * 4.5), 3);
        assert_eq!(map.visual_row_at_y(line_height * 5.), 4);
    });
}

#[test]
fn ime_bounds_and_the_caret_follow_row_heights() {
    let (mut app, mut window, state, line_height) = open_tall();
    let bounds = app.read_entity(&state, |state, _| state.geometry().unwrap().bounds());
    let ime = window.update(|field, window, cx| {
        field.state.update(cx, |state, cx| {
            state.bounds_for_range(0..12, bounds, window, cx)
        })
    });
    let ime = ime.unwrap();
    assert_near(ime.top(), bounds.top());
    assert_near(ime.size.height, line_height * 4.);
    // The caret in the large run is as tall as that run's line height.
    app.update_entity(&state, |state, cx| {
        state.set_selection(7, 7, cx);
    });
    window.draw();
    let caret = window.update(|field, window, cx| {
        field.state.update(cx, |state, cx| {
            state.bounds_for_range(7..7, bounds, window, cx)
        })
    });
    let caret = caret.unwrap();
    assert_near(caret.size.height, line_height * 2.);
    assert_near(caret.top(), bounds.top() + line_height);
}

#[test]
fn a_small_runs_caret_stays_inside_a_tall_row() {
    let (mut app, mut window, state) = open_with("ab", EditableTextLineMode::MultiLine, px(400.));
    let (font_size, line_height) = base(&app, &state);
    app.update_entity(&state, |state, cx| {
        let patch = TextAttributesPatch {
            font_size: AttributeChange::Set(font_size * 3.),
            ..Default::default()
        };
        assert!(state.set_attributes(1..2, &patch, cx));
    });
    window.draw();
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let row = map.visual_row(0).unwrap();
        let caret = map.caret_bounds(1).unwrap();
        assert_near(caret.size.height, line_height);
        assert!(caret.top() >= row.top && caret.bottom() <= row.top + row.height);
        assert_near(map.caret_bounds(2).unwrap().size.height, line_height * 3.);
    });
}

#[test]
fn selection_quads_take_each_rows_height() {
    let (app, _window, state, line_height) = open_tall();
    let quads = app.read_entity(&state, |state, _| {
        let geometry = state.geometry().unwrap();
        editable_text_selection_quads(
            state,
            0..TEXT.len(),
            geometry.bounds().origin,
            Bounds::new(geometry.bounds().origin, gpux::size(px(400.), px(400.))),
            gpux::black().into(),
        )
    });
    let heights: Vec<Pixels> = quads.iter().map(|quad| quad.bounds.size.height).collect();
    assert_eq!(heights.len(), 4);
    assert_eq!(heights[0], line_height);
    assert_near(heights[1], line_height * 2.);
    assert_near(
        quads[2].bounds.top() - quads[0].bounds.top(),
        line_height * 3.,
    );
}

#[test]
fn vertical_movement_keeps_the_preferred_x_across_a_tall_row() {
    let (mut app, mut window, state, _) = open_tall();
    app.update_entity(&state, |state, cx| {
        state.set_selection(2, 2, cx);
        state.move_down(false, cx);
    });
    window.draw();
    // "ro|w0" sits under "B" of the double-size "BIG1", the nearest boundary to its x.
    let (head, preferred) = app.read_entity(&state, |state, _| {
        (state.selection().head(), state.preferred_vertical_x())
    });
    assert_eq!(head, 6);
    app.update_entity(&state, |state, cx| state.move_down(false, cx));
    window.draw();
    let (head, preferred_after) = app.read_entity(&state, |state, _| {
        (state.selection().head(), state.preferred_vertical_x())
    });
    assert_eq!(head, 12);
    assert_eq!(preferred_after, preferred);
}

#[test]
fn page_movement_goes_by_viewport_height() {
    let text = (0..20)
        .map(|row| format!("r{row:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (mut app, mut window, state) = open_with(&text, EditableTextLineMode::MultiLine, px(400.));
    let (font_size, line_height) = base(&app, &state);
    // Rows 1-4 are twice as tall: a page down from row 0 crosses fewer of them.
    app.update_entity(&state, |state, cx| {
        let patch = TextAttributesPatch {
            font_size: AttributeChange::Set(font_size * 2.),
            ..Default::default()
        };
        assert!(state.set_attributes(4..19, &patch, cx));
        state.set_selection(0, 0, cx);
    });
    window.draw();
    app.update_entity(&state, |state, cx| state.move_page_down(false, cx));
    window.draw();
    let page_rows = (400. / f32::from(line_height)).floor();
    let target_y = line_height / 2. + line_height * page_rows;
    let expected = app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        map.index_for_position(point(px(0.), target_y)).unwrap()
    });
    assert_eq!(
        app.read_entity(&state, |state, _| state.selection().head()),
        expected
    );
    // Fewer rows than with uniform rows.
    let row = app.read_entity(&state, |state, _| {
        state.layout_map().buffer_to_display(expected).unwrap().row
    });
    assert!((row as f32) < page_rows, "row {row}, page {page_rows}");
}

#[test]
fn caret_reveal_shows_the_whole_tall_row() {
    let text = (0..12)
        .map(|row| format!("r{row:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (mut app, mut window, state) = open_with(&text, EditableTextLineMode::MultiLine, px(60.));
    let (font_size, _) = base(&app, &state);
    let last = text.len();
    app.update_entity(&state, |state, cx| {
        let patch = TextAttributesPatch {
            font_size: AttributeChange::Set(font_size * 2.),
            ..Default::default()
        };
        assert!(state.set_attributes(last - 3..last, &patch, cx));
        state.set_selection(last, last, cx);
    });
    window.draw();
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let row = map.visual_row(11).unwrap();
        let viewport = state.geometry().unwrap().bounds();
        let visible_bottom = row.top + row.height + state.scroll_offset().y;
        assert!(
            visible_bottom <= viewport.size.height + px(0.01),
            "row bottom {visible_bottom:?} below the {:?} viewport",
            viewport.size.height
        );
        assert!(row.top + state.scroll_offset().y >= px(0.));
    });
}

#[test]
fn a_single_line_field_is_as_tall_as_its_tallest_run() {
    let (mut app, mut window, state) =
        open_with("single", EditableTextLineMode::SingleLine, px(400.));
    let (font_size, line_height) = base(&app, &state);
    let before = app.read_entity(&state, |state, _| state.geometry().unwrap().bounds());
    assert_eq!(before.size.height, line_height);
    app.update_entity(&state, |state, cx| {
        let patch = TextAttributesPatch {
            font_size: AttributeChange::Set(font_size * 2.),
            ..Default::default()
        };
        assert!(state.set_attributes(0..3, &patch, cx));
    });
    window.draw();
    // The next frame lays the field out at its new row height.
    window.advance_frame(std::time::Duration::from_millis(50));
    let after = app.read_entity(&state, |state, _| state.geometry().unwrap().bounds());
    assert_near(after.size.height, line_height * 2.);
}

/// State notifications over ten idle frames of a settled single-line field at `scale`, with
/// its text at `size_ratio` times the base size.
fn idle_notifications(scale: f32, size_ratio: f32) -> usize {
    let (mut app, mut window, state) =
        open_with("plain", EditableTextLineMode::SingleLine, px(400.));
    if size_ratio != 1. {
        let (font_size, _) = base(&app, &state);
        app.update_entity(&state, |state, cx| {
            let patch = TextAttributesPatch {
                font_size: AttributeChange::Set(font_size * size_ratio),
                ..Default::default()
            };
            assert!(state.set_attributes(0..5, &patch, cx));
        });
    }
    window.simulate_scale_factor_change(scale);
    window.draw();
    for _ in 0..3 {
        window.advance_frame(std::time::Duration::from_millis(50));
    }
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = count.clone();
    let _subscription =
        app.update(|cx| cx.observe(&state, move |_, _| observed.set(observed.get() + 1)));
    for _ in 0..10 {
        window.advance_frame(std::time::Duration::from_millis(50));
    }
    count.get()
}

/// Found by review: laid-out heights snap to device pixels, so a single-line field whose row
/// height is fractional in device pixels must not ask for a new layout every frame.
#[test]
fn an_idle_single_line_field_schedules_no_frames() {
    for scale in [1., 1.25, 1.5, 2.] {
        assert_eq!(idle_notifications(scale, 1.), 0, "plain field at {scale}x");
        assert_eq!(idle_notifications(scale, 1.17), 0, "1.17x run at {scale}x");
    }
}
