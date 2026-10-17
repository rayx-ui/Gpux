//! Lane F's window-composition checks (`composition-ios-*`): a solid red
//! `UIView` placed through `native_surface` between GPUI's base content and a
//! GPUI menu deferred onto the overlay. The checks read UIKit's own view tree
//! and hit testing, and a Simulator screenshot the testkit takes on request.

use crate::ios::request;
use gpux::{
    AnyWindowHandle, AppContext as _, AsyncApp, Context, IntoElement, Render, Window,
    WindowCompositionSurface, WindowOptions, anchored, deferred, div, native_surface, point,
    prelude::*, px, rgb,
};
use gpux_tests_conformance::{
    app_host,
    report::{Outcome, ResultLine},
};
use objc2::{
    class,
    encode::{Encode, Encoding, RefEncode},
    msg_send,
    runtime::{AnyClass, AnyObject},
};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use std::{path::PathBuf, ptr, time::Duration};

/// Where the native view is placed, in points.
pub(crate) const NATIVE: (f64, f64, f64, f64) = (40., 140., 220., 160.);
/// Where the menu opens over it.
pub(crate) const MENU: (f64, f64, f64, f64) = (110., 190., 120., 50.);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct(
        "CGRect",
        &[
            Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]),
            Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]),
        ],
    );
}

unsafe impl RefEncode for Rect {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CgPoint {
    x: f64,
    y: f64,
}

unsafe impl Encode for CgPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
}

struct Composed {
    surface: Option<WindowCompositionSurface>,
    menu_open: bool,
}

impl Render for Composed {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .bg(rgb(0xffffff))
            .children(self.surface.clone().map(|surface| {
                native_surface(surface)
                    .absolute()
                    .left(px(NATIVE.0 as f32))
                    .top(px(NATIVE.1 as f32))
                    .w(px(NATIVE.2 as f32))
                    .h(px(NATIVE.3 as f32))
            }))
            .when(self.menu_open, |this| {
                this.child(deferred(
                    anchored()
                        .position(point(px(MENU.0 as f32), px(MENU.1 as f32)))
                        .child(
                            div()
                                .id("menu")
                                .w(px(MENU.2 as f32))
                                .h(px(MENU.3 as f32))
                                .bg(rgb(0xdddddd))
                                .shadow_lg(),
                        ),
                ))
            })
    }
}

/// A composed window, its Metal view, the native container and the red view
/// in it.
struct Setup {
    window: AnyWindowHandle,
    metal_view: *mut AnyObject,
    container: *mut AnyObject,
    red_view: *mut AnyObject,
    scale: f32,
}

async fn setup(cx: &mut AsyncApp) -> anyhow::Result<Setup> {
    let handle = cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(|_| Composed {
            surface: None,
            menu_open: false,
        })
    })?;
    let handle = AnyWindowHandle::from(handle);
    let setup = handle.update(cx, |root, window, cx| {
        let metal_view = match window
            .window_handle()
            .map_err(|error| anyhow::anyhow!("the window has no handle: {error}"))?
            .as_raw()
        {
            RawWindowHandle::UiKit(handle) => handle.ui_view.as_ptr() as *mut AnyObject,
            _ => anyhow::bail!("not a UIKit window"),
        };
        let composition = window.enable_window_composition()?;
        let surface = composition.create_native_surface()?;
        let container = *surface
            .platform_surface()?
            .platform_handle()?
            .downcast::<usize>()
            .map_err(|_| anyhow::anyhow!("the native surface is not a UIView"))?
            as *mut AnyObject;
        let red_view = unsafe {
            let frame = Rect {
                x: 0.,
                y: 0.,
                width: 2000.,
                height: 2000.,
            };
            let view: *mut AnyObject = msg_send![class!(UIView), alloc];
            let view: *mut AnyObject = msg_send![view, initWithFrame: frame];
            let red: *mut AnyObject = msg_send![class!(UIColor), redColor];
            let _: () = msg_send![view, setBackgroundColor: red];
            let _: () = msg_send![container, addSubview: view];
            view
        };
        let root = root
            .downcast::<Composed>()
            .map_err(|_| anyhow::anyhow!("not the composed view"))?;
        root.update(cx, |composed, cx| {
            composed.surface = Some(surface);
            cx.notify();
        });
        anyhow::Ok(Setup {
            window: handle,
            metal_view,
            container,
            red_view,
            scale: window.scale_factor(),
        })
    })??;
    app_host::wait_frames(cx, handle, 3).await?;
    Ok(setup)
}

