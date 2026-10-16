//! Lane B's browser host: a multithreaded WASM app that shows or runs conformance scenes in a
//! real browser through `gpui_web`.
//!
//! `gpux-testkit web build` builds it and generates its page; the page calls [`run`] once after
//! the module initializes. The URL selects what it does:
//!
//! - `?scene=<id>` shows the scene, for Playwright specs that drive trusted input at located
//!   bounds and probe screenshots;
//! - `?scene=<id>&run=1` also runs the scene's script in-app and logs its
//!   `gpux-conformance:` result line; `?run=all` runs every lane-B scene;
//! - `?backend=webgpu|webgl` forces a backend (WebGPU first with a WebGL2 fallback otherwise);
//! - `?layers=1|0` sets the shown scene's [`FastMode`]: every mechanism on (scroll layers among
//!   them) or all off. Without it the window keeps its default.
//!
//! It publishes `globalThis.__gpuxConformance`: `ready`, `backend`, `scene`, `results`, `done`,
//! `lastError`, `probes` (the scene's pixel expectations for its first frame, as
//! `{x, y, color: [r, g, b, a], tolerance}` in CSS pixels), and `locate(query)`, which resolves a
//! semantic query (its text form, for example `role=button name="Save"`) to CSS-pixel bounds
//! `{x, y, width, height}`, or `null` with the reason in `lastLocateError`.
//!
//! A shown scene also publishes its script for trusted-input driving (see [`bridge`]): `steps`,
//! `check(index)`, `pixels(index, png)` and `focusPoint(query)`.
//!
//! Browser-only scenes ([`web_scenes`]) cover gpui_web areas the shared catalog cannot, and
//! [`fixtures`] exports the queue and glyph fixtures the thread and text-fallback specs call.
//! `close()` closes the shown scene's window.

// The atomic-wait negative control needs the nightly intrinsic; browser builds use nightly.
#![cfg_attr(target_family = "wasm", feature(stdarch_wasm_atomic_wait))]

#[cfg(target_family = "wasm")]
mod bridge;
#[cfg(target_family = "wasm")]
mod fixtures;
#[cfg(target_family = "wasm")]
mod web_scenes;

#[cfg(target_family = "wasm")]
mod web {
    use crate::bridge::Bridge;
    use gpux::{
        AnyWindowHandle, AppContext as _, AsyncApp, FastMode, WindowOptions, point, px,
        semantic_query::Query,
    };
    use gpux_tests_conformance::{
        CATALOG, Lane, SceneSpec, Step,
        app_host::{self, AppHost, SceneFrame},
        report::ResultLine,
    };
    use std::{cell::RefCell, str::FromStr};
    use wasm_bindgen::prelude::*;

    struct Shown {
        cx: AsyncApp,
        window: AnyWindowHandle,
        bridge: Bridge,
    }

    thread_local! {
        static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
    }

    struct Params {
        scene: Option<String>,
        run: Option<String>,
        backend: Option<String>,
        layers: Option<String>,
    }

    impl Params {
        fn from_location() -> Self {
            let search = web_sys::window()
                .and_then(|window| window.location().search().ok())
                .unwrap_or_default();
            let params = web_sys::UrlSearchParams::new_with_str(&search).ok();
            let get = |name: &str| params.as_ref().and_then(|params| params.get(name));
            Self {
                scene: get("scene"),
                run: get("run"),
                backend: get("backend"),
                layers: get("layers"),
            }
        }
    }

