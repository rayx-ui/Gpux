//! The browser entry point: `web/index.html` loads the module and calls [`run`].
//!
//! The page's query names the run: `?scenario=<id>&fast=default|on|off&frames=600&warmup=120`
//! `&label=<config>&target=<name>&backend=webgpu|webgl`. The window is the page's canvas. The
//! report is published as `globalThis.__gpuxBench = { done, report, error }` and logged as one
//! `gpux-bench-result: <json>` line; `web/run.mjs` reads it.

use gpux::{App, WindowOptions};
use wasm_bindgen::prelude::*;

use crate::driver::{self, Config, Fast};

fn publish(key: &str, value: JsValue) {
    let global = js_sys::global();
    let bench = js_sys::Reflect::get(&global, &"__gpuxBench".into())
        .ok()
        .filter(|value| value.is_object())
        .unwrap_or_else(|| {
            let object: JsValue = js_sys::Object::new().into();
            js_sys::Reflect::set(&global, &"__gpuxBench".into(), &object).ok();
            object
        });
    js_sys::Reflect::set(&bench, &key.into(), &value).ok();
}

fn fail(message: String) {
    web_sys::console::error_1(&format!("gpux-bench-error: {message}").into());
    publish("error", message.into());
}

#[wasm_bindgen]
pub fn run() {
    console_error_panic_hook::set_once();
    gpui_platform::web_init();
    publish("done", false.into());

    let params = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|search| web_sys::UrlSearchParams::new_with_str(&search).ok())
        .expect("the page has a query");
    let param = |name: &str| params.get(name);
    let number = |name: &str, default: u64| {
        param(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let Some(scenario) = param("scenario").as_deref().and_then(crate::scenario) else {
        return fail("name a scenario with ?scenario=<id>".into());
    };
    let Some(fast) = Fast::parse(param("fast").as_deref().unwrap_or("default")) else {
        return fail("?fast= takes default, on or off".into());
    };
    let config = Config {
        scenario,
        frames: number("frames", 600),
        warmup: number("warmup", 120),
        label: param("label").unwrap_or_else(|| "unlabelled".into()),
        target: param("target").unwrap_or_else(|| "web".into()),
        fast,
        inspect: param("inspect").as_deref() == Some("1"),
        on_measuring: Some(Box::new(|| publish("measuring", true.into()))),
    };
    let backend = match param("backend").as_deref() {
        Some("webgl") => gpui_platform::WebBackendPreference::WebGl,
        Some("webgpu") => gpui_platform::WebBackendPreference::WebGpu,
        _ => gpui_platform::WebBackendPreference::Auto,
    };

    // The browser has no environment for `GPUX_ACCESSIBILITY`: the page names the mode.
    let accessibility = match param("accessibility").as_deref() {
        Some("always") => Some(gpux::AccessibilityMode::Always),
        Some("off") => Some(gpux::AccessibilityMode::Off),
        Some("auto") => Some(gpux::AccessibilityMode::Auto),
        None => None,
        Some(other) => return fail(format!("?accessibility= takes auto, always or off, not {other}")),
    };

    gpui_platform::application_with_web_backend(backend).run(move |cx| async move {
        cx.update(|cx: &mut App| {
            if let Some(mode) = accessibility {
                cx.set_accessibility_mode(mode);
            }
            crate::register_fonts(cx);
            driver::open(
                cx,
                config,
                WindowOptions::default(),
                Box::new(|mut report, window, _| {
                    report["backend"] = window
                        .gpu_specs()
                        .map(|specs| format!("{:?}", specs.backend).trim_matches('"').into())
                        .unwrap_or(serde_json::Value::Null);
                    let text = report.to_string();
                    web_sys::console::log_1(&format!("gpux-bench-result: {text}").into());
                    publish("report", text.into());
                    publish("done", true.into());
                }),
            );
        });
    });
}
