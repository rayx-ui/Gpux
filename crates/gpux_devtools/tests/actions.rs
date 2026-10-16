//! `act` and `input`: every AccessKit action through `Window::perform_a11y_action`, every
//! synthesized input kind through the window's dispatch methods, typing after a focusing click,
//! and a ref that is gone.
//!
//! `cargo test -p gpux-devtools --test actions`

mod support;

use std::{cell::RefCell, rc::Rc};

use gpux::{
    AppContext as _, Context, DispatchPhase, IntoElement, LongPressEvent, MouseButton,
    MouseMoveEvent, Render, Role, TestAppContext, Window, canvas, div, prelude::*, px,
};
use serde_json::json;
use support::*;

fn act(cx: &mut TestAppContext, query: &str, action: serde_json::Value) -> bool {
    let reference = reference(cx, query);
    let mut params = action;
    params["ref"] = json!(reference);
    let result = request(cx, "act", params).unwrap();
    cx.run_until_parked();
    result["handled"].as_bool().unwrap()
}

#[test]
fn act_clicks_and_focuses() {
    let mut cx = TestAppContext::single();
    let (window, _guard) = controls(&mut cx);
    assert!(act(
        &mut cx,
        "role=button name=Save",
        json!({"action": "click"})
    ));
    assert!(status(&mut cx).contains("click 1"), "{}", status(&mut cx));
    assert!(act(
        &mut cx,
        "role=checkbox name=Wrap",
        json!({"action": "focus"})
    ));
    let focused = cx
        .update_window(window.into(), |_, window, cx| window.focused(cx).is_some())
        .unwrap();
    assert!(focused);
}

#[test]
fn act_sets_steps_and_discloses() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    assert!(act(
        &mut cx,
        "role=slider",
        json!({"action": "setValue", "value": 8})
    ));
    assert!(act(&mut cx, "role=slider", json!({"action": "increment"})));
    assert!(act(&mut cx, "role=slider", json!({"action": "decrement"})));
    assert!(act(
        &mut cx,
        "role=button name=Details",
        json!({"action": "expand"})
    ));
    assert!(
        status(&mut cx).ends_with("volume 8 expanded true"),
        "{}",
        status(&mut cx)
    );
    assert!(act(
        &mut cx,
        "role=button name=Details",
        json!({"action": "collapse"})
    ));
    assert!(status(&mut cx).ends_with("expanded false"));
}

#[test]
fn act_runs_custom_actions_by_id_and_scrolls_into_view() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    assert!(act(
        &mut cx,
        "role=group name=Card",
        json!({"action": "customAction", "id": 3})
    ));
    assert!(!act(
        &mut cx,
        "role=group name=Card",
        json!({"action": "customAction", "id": 4})
    ));
    assert!(status(&mut cx).contains("archive"));
    let before = request(&mut cx, "query", json!({"query": "role=button name=Far"})).unwrap()
        ["nodes"][0]["bounds"]["y"]
        .as_f64()
        .unwrap();
    assert!(act(
        &mut cx,
        "role=button name=Far",
        json!({"action": "scrollIntoView"})
    ));
    let after = request(&mut cx, "query", json!({"query": "role=button name=Far"})).unwrap()["nodes"]
        [0]["bounds"]["y"]
        .as_f64()
        .unwrap();
    assert!(after < before, "{before} -> {after}");
}

#[test]
fn act_reports_a_request_nothing_handles() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    assert!(!act(
        &mut cx,
        "role=button name=Save",
        json!({"action": "increment"})
    ));
    let unknown = request(&mut cx, "act", json!({"ref": "1", "action": "juggle"})).unwrap_err();
    assert!(unknown.message.contains("juggle"));
}

#[test]
fn a_gone_ref_fails_naming_it() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    for method in ["act", "input"] {
        let error = request(
            &mut cx,
            method,
            json!({"ref": "424242", "action": "click", "kind": "click"}),
        )
        .unwrap_err();
        assert!(error.message.contains("424242"), "{method}: {error}");
    }
}

#[test]
fn input_clicks_double_clicks_and_hovers() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let save = reference(&mut cx, "role=button name=Save");
    request(&mut cx, "input", json!({"kind": "click", "ref": save})).unwrap();
    request(
        &mut cx,
        "input",
        json!({"kind": "click", "ref": save, "count": 2}),
    )
    .unwrap();
    request(&mut cx, "input", json!({"kind": "hover", "x": 5, "y": 5})).unwrap();
    cx.run_until_parked();
    assert!(
        status(&mut cx).contains("click 1,click 1,click 2"),
        "{}",
        status(&mut cx)
    );
}

#[test]
fn input_wheels_the_scroll_container_under_it() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let y = |cx: &mut TestAppContext| {
        request(cx, "query", json!({"query": "role=button name=Far"})).unwrap()["nodes"][0]["bounds"]["y"]
            .as_f64()
            .unwrap()
    };
    let before = y(&mut cx);
    let shelf = reference(&mut cx, "id=shelf");
    request(
        &mut cx,
        "input",
        json!({"kind": "wheel", "ref": shelf, "deltaY": -60}),
    )
    .unwrap();
    cx.run_until_parked();
    assert!(y(&mut cx) < before, "the wheel scrolled the shelf");
}

/// A pad recording the gestures that reach it: presses, moves with a button held, hovering,
/// pinches and long presses.
struct Gestures {
    log: Rc<RefCell<Vec<String>>>,
}

