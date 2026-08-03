use gpux::{
    AppContext as _, Bounds, Context, EditableText, EditableTextLineMode, EditableTextState,
    Entity, Instant, IntoElement, MouseButton, Render, ScrollBehavior, TestApp, TestAppContext,
    TextDisplayPoint, Window, div, editable_text_selection_quads, point, prelude::*, px, size,
};
use std::{ops::Range, time::Duration};

struct MultilineHarness {
    state: Entity<EditableTextState>,
    width: gpux::Pixels,
    height: gpux::Pixels,
}

impl Render for MultilineHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().p(px(12.)).child(
            div()
                .w(self.width)
                .h(self.height)
                .child(EditableText::new(&self.state)),
        )
    }
}

#[gpux::test]
fn line_mode_preserves_multiline_input_and_keeps_single_line_normalization(
    cx: &mut TestAppContext,
) {
    let multiline = cx.new(|cx| {
        EditableTextState::with_line_mode(
            "alpha\r\nbeta\rgamma",
            EditableTextLineMode::MultiLine,
            cx,
        )
    });
    multiline.update(cx, |state, cx| {
        assert_eq!(state.text(), "alpha\nbeta\ngamma");
        assert!(state.insert_newline(cx));
        assert_eq!(state.text(), "alpha\nbeta\ngamma\n");
        assert_eq!(state.line_mode(), EditableTextLineMode::MultiLine);
        assert!(state.soft_wrap());
    });

    let single = cx.new(|cx| EditableTextState::with_text("alpha\r\nbeta\ngamma", cx));
    single.update(cx, |state, cx| {
        assert_eq!(state.text(), "alpha beta gamma");
        assert!(!state.insert_newline(cx));
        assert_eq!(state.text(), "alpha beta gamma");
        assert_eq!(state.line_mode(), EditableTextLineMode::SingleLine);
    });
}

#[gpux::test]
fn newline_has_its_own_undo_transaction(cx: &mut TestAppContext) {
    let state =
        cx.new(|cx| EditableTextState::with_line_mode("", EditableTextLineMode::MultiLine, cx));

    state.update(cx, |state, cx| {
        assert!(state.replace_selection("message", cx));
        assert!(state.insert_newline(cx));
        assert_eq!(state.text(), "message\n");

        assert!(state.undo(cx));
        assert_eq!(state.text(), "message");
    });
}

#[test]
fn multiline_visual_navigation_selection_resize_and_ime_use_one_layout_map() {
    let mut app = TestApp::new();
    let text =
        "abcdefgh ijklmnop qrstuvwx\nsecond visual line\nthird visual line\nfourth visual line";
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx
            .new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx)),
        width: px(130.),
        height: px(66.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.draw();
    window.update(|harness, window, cx| {
        window.focus(&harness.state.read(cx).focus_handle(), cx);
    });
    window.draw();
    window.simulate_keystroke("enter");
    window.draw();
    assert!(app.read_entity(&state, |state, _| state.text().ends_with('\n')));

    let initial_rows = app.read_entity(&state, |state, _| state.layout_map().visual_line_count());
    assert!(initial_rows > 4, "the first logical line should soft-wrap");
    assert_eq!(
        app.read_entity(&state, |state, _| state
            .viewport_bounds()
            .unwrap()
            .size
            .height),
        px(66.)
    );

    app.update_entity(&state, |state, cx| {
        state.move_to_start(cx);
        state.move_down(false, cx);
        let first_down = state.selection().head();
        assert!(first_down > 0);
        assert!(state.preferred_vertical_x().is_some());
        state.move_page_down(true, cx);
        assert!(state.selection().head() > first_down);
        assert!(!state.selection().is_empty());
        let page_down = state.selection().head();
        state.move_page_up(true, cx);
        assert!(state.selection().head() < page_down);
        let page_up = state.selection().head();
        state.move_up(true, cx);
        assert!(state.selection().head() <= page_up);
        state.move_page_down(true, cx);
        assert!(!state.selection().is_empty());
    });
    window.draw();

    let selection_bounds = app.read_entity(&state, |state, _| {
        state
            .geometry()
            .unwrap()
            .bounds_for_range(state.layout_map(), state.selection().range())
    });
    assert!(selection_bounds.size.height > window.update(|_, window, _| window.line_height()));

    let old_rows = initial_rows;
    window.update(|harness, _, cx| {
        harness.width = px(260.);
        cx.notify();
    });
    window.draw();
    assert!(
        app.read_entity(&state, |state, _| state.layout_map().visual_line_count()) < old_rows,
        "resize must update the same wrap map used by navigation and IME"
    );

    let ime = app.read_entity(&state, |state, _| {
        state
            .geometry()
            .unwrap()
            .bounds_for_range(state.layout_map(), state.selection().range())
    });
    assert!(f32::from(ime.origin.x).is_finite());
    assert!(f32::from(ime.origin.y).is_finite());
}

