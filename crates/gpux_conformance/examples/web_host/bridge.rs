//! Trusted-input driving of a shown scene. Playwright performs the scene script's input steps
//! with real browser input (CDP mouse, keyboard, touch and IME events through `gpui_web`'s DOM
//! listeners), and asks the app to check the script's expectations and pixel steps, so lane B
//! keeps the scene's own assertions without injecting input behind the browser's back.
//!
//! [`Bridge::steps`] lists the flattened script as descriptors: `{index, describe, kind, ...}`
//! with `kind` one of `click`, `clickAt`, `hover`, `moveTo`, `press`, `release`, `drag`, `wheel`,
//! `touchPan`, `touchTap`, `type`, `keys`, `keyUp`, `imeCompose`, `imeCommit`, `imeCancel`,
//! `focus`, `advance`, `expect`, `pixels` and `skip`. Queries use their text form; keys use
//! Playwright's names. The driver answers `expect` steps through [`Bridge::check`] and `pixels`
//! steps through [`Bridge::pixels`] with a PNG screenshot of the page at device pixels.

use gpux::{AnyWindowHandle, AsyncApp, Keystroke, semantic_query};
use gpux_conformance::{
    Expect, SceneSpec, Step, evaluate,
    host::describe,
    scene::{Capability, flatten},
};
use image::RgbaImage;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

/// A resolved script step: one the driver runs, or one this lane skips.
enum Planned {
    Run(Step),
    Skip(Step, String),
}

/// The shown scene's script and the frames its `Capture` steps remembered.
pub struct Bridge {
    window: AnyWindowHandle,
    steps: Vec<Planned>,
    captures: HashMap<&'static str, RgbaImage>,
}

impl Bridge {
    /// Resolves `scene`'s script against the window's renderer capabilities.
    pub fn new(cx: &mut AsyncApp, window: AnyWindowHandle, scene: &'static SceneSpec) -> Self {
        let mut steps = Vec::new();
        resolve(cx, window, &(scene.script)(), &mut steps);
        Self {
            window,
            steps,
            captures: HashMap::new(),
        }
    }

    /// The step descriptors, in order.
    pub fn steps(&self) -> js_sys::Array {
        self.steps
            .iter()
            .enumerate()
            .map(|(index, planned)| JsValue::from(descriptor(index, planned)))
            .collect()
    }

    /// Checks the `expect` step at `index` against the latest frame: `None` when it holds.
    pub fn check(&self, cx: &mut AsyncApp, index: usize) -> Option<String> {
        let Some(Planned::Run(Step::Expect(expect))) = self.steps.get(index) else {
            return Some(format!("step {index} is not an expectation"));
        };
        let result = self.window.update(cx, |_, window, _| {
            let scale = window.scale_factor();
            // Layout snaps to device pixels, so allow one device pixel at the page's scale.
            evaluate::expect(
                &window.devtools_frame_snapshot(),
                expect,
                1. / scale.max(1.),
            )
        });
        match result {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(error) => Some(format!("the scene window is gone: {error:#}")),
        }
    }

    /// Checks the `pixels` step at `index` against a PNG screenshot of the page at device
    /// pixels: `None` when it holds.
    pub fn pixels(&mut self, cx: &mut AsyncApp, index: usize, png: &[u8]) -> Option<String> {
        let Some(Planned::Run(step)) = self.steps.get(index) else {
            return Some(format!("step {index} is not a pixel step"));
        };
        if !step.needs_pixels() {
            return Some(format!("step {index} is not a pixel step"));
        }
        let frame = match image::load_from_memory_with_format(png, image::ImageFormat::Png) {
            Ok(image) => image.to_rgba8(),
            Err(error) => return Some(format!("the screenshot is not a PNG: {error}")),
        };
        let scale = match self.window.update(cx, |_, window, _| window.scale_factor()) {
            Ok(scale) => scale,
            Err(error) => return Some(format!("the scene window is gone: {error:#}")),
        };
        evaluate::pixel_step(step, &frame, scale, &mut self.captures)
            .err()
            .map(|error| format!("{:#}", error.context(format!("at scale {scale}"))))
    }

