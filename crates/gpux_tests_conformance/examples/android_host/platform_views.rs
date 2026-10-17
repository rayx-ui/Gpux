//! Lane E's platform-view checks: platform views made by gpux's Java helper
//! (`dev.gpux.mobile.PlatformViews`) and placed by `platform_view_element`
//! through window composition.
//!
//! - `platform-view-android`: a red `container` view below a GPUI menu and
//!   inside a clipping parent; a second view comes and goes without the
//!   first one's panel window being mounted again; disposing the view takes
//!   its native surface, and so its panel window, away.
//! - `platform-view-android-text`: a `webview`, whose panel takes the window
//!   focus when touched (so the soft keyboard reaches it) without GPUI going
//!   inactive, and gives it back on a touch outside.

use crate::android::{Screens, request};
use crate::composition::{MENU, NATIVE, capture, close, line, near, pixel};
use gpui_mobile::{
    android::{jni as android_jni, platform_view::AndroidPlatformViewFactory},
    components::platform_view_element::platform_view_element,
    platform_view::{PlatformViewHandle, PlatformViewParams, PlatformViewRegistry},
};
use gpux::{
    AnyWindowHandle, AppContext as _, AsyncApp, Context, Entity, IntoElement, Render, Window,
    WindowOptions, anchored, deferred, div, point, prelude::*, px, rgb,
};
use gpux_tests_conformance::{app_host, report::Outcome, report::ResultLine};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// How far the view is scrolled above its clipping parent's top edge.
const SCROLLED: f32 = 20.;
/// How much shorter than the view its clipping parent is.
const CLIPPED: f32 = 60.;
/// A point on the view, clear of the menu.
const ON_VIEW: (f32, f32) = (NATIVE.0 + 8., NATIVE.1 + 8.);
/// A point on GPUI's base content beside the view.
const BESIDE: (f32, f32) = (NATIVE.0 + NATIVE.2 + 30., NATIVE.1 + 8.);

struct Placed {
    view: Option<Arc<PlatformViewHandle>>,
    /// A second view below the first, added and disposed while the first stays.
    second: Option<Arc<PlatformViewHandle>>,
    /// Taps GPUI received on its base content.
    base_taps: usize,
}