impl Render for Gestures {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let log = self.log.clone();
        let entry = move |text: String| log.borrow_mut().push(text);
        let (down, moved, up, hover, pinch, press) = (
            entry.clone(),
            entry.clone(),
            entry.clone(),
            entry.clone(),
            entry.clone(),
            entry,
        );
        div()
            .size_full()
            .on_mouse_down(MouseButton::Left, move |event, _, _| {
                down(format!("down {}", event.position.x.as_f32()))
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, _| {
                if event.pressed_button == Some(MouseButton::Left) {
                    moved(format!("drag {}", event.position.x.as_f32()))
                }
            })
            .on_mouse_up(MouseButton::Left, move |event, _, _| {
                up(format!("up {}", event.position.x.as_f32()))
            })
            .on_pinch(move |event, _, _| pinch(format!("pinch {:.1}", event.delta)))
            .child(
                div()
                    .id("pad")
                    .role(Role::Group)
                    .aria_label("Pad")
                    .w(px(40.))
                    .h(px(40.))
                    .on_hover(move |hovered, _, _| hover(format!("hover {hovered}"))),
            )
            .child(
                div()
                    .id("target")
                    .role(Role::Group)
                    .aria_label("Target")
                    .ml(px(100.))
                    .w(px(40.))
                    .h(px(40.)),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let press = press.clone();
                        window.on_mouse_event(move |event: &LongPressEvent, phase, window, _| {
                            if phase == DispatchPhase::Bubble {
                                press(format!("long {:?}", event.phase));
                                // Claimed, the press runs to its end instead of falling back
                                // to a tap.
                                window.capture_long_press(&entity);
                                window.prevent_default();
                            }
                        });
                    },
                )
                .size_full(),
            )
    }
}

/// What reached the pad, hovering aside.
fn without_hover(log: &Rc<RefCell<Vec<String>>>) -> Vec<String> {
    log.borrow()
        .iter()
        .filter(|entry| !entry.starts_with("hover"))
        .cloned()
        .collect()
}

fn gestures(cx: &mut TestAppContext) -> (Rc<RefCell<Vec<String>>>, gpux::A11yInspection) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let window = cx.add_window({
        let log = log.clone();
        |_, _| Gestures { log }
    });
    let guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    cx.run_until_parked();
    (log, guard)
}

#[test]
fn input_drags_from_one_node_to_another() {
    let mut cx = TestAppContext::single();
    let (log, _guard) = gestures(&mut cx);
    let pad = reference(&mut cx, "name=Pad");
    let target = reference(&mut cx, "name=Target");
    request(
        &mut cx,
        "input",
        json!({"kind": "drag", "ref": pad, "toRef": target}),
    )
    .unwrap();
    cx.run_until_parked();
    // The press lands on the pad's center and the move and release on the target's.
    assert_eq!(without_hover(&log), ["down 20", "drag 120", "up 120"]);
    let drag = request(&mut cx, "input", json!({"kind": "drag", "ref": pad})).unwrap_err();
    assert!(drag.message.contains("toRef"));
}

#[test]
fn input_presses_moves_and_releases_apart() {
    let mut cx = TestAppContext::single();
    let (log, _guard) = gestures(&mut cx);
    for (kind, x) in [("mouseDown", 10), ("mouseUp", 30)] {
        request(&mut cx, "input", json!({"kind": kind, "x": x, "y": 10})).unwrap();
    }
    assert_eq!(without_hover(&log), ["down 10", "up 30"]);
}

#[test]
fn input_hovers_a_node() {
    let mut cx = TestAppContext::single();
    let (log, _guard) = gestures(&mut cx);
    let pad = reference(&mut cx, "name=Pad");
    let target = reference(&mut cx, "name=Target");
    request(&mut cx, "input", json!({"kind": "hover", "ref": target})).unwrap();
    log.borrow_mut().clear();
    request(&mut cx, "input", json!({"kind": "hover", "ref": pad})).unwrap();
    request(
        &mut cx,
        "input",
        json!({"kind": "mouseMove", "ref": target}),
    )
    .unwrap();
    cx.run_until_parked();
    assert_eq!(*log.borrow(), ["hover true", "hover false"]);
}

#[test]
fn input_pinches_by_a_scale() {
    let mut cx = TestAppContext::single();
    let (log, _guard) = gestures(&mut cx);
    request(
        &mut cx,
        "input",
        json!({"kind": "pinch", "x": 10, "y": 10, "scale": 1.5}),
    )
    .unwrap();
    cx.run_until_parked();
    assert_eq!(without_hover(&log), ["pinch 0.5"]);
}

#[test]
fn input_long_presses_for_its_duration() {
    let mut cx = TestAppContext::single();
    let (log, _guard) = gestures(&mut cx);
    let pad = reference(&mut cx, "name=Pad");
    request(
        &mut cx,
        "input",
        json!({"kind": "longPress", "ref": pad, "durationMs": 700}),
    )
    .unwrap();
    cx.run_until_parked();
    // The hold outlasted the recognizer's delay, and lifting ended the press.
    assert_eq!(without_hover(&log), ["long Started", "long Ended"]);
}

#[test]
fn input_types_after_a_focusing_click_and_presses_keys() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let title = reference(&mut cx, "role=textbox name=Title");
    request(
        &mut cx,
        "input",
        json!({"kind": "type", "ref": title, "text": "Hello"}),
    )
    .unwrap();
    cx.run_until_parked();
    let value = |cx: &mut TestAppContext| {
        request(cx, "query", json!({"query": "role=textbox name=Title"})).unwrap()["nodes"][0]["value"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(value(&mut cx), "Hello");
    request(
        &mut cx,
        "input",
        json!({"kind": "press", "keys": ["backspace", "backspace"]}),
    )
    .unwrap();
    cx.run_until_parked();
    assert_eq!(value(&mut cx), "Hel");
    let bad = request(
        &mut cx,
        "input",
        json!({"kind": "press", "keys": ["not-a-key-at-all-"]}),
    )
    .unwrap_err();
    assert!(bad.message.contains("not-a-key"), "{bad}");
}