    /// Where to click to focus the element `query` matches, or `null` when focus is already
    /// inside it.
    pub fn focus_point(&self, cx: &mut AsyncApp, query: &str) -> Result<JsValue, String> {
        let query: semantic_query::Query = query
            .parse()
            .map_err(|error| format!("bad query {query:?}: {error}"))?;
        let snapshot = self
            .window
            .update(cx, |_, window, _| window.devtools_frame_snapshot())
            .map_err(|error| format!("the scene window is gone: {error:#}"))?;
        let element = query
            .resolve(&snapshot)
            .map_err(|error| error.to_string())?;
        if semantic_query::focus_is_within(&snapshot, element) {
            return Ok(JsValue::NULL);
        }
        let center = evaluate::center(element);
        Ok(point(f32::from(center.x), f32::from(center.y)).into())
    }
}

fn supports(cx: &mut AsyncApp, window: AnyWindowHandle, capability: Capability) -> bool {
    let Ok(capabilities) = window.update(cx, |_, window, _| window.effect_capabilities()) else {
        return false;
    };
    match capability {
        Capability::ElementEffects => {
            capabilities.element.backdrop_blur && capabilities.element.glow
        }
        Capability::SnapshotTransitions => capabilities.screen.snapshot_transitions,
    }
}

fn resolve(cx: &mut AsyncApp, window: AnyWindowHandle, script: &[Step], out: &mut Vec<Planned>) {
    for step in script {
        match step {
            Step::When(capability, inner) => {
                if supports(cx, window, *capability) {
                    resolve(cx, window, inner, out);
                } else {
                    let reason = format!("the renderer has no {}", capability.describe());
                    out.extend(
                        flatten(inner)
                            .into_iter()
                            .map(|step| Planned::Skip(step.clone(), reason.clone())),
                    );
                }
            }
            Step::Expect(Expect::Accessible(_)) => out.push(Planned::Skip(
                step.clone(),
                "the accessibility tree needs an assistive-technology client".into(),
            )),
            Step::Golden | Step::GoldenWithin(_) => out.push(Planned::Skip(
                step.clone(),
                "lane b keeps no goldens".into(),
            )),
            step => out.push(Planned::Run(step.clone())),
        }
    }
}

fn point(x: f32, y: f32) -> js_sys::Object {
    let object = js_sys::Object::new();
    set(&object, "x", x);
    set(&object, "y", y);
    object
}

fn set(object: &js_sys::Object, key: &str, value: impl Into<JsValue>) {
    let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), &value.into());
}

