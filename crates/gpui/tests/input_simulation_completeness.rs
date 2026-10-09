//! Test-support input covers what real platforms deliver (IME composition, key-up, click counts,
//! held-button drags) through the same dispatch and input-handler paths. Test windows take an
//! explicit size, scale factor and display, and record window and platform requests instead of
//! panicking.

use gpux::{
    AppContext as _, ClickEvent, Context, EditableText, EditableTextState, Empty, Entity,
    FocusHandle, IntoElement, KeyUpEvent, Modifiers, MouseButton, MouseMoveEvent, Pixels, Point,
    Render, TestAppContext, TestPlatformCall, TestWindowCall, TestWindowOptions, VisualTestContext,
    Window, div, point, prelude::*, px, size,
};
use std::{cell::RefCell, path::Path, rc::Rc};

struct EditableHarness {
    state: Entity<EditableTextState>,
}

impl Render for EditableHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .p(px(20.))
            .w(px(400.))
            .child(EditableText::new(&self.state))
    }
}

/// Opens an editable text element and focuses it with a click, as a user would.
fn focused_editable<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditableTextState>, &'a mut VisualTestContext) {
    let text = text.to_string();
    let (harness, cx) = cx.add_window_view(move |_, cx| EditableHarness {
        state: cx.new(|cx| EditableTextState::with_text(text, cx)),
    });
    let state = harness.read_with(cx, |harness, _| harness.state.clone());
    let bounds = state.read_with(cx, |state, _| state.geometry().unwrap().bounds());
    cx.simulate_click(bounds.center(), Modifiers::none());
    assert!(state.read_with(cx, |state, _| state.is_focused()));
    (state, cx)
}

fn text_and_marked(
    state: &Entity<EditableTextState>,
    cx: &mut VisualTestContext,
) -> (String, Option<std::ops::Range<usize>>) {
    state.read_with(cx, |state, _| (state.text(), state.marked_range()))
}

#[gpux::test]
fn ime_composition_marks_updates_and_commits_text(cx: &mut TestAppContext) {
    let (state, cx) = focused_editable(cx, "");

    assert!(cx.simulate_ime_compose("n", Some(1..1)));
    assert_eq!(text_and_marked(&state, cx), ("n".into(), Some(0..1)));
    assert_eq!(cx.ime_marked_text_range(), Some(0..1));

    assert!(cx.simulate_ime_compose("にほ", Some(2..2)));
    assert_eq!(text_and_marked(&state, cx), ("にほ".into(), Some(0..6)));
    assert_eq!(cx.ime_marked_text_range(), Some(0..2));

    assert!(cx.simulate_ime_commit("日本"));
    assert_eq!(text_and_marked(&state, cx), ("日本".into(), None));
    assert_eq!(cx.ime_marked_text_range(), None);
    assert_eq!(
        state.read_with(cx, |state, _| state.selection().range()),
        6..6
    );
}

#[gpux::test]
fn ime_cancel_removes_the_composition(cx: &mut TestAppContext) {
    let (state, cx) = focused_editable(cx, "ab");
    state.update(cx, |state, cx| state.move_to_end(cx));

    assert!(cx.simulate_ime_compose("xy", Some(2..2)));
    assert_eq!(text_and_marked(&state, cx), ("abxy".into(), Some(2..4)));

    assert!(cx.simulate_ime_cancel());
    assert_eq!(text_and_marked(&state, cx), ("ab".into(), None));
}

#[gpux::test]
fn ime_helpers_report_when_nothing_accepts_text(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    assert!(!cx.simulate_ime_compose("a", None));
    assert!(!cx.simulate_ime_commit("a"));
    assert!(!cx.simulate_ime_cancel());
    assert_eq!(cx.ime_marked_text_range(), None);
}