fn expected_selection_bounds(
    state: &EditableTextState,
    range: Range<usize>,
) -> Vec<Bounds<gpux::Pixels>> {
    let map = state.layout_map();
    let geometry = state.geometry().expect("layout geometry");
    let origin = geometry.bounds().origin + state.scroll_offset();
    let viewport = geometry.bounds();
    let mut previous_range: Option<Range<usize>> = None;
    let mut bounds = Vec::new();

    for row in 0..map.visual_line_count() {
        let row_range = map.visual_row_range(row).expect("visual row range");
        let segment_start = range.start.max(row_range.start).min(row_range.end);
        let segment_end = range.end.min(row_range.end).max(row_range.start);
        let has_text = segment_start < segment_end;
        let crosses_empty_row =
            row_range.is_empty() && range.start <= row_range.start && range.end >= row_range.end;
        let crosses_logical_newline = previous_range.as_ref().is_some_and(|previous| {
            previous.end < row_range.start
                && range.start <= previous.end
                && range.end >= row_range.start
        });
        previous_range = Some(row_range.clone());
        if !has_text && !crosses_empty_row && !crosses_logical_newline {
            continue;
        }

        let top = origin.y + map.line_height() * row;
        if top + map.line_height() <= viewport.top() || top >= viewport.bottom() {
            continue;
        }
        let start_column = segment_start.saturating_sub(row_range.start);
        let end_column = segment_end.saturating_sub(row_range.start);
        let start = map
            .position_for_display(TextDisplayPoint::new(row, start_column))
            .expect("row-local selection start");
        let end = map
            .position_for_display(TextDisplayPoint::new(row, end_column))
            .expect("row-local selection end");
        bounds.push(Bounds::new(
            origin + start,
            size((end.x - start.x).max(px(1.)), map.line_height()),
        ));
    }
    bounds
}

#[test]
fn selection_quads_cover_every_intersected_visual_row() {
    let mut app = TestApp::new();
    let text = "alpha beta gamma delta epsilon\n\nUnicode e\u{301} 👩\u{200d}🚀 τέλος\ntrailing\n";
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx
            .new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx)),
        width: px(92.),
        height: px(500.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.draw();

    let (wrap_boundary, first_newline) = app.read_entity(&state, |state, _| {
        let first = state.layout_map().visual_row_range(0).unwrap();
        (first.end, state.text().find('\n').unwrap())
    });
    assert!(
        wrap_boundary < first_newline,
        "fixture must soft-wrap its first logical line"
    );

    let empty_line = text.find("\n\n").unwrap() + 1;
    let unicode_start = text.find("e\u{301}").unwrap();
    let unicode_end = text.find(" τέλος").unwrap();
    let cases = [
        ("forward wrap boundary", 0, wrap_boundary),
        ("backward wrap boundary", wrap_boundary, 0),
        (
            "forward logical lines",
            first_newline - 2,
            unicode_start + 3,
        ),
        (
            "backward logical lines",
            unicode_start + 3,
            first_newline - 2,
        ),
        ("empty logical line", empty_line - 1, empty_line + 1),
        ("trailing newline", text.len() - 1, text.len()),
        ("mixed grapheme unicode", unicode_start, unicode_end),
    ];

    for (label, anchor, head) in cases {
        app.update_entity(&state, |state, cx| {
            assert!(state.set_selection(anchor, head, cx), "{label}");
        });
        window.draw();
        app.read_entity(&state, |state, _| {
            assert_eq!(state.selection().anchor(), anchor, "{label} anchor");
            assert_eq!(state.selection().head(), head, "{label} head");
            let expected = expected_selection_bounds(state, state.selection().range());
            let geometry = state.geometry().expect("layout geometry");
            let actual = editable_text_selection_quads(
                state,
                state.selection().range(),
                geometry.bounds().origin + state.scroll_offset(),
                geometry.bounds(),
                gpux::red().into(),
            );
            let actual = actual
                .into_iter()
                .map(|quad| quad.bounds)
                .collect::<Vec<_>>();
            assert_eq!(
                actual, expected,
                "{label} must use row-local quad endpoints"
            );
            assert!(
                actual.iter().all(|bounds| bounds.size.width >= px(1.)
                    && bounds.size.height == state.layout_map().line_height()),
                "{label} must produce nonzero one-row quads"
            );
        });
    }
}

