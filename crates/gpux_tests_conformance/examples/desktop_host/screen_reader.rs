//! VoiceOver's view of a macOS window, read in process the way AppKit's accessibility clients
//! read it: the window view's `accessibilityChildren` (which the VoiceOver adapter supplies once
//! a client asks), each element's role, title and the attributes VoiceOver announces. The
//! `accessibility` scene's table must expose its rows and cells, its disclosure button its
//! expanded state, its spin button its value, its disabled button a press action (so VoiceOver
//! says "dimmed"), and its slider an orientation and increment rather than a settable value
//! (VoiceOver's settable-value path overshoots).

#[cfg(not(target_os = "macos"))]
pub async fn voiceover(_: &mut gpux::AsyncApp) -> anyhow::Result<crate::checks::Check> {
    Ok(crate::checks::Check::Skipped(
        "the VoiceOver check runs on macOS".into(),
    ))
}

#[cfg(target_os = "macos")]
pub use macos::voiceover;

#[cfg(target_os = "macos")]
mod macos {
    use crate::checks::{Check, close_window, open};
    use anyhow::{Context as _, anyhow, bail};
    use gpux::{
        AnyWindowHandle, AsyncApp,
        semantic_query::{Query, SemanticSnapshot},
    };
    use gpux_tests_conformance::{app_host, scene};
    use objc2::{msg_send, runtime::AnyObject, sel};
    use objc2_foundation::NSRange;
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use std::{
        ffi::{CStr, c_char},
        time::Duration,
    };

    /// `NSAccessibilityOrientationHorizontal`.
    const HORIZONTAL: isize = 2;

    /// What VoiceOver reads of an element.
    #[derive(Debug)]
    struct Element {
        object: *mut AnyObject,
        role: Option<String>,
        title: Option<String>,
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

    fn allowed(object: *mut AnyObject, selector: objc2::runtime::Sel) -> bool {
        unsafe { msg_send![object, isAccessibilitySelectorAllowed: selector] }
    }

    fn children(container: *mut AnyObject) -> Vec<*mut AnyObject> {
        let responds: bool =
            unsafe { msg_send![container, respondsToSelector: sel!(accessibilityChildren)] };
        if !responds {
            return Vec::new();
        }
        let array: *mut AnyObject = unsafe { msg_send![container, accessibilityChildren] };
        if array.is_null() {
            return Vec::new();
        }
        let count: usize = unsafe { msg_send![array, count] };
        (0..count)
            .map(|index| unsafe { msg_send![array, objectAtIndex: index] })
            .collect()
    }

    /// Every accessibility element under `container`, depth first.
    fn elements(container: *mut AnyObject, out: &mut Vec<Element>) {
        for child in children(container) {
            let responds = |selector: objc2::runtime::Sel| -> bool {
                unsafe { msg_send![child, respondsToSelector: selector] }
            };
            out.push(Element {
                object: child,
                role: responds(sel!(accessibilityRole))
                    .then(|| string(unsafe { msg_send![child, accessibilityRole] }))
                    .flatten(),
                title: responds(sel!(accessibilityTitle))
                    .then(|| string(unsafe { msg_send![child, accessibilityTitle] }))
                    .flatten(),
            });
            elements(child, out);
        }
    }

    fn content_view(cx: &mut AsyncApp, window: AnyWindowHandle) -> *mut AnyObject {
        window
            .update(cx, |_, window, _| {
                match window.window_handle().map(|handle| handle.as_raw()) {
                    Ok(RawWindowHandle::AppKit(handle)) => {
                        handle.ns_view.as_ptr() as *mut AnyObject
                    }
                    _ => std::ptr::null_mut(),
                }
            })
            .unwrap_or(std::ptr::null_mut())
    }

    fn status(cx: &mut AsyncApp, window: AnyWindowHandle, value: &str) -> bool {
        window
            .update(cx, |_, window, _| {
                SemanticSnapshot::of(window).is_some_and(|snapshot| {
                    !Query::role("status")
                        .with_value(value)
                        .matches(&snapshot)
                        .is_empty()
                })
            })
            .unwrap_or(false)
    }

    async fn settle(cx: &mut AsyncApp) {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }

    pub async fn voiceover(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let scene = scene("accessibility").context("the catalog has no accessibility scene")?;
        let window = open(cx, "screen reader", (480., 640.), move |window, cx| {
            app_host::SceneFrame::new(scene, window, cx)
        })?;
        let result = check(cx, window).await;
        close_window(cx, window);
        result
    }

