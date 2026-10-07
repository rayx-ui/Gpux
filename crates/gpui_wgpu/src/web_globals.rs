//! Browser globals that diagnostics and experiment opt-ins share with the page.
//!
//! gpux names its globals `__gpux<Name>`. The `__rayx<Name>` names predate them: reads still
//! honour them, and the `rayx_diagnostics` alias feature publishes them alongside the gpux names
//! until consumers have moved.

use wasm_bindgen::JsValue;

/// The prefix of gpux's own globals.
pub const PREFIX: &str = "__gpux";
/// The prefix of the legacy globals, kept as aliases.
pub const LEGACY_PREFIX: &str = "__rayx";

fn get(prefix: &str, name: &str) -> Option<JsValue> {
    js_sys::Reflect::get(
        &js_sys::global(),
        &JsValue::from_str(&format!("{prefix}{name}")),
    )
    .ok()
    .filter(|value| !value.is_undefined() && !value.is_null())
}

/// The global `__gpux<name>`, or the legacy `__rayx<name>` when only that is set.
pub fn read(name: &str) -> Option<JsValue> {
    get(PREFIX, name).or_else(|| get(LEGACY_PREFIX, name))
}

/// Whether the boolean global `__gpux<name>` (or the legacy `__rayx<name>`) is `true`.
pub fn flag(name: &str) -> bool {
    read(name)
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn set(prefix: &str, name: &str, value: &JsValue) {
    let _ = js_sys::Reflect::set(
        &js_sys::global(),
        &JsValue::from_str(&format!("{prefix}{name}")),
        value,
    );
}

/// Publishes `value` as `__gpux<name>`, and under the `rayx_diagnostics` alias feature also as
/// the legacy `__rayx<name>`.
pub fn publish(name: &str, value: &JsValue) {
    set(PREFIX, name, value);
    #[cfg(feature = "rayx_diagnostics")]
    set(LEGACY_PREFIX, name, value);
}

/// Resets the global under both names, so neither keeps an injection that was consumed.
pub fn clear(name: &str) {
    set(PREFIX, name, &JsValue::NULL);
    set(LEGACY_PREFIX, name, &JsValue::NULL);
}

/// Publishes `value` under both names whatever the features: for publications that predate the
/// gpux names and were never gated on a diagnostics feature.
pub fn publish_with_legacy(name: &str, value: &JsValue) {
    set(PREFIX, name, value);
    set(LEGACY_PREFIX, name, value);
}