#[gpux::test]
fn multi_click_counts_select_words_and_lines(cx: &mut TestAppContext) {
    let (state, cx) = focused_editable(cx, "hello brave world");
    let bounds = state.read_with(cx, |state, _| state.geometry().unwrap().bounds());
    let selected = |cx: &mut VisualTestContext| {
        state.read_with(cx, |state, _| {
            let range = state.selection().range();
            state.text()[range].to_string()
        })
    };
    // The test platform's no-op text system advances every glyph 9.6 px at the default 16 px
    // size, so "brave" (bytes 6..11) spans 57.6..105.6 px from the text origin.
    let middle = point(bounds.origin.x + px(80.), bounds.center().y);

    cx.simulate_click_count(middle, MouseButton::Left, 1, Modifiers::none());
    assert_eq!(selected(cx), "");
    cx.simulate_click_count(middle, MouseButton::Left, 2, Modifiers::none());
    assert_eq!(selected(cx), "brave");
    cx.simulate_click_count(middle, MouseButton::Left, 3, Modifiers::none());
    assert_eq!(selected(cx), "hello brave world");
}

struct Clicks {
    counts: Rc<RefCell<Vec<usize>>>,
}

impl Render for Clicks {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let counts = self.counts.clone();
        div()
            .id("target")
            .size(px(100.))
            .on_click(move |event: &ClickEvent, _, _| counts.borrow_mut().push(event.click_count()))
    }
}

#[gpux::test]
fn click_counts_reach_click_listeners(cx: &mut TestAppContext) {
    let counts = Rc::new(RefCell::new(Vec::new()));
    let (_, cx) = cx.add_window_view({
        let counts = counts.clone();
        move |_, _| Clicks { counts }
    });
    let center = point(px(50.), px(50.));
    for count in 1..=3 {
        cx.simulate_click_count(center, MouseButton::Left, count, Modifiers::none());
    }
    assert_eq!(*counts.borrow(), [1, 2, 3]);
}

struct KeyUps {
    focus_handle: FocusHandle,
    released: Rc<RefCell<Vec<String>>>,
}

impl Render for KeyUps {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let released = self.released.clone();
        div().track_focus(&self.focus_handle).size_full().on_key_up(
            move |event: &KeyUpEvent, _, _| released.borrow_mut().push(event.keystroke.unparse()),
        )
    }
}

#[gpux::test]
fn key_up_reaches_the_focused_element(cx: &mut TestAppContext) {
    let released = Rc::new(RefCell::new(Vec::new()));
    let (view, cx) = cx.add_window_view({
        let released = released.clone();
        move |_, cx| KeyUps {
            focus_handle: cx.focus_handle(),
            released,
        }
    });
    let focus_handle = view.read_with(cx, |view, _| view.focus_handle.clone());
    cx.update(|window, cx| focus_handle.focus(window, cx));
    cx.run_until_parked();

    cx.simulate_key_up("a");
    cx.simulate_key_up("shift-enter");
    assert_eq!(*released.borrow(), ["a", "shift-enter"]);
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Payload(u32);

struct DragPreview;

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct DragScene {
    dropped: Rc<RefCell<Vec<Payload>>>,
    moves: Rc<RefCell<Vec<(Point<Pixels>, Option<MouseButton>, Modifiers)>>>,
}

impl Render for DragScene {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let dropped = self.dropped.clone();
        let moves = self.moves.clone();
        div()
            .size_full()
            .on_mouse_move(move |event: &MouseMoveEvent, _, _| {
                moves
                    .borrow_mut()
                    .push((event.position, event.pressed_button, event.modifiers))
            })
            .child(
                div()
                    .id("source")
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .size(px(50.))
                    .on_drag(Payload(7), |_, _, _, cx| cx.new(|_| DragPreview)),
            )
            .child(
                div()
                    .id("target")
                    .absolute()
                    .top(px(0.))
                    .left(px(200.))
                    .size(px(50.))
                    .on_drop(move |payload: &Payload, _, _| dropped.borrow_mut().push(*payload)),
            )
    }
}