fn close(cx: &mut AsyncApp, setup: &Setup) {
    setup
        .window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

fn open_menu(cx: &mut AsyncApp, setup: &Setup) {
    setup
        .window
        .update(cx, |root, _, cx| {
            if let Ok(root) = root.downcast::<Composed>() {
                root.update(cx, |composed, cx| {
                    composed.menu_open = true;
                    cx.notify();
                });
            }
        })
        .ok();
}

pub(crate) fn class_name(view: *mut AnyObject) -> String {
    if view.is_null() {
        return "nothing".into();
    }
    let class: &AnyClass = unsafe { msg_send![view, class] };
    class.name().to_string_lossy().into_owned()
}

pub(crate) fn subviews(view: *mut AnyObject) -> Vec<*mut AnyObject> {
    unsafe {
        let subviews: *mut AnyObject = msg_send![view, subviews];
        let count: usize = msg_send![subviews, count];
        (0..count)
            .map(|index| msg_send![subviews, objectAtIndex: index])
            .collect()
    }
}

pub(crate) fn hit(view: *mut AnyObject, (x, y): (f64, f64)) -> *mut AnyObject {
    unsafe { msg_send![view, hitTest: CgPoint { x, y }, withEvent: ptr::null_mut::<AnyObject>()] }
}

pub(crate) fn line(outcome: Outcome, id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(outcome, id.to_string(), detail.into())
}

/// The native container is a subview of the Metal view below the overlay
/// view, at its element's bounds in points.
pub async fn order(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "composition-ios-order";
    let setup = match setup(cx).await {
        Ok(setup) => setup,
        Err(error) => return line(Outcome::Fail, ID, format!("{error:#}")),
    };
    let views = subviews(setup.metal_view);
    let container = views.iter().position(|view| *view == setup.container);
    let overlay = views
        .iter()
        .position(|view| class_name(*view) == "GPUIOverlayView");
    let frame: Rect = unsafe { msg_send![setup.container, frame] };
    let result = match (container, overlay) {
        (None, _) => line(
            Outcome::Fail,
            ID,
            "the native container is not under the Metal view",
        ),
        (_, None) => line(Outcome::Fail, ID, "there is no overlay view"),
        (Some(container), Some(overlay)) if overlay < container => line(
            Outcome::Fail,
            ID,
            format!(
                "the overlay (subview {overlay}) is below the native view (subview {container})"
            ),
        ),
        (Some(container), Some(overlay)) => {
            let actual = [frame.x, frame.y, frame.width, frame.height];
            let expected = [NATIVE.0, NATIVE.1, NATIVE.2, NATIVE.3];
            let tolerance = 1. / f64::from(setup.scale);
            if actual
                .iter()
                .zip(expected)
                .any(|(actual, expected)| (actual - expected).abs() > tolerance)
            {
                line(
                    Outcome::Fail,
                    ID,
                    format!("the native view is at {actual:?}, expected {expected:?}"),
                )
            } else {
                line(
                    Outcome::Pass,
                    ID,
                    format!("native view {container} below overlay {overlay}, at {actual:?}"),
                )
            }
        }
    };
    close(cx, &setup);
    result
}

/// UIKit's hit testing passes a tap through the empty overlay to the native
/// view, and gives it to the overlay while a menu covers it.
pub async fn input(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "composition-ios-input";
    let setup = match setup(cx).await {
        Ok(setup) => setup,
        Err(error) => return line(Outcome::Fail, ID, format!("{error:#}")),
    };
    let at = (MENU.0 + 20., MENU.1 + 20.);
    let before = hit(setup.metal_view, at);
    let result = if before != setup.red_view {
        line(
            Outcome::Fail,
            ID,
            format!("with the overlay empty, a tap hits {}", class_name(before)),
        )
    } else {
        open_menu(cx, &setup);
        app_host::wait_frames(cx, setup.window, 3).await.ok();
        let after = hit(setup.metal_view, at);
        let beside = hit(setup.metal_view, (NATIVE.0 + 10., NATIVE.1 + 10.));
        if class_name(after) != "GPUIOverlayView" {
            line(
                Outcome::Fail,
                ID,
                format!("with the menu open, a tap on it hits {}", class_name(after)),
            )
        } else if beside != setup.red_view {
            line(
                Outcome::Fail,
                ID,
                format!(
                    "with the menu open, a tap beside it hits {}",
                    class_name(beside)
                ),
            )
        } else {
            line(
                Outcome::Pass,
                ID,
                "passes through when empty and beside the menu, taken by the menu",
            )
        }
    };
    close(cx, &setup);
    result
}

fn documents() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Documents")
}

/// A Simulator screenshot shows the menu's shadow blended over the red
/// native view: darker red below the menu, not black and not plain red.
pub async fn shadow(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "composition-ios-shadow";
    let setup = match setup(cx).await {
        Ok(setup) => setup,
        Err(error) => {
            let error = format!("{error:#}");
            return if error.contains("Metal is unavailable") {
                line(Outcome::Skip, ID, error)
            } else {
                line(Outcome::Fail, ID, error)
            };
        }
    };
    open_menu(cx, &setup);
    app_host::wait_frames(cx, setup.window, 5).await.ok();
    let path = documents().join("composition-shadow.png");
    std::fs::create_dir_all(documents()).ok();
    std::fs::remove_file(&path).ok();
    request(&format!("screenshot {}", path.display()));
    let mut image = None;
    for _ in 0..100 {
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
        if let Ok(decoded) = image::open(&path) {
            image = Some(decoded.to_rgba8());
            break;
        }
    }
    let result = match image {
        None => line(Outcome::Fail, ID, "the testkit wrote no screenshot"),
        Some(image) => {
            let scale = f64::from(setup.scale);
            let at = |(x, y): (f64, f64)| {
                let pixel = image.get_pixel(
                    ((x * scale) as u32).min(image.width() - 1),
                    ((y * scale) as u32).min(image.height() - 1),
                );
                [pixel[0], pixel[1], pixel[2]]
            };
            let plain = at((NATIVE.0 + 10., NATIVE.1 + 10.));
            let shaded = at((MENU.0 + 30., MENU.1 + MENU.3 + 4.));
            let red = |pixel: [u8; 3]| pixel[0] > 120 && pixel[1] < 90 && pixel[2] < 90;
            if !red(plain) {
                line(
                    Outcome::Fail,
                    ID,
                    format!("the native view reads {plain:?}, not red"),
                )
            } else if !red(shaded) || shaded[0] >= plain[0].saturating_sub(4) {
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
    close(cx, &setup);
    result
}