impl Render for Placed {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("base")
            .size_full()
            .relative()
            .bg(rgb(0xffffff))
            .on_click(cx.listener(|this, _, _, cx| {
                this.base_taps += 1;
                cx.notify();
            }))
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
            .children(self.second.clone().map(|view| {
                platform_view_element(view)
                    .absolute()
                    .left(px(NATIVE.0))
                    .top(px(NATIVE.1 + NATIVE.3 + 20.))
                    .w(px(60.))
                    .h(px(40.))
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

/// A window showing a `view_type` platform view made with `params`.
async fn place(
    cx: &mut AsyncApp,
    view_type: &str,
    params: &[(&str, &str)],
) -> Result<
    (
        AnyWindowHandle,
        Entity<Placed>,
        Arc<PlatformViewHandle>,
        f32,
    ),
    String,
> {
    PlatformViewRegistry::global().register(
        view_type,
        Box::new(AndroidPlatformViewFactory::new(view_type)),
    );
    let view = Arc::new(
        PlatformViewRegistry::global().create_view(
            view_type,
            PlatformViewParams {
                creation_params: params
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect::<HashMap<_, _>>(),
                ..Default::default()
            },
        )?,
    );
    let handle = cx
        .open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Placed {
                view: Some(view.clone()),
                second: None,
                base_taps: 0,
            })
        })
        .map_err(|error| format!("{error:#}"))?;
    let root = handle
        .update(cx, |_, _, cx| cx.entity())
        .map_err(|error| format!("{error:#}"))?;
    let window = AnyWindowHandle::from(handle);
    let scale = window
        .update(cx, |_, window, _| window.scale_factor())
        .unwrap_or(1.);
    app_host::wait_frames(cx, window, 3).await.ok();
    Ok((window, root, view, scale))
}

fn tap(scale: f32, (x, y): (f32, f32)) {
    request(&format!(
        "tap {} {}",
        (x * scale) as i32,
        (y * scale) as i32
    ));
}

/// Waits up to `seconds` for `done`.
async fn wait_for(
    cx: &mut AsyncApp,
    seconds: u64,
    mut done: impl FnMut(&mut AsyncApp) -> bool,
) -> bool {
    let start = Instant::now();
    loop {
        if done(cx) {
            return true;
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            return false;
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
}

/// Taps `point` until `done`, up to three times. The request reaches the
/// testkit through logcat, whose stream can drop a line while a screenshot
/// transfers, so a tap whose effect never shows is asked for again; every
/// tap here may land more than once.
async fn tap_until(
    cx: &mut AsyncApp,
    scale: f32,
    point: (f32, f32),
    mut done: impl FnMut(&mut AsyncApp) -> bool,
) -> bool {
    for _ in 0..3 {
        tap(scale, point);
        if wait_for(cx, 4, &mut done).await {
            return true;
        }
    }
    false
}

/// The red view shows between the base and the menu, clipped by its parent,
/// and goes once it is disposed.
pub async fn platform_view(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "platform-view-android";
    let (window, root, view, scale) =
        match place(cx, "container", &[("background", "#ff0000")]).await {
            Ok(placed) => placed,
            Err(error) => return line(Outcome::Fail, ID, error),
        };
    let result = match capture(cx, screens, window, scale).await {
        Err(error) => line(Outcome::Fail, ID, format!("no screenshot: {error:#}")),
        Ok(screen) => {
            let native = pixel(&screen, scale, ON_VIEW);
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

/// After the view is disposed and its element gone, its panel window is
/// gone too, and the base shows where it was and takes a tap there.
async fn disposed(
    cx: &mut AsyncApp,
    screens: &Screens,
    window: AnyWindowHandle,
    root: &Entity<Placed>,
    view: Arc<PlatformViewHandle>,
    scale: f32,
) -> ResultLine {
    const ID: &str = "platform-view-android";
    let first = match panels() {
        Ok(panels) if panels.len() == 1 => panels[0],
        Ok(panels) => {
            return line(
                Outcome::Fail,
                ID,
                format!(
                    "the placed view has {} panel windows, expected 1",
                    panels.len()
                ),
            );
        }
        Err(error) => return line(Outcome::Fail, ID, format!("listing panels: {error}")),
    };
    if let Err(detail) = second_view(cx, window, root, first).await {
        return line(Outcome::Fail, ID, detail);
    }
    view.dispose();
    root.update(cx, |placed, cx| {
        placed.view = None;
        cx.notify();
    });
    app_host::wait_frames(cx, window, 3).await.ok();
    if !wait_for(cx, 5, |_| panels().is_ok_and(|panels| panels.is_empty())).await {
        return line(
            Outcome::Fail,
            ID,
            format!(
                "once disposed, the view's panel window is still mounted ({:?})",
                panels()
            ),
        );
    }
    let native = match screens.capture(cx, window).await {
        Err(error) => return line(Outcome::Fail, ID, format!("no screenshot: {error:#}")),
        Ok(screen) => pixel(&screen, scale, ON_VIEW),
    };
    if !near(native, [255, 255, 255]) {
        return line(
            Outcome::Fail,
            ID,
            format!("once disposed, its area reads {native:?}, not the base"),
        );
    }
    if tap_until(cx, scale, ON_VIEW, |cx| {
        root.read_with(cx, |placed, _| placed.base_taps) > 0
    })
    .await
    {
        line(
            Outcome::Pass,
            ID,
            "base, then the platform view clipped by its parent, then the menu; a second view \
             comes and goes without remounting it; disposed, its panel window is gone and the \
             base takes a tap there",
        )
    } else {
        line(
            Outcome::Fail,
            ID,
            "once disposed, a tap where the view was never reached GPUI",
        )
    }
}

/// Adds a second platform view above the first and disposes it again: the
/// first view's panel window, `first`, stays mounted throughout.
async fn second_view(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    root: &Entity<Placed>,
    first: i32,
) -> Result<(), String> {
    let second = Arc::new(PlatformViewRegistry::global().create_view(
        "container",
        PlatformViewParams {
            creation_params: HashMap::from([("background".into(), "#0000ff".into())]),
            ..Default::default()
        },
    )?);
    root.update(cx, |placed, cx| {
        placed.second = Some(second.clone());
        cx.notify();
    });
    app_host::wait_frames(cx, window, 3).await.ok();
    if !wait_for(cx, 5, |_| panels().is_ok_and(|panels| panels.len() == 2)).await {
        return Err(format!(
            "a second platform view did not get a panel window ({:?})",
            panels()
        ));
    }
    if !panels()?.contains(&first) {
        return Err("adding a second platform view remounted the first one's panel window".into());
    }
    second.dispose();
    root.update(cx, |placed, cx| {
        placed.second = None;
        cx.notify();
    });
    app_host::wait_frames(cx, window, 3).await.ok();
    if !wait_for(cx, 5, |_| panels().is_ok_and(|panels| panels.len() == 1)).await {
        return Err(format!(
            "the disposed second view's panel window is still mounted ({:?})",
            panels()
        ));
    }
    if panels()? != [first] {
        return Err(
            "disposing the second platform view remounted the first one's panel window".into(),
        );
    }
    Ok(())
}

/// The process's composition panel windows: the window token's identity of
/// every window whose root view is one of the helper's containers. A panel
/// that is unmounted and mounted again gets a new token.
fn panels() -> Result<Vec<i32>, String> {
    android_jni::with_env(|env| {
        let describe = |env: &mut jni::Env, error: jni::errors::Error| {
            env.exception_describe();
            env.exception_clear();
            error.to_string()
        };
        let roots = env
            .call_static_method(
                jni::jni_str!("android/view/inspector/WindowInspector"),
                jni::jni_str!("getGlobalWindowViews"),
                jni::jni_sig!("()Ljava/util/List;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, error))?;
        let count = env
            .call_method(&roots, jni::jni_str!("size"), jni::jni_sig!("()I"), &[])
            .and_then(|value| value.i())
            .map_err(|error| describe(env, error))?;
        let mut panels = Vec::new();
        for index in 0..count {
            let root = env
                .call_method(
                    &roots,
                    jni::jni_str!("get"),
                    jni::jni_sig!("(I)Ljava/lang/Object;"),
                    &[jni::objects::JValue::Int(index)],
                )
                .and_then(|value| value.l())
                .map_err(|error| describe(env, error))?;
            let class = env
                .call_method(
                    &root,
                    jni::jni_str!("getClass"),
                    jni::jni_sig!("()Ljava/lang/Class;"),
                    &[],
                )
                .and_then(|value| value.l())
                .map_err(|error| describe(env, error))?;
            let name = env
                .call_method(
                    &class,
                    jni::jni_str!("getName"),
                    jni::jni_sig!("()Ljava/lang/String;"),
                    &[],
                )
                .and_then(|value| value.l())
                .map_err(|error| describe(env, error))?;
            if android_jni::get_string(env, &name) != "dev.gpux.mobile.Composition$Container" {
                continue;
            }
            let token = env
                .call_method(
                    &root,
                    jni::jni_str!("getWindowToken"),
                    jni::jni_sig!("()Landroid/os/IBinder;"),
                    &[],
                )
                .and_then(|value| value.l())
                .map_err(|error| describe(env, error))?;
            if token.is_null() {
                continue;
            }
            let identity = env
                .call_static_method(
                    jni::jni_str!("java/lang/System"),
                    jni::jni_str!("identityHashCode"),
                    jni::jni_sig!("(Ljava/lang/Object;)I"),
                    &[jni::objects::JValue::Object(&token)],
                )
                .and_then(|value| value.i())
                .map_err(|error| describe(env, error))?;
            panels.push(identity);
        }
        Ok(panels)
    })
}

/// Whether the activity's own window holds the window focus.
fn activity_has_focus() -> bool {
    android_jni::with_env(|env| {
        let activity = android_jni::activity(env)?;
        env.call_method(
            &activity,
            jni::jni_str!("hasWindowFocus"),
            jni::jni_sig!("()Z"),
            &[],
        )
        .and_then(|value| value.z())
        .map_err(|error| error.to_string())
    })
    .unwrap_or(true)
}

/// A touch on a web view gives its panel the window focus, GPUI staying
/// active; a touch beside it gives the focus back and reaches GPUI.
pub async fn text_input(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "platform-view-android-text";
    const PAGE: &str =
        "<body style='margin:0;background:#ff0000'><input style='width:60px'></body>";
    let (window, root, _view, scale) = match place(cx, "webview", &[("html", PAGE)]).await {
        Ok(placed) => placed,
        Err(error) => return line(Outcome::Fail, ID, error),
    };
    let active = |cx: &mut AsyncApp| {
        window
            .update(cx, |_, window, _| window.is_window_active())
            .unwrap_or(false)
    };
    let shown = capture(cx, screens, window, scale)
        .await
        .is_ok_and(|screen| near(pixel(&screen, scale, ON_VIEW), [255, 0, 0]));
    let result = if !shown {
        line(Outcome::Fail, ID, "the web view never drew its page")
    } else if !activity_has_focus() {
        line(
            Outcome::Fail,
            ID,
            "the activity's window lost the focus before the web view was touched",
        )
    } else {
        if !tap_until(cx, scale, ON_VIEW, |_| !activity_has_focus()).await {
            line(
                Outcome::Fail,
                ID,
                "a touch on the web view left the focus with the activity's window: no soft \
                 keyboard can reach it",
            )
        } else {
            // Let the lost focus reach GPUI's loop.
            cx.background_executor()
                .timer(Duration::from_millis(600))
                .await;
            if !active(cx) {
                line(
                    Outcome::Fail,
                    ID,
                    "GPUI went inactive when the web view's panel took the focus",
                )
            } else {
                let returned = tap_until(cx, scale, BESIDE, |_| activity_has_focus()).await;
                let tapped = wait_for(cx, 5, |cx| {
                    root.read_with(cx, |placed, _| placed.base_taps) > 0
                })
                .await;
                if !returned {
                    line(
                        Outcome::Fail,
                        ID,
                        "a touch beside the web view did not give the focus back",
                    )
                } else if !tapped {
                    line(
                        Outcome::Fail,
                        ID,
                        "the touch beside the web view never reached GPUI",
                    )
                } else if !active(cx) {
                    line(
                        Outcome::Fail,
                        ID,
                        "GPUI is inactive after the focus came back",
                    )
                } else {
                    line(
                        Outcome::Pass,
                        ID,
                        "touched, the web view's panel takes the focus and GPUI stays active; a \
                         touch beside it gives the focus back and reaches GPUI",
                    )
                }
            }
        }
    };
    close(cx, window);
    result
}
