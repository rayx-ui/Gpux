//! Lane E's platform-view check (`platform-view-android`): a `container`
//! platform view with a red background, made by gpux's Java helper
//! (`dev.gpux.mobile.PlatformViews`) and placed by `platform_view_element`
//! through window composition below a GPUI menu and inside a clipping
//! parent; disposing the view takes its native surface away.

use crate::android::Screens;
use crate::composition::{MENU, NATIVE, capture, close, line, near, pixel};
use gpui_mobile::{
    android::platform_view::AndroidPlatformViewFactory,
    components::platform_view_element::platform_view_element,
    platform_view::{PlatformViewHandle, PlatformViewParams, PlatformViewRegistry},
};
use gpux::{
    AnyWindowHandle, AppContext as _, AsyncApp, Context, Entity, IntoElement, Render, Window,
    WindowOptions, anchored, deferred, div, point, prelude::*, px, rgb,
};
use gpux_conformance::{app_host, report::Outcome, report::ResultLine};
use std::{collections::HashMap, sync::Arc, time::Duration};

/// How far the view is scrolled above its clipping parent's top edge.
const SCROLLED: f32 = 20.;
/// How much shorter than the view its clipping parent is.
const CLIPPED: f32 = 60.;

struct Placed {
    view: Option<Arc<PlatformViewHandle>>,
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
                    .left(px(NATIVE.0))
                    .top(px(NATIVE.1))
                    .w(px(NATIVE.2))
                    .h(px(NATIVE.3 - CLIPPED))
                    .overflow_hidden()
                    .child(
                        platform_view_element(view)
                            .absolute()
                            .left(px(0.))
                            .top(px(-SCROLLED))
                            .w(px(NATIVE.2))
                            .h(px(NATIVE.3)),
                    )
            }))
            .when(self.view.is_some(), |this| {
                this.child(deferred(
                    anchored()
                        .position(point(px(MENU.0), px(MENU.1)))
                        .child(div().w(px(MENU.2)).h(px(MENU.3)).bg(rgb(0xdddddd))),
                ))
            })
    }
}

/// The red view shows between the base and the menu, and goes once it is
/// disposed.
pub async fn platform_view(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "platform-view-android";
    PlatformViewRegistry::global().register(
        "container",
        Box::new(AndroidPlatformViewFactory::new("container")),
    );
    let view = PlatformViewRegistry::global().create_view(
        "container",
        PlatformViewParams {
            creation_params: HashMap::from([("background".into(), "#ff0000".into())]),
            ..Default::default()
        },
    );
    let view = match view {
        Ok(view) => Arc::new(view),
        Err(error) => return line(Outcome::Fail, ID, error),
    };
    let opened = cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(|_| Placed {
            view: Some(view.clone()),
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
    let scale = window
        .update(cx, |_, window, _| window.scale_factor())
        .unwrap_or(1.);
    app_host::wait_frames(cx, window, 3).await.ok();
    let result = match capture(cx, screens, window, scale).await {
        Err(error) => line(Outcome::Fail, ID, format!("no screenshot: {error:#}")),
        Ok(screen) => {
            let native = pixel(&screen, scale, (NATIVE.0 + 8., NATIVE.1 + 8.));
            let menu = pixel(&screen, scale, (MENU.0 + 20., MENU.1 + 20.));
            let base = pixel(&screen, scale, (NATIVE.0 + NATIVE.2 + 16., NATIVE.1 + 8.));
            let clipped = pixel(
                &screen,
                scale,
                (NATIVE.0 + 8., NATIVE.1 + NATIVE.3 - CLIPPED + 10.),
            );
            if !near(native, [255, 0, 0]) {
                line(
                    Outcome::Fail,
                    ID,
                    format!("the platform view reads {native:?}, not red"),
                )
            } else if !near(menu, [0xdd, 0xdd, 0xdd]) {
                line(
                    Outcome::Fail,
                    ID,
                    format!("the menu over it reads {menu:?}"),
                )
            } else if !near(base, [255, 255, 255]) {
                line(
                    Outcome::Fail,
                    ID,
                    format!("the base beside it reads {base:?}"),
                )
            } else if !near(clipped, [255, 255, 255]) {
                line(
                    Outcome::Fail,
                    ID,
                    format!("below its clipping parent the view still shows: {clipped:?}"),
                )
            } else {
                disposed(cx, screens, window, &root, view, scale).await
            }
        }
    };
    close(cx, window);
    result
}

/// After the view is disposed and its element gone, the base shows where it was.
async fn disposed(
    cx: &mut AsyncApp,
    screens: &Screens,
    window: AnyWindowHandle,
    root: &Entity<Placed>,
    view: Arc<PlatformViewHandle>,
    scale: f32,
) -> ResultLine {
    const ID: &str = "platform-view-android";
    view.dispose();
    root.update(cx, |placed, cx| {
        placed.view = None;
        cx.notify();
    });
    app_host::wait_frames(cx, window, 3).await.ok();
    cx.background_executor()
        .timer(Duration::from_millis(800))
        .await;
    match screens.capture(cx, window).await {
        Err(error) => line(Outcome::Fail, ID, format!("no screenshot: {error:#}")),
        Ok(screen) => {
            let native = pixel(&screen, scale, (NATIVE.0 + 8., NATIVE.1 + 8.));
            if near(native, [255, 255, 255]) {
                line(
                    Outcome::Pass,
                    ID,
                    "base, then the platform view clipped by its parent, then the menu; gone once disposed",
                )
            } else {
                line(
                    Outcome::Fail,
                    ID,
                    format!("once disposed, its area reads {native:?}, not the base"),
                )
            }
        }
    }
}
