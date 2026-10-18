//! Lane F's platform-view check (`platform-view-ios`): a `container`
//! platform view placed by `platform_view_element` through window
//! composition below a GPUI menu. It reads UIKit's view tree and hit
//! testing; disposing the view takes it and its container away.

use crate::composition::{MENU, NATIVE, Rect, class_name, hit, line, subviews};
use gpui_mobile::{
    components::platform_view_element::platform_view_element,
    ios::platform_view::IosPlatformView,
    platform_view::{PlatformViewHandle, PlatformViewParams},
};
use gpux::{
    AnyWindowHandle, AppContext as _, AsyncApp, Context, Entity, IntoElement, Render, Window,
    WindowOptions, anchored, deferred, div, point, prelude::*, px, rgb,
};
use gpux_tests_conformance::{app_host, report::Outcome, report::ResultLine};
use objc2::{msg_send, runtime::AnyObject};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use std::sync::Arc;

const ID: &str = "platform-view-ios";
/// How far the view is scrolled above its clipping parent's top edge.
const SCROLLED: f64 = 20.;
/// How much shorter than the view its clipping parent is.
const CLIPPED: f64 = 60.;

struct Placed {
    view: Option<Arc<PlatformViewHandle>>,
    menu_open: bool,
}

impl Render for Placed {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .bg(rgb(0xffffff))
            // A clipping parent, as a scroll area is: the view is scrolled up past its top
            // edge and reaches below its bottom edge.
            .children(self.view.clone().map(|view| {
                div()
                    .absolute()
                    .left(px(NATIVE.0 as f32))
                    .top(px(NATIVE.1 as f32))
                    .w(px(NATIVE.2 as f32))
                    .h(px((NATIVE.3 - CLIPPED) as f32))
                    .overflow_hidden()
                    .child(
                        platform_view_element(view)
                            .absolute()
                            .left(px(0.))
                            .top(px(-SCROLLED as f32))
                            .w(px(NATIVE.2 as f32))
                            .h(px(NATIVE.3 as f32)),
                    )
            }))
            .when(self.menu_open, |this| {
                this.child(deferred(
                    anchored()
                        .position(point(px(MENU.0 as f32), px(MENU.1 as f32)))
                        .child(
                            div()
                                .w(px(MENU.2 as f32))
                                .h(px(MENU.3 as f32))
                                .bg(rgb(0xdddddd)),
                        ),
                ))
            })
    }
}

fn superview(view: *mut AnyObject) -> *mut AnyObject {
    unsafe { msg_send![view, superview] }
}

/// The platform view fills a native container under the Metal view, at its
/// element's bounds; UIKit hit testing reaches it beside the menu and the
/// overlay on the menu; disposed, it leaves the container.
pub async fn platform_view(cx: &mut AsyncApp) -> ResultLine {
    let view = match IosPlatformView::new("container", &PlatformViewParams::default()) {
        Ok(view) => view,
        Err(error) => return line(Outcome::Fail, ID, error),
    };
    let native_view = view.native_view_ptr();
    let view = Arc::new(PlatformViewHandle::new(Box::new(view)));
    let opened = cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(|_| Placed {
            view: Some(view.clone()),
            menu_open: true,
        })
    });
    let handle = match opened {
        Ok(handle) => handle,
        Err(error) => return line(Outcome::Fail, ID, format!("{error:#}")),
    };
    let root: Entity<Placed> = match handle.update(cx, |_, _, cx| cx.entity()) {
        Ok(root) => root,
        Err(error) => return line(Outcome::Fail, ID, format!("{error:#}")),
    };
    let window = AnyWindowHandle::from(handle);
    let metal_view = window.update(cx, |_, window, _| {
        match window.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::UiKit(handle)) => handle.ui_view.as_ptr() as *mut AnyObject,
            _ => std::ptr::null_mut(),
        }
    });
    let metal_view = match metal_view {
        Ok(view) if !view.is_null() => view,
        _ => return line(Outcome::Fail, ID, "not a UIKit window"),
    };
    app_host::wait_frames(cx, window, 3).await.ok();
    let result = placed(
        metal_view,
        native_view,
        window.update(cx, |_, w, _| w.scale_factor()).unwrap_or(1.),
    );
    let result = match result {
        Err(detail) => line(Outcome::Fail, ID, detail),
        Ok(()) => {
            let container = superview(native_view);
            view.dispose();
            root.update(cx, |placed, cx| {
                placed.view = None;
                cx.notify();
            });
            app_host::wait_frames(cx, window, 3).await.ok();
            if !superview(native_view).is_null() {
                line(
                    Outcome::Fail,
                    ID,
                    "once disposed, the view is still in a container",
                )
            } else if subviews(metal_view).contains(&container) {
                line(
                    Outcome::Fail,
                    ID,
                    "once disposed, the view's container is still under the Metal view",
                )
            } else {
                line(
                    Outcome::Pass,
                    ID,
                    "in a clipping container below the overlay, at its visible bounds; the view \
                     and its container gone once disposed",
                )
            }
        }
    };
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
    result
}

