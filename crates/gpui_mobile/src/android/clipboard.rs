//! The Android clipboard: the system `ClipboardManager` through gpux's Java helper
//! (`dev.gpux.mobile.Clipboard`, under `crates/gpui_mobile/java`), which writes and reads plain
//! text with HTML beside it, in one clip, on the UI thread.

use super::jni as android_jni;
use gpui::{ClipboardEntry, ClipboardItem};
use jni::objects::{JClass, JObject, JObjectArray, JValue};

/// The helper class, in dot notation for the activity's class loader.
const HELPER: &str = "dev.gpux.mobile.Clipboard";

/// Runs `f` with the helper class and the activity; `name` labels errors.
fn call_helper<T>(
    name: &str,
    f: impl FnOnce(&mut jni::Env, &JClass, &JObject) -> jni::errors::Result<T>,
) -> Result<T, String> {
    android_jni::with_env(|env| {
        let helper = android_jni::find_app_class(env, HELPER)?;
        let activity = android_jni::activity(env)?;
        f(env, &helper, &activity).map_err(|error| {
            env.exception_describe();
            env.exception_clear();
            format!("{HELPER}.{name}: {error}")
        })
    })
}

/// Writes `item`'s text, with its HTML beside it, to the system clipboard.
pub fn write(item: &ClipboardItem) -> Result<(), String> {
    let text = item.text().unwrap_or_default();
    let html = item.html().map(str::to_owned);
    let written = call_helper("write", |env, helper, activity| {
        let text = env.new_string(&text)?;
        let html = match &html {
            Some(html) => JObject::from(env.new_string(html)?),
            None => JObject::null(),
        };
        env.call_static_method(
            helper,
            jni::jni_str!("write"),
            jni::jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)Z"),
            &[
                JValue::Object(activity),
                JValue::Object(&text),
                JValue::Object(&html),
            ],
        )?
        .z()
    })?;
    written
        .then_some(())
        .ok_or_else(|| "the clipboard was not written".into())
}

/// Reads the system clipboard's text and, beside it from the same clip, its HTML. `Err` when
/// the system clipboard is unreachable (no Java VM or activity), never an in-process copy.
pub fn read() -> Result<Option<ClipboardItem>, String> {
    let (text, html) = call_helper("read", |env, helper, activity| {
        let clip = env
            .call_static_method(
                helper,
                jni::jni_str!("read"),
                jni::jni_sig!("(Landroid/app/Activity;)[Ljava/lang/String;"),
                &[JValue::Object(activity)],
            )?
            .l()?;
        if clip.is_null() {
            return Ok((None, None));
        }
        let clip = env.cast_local::<JObjectArray>(clip)?;
        let mut field = |index| -> jni::errors::Result<Option<String>> {
            let value = clip.get_element(env, index)?;
            Ok((!value.is_null()).then(|| android_jni::get_string(env, &value)))
        };
        Ok((field(0)?, field(1)?))
    })?;
    let Some(text) = text else {
        return Ok(None);
    };
    let mut item = ClipboardItem::new_string(text);
    if let Some(html) = html.filter(|html| !html.is_empty()) {
        item.entries.push(ClipboardEntry::Html(html));
    }
    Ok(Some(item))
}
