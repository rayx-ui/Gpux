//! TalkBack's view of a window: the accessibility nodes `accesskit_android` exposes for the
//! `accessibility` scene, read from outside the process the way an accessibility service reads
//! them (`uiautomator dump`, which the testkit runs on request), and actions performed through
//! the adapter's node provider as a service performs them: a click on the scene's button and a
//! scroll forward on its slider.

use crate::android::{REQUEST_REPEAT, request};
use crate::checks::{close, fail, find, open, pass, wait_for};
use gpux::{AnyWindowHandle, AsyncApp, semantic_query::Query};
use gpux_conformance::{report::ResultLine, scene};
use jni_021::{
    JNIEnv, JavaVM,
    objects::{JObject, JValue},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// `AccessibilityNodeInfo.ACTION_CLICK`.
const ACTION_CLICK: i32 = 0x10;
/// `AccessibilityNodeInfo.ACTION_SCROLL_FORWARD`, how services move a slider up.
const ACTION_SCROLL_FORWARD: i32 = 0x1000;

/// A node of a `uiautomator dump`.
#[derive(Debug, Default)]
struct DumpNode {
    text: String,
    description: String,
    class: String,
    /// `[left, top, right, bottom]` in screen pixels.
    bounds: [i32; 4],
}

impl DumpNode {
    fn name(&self) -> &str {
        if self.description.is_empty() {
            &self.text
        } else {
            &self.description
        }
    }
}

/// The value of `name="…"` in a node's attributes.
fn attribute<'a>(node: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    node.find(&key)
        .map(|start| {
            let rest = &node[start + key.len()..];
            &rest[..rest.find('"').unwrap_or(rest.len())]
        })
        .unwrap_or_default()
}

fn parse(xml: &str) -> Vec<DumpNode> {
    xml.split("<node ")
        .skip(1)
        .map(|node| {
            let numbers: Vec<i32> = attribute(node, "bounds")
                .split(|c: char| !c.is_ascii_digit() && c != '-')
                .filter_map(|number| number.parse().ok())
                .collect();
            DumpNode {
                text: attribute(node, "text").to_string(),
                description: attribute(node, "content-desc").to_string(),
                class: attribute(node, "class").to_string(),
                bounds: numbers.try_into().unwrap_or_default(),
            }
        })
        .collect()
}

