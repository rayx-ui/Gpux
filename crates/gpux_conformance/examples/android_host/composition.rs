//! Lane E's window-composition checks (`composition-android-*`): a solid
//! red Android `View` in a native surface's container, placed through
//! `native_surface` between GPUI's base content and a GPUI menu deferred
//! onto the overlay. The checks read the screen through `adb screencap` and
//! tap through `adb shell input`, which the testkit performs on request.

use crate::android::{Screens, request};
use gpui_mobile::android::jni as android_jni;
use gpux::{
    AnyWindowHandle, AppContext as _, AsyncApp, Context, Entity, IntoElement, Render, ScrollHandle,
    Window, WindowCompositionSurface, WindowOptions, anchored, deferred, div, native_surface,
    point, prelude::*, px, rgb,
};
use gpux_conformance::{
    app_host,
    report::{Outcome, ResultLine},
};
use jni::objects::{JObject, JValue};
use std::time::{Duration, Instant};

/// Where the native view is placed, in logical pixels.
pub(crate) const NATIVE: (f32, f32, f32, f32) = (24., 120., 220., 160.);
/// Where the menu opens over it.
pub(crate) const MENU: (f32, f32, f32, f32) = (90., 170., 110., 50.);
/// Emulator surfaces round colours to 16 bits.
const TOLERANCE: i32 = 12;

struct Composed {
    surface: Option<WindowCompositionSurface>,
    menu_open: bool,
    /// Taps GPUI received on the base content under the native view.
    base_taps: usize,
    /// Taps GPUI received on the menu.
    menu_taps: usize,
    /// The menu scrolls: its content is taller than it is.
    menu_scroll: ScrollHandle,
}

impl Render for Composed {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let area = |id: &'static str| {
            div()
                .id(id)
                .absolute()
                .left(px(NATIVE.0))
                .top(px(NATIVE.1))
                .w(px(NATIVE.2))
                .h(px(NATIVE.3))
        };
        div()
            .size_full()
            .relative()
            .bg(rgb(0xffffff))
            .child(area("under-native").on_click(cx.listener(|this, _, _, cx| {
                this.base_taps += 1;
                cx.notify();
            })))
            .children(self.surface.clone().map(|surface| {
                native_surface(surface)
                    .absolute()
                    .left(px(NATIVE.0))
                    .top(px(NATIVE.1))
                    .w(px(NATIVE.2))
                    .h(px(NATIVE.3))
            }))
            .when(self.menu_open, |this| {
                this.child(deferred(
                    anchored().position(point(px(MENU.0), px(MENU.1))).child(
                        div()
                            .id("menu")
                            .w(px(MENU.2))
                            .h(px(MENU.3))
                            .bg(rgb(0xdddddd))
                            .shadow_lg()
                            .overflow_y_scroll()
                            .track_scroll(&self.menu_scroll)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.menu_taps += 1;
                                cx.notify();
                            }))
                            .child(div().w_full().h(px(MENU.3 * 8.))),
                    ),
                ))
            })
    }
}

pub(crate) fn line(outcome: Outcome, id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(outcome, id.to_string(), detail.into())
}

/// Adds a red `View` filling `container`, which is not in a window yet.
fn add_red_view(container: &jni::refs::Global<JObject<'static>>) -> Result<(), String> {
    android_jni::with_env(|env| {
        let activity = android_jni::activity(env)?;
        let view = env
            .new_object(
                jni::jni_str!("android/view/View"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .map_err(|error| error.to_string())?;
        env.call_method(
            &view,
            jni::jni_str!("setBackgroundColor"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0xffff0000_u32 as i32)],
        )
        .map_err(|error| error.to_string())?;
        // -1 is MATCH_PARENT.
        env.call_method(
            container.as_obj(),
            jni::jni_str!("addView"),
            jni::jni_sig!("(Landroid/view/View;II)V"),
            &[JValue::Object(&view), JValue::Int(-1), JValue::Int(-1)],
        )
        .map_err(|error| {
            env.exception_describe();
            env.exception_clear();
            error.to_string()
        })?;
        Ok(())
    })
}

/// A composed window with the red native view, and its root view.
async fn setup(cx: &mut AsyncApp) -> anyhow::Result<(AnyWindowHandle, Entity<Composed>, f32)> {
    let handle = cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(|_| Composed {
            surface: None,
            menu_open: false,
            base_taps: 0,
            menu_taps: 0,
            menu_scroll: ScrollHandle::new(),
        })
    })?;
    let root = handle.update(cx, |_, _, cx| cx.entity())?;
    let handle = AnyWindowHandle::from(handle);
    let scale = handle.update(cx, |_, window, cx| {
        let composition = window.enable_window_composition()?;
        let surface = composition.create_native_surface()?;
        let container = surface
            .platform_surface()?
            .platform_handle()?
            .downcast::<jni::refs::Global<JObject<'static>>>()
            .map_err(|_| anyhow::anyhow!("the native surface is not an Android view"))?;
        add_red_view(&container).map_err(anyhow::Error::msg)?;
        root.update(cx, |composed, cx| {
            composed.surface = Some(surface);
            cx.notify();
        });
        anyhow::Ok(window.scale_factor())
    })??;
    app_host::wait_frames(cx, handle, 3).await?;
    // The panel windows appear after the UI thread mounts them.
    cx.background_executor()
        .timer(Duration::from_millis(800))
        .await;
    Ok((handle, root, scale))
}

