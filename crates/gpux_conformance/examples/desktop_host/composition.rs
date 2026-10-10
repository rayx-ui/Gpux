//! Lane D's window-composition checks on macOS (`composition-macos-*`): a
//! solid-colour native view placed through `native_surface` between GPUI's
//! base content and a GPUI menu deferred onto the overlay. AppKit's own view
//! tree, hit testing and window screenshots are what the checks read; on
//! other platforms they report skipped.

use crate::checks::Check;
#[cfg(target_os = "macos")]
use crate::checks::{close_window, open};
use gpux::AsyncApp;

#[cfg(not(target_os = "macos"))]
pub async fn order_bounds(_: &mut AsyncApp) -> anyhow::Result<Check> {
    Ok(Check::Skipped(
        "window composition checks run on macOS".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub async fn input(_: &mut AsyncApp) -> anyhow::Result<Check> {
    Ok(Check::Skipped(
        "window composition checks run on macOS".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub async fn shadow(_: &mut AsyncApp) -> anyhow::Result<Check> {
    Ok(Check::Skipped(
        "window composition checks run on macOS".into(),
    ))
}

#[cfg(target_os = "macos")]
pub use macos::{input, order_bounds, shadow};

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use anyhow::{Context as _, bail};
    use gpux::{
        AnyWindowHandle, Context, IntoElement, Render, Window, WindowCompositionSurface, anchored,
        deferred, div, native_surface, point, prelude::*, px, rgb,
    };
    use gpux_conformance::app_host::wait_frames;
    use objc2::{MainThreadOnly as _, Message as _, msg_send, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Where the native view is placed, in logical pixels.
    const NATIVE: (f32, f32, f32, f32) = (40., 50., 160., 100.);
    /// Where the menu opens over it, in logical pixels.
    const MENU: (f32, f32, f32, f32) = (80., 80., 80., 40.);

    /// White base content, a native surface placed over part of it, and a
    /// grey menu with a shadow that may be open over the native view.
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
                                .shadow_lg(),
                        ),
                    ))
                })
        }
    }

    /// A composed window with a solid red native view in its native surface.
    struct Setup {
        window: AnyWindowHandle,
        content_view: Retained<NSView>,
        native_view: Retained<NSView>,
        scale: f32,
    }

    async fn setup(cx: &mut AsyncApp, title: &str) -> anyhow::Result<Setup> {
        let window = open(cx, title, (320., 220.), |_, _| Composed {
            surface: None,
            menu_open: false,
        })?;
        let (content_view, native_view, scale) = window.update(cx, |root, window, cx| {
            let composition = window.enable_window_composition()?;
            let surface = composition.create_native_surface()?;
            let container = *surface
                .platform_surface()?
                .platform_handle()?
                .downcast::<usize>()
                .map_err(|_| anyhow::anyhow!("the native surface is not an AppKit view"))?;
            // SAFETY: the container is the live NSView the surface owns; the
            // host runs on the main thread.
            let container: &NSView = unsafe { &*(container as *const NSView) };
            let native_view = red_view(container)?;
            let root = root
                .downcast::<Composed>()
                .map_err(|_| anyhow::anyhow!("not the composed view"))?;
            root.update(cx, |composed, cx| {
                composed.surface = Some(surface);
                cx.notify();
            });
            anyhow::Ok((content_view(window)?, native_view, window.scale_factor()))
        })??;
        wait_frames(cx, window, 3).await?;
        Ok(Setup {
            window,
            content_view,
            native_view,
            scale,
        })
    }

    /// The window's content view: GPUI's base view.
    fn content_view(window: &Window) -> anyhow::Result<Retained<NSView>> {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow::anyhow!("no window handle: {error:?}"))?
            .as_raw()
        else {
            bail!("not an AppKit window");
        };
        // SAFETY: the handle names the window's content view, alive while
        // the window is.
        let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        Ok(view.retain())
    }

    /// CoreGraphics' colour, opaque here, encoded as AppKit declares it.
    #[repr(C)]
    struct CGColor {
        _private: [u8; 0],
    }

    // SAFETY: the encoding of `CGColorRef`, `^{CGColor=}`.
    unsafe impl objc2::encode::RefEncode for CGColor {
        const ENCODING_REF: objc2::encode::Encoding =
            objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("CGColor", &[]));
    }

    /// A layer-backed view filling `container`, coloured solid red.
    fn red_view(container: &NSView) -> anyhow::Result<Retained<NSView>> {
        // SAFETY: plain AppKit calls on the main thread.
        unsafe {
            let view = NSView::initWithFrame(
                NSView::alloc(objc2::MainThreadMarker::new().context("not the main thread")?),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
            );
            view.setWantsLayer(true);
            let layer: *mut AnyObject = msg_send![&*view, layer];
            let red: *mut AnyObject = msg_send![objc2::class!(NSColor), redColor];
            let cg: *const CGColor = msg_send![red, CGColor];
            let _: () = msg_send![layer, setBackgroundColor: cg];
            // Width and height sizable: the view fills the container.
            view.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions(18));
            view.setFrame(container.bounds());
            container.addSubview(&view);
            Ok(view)
        }
    }

    /// The subviews of `view`, bottom to top.
    fn subviews(view: &NSView) -> Vec<Retained<NSView>> {
        view.subviews().iter().collect()
    }

    fn class_name(view: &NSView) -> String {
        let class: &objc2::runtime::AnyClass = unsafe { msg_send![view, class] };
        class.name().to_string_lossy().into_owned()
    }

    /// A window-local logical point as a point in the content view, whose
    /// origin is at its bottom left.
    fn view_point(content: &NSView, (x, y): (f32, f32)) -> NSPoint {
        NSPoint::new(f64::from(x), content.bounds().size.height - f64::from(y))
    }

    /// The native surface's container sits between GPUI's base view and the
    /// overlay view, at its element's bounds in device pixels.
    pub async fn order_bounds(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let setup = setup(cx, "composition-order-bounds").await?;
        let result = (|| {
            let views = subviews(&setup.content_view);
            let container = views
                .iter()
                .position(|view| {
                    subviews(view)
                        .iter()
                        .any(|child| **child == *setup.native_view)
                })
                .context("the native surface's container is not under GPUI's base view")?;
            let overlay = views
                .iter()
                .position(|view| class_name(view) == "GPUIOverlayView")
                .context("there is no overlay view")?;
            if overlay < container {
                bail!(
                    "the overlay view (subview {overlay}) is below the native view (subview {container})"
                );
            }
            let frame = views[container].frame();
            let height = setup.content_view.bounds().size.height;
            let actual = [
                frame.origin.x,
                height - frame.origin.y - frame.size.height,
                frame.size.width,
                frame.size.height,
            ];
            let expected = [NATIVE.0, NATIVE.1, NATIVE.2, NATIVE.3].map(f64::from);
            let device = 1. / f64::from(setup.scale);
            if actual
                .iter()
                .zip(expected)
                .any(|(actual, expected)| (actual - expected).abs() > device)
            {
                bail!("the native view is at {actual:?}, expected {expected:?}");
            }
            Ok(Check::Passed(format!(
                "native view {container} below overlay {overlay}, at {actual:?} at scale {}",
                setup.scale
            )))
        })();
        close_window(cx, setup.window);
        result
    }

    /// AppKit's hit testing passes a click through the empty overlay to the
    /// native view, and gives it to the overlay while a menu covers it.
    pub async fn input(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let setup = setup(cx, "composition-input").await?;
        let result = async {
            let point = view_point(&setup.content_view, (MENU.0 + 10., MENU.1 + 10.));
            let hit = setup
                .content_view
                .hitTest(point)
                .context("nothing is under the menu's position")?;
            if *hit != *setup.native_view {
                bail!(
                    "with the overlay empty, a click hits {}, not the native view",
                    class_name(&hit)
                );
            }
            setup.window.update(cx, |root, _, cx| {
                if let Ok(root) = root.downcast::<Composed>() {
                    root.update(cx, |composed, cx| {
                        composed.menu_open = true;
                        cx.notify();
                    });
                }
            })?;
            wait_frames(cx, setup.window, 3).await?;
            let hit = setup
                .content_view
                .hitTest(point)
                .context("nothing is under the open menu")?;
            if class_name(&hit) != "GPUIOverlayView" {
                bail!(
                    "with the menu open, a click hits {}, not the overlay",
                    class_name(&hit)
                );
            }
            Ok(Check::Passed(
                "passes through when empty, taken by the open menu".into(),
            ))
        }
        .await;
        close_window(cx, setup.window);
        result
    }

    /// An OS screenshot of the window shows the menu's shadow blended over
    /// the red native view: darker red below the menu, not black and not
    /// pure red.
    pub async fn shadow(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        unsafe extern "C" {
            fn CGPreflightScreenCaptureAccess() -> bool;
        }
        // SAFETY: a CoreGraphics query without arguments.
        if !unsafe { CGPreflightScreenCaptureAccess() } {
            return Ok(Check::Skipped(
                "the host has no screen-capture permission for OS screenshots".into(),
            ));
        }
        let setup = setup(cx, "composition-shadow").await?;
        let result = async {
            setup.window.update(cx, |root, _, cx| {
                if let Ok(root) = root.downcast::<Composed>() {
                    root.update(cx, |composed, cx| {
                        composed.menu_open = true;
                        cx.notify();
                    });
                }
            })?;
            wait_frames(cx, setup.window, 4).await?;
            let window_number: isize = {
                let window = setup.content_view.window().context("no window")?;
                window.windowNumber()
            };
            let image = core_graphics::window::create_image(
                core_graphics::geometry::CG_ZERO_RECT,
                core_graphics::window::kCGWindowListOptionIncludingWindow,
                window_number as u32,
                core_graphics::window::kCGWindowImageBoundsIgnoreFraming
                    | core_graphics::window::kCGWindowImageBestResolution,
            )
            .context("the OS screenshot failed")?;
            let width = image.width();
            let height = image.height();
            let bytes_per_row = image.bytes_per_row();
            let data = image.data();
            let bytes = data.bytes();
            let content_height = setup.content_view.bounds().size.height as f32;
            let titlebar = (height as f32 / setup.scale - content_height).max(0.);
            let pixel = |(x, y): (f32, f32)| {
                let px = ((x * setup.scale) as usize).min(width - 1);
                let py = (((y + titlebar) * setup.scale) as usize).min(height - 1);
                let index = py * bytes_per_row + px * 4;
                // BGRA, premultiplied.
                [bytes[index + 2], bytes[index + 1], bytes[index]]
            };
            // Just below the menu: inside the native view, under the shadow.
            let shaded = pixel((MENU.0 + MENU.2 / 2., MENU.1 + MENU.3 + 4.));
            let plain = pixel((NATIVE.0 + 6., NATIVE.1 + NATIVE.3 - 6.));
            if plain[0] < 200 || plain[1] > 60 || plain[2] > 60 {
                bail!("the native view reads {plain:?}, not red");
            }
            if !(shaded[0] < plain[0] && shaded[0] > 40 && shaded[1] < 80 && shaded[2] < 80) {
                bail!("below the menu reads {shaded:?}: no shadow blended over the red view ({plain:?})");
            }
            Ok(Check::Passed(format!("shadow {shaded:?} over native {plain:?}")))
        }
        .await;
        close_window(cx, setup.window);
        result
    }
}