/// Playwright's name for a gpui keystroke, such as `Control+Shift+Tab`.
pub fn playwright_key(keystroke: &Keystroke) -> String {
    let modifiers = &keystroke.modifiers;
    let mut parts: Vec<String> = [
        (modifiers.control, "Control"),
        (modifiers.alt, "Alt"),
        (modifiers.shift, "Shift"),
        (modifiers.platform, "Meta"),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .map(|(_, name)| name.to_string())
    .collect();
    let key = match keystroke.key.as_str() {
        "enter" => "Enter".to_string(),
        "escape" => "Escape".to_string(),
        "tab" => "Tab".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Delete".to_string(),
        "space" => "Space".to_string(),
        "left" => "ArrowLeft".to_string(),
        "right" => "ArrowRight".to_string(),
        "up" => "ArrowUp".to_string(),
        "down" => "ArrowDown".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "pageup" => "PageUp".to_string(),
        "pagedown" => "PageDown".to_string(),
        key if key.len() > 1 && key.starts_with('f') && key[1..].parse::<u8>().is_ok() => {
            key.to_uppercase()
        }
        key => key.to_string(),
    };
    parts.push(key);
    parts.join("+")
}

fn descriptor(index: usize, planned: &Planned) -> js_sys::Object {
    let object = js_sys::Object::new();
    set(&object, "index", index as u32);
    let (step, skip) = match planned {
        Planned::Run(step) => (step, None),
        Planned::Skip(step, reason) => (step, Some(reason)),
    };
    set(&object, "describe", describe(step));
    if let Some(reason) = skip {
        set(&object, "kind", "skip");
        set(&object, "reason", reason.as_str());
        return object;
    }
    let query = |query: &semantic_query::Query| query.to_string();
    let kind = match step {
        Step::Click(target) | Step::ClickCount(target, _) => {
            set(&object, "query", query(target));
            let count = match step {
                Step::ClickCount(_, count) => *count,
                _ => 1,
            };
            set(&object, "count", count as u32);
            "click"
        }
        Step::ClickAt(x, y) | Step::ClickCountAt(x, y, _) => {
            set(&object, "x", *x);
            set(&object, "y", *y);
            let count = match step {
                Step::ClickCountAt(_, _, count) => *count,
                _ => 1,
            };
            set(&object, "count", count as u32);
            "clickAt"
        }
        Step::Hover(target) => {
            set(&object, "query", query(target));
            "hover"
        }
        Step::MoveTo(x, y) => {
            set(&object, "x", *x);
            set(&object, "y", *y);
            "moveTo"
        }
        Step::Press(target) => {
            set(&object, "query", query(target));
            "press"
        }
        Step::Release(target) => {
            set(&object, "query", query(target));
            "release"
        }
        Step::Drag(from, to) => {
            set(&object, "from", query(from));
            set(&object, "to", query(to));
            "drag"
        }
        Step::Wheel(target, dx, dy) => {
            set(&object, "query", query(target));
            set(&object, "dx", *dx);
            set(&object, "dy", *dy);
            "wheel"
        }
        Step::TouchPan(target, dx, dy, steps) => {
            set(&object, "query", query(target));
            set(&object, "dx", *dx);
            set(&object, "dy", *dy);
            set(&object, "steps", (*steps).max(1) as u32);
            "touchPan"
        }
        Step::TouchTap(target) => {
            set(&object, "query", query(target));
            "touchTap"
        }
        Step::Type(target, text) => {
            set(&object, "query", query(target));
            set(&object, "text", text.as_str());
            "type"
        }
        Step::Keys(keys) | Step::KeyUp(keys) => {
            let names: Result<Vec<String>, String> = keys
                .split_whitespace()
                .map(|key| {
                    Keystroke::parse(key)
                        .map(|keystroke| playwright_key(&keystroke))
                        .map_err(|error| format!("bad keystroke {key:?}: {error}"))
                })
                .collect();
            match names {
                Ok(names) => {
                    let names: js_sys::Array = names
                        .into_iter()
                        .map(|name| JsValue::from_str(&name))
                        .collect();
                    set(&object, "keys", names);
                }
                Err(error) => {
                    set(&object, "kind", "skip");
                    set(&object, "reason", error);
                    return object;
                }
            }
            if matches!(step, Step::KeyUp(_)) {
                "keyUp"
            } else {
                "keys"
            }
        }
        Step::ImeCompose(text, caret) => {
            set(&object, "text", text.as_str());
            set(&object, "caret", *caret as u32);
            "imeCompose"
        }
        Step::ImeCommit(text) => {
            set(&object, "text", text.as_str());
            "imeCommit"
        }
        Step::ImeCancel => "imeCancel",
        Step::Focus(target) => {
            set(&object, "query", query(target));
            "focus"
        }
        Step::Advance(duration) => {
            set(&object, "ms", duration.as_secs_f64() * 1000.);
            "advance"
        }
        Step::Expect(_) => "expect",
        Step::Probe(..)
        | Step::ProbeUniform(..)
        | Step::ProbeDiffer(..)
        | Step::Capture(_)
        | Step::ChangedSince(..) => "pixels",
        Step::Golden | Step::GoldenWithin(_) | Step::When(..) => "skip",
    };
    set(&object, "kind", kind);
    object
}