pub(crate) fn close(cx: &mut AsyncApp, window: AnyWindowHandle) {
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

fn open_menu(cx: &mut AsyncApp, root: &Entity<Composed>) {
    root.update(cx, |composed, cx| {
        composed.menu_open = true;
        cx.notify();
    });
}

pub(crate) fn pixel(screen: &image::RgbaImage, scale: f32, (x, y): (f32, f32)) -> [u8; 3] {
    let x = ((x * scale) as u32).min(screen.width() - 1);
    let y = ((y * scale) as u32).min(screen.height() - 1);
    let pixel = screen.get_pixel(x, y);
    [pixel[0], pixel[1], pixel[2]]
}

pub(crate) fn near(actual: [u8; 3], expected: [u8; 3]) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(actual, expected)| (*actual as i32 - expected as i32).abs() <= TOLERANCE)
}

/// The screen once the red native view shows, retried while the panel
/// windows come up.
pub(crate) async fn capture(
    cx: &mut AsyncApp,
    screens: &Screens,
    window: AnyWindowHandle,
    scale: f32,
) -> anyhow::Result<image::RgbaImage> {
    let start = Instant::now();
    loop {
        let screen = screens.capture(cx, window).await?;
        if near(
            pixel(&screen, scale, (NATIVE.0 + 8., NATIVE.1 + 8.)),
            [255, 0, 0],
        ) || start.elapsed() > Duration::from_secs(10)
        {
            return Ok(screen);
        }
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
    }
}