fn placed(
    metal_view: *mut AnyObject,
    native_view: *mut AnyObject,
    scale: f32,
) -> Result<(), String> {
    let container = superview(native_view);
    if container.is_null() {
        return Err("the platform view was never attached".into());
    }
    let views = subviews(metal_view);
    let Some(container_index) = views.iter().position(|view| *view == container) else {
        return Err("the platform view's container is not under the Metal view".into());
    };
    let Some(overlay) = views
        .iter()
        .position(|view| class_name(*view) == "GPUIOverlayView")
    else {
        return Err("there is no overlay view".into());
    };
    if overlay < container_index {
        return Err(format!(
            "the overlay (subview {overlay}) is below the platform view (subview {container_index})"
        ));
    }
    let frame: Rect = unsafe { msg_send![container, frame] };
    let filled: Rect = unsafe { msg_send![native_view, frame] };
    // The container shows the part of the view inside its clipping parent; the view keeps
    // its full size, offset by what is scrolled out above.
    let actual = [frame.x, frame.y, frame.width, frame.height];
    let expected = [NATIVE.0, NATIVE.1, NATIVE.2, NATIVE.3 - CLIPPED];
    let tolerance = 1. / f64::from(scale);
    let differs = |actual: [f64; 4], expected: [f64; 4]| {
        actual
            .iter()
            .zip(expected)
            .any(|(actual, expected)| (actual - expected).abs() > tolerance)
    };
    if differs(actual, expected) {
        return Err(format!(
            "the container is at {actual:?}, expected {expected:?}"
        ));
    }
    let inside = [filled.x, filled.y, filled.width, filled.height];
    let expected = [0., -SCROLLED, NATIVE.2, NATIVE.3];
    if differs(inside, expected) {
        return Err(format!(
            "the view is at {inside:?} in its container, expected {expected:?}"
        ));
    }
    let clips: bool = unsafe { msg_send![container, clipsToBounds] };
    if !clips {
        return Err("the container does not clip the view".into());
    }
    let on_menu = hit(metal_view, (MENU.0 + 20., MENU.1 + 20.));
    let beside = hit(metal_view, (NATIVE.0 + 10., NATIVE.1 + 10.));
    if class_name(on_menu) != "GPUIOverlayView" {
        return Err(format!("a tap on the menu hits {}", class_name(on_menu)));
    }
    if beside != native_view {
        return Err(format!("a tap beside the menu hits {}", class_name(beside)));
    }
    Ok(())
}

const WEBVIEW_ID: &str = "platform-view-ios-webview";

struct Webviews {
    views: Vec<Arc<PlatformViewHandle>>,
}

impl Render for Webviews {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .bg(rgb(0xffffff))
            .children(self.views.iter().cloned().map(|view| {
                platform_view_element(view)
                    .absolute()
                    .left(px(NATIVE.0 as f32))
                    .top(px(NATIVE.1 as f32))
                    .w(px(NATIVE.2 as f32))
                    .h(px(NATIVE.3 as f32))
            }))
    }
}

/// Webview platform views created with a `url` and with an `html` creation param become
/// `WKWebView`s, and survive the main run loop's next autorelease pool drains. The creation
/// strings are autoreleased, so a view that also released them would send `release` to a freed
/// `NSString` at the drain: the launch enables `NSZombieEnabled` when this check is selected on
/// its own (`ios::launch_environment`), which turns that message into an abort of the host and
/// so into a missing result.
pub async fn webview(cx: &mut AsyncApp) -> ResultLine {
    let mut views = Vec::new();
    let mut natives = Vec::new();
    for (key, value) in [("url", "about:blank"), ("html", "<p>gpux webview html</p>")] {
        let mut params = PlatformViewParams::default();
        params
            .creation_params
            .insert(key.to_string(), value.to_string());
        let view = match IosPlatformView::new("webview", &params) {
            Ok(view) => view,
            Err(error) => return line(Outcome::Fail, WEBVIEW_ID, format!("{key}: {error}")),
        };
        natives.push((key, view.native_view_ptr()));
        views.push(Arc::new(PlatformViewHandle::new(Box::new(view))));
    }
    let opened = cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(|_| Webviews {
            views: views.clone(),
        })
    });
    let handle = match opened {
        Ok(handle) => handle,
        Err(error) => return line(Outcome::Fail, WEBVIEW_ID, format!("{error:#}")),
    };
    let window = AnyWindowHandle::from(handle);
    // Several frames: the run loop drains the autorelease pool the creation strings sit in.
    app_host::wait_frames(cx, window, 5).await.ok();
    let result = match natives
        .iter()
        .find(|(_, native)| class_name(*native) != "WKWebView")
    {
        Some((key, native)) => line(
            Outcome::Fail,
            WEBVIEW_ID,
            format!(
                "the {key} view is a {}, not a WKWebView",
                class_name(*native)
            ),
        ),
        None => line(
            Outcome::Pass,
            WEBVIEW_ID,
            "webviews created with a url and with html are WKWebViews, and live on past the \
             autorelease pool drains",
        ),
    };
    for view in &views {
        view.dispose();
    }
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
    result
}