/// Dumps the screen's accessibility nodes through the testkit, `number` naming the dump.
async fn dump(cx: &mut AsyncApp, dir: &Path, number: u32) -> Option<String> {
    let path = dir.join(format!("{number}.xml"));
    let start = Instant::now();
    let mut asked = None::<Instant>;
    while !path.is_file() {
        if start.elapsed() > Duration::from_secs(60) {
            return None;
        }
        if asked.is_none_or(|asked| asked.elapsed() >= REQUEST_REPEAT) {
            request(&format!("a11y-dump {number}"));
            asked = Some(Instant::now());
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    let xml = std::fs::read_to_string(&path).ok();
    std::fs::remove_file(&path).ok();
    xml
}

/// The scene's nodes in `uiautomator dump`: classes, names and bounds on the painted controls.
pub async fn tree(cx: &mut AsyncApp, data: &Path) -> ResultLine {
    const ID: &str = "screen-reader-android-tree";
    let Some(scene) = scene("accessibility") else {
        return fail(ID, "the catalog has no accessibility scene");
    };
    let window = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the scene never drew: {error:#}")),
    };
    let dir = data.join("a11y");
    std::fs::create_dir_all(&dir).ok();
    // The first dump connects an accessibility client, which activates the adapter; a later
    // one finds the full tree.
    let mut nodes = Vec::new();
    for number in 1..=8 {
        let Some(xml) = dump(cx, &dir, number).await else {
            close(cx, window);
            return fail(ID, format!("the testkit sent no dump {number}"));
        };
        nodes = parse(&xml);
        if nodes.iter().any(|node| node.name() == "Activate") {
            break;
        }
    }
    let scale = window
        .update(cx, |_, window, _| window.scale_factor())
        .unwrap_or(1.);
    let painted = find(cx, window, &Query::role("button").with_name("Activate"));
    let named = |name: &str| nodes.iter().find(|node| node.name() == name);
    let result = (|| {
        let activate = named("Activate").ok_or_else(|| format!("no Activate node in {nodes:?}"))?;
        if activate.class != "android.widget.Button" {
            return Err(format!("Activate is a {}", activate.class));
        }
        let subscribe = named("Subscribe").ok_or("no Subscribe node")?;
        if subscribe.class != "android.widget.CheckBox" {
            return Err(format!("Subscribe is a {}", subscribe.class));
        }
        let volume = named("Volume").ok_or("no Volume node")?;
        if volume.class != "android.widget.SeekBar" {
            return Err(format!("Volume is a {}", volume.class));
        }
        let painted = painted.ok_or("the button is not in the window's tree")?;
        let expected = [
            painted.bounds.x * scale,
            painted.bounds.y * scale,
            (painted.bounds.x + painted.bounds.width) * scale,
            (painted.bounds.y + painted.bounds.height) * scale,
        ];
        let off: f32 = activate
            .bounds
            .iter()
            .zip(expected)
            .map(|(actual, expected)| (*actual as f32 - expected).abs())
            .sum();
        if off > 8. {
            return Err(format!(
                "Activate's bounds {:?} are not on the painted button {expected:?}",
                activate.bounds
            ));
        }
        Ok(format!(
            "{} nodes; Activate is a Button at {:?}, Subscribe a CheckBox, Volume a SeekBar",
            nodes.len(),
            activate.bounds
        ))
    })();
    close(cx, window);
    match result {
        Ok(detail) => pass(ID, detail),
        Err(detail) => fail(ID, detail),
    }
}

/// Runs `f` with an environment attached to this thread and the Activity's decor view, whose
/// accessibility delegate the adapter installed (the NativeActivity host draws its window).
fn with_host<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni_021::errors::Result<T>) -> Option<T> {
    let vm = gpui_mobile::android::jni::java_vm();
    let activity = gpui_mobile::android::jni::activity_as_ptr();
    if vm.is_null() || activity.is_null() {
        return None;
    }
    let vm = unsafe { JavaVM::from_raw(vm.cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let result = (|| {
        let window = env
            .call_method(&activity, "getWindow", "()Landroid/view/Window;", &[])?
            .l()?;
        let decor = env
            .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?
            .l()?;
        f(&mut env, &decor)
    })();
    if result.is_err() {
        env.exception_clear().ok();
    }
    result.ok()
}

/// The virtual view id of the node the adapter names `name`, found by asking its provider for
/// the nodes it numbered so far (it numbers a node's children when it describes the node).
fn virtual_id(name: &str) -> Option<i32> {
    with_host(|env, host| {
        let provider = env
            .call_method(
                host,
                "getAccessibilityNodeProvider",
                "()Landroid/view/accessibility/AccessibilityNodeProvider;",
                &[],
            )?
            .l()?;
        if provider.is_null() {
            return Ok(None);
        }
        let describe = |env: &mut JNIEnv, id: i32| -> jni_021::errors::Result<Option<String>> {
            let info = env
                .call_method(
                    &provider,
                    "createAccessibilityNodeInfo",
                    "(I)Landroid/view/accessibility/AccessibilityNodeInfo;",
                    &[JValue::Int(id)],
                )?
                .l()?;
            if info.is_null() {
                return Ok(None);
            }
            let mut text = String::new();
            for getter in ["getContentDescription", "getText"] {
                let value = env
                    .call_method(&info, getter, "()Ljava/lang/CharSequence;", &[])?
                    .l()?;
                if !value.is_null() && text.is_empty() {
                    let string = env
                        .call_method(&value, "toString", "()Ljava/lang/String;", &[])?
                        .l()?;
                    text = env.get_string((&string).into())?.into();
                }
            }
            Ok(Some(text))
        };
        // The host first, which numbers the root's children; then the numbers in turn.
        describe(env, -1)?;
        for id in 0..256 {
            if describe(env, id)?.as_deref() == Some(name) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    })
    .flatten()
}

/// Performs `action` on the virtual view `id` through the adapter's provider.
fn perform(id: i32, action: i32) -> bool {
    with_host(|env, host| {
        let provider = env
            .call_method(
                host,
                "getAccessibilityNodeProvider",
                "()Landroid/view/accessibility/AccessibilityNodeProvider;",
                &[],
            )?
            .l()?;
        let null = JObject::null();
        env.call_method(
            &provider,
            "performAction",
            "(IILandroid/os/Bundle;)Z",
            &[JValue::Int(id), JValue::Int(action), JValue::Object(&null)],
        )?
        .z()
    })
    .unwrap_or(false)
}

async fn act(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    name: &str,
    action: i32,
    after: &Query,
) -> Result<String, String> {
    let mut id = None;
    let found = wait_for(cx, window, |_| {
        id = virtual_id(name);
        id.is_some()
    })
    .await;
    let id = match (found, id) {
        (true, Some(id)) => id,
        _ => return Err(format!("the adapter exposes no node named {name}")),
    };
    if !perform(id, action) {
        return Err(format!(
            "performAction({id}, {action:#x}) on {name} returned false"
        ));
    }
    if wait_for(cx, window, |cx| find(cx, window, after).is_some()).await {
        Ok(format!("{name} (virtual view {id})"))
    } else {
        Err(format!(
            "performAction on {name} returned true, but nothing changed"
        ))
    }
}

/// Performs `action` on the node named `name` in a fresh `accessibility` scene, expecting the
/// status line to read `after`.
async fn action_check(
    cx: &mut AsyncApp,
    id: &str,
    name: &str,
    action: i32,
    after: &str,
) -> ResultLine {
    let Some(scene) = scene("accessibility") else {
        return fail(id, "the catalog has no accessibility scene");
    };
    let window = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(id, format!("the scene never drew: {error:#}")),
    };
    let after = Query::role("status").with_value(after);
    let result = match act(cx, window, name, action, &after).await {
        Err(error) => fail(id, error),
        Ok(node) => pass(id, format!("{node} acted through the adapter")),
    };
    close(cx, window);
    result
}

/// An accessibility click on the button clicks it.
pub async fn click(cx: &mut AsyncApp) -> ResultLine {
    action_check(
        cx,
        "screen-reader-android-click",
        "Activate",
        ACTION_CLICK,
        "clicks 1 subscribed false volume 5",
    )
    .await
}

/// An accessibility scroll forward on the slider raises it a step (AccessKit 0.9's slider
/// adjustment).
pub async fn adjust(cx: &mut AsyncApp) -> ResultLine {
    action_check(
        cx,
        "screen-reader-android-adjust",
        "Volume",
        ACTION_SCROLL_FORWARD,
        "clicks 0 subscribed false volume 6",
    )
    .await
}