#[test]
fn selection_only_movement_preserves_viewport_geometry() {
    let mut app = TestApp::new();
    let text = (0..40)
        .map(|line| format!("line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx
            .new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx)),
        width: px(160.),
        height: px(66.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.draw();

    app.update_entity(&state, |state, cx| state.move_to_start(cx));
    window.draw();
    app.update_entity(&state, |state, cx| {
        assert!(state.set_scroll_offset(point(px(0.), px(-88.)), cx));
    });
    let (bounds_before, scroll_before, revision_before) = app.read_entity(&state, |state, _| {
        (
            state.viewport_bounds().expect("valid viewport geometry"),
            state.scroll_offset(),
            state.geometry().unwrap().revision(),
        )
    });

    let retained = app.update_entity(&state, |state, cx| {
        state.move_to_end_extending(cx);
        (
            state.viewport_bounds(),
            state.scroll_offset(),
            state.geometry().map(|geometry| geometry.revision()),
        )
    });
    assert_eq!(retained.0, Some(bounds_before));
    assert_eq!(retained.1, scroll_before);
    assert_eq!(retained.2, Some(revision_before));

    window.draw();
    let revealed_scroll = app.read_entity(&state, |state, _| state.scroll_offset());
    assert!(
        revealed_scroll.y < scroll_before.y,
        "next layout must reveal the caret once"
    );

    app.update_entity(&state, |state, cx| {
        assert!(state.set_scroll_offset(point(px(0.), px(-44.)), cx));
    });
    window.draw();
    let manual_scroll = app.read_entity(&state, |state, _| state.scroll_offset());
    window.draw();
    assert_eq!(
        app.read_entity(&state, |state, _| state.scroll_offset()),
        manual_scroll,
        "paint-only layouts must not repeat caret reveal"
    );
}

#[test]
fn secondary_shift_extends_by_logical_line() {
    let mut app = TestApp::new();
    let text = "alpha beta gamma delta epsilon zeta\n\ncafe\u{301} 👩\u{200d}💻 العربية\nlast line";
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx
            .new(|cx| EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx)),
        width: px(92.),
        height: px(260.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.draw();
    window.update(|harness, window, cx| {
        window.focus(&harness.state.read(cx).focus_handle(), cx);
    });
    window.draw();

    let first_end = text.find('\n').unwrap();
    let empty_start = first_end + 1;
    let unicode_start = empty_start + 1;
    let unicode_end = text[unicode_start..].find('\n').unwrap() + unicode_start;
    let anchor = unicode_start + "cafe\u{301}".len();
    assert!(app.read_entity(&state, |state, _| {
        state.layout_map().visual_line_count() > text.split('\n').count()
    }));
    app.update_entity(&state, |state, cx| {
        assert!(state.set_selection(anchor, anchor, cx));
    });

    window.simulate_keystroke("secondary-shift-up");
    assert_eq!(
        app.read_entity(&state, |state, _| *state.selection()),
        gpux::EditableSelection::new(anchor, empty_start)
    );
    assert!(app.read_entity(&state, |state, _| state.selection().is_reversed()));

    window.simulate_keystroke("secondary-shift-up");
    window.simulate_keystroke("secondary-shift-up");
    assert_eq!(
        app.read_entity(&state, |state, _| *state.selection()),
        gpux::EditableSelection::new(anchor, 0),
        "repeated upward extension must clamp at BOF"
    );

    window.simulate_keystroke("secondary-shift-down");
    assert_eq!(
        app.read_entity(&state, |state, _| *state.selection()),
        gpux::EditableSelection::new(anchor, empty_start),
        "down from BOF must select through the following empty logical line"
    );
    window.simulate_keystroke("secondary-shift-down");
    assert_eq!(
        app.read_entity(&state, |state, _| *state.selection()),
        gpux::EditableSelection::new(anchor, unicode_end),
        "selection head must reverse past its preserved anchor on logical lines"
    );
    assert!(!app.read_entity(&state, |state, _| state.selection().is_reversed()));
    window.simulate_keystroke("secondary-shift-down");
    window.simulate_keystroke("secondary-shift-down");
    assert_eq!(
        app.read_entity(&state, |state, _| *state.selection()),
        gpux::EditableSelection::new(anchor, text.len()),
        "repeated downward extension must clamp at EOF"
    );
    assert!(text.is_char_boundary(app.read_entity(&state, |state, _| state.selection().head())));
}

#[test]
fn stationary_outside_pointer_drag_advances_for_five_ticks_and_stops_on_release() {
    let mut app = TestApp::new();
    let text = (0..2_000)
        .map(|line| format!("line {line:04}: abcdefghijklmnopqrstuvwxyz"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx.new(|cx| {
            let mut state =
                EditableTextState::with_line_mode(text, EditableTextLineMode::MultiLine, cx);
            state.set_soft_wrap(false, cx);
            state
        }),
        width: px(180.),
        height: px(66.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.draw();
    app.update_entity(&state, |state, cx| state.move_to_start(cx));
    window.draw();
    let bounds = app.read_entity(&state, |state, _| state.viewport_bounds().unwrap());
    let outside = point(bounds.left() + px(10.), bounds.bottom() + px(80.));
    window.simulate_mouse_down(bounds.origin + point(px(2.), px(2.)), MouseButton::Left);
    window.simulate_mouse_move(outside);

    let mut previous = app.read_entity(&state, |state, _| {
        (state.scroll_offset().y, state.selection().head())
    });
    for tick in 1..=5 {
        app.advance_clock(Duration::from_millis(17));
        app.run_until_parked();
        let current = app.read_entity(&state, |state, _| {
            (state.scroll_offset().y, state.selection().head())
        });
        assert!(current.0 < previous.0, "tick {tick} must advance scroll");
        assert!(current.1 > previous.1, "tick {tick} must advance selection");
        previous = current;
    }

    window.simulate_mouse_up(outside, MouseButton::Left);
    let released = app.read_entity(&state, |state, _| {
        (state.scroll_offset().y, state.selection().head())
    });
    app.advance_clock(Duration::from_millis(160));
    app.run_until_parked();
    assert_eq!(
        app.read_entity(&state, |state, _| {
            (state.scroll_offset().y, state.selection().head())
        }),
        released,
        "pointer-up must cancel every later drag tick"
    );
}

#[test]
fn single_line_element_rejects_enter() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx.new(|cx| EditableTextState::with_text("alpha", cx)),
        width: px(150.),
        height: px(22.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.draw();
    window.update(|harness, window, cx| {
        window.focus(&harness.state.read(cx).focus_handle(), cx);
    });
    window.draw();
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(app.read_entity(&state, |state, _| state.text()), "alpha");
}

#[test]
fn multiline_no_wrap_scrolls_horizontally_and_drag_auto_scrolls_vertically() {
    let mut app = TestApp::new();
    let text = (0..20)
        .map(|line| format!("line {line:02}: abcdefghijklmnopqrstuvwxyz0123456789"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx.new(|cx| {
            let mut state =
                EditableTextState::with_line_mode(&text, EditableTextLineMode::MultiLine, cx);
            state.set_soft_wrap(false, cx);
            state
        }),
        width: px(150.),
        height: px(66.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.draw();
    app.update_entity(&state, |state, cx| state.move_to_end(cx));
    window.draw();
    let scrolled = app.read_entity(&state, |state, _| state.scroll_offset());
    assert!(scrolled.x < px(0.));
    assert!(scrolled.y < px(0.));

    app.update_entity(&state, |state, cx| state.move_to_start(cx));
    window.draw();
    let bounds = app.read_entity(&state, |state, _| state.viewport_bounds().unwrap());
    window.simulate_scroll(bounds.center(), point(px(0.), px(-44.)));
    assert!(app.read_entity(&state, |state, _| state.scroll_offset().y < px(0.)));
    app.update_entity(&state, |state, cx| state.move_to_start(cx));
    window.draw();
    window.simulate_mouse_down(bounds.origin + point(px(2.), px(2.)), MouseButton::Left);
    window.simulate_mouse_move(point(bounds.left() + px(10.), bounds.bottom() + px(80.)));
    let immediate_scroll = app.read_entity(&state, |state, _| state.scroll_offset().y);
    let immediate_head = app.read_entity(&state, |state, _| state.selection().head());
    app.advance_clock(Duration::from_millis(80));
    app.run_until_parked();
    assert!(app.read_entity(&state, |state, _| state.scroll_offset().y
        < immediate_scroll));
    assert!(app.read_entity(&state, |state, _| state.selection().head() > immediate_head));
    window.draw();
    window.simulate_mouse_up(
        point(bounds.left() + px(10.), bounds.bottom() + px(80.)),
        MouseButton::Left,
    );

    assert!(app.read_entity(&state, |state, _| state.scroll_offset().y < px(0.)));
    assert!(app.read_entity(&state, |state, _| !state.selection().is_empty()));
    assert!(app.read_entity(&state, |state, _| {
        let range = state.selection().range();
        state
            .layout_map()
            .buffer_to_display(range.start)
            .unwrap()
            .row
            < state.layout_map().buffer_to_display(range.end).unwrap().row
    }));
}

#[test]
fn zero_sized_multiline_viewport_is_deterministic() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx.new(|cx| {
            EditableTextState::with_line_mode("alpha\nbeta", EditableTextLineMode::MultiLine, cx)
        }),
        width: px(0.),
        height: px(0.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.simulate_resize(size(px(320.), px(240.)));
    window.draw();
    assert_eq!(
        app.read_entity(&state, |state, _| state.scroll_offset()),
        point(px(0.), px(0.))
    );
    assert!(
        app.read_entity(&state, |state, _| state.layout_map().visual_line_count()
            >= 2)
    );
}

#[test]
fn editable_text_programmatic_scroll_uses_shared_driver_and_direct_paths_cancel_without_late_frames()
 {
    let mut app = TestApp::new();
    let text = (0..40)
        .map(|line| format!("line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut window = app.open_window(|_, cx| MultilineHarness {
        state: cx.new(|cx| {
            EditableTextState::with_line_mode(&text, EditableTextLineMode::MultiLine, cx)
        }),
        width: px(160.),
        height: px(66.),
    });
    let state = window.read(|harness, _| harness.state.clone());
    window.draw();
    app.update_entity(&state, |state, cx| {
        state.set_scroll_offset(point(px(0.), px(0.)), cx);
    });

    let generation = app.update_entity(&state, |state, cx| {
        assert!(state.request_scroll_offset_with_behavior(
            point(px(0.), px(-300.)),
            ScrollBehavior::Smooth(Duration::from_secs(1)),
            cx,
        ));
        state
            .pending_programmatic_generation_for_test()
            .expect("smooth request queues its driver generation")
    });
    window.draw();
    app.update_entity(&state, |state, cx| {
        assert!(state.advance_programmatic_scroll_for_test(
            generation,
            Instant::now() + Duration::from_millis(500),
            cx,
        ));
    });
    let midpoint = app.read_entity(&state, |state, _| state.scroll_offset().y);
    let maximum = app.read_entity(&state, |state, _| state.max_scroll_offset().y);
    assert!(
        midpoint < px(0.) && midpoint > px(-300.),
        "midpoint={midpoint:?} maximum={maximum:?}"
    );

    app.update_entity(&state, |state, cx| {
        state.set_scroll_offset(point(px(0.), px(-40.)), cx);
        assert!(!state.advance_programmatic_scroll_for_test(
            generation,
            Instant::now() + Duration::from_secs(2),
            cx,
        ));
    });
    let direct = app.read_entity(&state, |state, _| state.scroll_offset());
    app.advance_clock(Duration::from_secs(2));
    app.run_until_parked();
    window.draw();
    assert_eq!(
        app.read_entity(&state, |state, _| state.scroll_offset()),
        direct
    );

    app.update_entity(&state, |state, cx| {
        assert!(state.request_scroll_offset_with_behavior(
            point(px(0.), px(-280.)),
            ScrollBehavior::Smooth(Duration::from_secs(1)),
            cx,
        ));
        let generation = state
            .pending_programmatic_generation_for_test()
            .expect("smooth request queues its driver generation");
        state.set_soft_wrap(false, cx);
        assert!(!state.advance_programmatic_scroll_for_test(
            generation,
            Instant::now() + Duration::from_secs(2),
            cx,
        ));
    });
    window.draw();
    let after_soft_wrap = app.read_entity(&state, |state, _| state.scroll_offset());
    app.advance_clock(Duration::from_secs(2));
    app.run_until_parked();
    window.draw();
    assert_eq!(
        app.read_entity(&state, |state, _| state.scroll_offset()),
        after_soft_wrap
    );

    let caret_generation = app.update_entity(&state, |state, cx| {
        assert!(state.request_scroll_offset_with_behavior(
            point(px(0.), px(-260.)),
            ScrollBehavior::Smooth(Duration::from_secs(1)),
            cx,
        ));
        let generation = state
            .pending_programmatic_generation_for_test()
            .expect("smooth request queues its driver generation");
        state.move_to_end(cx);
        generation
    });
    window.draw();
    app.update_entity(&state, |state, cx| {
        assert!(!state.advance_programmatic_scroll_for_test(
            caret_generation,
            Instant::now() + Duration::from_secs(2),
            cx,
        ));
    });
    let after_caret = app.read_entity(&state, |state, _| state.scroll_offset());
    app.advance_clock(Duration::from_secs(2));
    app.run_until_parked();
    window.draw();
    assert_eq!(
        app.read_entity(&state, |state, _| state.scroll_offset()),
        after_caret
    );
}