    fn conformance() -> js_sys::Object {
        let global = js_sys::global();
        let key = JsValue::from_str("__gpuxConformance");
        if let Ok(object) =
            js_sys::Reflect::get(&global, &key).and_then(|value| value.dyn_into::<js_sys::Object>())
        {
            return object;
        }
        let object = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&global, &key, &object);
        object
    }

    fn set(key: &str, value: impl Into<JsValue>) {
        let _ = js_sys::Reflect::set(&conformance(), &JsValue::from_str(key), &value.into());
    }

    fn fail(message: impl Into<String>) {
        let message = message.into();
        web_sys::console::error_1(&JsValue::from_str(&format!("web_host: {message}")));
        set("lastError", message);
    }

    /// Reports a locate miss without failing the page: specs assert absence through it.
    fn miss(message: impl Into<String>) {
        let message = message.into();
        web_sys::console::warn_1(&JsValue::from_str(&format!("web_host: {message}")));
        set("lastLocateError", message);
    }

    /// The scene's pixel probes that hold before its script changes anything.
    fn first_frame_probes(scene: &SceneSpec) -> js_sys::Array {
        let probes = js_sys::Array::new();
        for step in (scene.script)() {
            match step {
                Step::Probe(x, y, color, tolerance) => {
                    let probe = js_sys::Object::new();
                    let color: js_sys::Array = color
                        .iter()
                        .map(|channel| JsValue::from(*channel))
                        .collect();
                    for (key, value) in [
                        ("x", JsValue::from(x)),
                        ("y", JsValue::from(y)),
                        ("color", color.into()),
                        ("tolerance", JsValue::from(tolerance)),
                    ] {
                        let _ = js_sys::Reflect::set(&probe, &JsValue::from_str(key), &value);
                    }
                    probes.push(&probe);
                }
                Step::Expect(_)
                | Step::ProbeUniform(..)
                | Step::ProbeDiffer(..)
                | Step::Capture(_)
                | Step::Golden
                | Step::GoldenWithin(_) => {}
                _ => break,
            }
        }
        probes
    }

    fn record(line: ResultLine) {
        web_sys::console::log_1(&JsValue::from_str(&line.to_string()));
        let results = js_sys::Reflect::get(&conformance(), &JsValue::from_str("results"))
            .ok()
            .and_then(|value| value.dyn_into::<js_sys::Array>().ok())
            .unwrap_or_else(|| {
                let results = js_sys::Array::new();
                set("results", results.clone());
                results
            });
        results.push(&JsValue::from_str(&line.to_string()));
    }

    /// Resolves a semantic query against the shown scene's latest frame.
    fn locate(query: String) -> JsValue {
        let query = match Query::from_str(&query) {
            Ok(query) => query,
            Err(error) => {
                miss(format!("bad query {query:?}: {error}"));
                return JsValue::NULL;
            }
        };
        SHOWN.with(|shown| {
            let mut shown = shown.borrow_mut();
            let Some(shown) = shown.as_mut() else {
                miss("no scene is shown");
                return JsValue::NULL;
            };
            let window = shown.window;
            let snapshot = match window.update(&mut shown.cx, |_, window, _| {
                window.devtools_frame_snapshot()
            }) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    miss(format!("the scene window is gone: {error}"));
                    return JsValue::NULL;
                }
            };
            match query.resolve(&snapshot) {
                Ok(element) => {
                    let bounds = js_sys::Object::new();
                    for (key, value) in [
                        ("x", element.bounds.x),
                        ("y", element.bounds.y),
                        ("width", element.bounds.width),
                        ("height", element.bounds.height),
                    ] {
                        let _ = js_sys::Reflect::set(
                            &bounds,
                            &JsValue::from_str(key),
                            &JsValue::from_f64(value as f64),
                        );
                    }
                    bounds.into()
                }
                Err(error) => {
                    miss(error.to_string());
                    JsValue::NULL
                }
            }
        })
    }

    fn with_shown(run: impl FnOnce(&mut Shown) -> JsValue) -> JsValue {
        SHOWN.with(|shown| match shown.borrow_mut().as_mut() {
            Some(shown) => run(shown),
            None => JsValue::from_str("no scene is shown"),
        })
    }

    fn outcome(problem: Option<String>) -> JsValue {
        problem.map_or(JsValue::NULL, |problem| JsValue::from_str(&problem))
    }

    /// `check(index)`: `null` when the expectation holds, the reason otherwise.
    fn check(index: u32) -> JsValue {
        with_shown(|shown| outcome(shown.bridge.check(&mut shown.cx, index as usize)))
    }

    /// `pixels(index, png)`: `null` when the pixel step holds for the page's PNG screenshot.
    fn pixels(index: u32, png: js_sys::Uint8Array) -> JsValue {
        with_shown(|shown| {
            outcome(
                shown
                    .bridge
                    .pixels(&mut shown.cx, index as usize, &png.to_vec()),
            )
        })
    }

    /// `focusPoint(query)`: where to click to focus the element, `null` when focus is already
    /// inside it; throws when nothing matches.
    fn focus_point(query: String) -> Result<JsValue, JsValue> {
        SHOWN.with(|shown| match shown.borrow_mut().as_mut() {
            Some(shown) => shown
                .bridge
                .focus_point(&mut shown.cx, &query)
                .map_err(|error| JsValue::from_str(&error)),
            None => Err(JsValue::from_str("no scene is shown")),
        })
    }

    /// `close()`: closes the shown scene's window; `true` when one was open.
    fn close() -> JsValue {
        SHOWN.with(|shown| {
            let Some(mut shown) = shown.borrow_mut().take() else {
                return JsValue::FALSE;
            };
            let window = shown.window;
            JsValue::from_bool(
                window
                    .update(&mut shown.cx, |_, window, _| window.remove_window())
                    .is_ok(),
            )
        })
    }

    fn backend_name(cx: &mut AsyncApp, window: AnyWindowHandle) -> String {
        let backend = window
            .update(cx, |_, window, _| window.gpu_specs())
            .ok()
            .flatten()
            .map(|specs| specs.backend.to_ascii_lowercase())
            .unwrap_or_default();
        if backend.contains("webgpu") {
            "webgpu".into()
        } else if backend.contains("gl") {
            "webgl".into()
        } else {
            backend
        }
    }

    async fn show(cx: &mut AsyncApp, scene: &'static SceneSpec, layers: Option<&str>) {
        let window: AnyWindowHandle = match cx
            .open_window(WindowOptions::default(), |window, cx| {
                cx.new(|cx| SceneFrame::new(scene, window, cx))
            }) {
            Ok(window) => window.into(),
            Err(error) => return fail(format!("could not open {}: {error:#}", scene.id)),
        };
        let mode = match layers {
            Some("1") => Some(FastMode::ALL),
            Some("0") => Some(FastMode::OFF),
            _ => None,
        };
        if let Some(mode) = mode {
            let _ = window.update(cx, |_, window, _| window.set_fast_mode(mode));
        }
        if let Err(error) = app_host::wait_frames(cx, window, 2).await {
            return fail(format!("{} never drew: {error:#}", scene.id));
        }
        set("backend", backend_name(cx, window));
        set("probes", first_frame_probes(scene));
        let bridge = Bridge::new(cx, window, scene);
        set("steps", bridge.steps());
        SHOWN.with(|shown| {
            *shown.borrow_mut() = Some(Shown {
                cx: cx.clone(),
                window,
                bridge,
            })
        });
        set("ready", true);
    }

    async fn run_scripts(cx: &mut AsyncApp, scenes: Vec<&'static SceneSpec>) {
        let host = AppHost::new(Lane::B, point(px(0.), px(0.)));
        for scene in scenes {
            record(host.run_scene(cx, scene).await);
        }
        set("done", true);
    }

    /// Starts the host. The generated page calls it once, after the module initializes.
    #[wasm_bindgen]
    pub fn run() {
        console_error_panic_hook::set_once();
        gpui_platform::web_init();
        let params = Params::from_location();
        set("ready", false);
        set("done", false);
        set("results", js_sys::Array::new());
        set("probes", js_sys::Array::new());
        set(
            "scene",
            params
                .scene
                .clone()
                .map(JsValue::from)
                .unwrap_or(JsValue::NULL),
        );
        let locate = Closure::<dyn Fn(String) -> JsValue>::new(locate);
        set("locate", locate.as_ref().clone());
        locate.forget();
        let check = Closure::<dyn Fn(u32) -> JsValue>::new(check);
        set("check", check.as_ref().clone());
        check.forget();
        let pixels = Closure::<dyn Fn(u32, js_sys::Uint8Array) -> JsValue>::new(pixels);
        set("pixels", pixels.as_ref().clone());
        pixels.forget();
        let close = Closure::<dyn Fn() -> JsValue>::new(close);
        set("close", close.as_ref().clone());
        close.forget();
        let focus_point = Closure::<dyn Fn(String) -> Result<JsValue, JsValue>>::new(focus_point);
        set("focusPoint", focus_point.as_ref().clone());
        focus_point.forget();

        let preference = match params.backend.as_deref() {
            Some("webgl") => gpui_platform::WebBackendPreference::WebGl,
            Some("webgpu") => gpui_platform::WebBackendPreference::WebGpu,
            _ => gpui_platform::WebBackendPreference::Auto,
        };
        gpui_platform::application_with_web_backend(preference).run(move |mut cx| async move {
            if let Err(error) = cx.update(app_host::register_bundled_fonts) {
                fail(format!("could not register the bundled fonts: {error:#}"));
            }
            let scene = params.scene.as_deref().map(|id| {
                CATALOG
                    .iter()
                    .find(|scene| scene.id == id)
                    .or_else(|| crate::web_scenes::scene(id))
                    .ok_or_else(|| format!("no conformance scene `{id}`"))
            });
            match (params.run.as_deref(), scene) {
                (Some("all"), _) => {
                    let scenes = CATALOG
                        .iter()
                        .filter(|scene| scene.runs_in(Lane::B))
                        .collect();
                    run_scripts(&mut cx, scenes).await;
                }
                (Some(_), Some(Ok(scene))) => run_scripts(&mut cx, vec![scene]).await,
                (None, Some(Ok(scene))) => show(&mut cx, scene, params.layers.as_deref()).await,
                (_, Some(Err(error))) => fail(error),
                (_, None) => {
                    fail("name a scene with `?scene=<id>`, or run every scene with `?run=all`")
                }
            }
        });
    }
}

#[cfg(target_family = "wasm")]
pub use web::run;