/// The native view shows above the base and below the menu: red over its
/// area, the menu's grey where the menu covers it, white around it.
pub async fn order(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "composition-android-order";
    let (window, root, scale) = match setup(cx).await {
        Ok(setup) => setup,
        Err(error) => return line(Outcome::Fail, ID, format!("{error:#}")),
    };
    open_menu(cx, &root);
    let result = match capture(cx, screens, window, scale).await {
        Err(error) => line(Outcome::Fail, ID, format!("no screenshot: {error:#}")),
        Ok(screen) => {
            let native = pixel(&screen, scale, (NATIVE.0 + 8., NATIVE.1 + 8.));
            let menu = pixel(&screen, scale, (MENU.0 + 20., MENU.1 + 20.));
            let base = pixel(&screen, scale, (NATIVE.0 + NATIVE.2 + 16., NATIVE.1 + 8.));
            if !near(native, [255, 0, 0]) {
                line(
                    Outcome::Fail,
                    ID,
                    format!("the native view reads {native:?}, not red"),
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
            } else {
                line(
                    Outcome::Pass,
                    ID,
                    "base, then the native view, then the menu",
                )
            }
        }
    };
    close(cx, window);
    result
}

/// With the overlay empty a tap on the native view does not reach GPUI (the
/// native view's window takes it); with the menu open a tap on it reaches
/// GPUI's menu through the overlay.
pub async fn input(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "composition-android-input";
    let (window, root, scale) = match setup(cx).await {
        Ok(setup) => setup,
        Err(error) => return line(Outcome::Fail, ID, format!("{error:#}")),
    };
    let tap = |(x, y): (f32, f32)| {
        request(&format!(
            "tap {} {}",
            (x * scale) as i32,
            (y * scale) as i32
        ))
    };
    // Wait for the panel window before tapping over it.
    capture(cx, screens, window, scale).await.ok();
    tap((MENU.0 + 20., MENU.1 + 20.));
    cx.background_executor().timer(Duration::from_secs(3)).await;
    let base_taps = root.read_with(cx, |composed, _| composed.base_taps);
    let result = if base_taps != 0 {
        line(
            Outcome::Fail,
            ID,
            "with the overlay empty, a tap over the native view reached GPUI",
        )
    } else {
        open_menu(cx, &root);
        app_host::wait_frames(cx, window, 3).await.ok();
        cx.background_executor()
            .timer(Duration::from_millis(800))
            .await;
        tap((MENU.0 + 20., MENU.1 + 20.));
        let start = Instant::now();
        let mut menu_taps = 0;
        while start.elapsed() < Duration::from_secs(10) {
            menu_taps = root.read_with(cx, |composed, _| composed.menu_taps);
            if menu_taps > 0 {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
        }
        if menu_taps == 0 {
            line(
                Outcome::Fail,
                ID,
                "with the menu open, a tap on it never reached GPUI",
            )
        } else {
            line(
                Outcome::Pass,
                ID,
                "passes through to the native view when empty, taken by the open menu",
            )
        }
    };
    close(cx, window);
    result
}

/// A swipe on the menu, over the native view, pans the menu's content: the
/// touches the overlay (or, in cut-out mode, the native container) takes
/// reach GPUI as a whole gesture.
pub async fn pan(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "composition-android-pan";
    let (window, root, scale) = match setup(cx).await {
        Ok(setup) => setup,
        Err(error) => return line(Outcome::Fail, ID, format!("{error:#}")),
    };
    open_menu(cx, &root);
    // Wait for the panel window and the menu before swiping over them.
    capture(cx, screens, window, scale).await.ok();
    app_host::wait_frames(cx, window, 3).await.ok();
    cx.background_executor()
        .timer(Duration::from_millis(800))
        .await;
    let x = ((MENU.0 + MENU.2 / 2.) * scale) as i32;
    let from = ((MENU.1 + MENU.3 - 8.) * scale) as i32;
    let to = ((MENU.1 + 6.) * scale) as i32;
    request(&format!("swipe {x} {from} {x} {to} 600"));
    let scrolled = |cx: &mut AsyncApp| {
        root.read_with(cx, |composed, _| -composed.menu_scroll.offset().y.as_f32())
    };
    let start = Instant::now();
    while scrolled(cx) <= 0. && start.elapsed() < Duration::from_secs(10) {
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    cx.background_executor()
        .timer(Duration::from_millis(500))
        .await;
    let distance = scrolled(cx);
    let (base_taps, menu_taps) =
        root.read_with(cx, |composed, _| (composed.base_taps, composed.menu_taps));
    let result = if distance < 10. {
        line(
            Outcome::Fail,
            ID,
            format!("a 36-pixel swipe on the menu scrolled it {distance} pixels"),
        )
    } else if base_taps != 0 || menu_taps != 0 {
        line(
            Outcome::Fail,
            ID,
            format!("the swipe arrived as {menu_taps} menu and {base_taps} base taps"),
        )
    } else {
        line(
            Outcome::Pass,
            ID,
            format!("a swipe on the menu over the native view scrolled it {distance} pixels"),
        )
    };
    close(cx, window);
    result
}

/// A screenshot shows the menu's shadow blended over the red native view:
/// darker red below the menu, not black and not plain red.
pub async fn shadow(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "composition-android-shadow";
    let (window, root, scale) = match setup(cx).await {
        Ok(setup) => setup,
        Err(error) => return line(Outcome::Fail, ID, format!("{error:#}")),
    };
    let backend = window
        .update(cx, |_, window, _| {
            window.gpu_specs().map(|specs| specs.backend)
        })
        .ok()
        .flatten()
        .unwrap_or_default();
    if backend != "vulkan" {
        close(cx, window);
        return line(
            Outcome::Skip,
            ID,
            format!(
                "the window renders through {backend}, whose surfaces wgpu presents opaque: the \
                 overlay is cut out of native views, and shadows cannot blend over them"
            ),
        );
    }
    open_menu(cx, &root);
    let result = match capture(cx, screens, window, scale).await {
        Err(error) => line(Outcome::Fail, ID, format!("no screenshot: {error:#}")),
        Ok(screen) => {
            let plain = pixel(&screen, scale, (NATIVE.0 + 8., NATIVE.1 + 8.));
            let shaded = pixel(&screen, scale, (MENU.0 + 30., MENU.1 + MENU.3 + 4.));
            let red = |pixel: [u8; 3]| pixel[0] > 120 && pixel[1] < 90 && pixel[2] < 90;
            if !red(plain) {
                line(
                    Outcome::Fail,
                    ID,
                    format!("the native view reads {plain:?}, not red"),
                )
            } else if !red(shaded) || shaded[0] as i32 >= plain[0] as i32 - 6 {
                line(
                    Outcome::Fail,
                    ID,
                    format!(
                        "below the menu reads {shaded:?}: no shadow over the red view ({plain:?})"
                    ),
                )
            } else {
                line(
                    Outcome::Pass,
                    ID,
                    format!("shadow {shaded:?} over native {plain:?}"),
                )
            }
        }
    };
    close(cx, window);
    result
}
