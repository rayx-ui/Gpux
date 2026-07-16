use gpux::{
    AppContext as _, Context, EditableText, EditableTextLineMode, EditableTextState, Entity,
    IntoElement, MouseButton, Render, TestApp, TestAppContext, Window, div, point, prelude::*, px,
    size,
};
use std::time::Duration;

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
