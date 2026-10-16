//! `act` and `input`: every AccessKit action through `Window::perform_a11y_action`, every
//! synthesized input kind through the window's dispatch methods, typing after a focusing click,
//! and a ref that is gone.
//!
//! `cargo test -p gpux-devtools --test actions`

mod support;

use gpux::{AppContext as _, TestAppContext};
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
fn input_drags_wheels_pinches_and_long_presses() {
    let mut cx = TestAppContext::single();
    let (_window, _guard) = controls(&mut cx);
    let far = reference(&mut cx, "role=button name=Far");
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
    let save = reference(&mut cx, "role=button name=Save");
    request(
        &mut cx,
        "input",
        json!({"kind": "drag", "ref": save, "toRef": far}),
    )
    .unwrap();
    request(
        &mut cx,
        "input",
        json!({"kind": "pinch", "x": 10, "y": 10, "scale": 1.5}),
    )
    .unwrap();
    request(
        &mut cx,
        "input",
        json!({"kind": "longPress", "ref": save, "durationMs": 100}),
    )
    .unwrap();
    let drag = request(&mut cx, "input", json!({"kind": "drag", "ref": save})).unwrap_err();
    assert!(drag.message.contains("toRef"));
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
