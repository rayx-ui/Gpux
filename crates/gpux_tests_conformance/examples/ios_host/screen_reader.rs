//! VoiceOver's view of a window, read in process the way UIKit's accessibility clients read it:
//! the Metal view's `accessibilityElements` (which `accesskit_ios` supplies once a client asks),
//! each element's label, value, traits and frame, and `accessibilityActivate` on the
//! `accessibility` scene's button.

use crate::checks::{close, fail, find, open, pass, wait_for};
use gpux::{AnyWindowHandle, AsyncApp, semantic_query::Query};
use gpux_tests_conformance::{app_host, report::ResultLine, scene};
use objc2::{msg_send, runtime::AnyObject, sel};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use std::ffi::{CStr, c_char};

/// `UIAccessibilityTraitButton`.
const TRAIT_BUTTON: u64 = 1 << 0;
/// `UIAccessibilityTraitAdjustable`.
const TRAIT_ADJUSTABLE: u64 = 1 << 12;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Rect {
    origin: Point,
    size: Size,
}

unsafe impl objc2::Encode for Point {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl objc2::Encode for Size {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl objc2::Encode for Rect {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

/// What VoiceOver reads of an element.
#[derive(Debug)]
struct Element {
    object: *mut AnyObject,
    label: Option<String>,
    value: Option<String>,
    traits: u64,
    frame: Rect,
}

fn string(object: *mut AnyObject) -> Option<String> {
    if object.is_null() {
        return None;
    }
    let utf8: *const c_char = unsafe { msg_send![object, UTF8String] };
    (!utf8.is_null()).then(|| {
        unsafe { CStr::from_ptr(utf8) }
            .to_string_lossy()
            .into_owned()
    })
}

fn responds(object: *mut AnyObject, selector: objc2::runtime::Sel) -> bool {
    unsafe { msg_send![object, respondsToSelector: selector] }
}

/// Every accessibility element under `container`, depth first.
fn elements(container: *mut AnyObject, out: &mut Vec<Element>) {
    if !responds(container, sel!(accessibilityElements)) {
        return;
    }
    let array: *mut AnyObject = unsafe { msg_send![container, accessibilityElements] };
    if array.is_null() {
        return;
    }
    let count: usize = unsafe { msg_send![array, count] };
    for index in 0..count {
        let element: *mut AnyObject = unsafe { msg_send![array, objectAtIndex: index] };
        let is_element: bool = unsafe { msg_send![element, isAccessibilityElement] };
        if is_element {
            out.push(Element {
                object: element,
                label: string(unsafe { msg_send![element, accessibilityLabel] }),
                value: string(unsafe { msg_send![element, accessibilityValue] }),
                traits: unsafe { msg_send![element, accessibilityTraits] },
                frame: unsafe { msg_send![element, accessibilityFrame] },
            });
        }
        elements(element, out);
    }
}

fn metal_view(cx: &mut AsyncApp, window: AnyWindowHandle) -> *mut AnyObject {
    window
        .update(cx, |_, window, _| {
            match window.window_handle().map(|handle| handle.as_raw()) {
                Ok(RawWindowHandle::UiKit(handle)) => handle.ui_view.as_ptr() as *mut AnyObject,
                _ => std::ptr::null_mut(),
            }
        })
        .unwrap_or(std::ptr::null_mut())
}

/// Opens the `accessibility` scene and reads its elements once the adapter, asked for them,
/// has the window's full tree.
async fn read(cx: &mut AsyncApp, id: &str) -> Result<(AnyWindowHandle, Vec<Element>), ResultLine> {
    let Some(scene) = scene("accessibility") else {
        return Err(fail(id, "the catalog has no accessibility scene"));
    };
    let window = open(cx, scene)
        .await
        .map_err(|error| fail(id, format!("the scene never drew: {error:#}")))?;
    let view = metal_view(cx, window);
    if view.is_null() {
        close(cx, window);
        return Err(fail(id, "not a UIKit window"));
    }
    // The first question activates the adapter; the frame it asks for brings the full tree.
    let mut found = Vec::new();
    let ready = wait_for(cx, window, |_| {
        found.clear();
        elements(view, &mut found);
        found
            .iter()
            .any(|element| element.label.as_deref() == Some("Activate"))
    })
    .await;
    if !ready {
        close(cx, window);
        return Err(fail(
            id,
            format!("VoiceOver never saw the scene's button: {found:?}"),
        ));
    }
    app_host::wait_frames(cx, window, 1).await.ok();
    Ok((window, found))
}

/// The elements carry the scene's names, values, traits, and frames on its painted controls.
pub async fn tree(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "screen-reader-ios-tree";
    let (window, found) = match read(cx, ID).await {
        Ok(read) => read,
        Err(line) => return line,
    };
    let named = |label: &str| {
        found
            .iter()
            .find(|element| element.label.as_deref() == Some(label))
    };
    let result = (|| {
        let activate = named("Activate").ok_or("no Activate element")?;
        if activate.traits & TRAIT_BUTTON == 0 {
            return Err(format!(
                "Activate is not a button: traits {:#x}",
                activate.traits
            ));
        }
        let volume = named("Volume").ok_or("no Volume element")?;
        if volume.traits & TRAIT_ADJUSTABLE == 0 {
            return Err(format!(
                "Volume is not adjustable: traits {:#x}",
                volume.traits
            ));
        }
        if !volume
            .value
            .as_deref()
            .is_some_and(|value| value.contains('5'))
        {
            return Err(format!("Volume reads {:?}, not 5", volume.value));
        }
        named("Subscribe").ok_or("no Subscribe element")?;
        named("Title").ok_or("no Title element")?;
        // The frame is in screen points, where the window's view sits at the origin.
        let painted = find(cx, window, &Query::role("button").with_name("Activate"))
            .ok_or("the button is not in the window's tree")?;
        let frame = activate.frame;
        let off = (frame.origin.x - painted.bounds.x as f64).abs()
            + (frame.origin.y - painted.bounds.y as f64).abs()
            + (frame.size.width - painted.bounds.width as f64).abs()
            + (frame.size.height - painted.bounds.height as f64).abs();
        if off > 4. {
            return Err(format!(
                "Activate's frame {frame:?} is not on the painted button {:?}",
                painted.bounds
            ));
        }
        Ok(format!(
            "{} elements; Activate is a button at {:?}, Volume adjustable at {:?}",
            found.len(),
            frame,
            volume.value
        ))
    })();
    close(cx, window);
    match result {
        Ok(detail) => pass(ID, detail),
        Err(detail) => fail(ID, detail),
    }
}

/// `accessibilityActivate` on the button clicks it, as a double tap under VoiceOver does.
pub async fn activate(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "screen-reader-ios-activate";
    let (window, found) = match read(cx, ID).await {
        Ok(read) => read,
        Err(line) => return line,
    };
    let Some(button) = found
        .iter()
        .find(|element| element.label.as_deref() == Some("Activate"))
    else {
        close(cx, window);
        return fail(ID, "no Activate element");
    };
    let activated: bool = unsafe { msg_send![button.object, accessibilityActivate] };
    let clicked = Query::role("status").with_value("clicks 1 subscribed false volume 5");
    let result = if !activated {
        fail(ID, "accessibilityActivate returned NO")
    } else if wait_for(cx, window, |cx| find(cx, window, &clicked).is_some()).await {
        pass(ID, "accessibilityActivate clicked the button")
    } else {
        fail(
            ID,
            "accessibilityActivate returned YES, but the button was not clicked",
        )
    };
    close(cx, window);
    result
}
