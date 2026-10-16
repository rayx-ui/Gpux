//! `screenshot`, `wait` and `stats`: PNGs of a window and of a node's bounds, waits that match,
//! stop matching and time out, frame by frame, and the window's counters.
//!
//! `cargo test -p gpux-devtools --test observe`
#![cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]

mod support;

use base64::Engine as _;
use gpux::{AppContext as _, TestAppContext, TestDispatcher, TestWindowOptions, px, size};
use serde_json::json;
use support::*;

/// An app whose windows render, through the headless renderer.
fn rendering() -> TestAppContext {
    TestAppContext::build_with_renderer(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
        gpui_platform::current_headless_renderer,
    )
}

fn png(result: &serde_json::Value) -> image::RgbaImage {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(result["png"].as_str().unwrap())
        .unwrap();
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .unwrap()
        .to_rgba8()
}

#[test]
fn screenshots_of_a_window_and_of_a_node() {
    let mut cx = rendering();
    let window = cx.add_window_with_options(
        TestWindowOptions::new(size(px(300.), px(200.))).scale_factor(2.),
        |_, _| Panel { clicks: 0 },
    );
    let _guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    cx.run_until_parked();
    let whole = request(&mut cx, "screenshot", json!({})).unwrap();
    let image = png(&whole);
    assert_eq!((image.width(), image.height()), (600, 400), "device pixels");
    assert_eq!(whole["width"], 600);
    let save = reference(&mut cx, "role=button name=Save");
    let node = png(&request(&mut cx, "screenshot", json!({"ref": save})).unwrap());
    assert_eq!(
        (node.width(), node.height()),
        (160, 48),
        "the button's bounds at scale 2"
    );
}

#[test]
fn wait_resolves_when_a_query_starts_matching() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| Panel { clicks: 0 });
    let _guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    cx.run_until_parked();
    let task = cx.update(|cx| {
        gpux_devtools::requests::handle(
            cx,
            "wait",
            json!({"query": "text=\"clicks 2\"", "timeoutMs": 5000}),
        )
    });
    for _ in 0..2 {
        window
            .update(&mut cx, |panel, _, cx| {
                panel.clicks += 1;
                cx.notify();
            })
            .unwrap();
        pump(&mut cx);
    }
    let result = std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot = result.clone();
    cx.update(|cx| {
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach()
    });
    let result = until(&mut cx, "the wait", |_| result.borrow_mut().take()).unwrap();
    assert_eq!(result["matched"], true);
}

#[test]
fn wait_for_detached_and_timing_out() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| Panel { clicks: 0 });
    let _guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    cx.run_until_parked();
    let gone = request(
        &mut cx,
        "wait",
        json!({"query": "text=\"clicks 9\"", "state": "detached"}),
    )
    .unwrap();
    assert_eq!(gone["matched"], true, "nothing matches: detached at once");
    let error = request(
        &mut cx,
        "wait",
        json!({"query": "role=button name=Nope", "timeoutMs": 200}),
    )
    .unwrap_err();
    assert!(
        error.message.contains("did not match within 200 ms"),
        "{error}"
    );
}

#[test]
fn stats_report_the_tree_and_fast_mode_counters() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| Panel { clicks: 0 });
    let _guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let stats = request(&mut cx, "stats", json!({})).unwrap();
    assert!(stats["treeNodes"].as_u64().unwrap() >= 3, "{stats}");
    // Fast mode's counters come with the crate's `diagnostics` feature.
    if cfg!(feature = "diagnostics") {
        assert!(stats["fast"]["frames"].as_u64().is_some(), "{stats}");
    } else {
        assert!(stats["fast"].is_null(), "{stats}");
    }
    assert_eq!(stats["window"], window.window_id().as_u64());
}