#[gpux::test]
fn drag_along_a_path_holds_the_button_and_modifiers_and_drops(cx: &mut TestAppContext) {
    let dropped = Rc::new(RefCell::new(Vec::new()));
    let moves = Rc::new(RefCell::new(Vec::new()));
    let (_, cx) = cx.add_window_view({
        let dropped = dropped.clone();
        let moves = moves.clone();
        move |_, _| DragScene { dropped, moves }
    });
    let path = [
        point(px(25.), px(25.)),
        point(px(80.), px(25.)),
        point(px(160.), px(25.)),
        point(px(225.), px(25.)),
    ];
    cx.simulate_drag(&path, MouseButton::Left, Modifiers::shift());

    assert_eq!(*dropped.borrow(), [Payload(7)]);
    let moves = moves.borrow();
    // The move that starts the drag belongs to the drag source; later moves reach the scene.
    assert_eq!(
        moves
            .iter()
            .map(|(position, _, _)| *position)
            .collect::<Vec<_>>(),
        path[2..]
    );
    assert!(
        moves.iter().all(|(_, button, modifiers)| {
            *button == Some(MouseButton::Left) && *modifiers == Modifiers::shift()
        }),
        "every move holds the button and modifiers: {moves:?}"
    );
}

#[gpux::test]
fn window_requests_are_recorded_instead_of_panicking(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    cx.update(|window, _| {
        window.minimize_window();
        window.zoom_window();
        window.show_window_menu(point(px(12.), px(34.)));
        window.start_window_move();
        window.show_character_palette();
    });
    assert_eq!(
        cx.window_calls(),
        [
            TestWindowCall::Minimize,
            TestWindowCall::Zoom,
            TestWindowCall::ShowWindowMenu(point(px(12.), px(34.))),
            TestWindowCall::StartWindowMove,
            TestWindowCall::ShowCharacterPalette,
        ]
    );
}

#[gpux::test]
fn platform_requests_are_recorded_instead_of_panicking(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.hide();
        cx.hide_other_apps();
        cx.unhide_other_apps();
        cx.reveal_path(Path::new("conformance/report.txt"));
        cx.open_with_system(Path::new("conformance/scene.png"));
        cx.register_url_scheme("gpux-test").detach();
    });
    let launched = Rc::new(RefCell::new(false));
    cx.platform().run(Box::new({
        let launched = launched.clone();
        move || *launched.borrow_mut() = true
    }));
    assert!(*launched.borrow(), "run invokes the launch callback");
    assert!(cx.update(|cx| cx.app_path()).is_err());

    assert_eq!(
        cx.platform_calls(),
        [
            TestPlatformCall::Hide,
            TestPlatformCall::HideOtherApps,
            TestPlatformCall::UnhideOtherApps,
            TestPlatformCall::RevealPath("conformance/report.txt".into()),
            TestPlatformCall::OpenWithSystem("conformance/scene.png".into()),
            TestPlatformCall::RegisterUrlScheme("gpux-test".into()),
            TestPlatformCall::Run,
        ]
    );
}

#[gpux::test]
fn window_options_set_size_scale_factor_and_display(cx: &mut TestAppContext) {
    for scale_factor in [1., 2.] {
        let options = TestWindowOptions::new(size(px(320.), px(240.)))
            .scale_factor(scale_factor)
            .display_size(size(px(800.), px(600.)));
        let (_, cx) = cx.add_window_view_with_options(options, |_, _| Empty);
        let (viewport, window_scale, display) = cx.update(|window, cx| {
            (
                window.viewport_size(),
                window.scale_factor(),
                window.display(cx).map(|display| display.bounds().size),
            )
        });
        assert_eq!(viewport, size(px(320.), px(240.)));
        assert_eq!(window_scale, scale_factor);
        assert_eq!(cx.scale_factor(), scale_factor);
        assert_eq!(display, Some(size(px(800.), px(600.))));
    }
}

#[gpux::test]
fn default_test_windows_keep_their_historical_shape(cx: &mut TestAppContext) {
    let defaults = TestWindowOptions::default();
    assert_eq!(defaults.size, size(px(1920.), px(1080.)));
    assert_eq!(defaults.scale_factor, 2.);
    assert_eq!(defaults.display_size, None);

    let cx = cx.add_empty_window();
    let (viewport, scale_factor) =
        cx.update(|window, _| (window.viewport_size(), window.scale_factor()));
    assert_eq!(viewport, defaults.size);
    assert_eq!(scale_factor, defaults.scale_factor);
}
