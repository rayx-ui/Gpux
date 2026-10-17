//! Owner-rendered inline objects (OBJ-2): the element paints each visible object's element in
//! the object's box; pointer events over an object reach its element first and fall back to
//! text selection when it does not handle them; without a renderer an object keeps its space and
//! paints nothing.

use gpux::{
    AppContext as _, Bounds, Context, EditableText, EditableTextLineMode, EditableTextState,
    Entity, InlineObject, IntoElement, MouseButton, MouseDownEvent, Pixels, Render, SharedString,
    TestApp, TestAppWindow, Window, canvas, div, point, prelude::*, px,
};
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct Probe {
    renders: Cell<usize>,
    bounds: Cell<Option<Bounds<Pixels>>>,
    clicks: Cell<usize>,
}

struct Field {
    state: Entity<EditableTextState>,
    probe: Option<Rc<Probe>>,
    handles_clicks: bool,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut field = EditableText::new(&self.state);
        if let Some(probe) = self.probe.clone() {
            let handles_clicks = self.handles_clicks;
            field = field.inline_object_renderer(move |object, _, _| {
                probe.renders.set(probe.renders.get() + 1);
                let painted = probe.clone();
                let clicked = probe.clone();
                div()
                    .id(object.key.clone())
                    .size_full()
                    .bg(gpux::rgb(0x00ff00))
                    .child(
                        canvas(
                            move |bounds, _, _| painted.bounds.set(Some(bounds)),
                            |_, _, _, _| {},
                        )
                        .size_full(),
                    )
                    .when(handles_clicks, |chip| {
                        chip.on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx| {
                            clicked.clicks.set(clicked.clicks.get() + 1);
                            cx.stop_propagation();
                        })
                    })
                    .into_any_element()
            });
        }
        div().w(px(300.)).h(px(200.)).child(field)
    }
}

fn chip() -> InlineObject {
    InlineObject {
        key: SharedString::from("chip"),
        width: px(50.),
        height: px(14.),
        baseline_offset: px(2.),
        text_fallback: SharedString::from("chip"),
        html_fallback: None,
    }
}

/// "ab" with the chip between "a" and "b", focused, with the caret at the end.
fn open(
    probe: Option<Rc<Probe>>,
    handles_clicks: bool,
) -> (TestApp, TestAppWindow<Field>, Entity<EditableTextState>) {
    let mut app = TestApp::new();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx.new(|cx| {
            let mut state =
                EditableTextState::with_line_mode("ab", EditableTextLineMode::MultiLine, cx);
            state.insert_inline_object(1..1, chip(), cx);
            state
        }),
        probe,
        handles_clicks,
    });
    let state = window.read(|field, _| field.state.clone());
    window.draw();
    window.update(|field, window, cx| {
        let handle = field.state.read(cx).focus_handle();
        window.focus(&handle, cx);
    });
    app.update_entity(&state, |state, cx| state.move_to_end(cx));
    window.draw();
    (app, window, state)
}

/// The chip's box in window coordinates.
fn chip_box(app: &TestApp, state: &Entity<EditableTextState>) -> Bounds<Pixels> {
    app.read_entity(state, |state, _| {
        let geometry = state.geometry().unwrap();
        let (_, _, bounds) = state.layout_map().inline_object_bounds()[0].clone();
        Bounds::new(
            geometry.bounds().origin
                + geometry.scroll_offset()
                + point(geometry.alignment_offset(), px(0.))
                + bounds.origin,
            bounds.size,
        )
    })
}

#[test]
fn the_owners_element_paints_in_the_objects_box() {
    let probe = Rc::new(Probe::default());
    let (app, _window, state) = open(Some(probe.clone()), true);
    let expected = chip_box(&app, &state);
    assert!(probe.renders.get() > 0);
    let painted = probe.bounds.get().expect("the chip's element was laid out");
    // Layout snaps the element's origin to device pixels.
    assert!(
        (painted.left() - expected.left()).abs() <= px(0.5),
        "{painted:?} vs {expected:?}"
    );
    assert!(
        (painted.top() - expected.top()).abs() <= px(0.5),
        "{painted:?} vs {expected:?}"
    );
    assert_eq!(painted.size, gpux::size(px(50.), px(14.)));
}

#[test]
fn an_objects_element_handles_pointer_events_over_it_first() {
    let probe = Rc::new(Probe::default());
    let (app, mut window, state) = open(Some(probe.clone()), true);
    let center = chip_box(&app, &state).center();
    window.simulate_click(center, MouseButton::Left);
    assert_eq!(probe.clicks.get(), 1);
    // The text did not take the press: the caret stays at the end.
    let end = app.read_entity(&state, |state, _| state.text().len());
    assert_eq!(
        app.read_entity(&state, |state, _| state.selection().head()),
        end
    );
}

#[test]
fn unhandled_pointer_events_over_an_object_select_text() {
    let probe = Rc::new(Probe::default());
    let (app, mut window, state) = open(Some(probe.clone()), false);
    let chip = chip_box(&app, &state);
    // Just inside the chip's left edge: the text puts the caret before it.
    window.simulate_click(
        point(chip.left() + px(2.), chip.center().y),
        MouseButton::Left,
    );
    assert_eq!(probe.clicks.get(), 0);
    assert_eq!(
        app.read_entity(&state, |state, _| state.selection().head()),
        1
    );
}

#[test]
fn without_a_renderer_an_object_keeps_its_space() {
    let (app, _window, state) = open(None, false);
    app.read_entity(&state, |state, _| {
        let map = state.layout_map();
        let before = map.position_for_index(1).unwrap().x;
        let after = map.position_for_index(1 + '\u{FFFC}'.len_utf8()).unwrap().x;
        assert!((after - before - px(50.)).abs() < px(0.01));
    });
}

#[test]
fn objects_outside_the_field_are_not_rendered() {
    let probe = Rc::new(Probe::default());
    let mut app = TestApp::new();
    let renderer_probe = probe.clone();
    let mut window = app.open_window(move |_, cx| Field {
        state: cx.new(|cx| {
            let text = format!("{}end", "row\n".repeat(30));
            let mut state = EditableTextState::with_line_mode(
                text.clone(),
                EditableTextLineMode::MultiLine,
                cx,
            );
            let end = text.len();
            state.insert_inline_object(end..end, chip(), cx);
            state.move_to_start(cx);
            state
        }),
        probe: Some(renderer_probe),
        handles_clicks: false,
    });
    window.draw();
    window.draw();
    // The chip's row is far below the 200 px viewport.
    assert_eq!(probe.renders.get(), 0);
}