    async fn check(cx: &mut AsyncApp, window: AnyWindowHandle) -> anyhow::Result<Check> {
        let view = content_view(cx, window);
        if view.is_null() {
            bail!("not an AppKit window");
        }
        // VoiceOver starts from the window, as AppKit's accessibility clients do.
        let ns_window: *mut AnyObject = unsafe { msg_send![view, window] };
        let view = if ns_window.is_null() { view } else { ns_window };
        // The first question activates the adapter; the frame it asks for brings the full tree.
        let mut found = Vec::new();
        for _ in 0..200 {
            found.clear();
            elements(view, &mut found);
            if found
                .iter()
                .any(|element| element.title.as_deref() == Some("Scores"))
            {
                break;
            }
            settle(cx).await;
        }
        let named = |title: &str| {
            found
                .iter()
                .find(|element| element.title.as_deref() == Some(title))
                .with_context(|| {
                    format!(
                        "VoiceOver sees no {title:?}: {:?}",
                        found
                            .iter()
                            .map(|element| (&element.role, &element.title))
                            .collect::<Vec<_>>()
                    )
                })
        };

        let scores = named("Scores")?;
        let rows: *mut AnyObject = unsafe { msg_send![scores.object, accessibilityRows] };
        let row_count: usize = if rows.is_null() {
            0
        } else {
            unsafe { msg_send![rows, count] }
        };
        if !allowed(scores.object, sel!(accessibilityRows)) || row_count != 2 {
            bail!("the Scores table shows VoiceOver {row_count} rows, not 2");
        }
        let second_row: *mut AnyObject = unsafe { msg_send![rows, objectAtIndex: 1usize] };
        let row_index: isize = unsafe { msg_send![second_row, accessibilityIndex] };
        let cells = children(second_row);
        let cell = *cells.get(1).context("the second row has no second cell")?;
        let row_range: NSRange = unsafe { msg_send![cell, accessibilityRowIndexRange] };
        let column_range: NSRange = unsafe { msg_send![cell, accessibilityColumnIndexRange] };
        if row_index != 1 || row_range.location != 1 || column_range.location != 1 {
            bail!(
                "the second row's second cell is at row {row_index}, cell range {row_range:?} by {column_range:?}"
            );
        }

        let details = named("Details")?;
        let expanded: bool = unsafe { msg_send![details.object, isAccessibilityExpanded] };
        if !allowed(details.object, sel!(isAccessibilityExpanded)) || expanded {
            bail!("Details does not report itself collapsed");
        }

        let copies = named("Copies")?;
        let copies_value =
            string(unsafe { msg_send![copies.object, accessibilityValueDescription] });
        if copies_value.as_deref() != Some("1") {
            bail!("Copies describes its value as {copies_value:?}, not \"1\"");
        }

        let delete = named("Delete")?;
        let enabled: bool = unsafe { msg_send![delete.object, isAccessibilityEnabled] };
        if enabled || !allowed(delete.object, sel!(accessibilityPerformPress)) {
            bail!("Delete is not a disabled control with a press action (enabled {enabled})");
        }

        let volume = named("Volume")?;
        let orientation: isize = unsafe { msg_send![volume.object, accessibilityOrientation] };
        if orientation != HORIZONTAL {
            bail!("Volume's orientation is {orientation}, not horizontal");
        }
        if allowed(volume.object, sel!(setAccessibilityValue:)) {
            bail!("Volume lets VoiceOver set its value, which overshoots, instead of incrementing");
        }
        let incremented: bool = unsafe { msg_send![volume.object, accessibilityPerformIncrement] };
        if !incremented {
            bail!("Volume refused accessibilityPerformIncrement");
        }
        let mut stepped = false;
        for _ in 0..100 {
            if status(cx, window, "clicks 0 subscribed false volume 6") {
                stepped = true;
                break;
            }
            settle(cx).await;
        }
        if !stepped {
            return Err(anyhow!("one increment did not move Volume from 5 to 6"));
        }
        Ok(Check::Passed(format!(
            "{} elements: Scores has 2 rows with cell positions, Details collapsed, Copies \"1\", Delete dimmed, Volume horizontal and stepped 5 to 6",
            found.len()
        )))
    }
}
