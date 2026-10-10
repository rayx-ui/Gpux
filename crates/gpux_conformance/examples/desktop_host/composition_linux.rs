//! Lane D's window-composition checks on Linux: `composition-wayland-*` in a
//! Wayland session and `composition-x11-*` under an X server, each skipped
//! with the reason elsewhere. Two solid-colour native surfaces placed through
//! `native_surface` sit between GPUI's base content and a GPUI menu deferred
//! onto the overlay.
//!
//! An X server can be asked everything: the checks read the native child
//! windows' stacking, geometry and shape, and its own hit testing, through a
//! connection of their own. A Wayland client cannot read the compositor's
//! stacking back, so the Wayland checks read the requests gpux logged under
//! `gpux::composition` as it handed them over, and the compositor checks them
//! itself: a wrong placement is a protocol error, which ends the connection
//! and every later frame.

use crate::checks::Check;
use gpux::AsyncApp;

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod unsupported {
    use super::*;

    fn skipped() -> anyhow::Result<Check> {
        Ok(Check::Skipped(
            "Linux window composition checks run on Linux".into(),
        ))
    }

    pub async fn wayland_order(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn wayland_geometry(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn wayland_input(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn x11_order_bounds(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn x11_cutouts(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn x11_input(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
    pub async fn x11_overlay(_: &mut AsyncApp) -> anyhow::Result<Check> {
        skipped()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
pub use unsupported::*;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub use linux::*;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux {
    use super::*;
    use crate::checks::{close_window, open};
    use anyhow::{Context as _, bail};
    use gpux::{
        AnyWindowHandle, CompositionSurfaceId, Context, IntoElement, Render, Window,
        WindowCompositionSurface, anchored, deferred, div, native_surface, point, prelude::*, px,
        rgb,
    };
    use gpux_conformance::app_host::wait_frames;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::sync::Mutex;

    /// Where the first native surface is placed, in logical pixels.
    const NATIVE: (f32, f32, f32, f32) = (40., 50., 160., 100.);
    /// Where the second, smaller one is placed, inside the first.
    const INNER: (f32, f32, f32, f32) = (60., 60., 40., 30.);
    /// Where the menu opens over the first, in logical pixels.
    const MENU: (f32, f32, f32, f32) = (110., 80., 80., 40.);

    /// White base content, two native surfaces and a grey menu with a shadow
    /// that may be open over the first.
    struct Composed {
        native: Option<WindowCompositionSurface>,
        inner: Option<WindowCompositionSurface>,
        native_left: f32,
        menu_open: bool,
    }

    impl Render for Composed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let place = |surface: WindowCompositionSurface, (x, y, w, h): (f32, f32, f32, f32)| {
                native_surface(surface)
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(w))
                    .h(px(h))
            };
            div()
                .size_full()
                .relative()
                .bg(rgb(0xffffff))
                .children(self.native.clone().map(|surface| {
                    place(surface, (self.native_left, NATIVE.1, NATIVE.2, NATIVE.3))
                }))
                .children(self.inner.clone().map(|surface| place(surface, INNER)))
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

    /// A composed window and what the checks read of it.
    struct Setup {
        window: AnyWindowHandle,
        raw: RawWindowHandle,
        base: CompositionSurfaceId,
        overlay: CompositionSurfaceId,
        native: WindowCompositionSurface,
        inner: WindowCompositionSurface,
        scale: f32,
    }

    async fn setup(cx: &mut AsyncApp, title: &str) -> anyhow::Result<Setup> {
        let handle = open(cx, title, (320., 220.), |_, _| Composed {
            native: None,
            inner: None,
            native_left: NATIVE.0,
            menu_open: false,
        })?;
        let setup = handle.update(cx, |root, window, cx| {
            let raw = window
                .window_handle()
                .map_err(|error| anyhow::anyhow!("the window has no handle: {error}"))?
                .as_raw();
            let composition = window.enable_window_composition()?;
            let native = composition.create_native_surface()?;
            let inner = composition.create_native_surface()?;
            let root = root
                .downcast::<Composed>()
                .map_err(|_| anyhow::anyhow!("not the composed view"))?;
            let (placed, placed_inner) = (native.clone(), inner.clone());
            root.update(cx, |composed, cx| {
                composed.native = Some(placed);
                composed.inner = Some(placed_inner);
                cx.notify();
            });
            anyhow::Ok(Setup {
                window: handle,
                raw,
                base: composition.base_surface(),
                overlay: composition.overlay_surface(),
                native,
                inner,
                scale: window.scale_factor(),
            })
        })??;
        wait_frames(cx, handle, 3).await?;
        Ok(setup)
    }

    fn update(cx: &mut AsyncApp, setup: &Setup, change: impl FnOnce(&mut Composed)) {
        setup
            .window
            .update(cx, |root, _, cx| {
                if let Ok(root) = root.downcast::<Composed>() {
                    root.update(cx, |composed, cx| {
                        change(composed);
                        cx.notify();
                    });
                }
            })
            .ok();
    }

    /// Whether every component of `actual` is within a pixel of `expected`:
    /// regions are rounded out, so at a fractional scale an edge may land a
    /// pixel further out than the rounded bounds.
    fn within_a_pixel(actual: &[i32], expected: &[i32; 4]) -> bool {
        actual.len() == 4
            && actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() <= 1)
    }

    /// The integers in `text`, in order.
    fn numbers(text: &str) -> Vec<i32> {
        text.split(|c: char| !(c.is_ascii_digit() || c == '-'))
            .filter_map(|part| part.parse().ok())
            .collect()
    }

    /// `(x, y, w, h)` in logical pixels as device pixels, rounded.
    fn device((x, y, w, h): (f32, f32, f32, f32), scale: f32) -> [i32; 4] {
        [x, y, w, h].map(|value| (value * scale).round() as i32)
    }

    // ---------------------------------------------------------------- Wayland

    /// The composition requests gpux logged under `gpux::composition`.
    struct Trace(Mutex<Vec<String>>);

    static TRACE: Trace = Trace(Mutex::new(Vec::new()));

    impl log::Log for Trace {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.target() == "gpux::composition"
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata())
                && let Ok(mut lines) = self.0.lock()
            {
                lines.push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    /// Starts capturing the trace, which needs the host to have no logger of
    /// its own, and empties what was captured.
    fn capture_trace() -> anyhow::Result<()> {
        static INSTALLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let installed = *INSTALLED.get_or_init(|| {
            log::set_logger(&TRACE)
                .map(|()| log::set_max_level(log::LevelFilter::Debug))
                .is_ok()
        });
        if !installed {
            bail!("another logger is installed, so the composition trace cannot be read");
        }
        TRACE.0.lock().map(|mut lines| lines.clear()).ok();
        Ok(())
    }

    fn take_trace() -> Vec<String> {
        TRACE
            .0
            .lock()
            .map(|mut lines| std::mem::take(&mut *lines))
            .unwrap_or_default()
    }

    /// Whether `lines` hold every one of `expected`, in that order.
    fn in_order(lines: &[String], expected: &[String]) -> anyhow::Result<()> {
        let mut from = 0;
        for line in expected {
            let found = lines[from..]
                .iter()
                .position(|candidate| candidate.starts_with(line.as_str()))
                .with_context(|| format!("no `{line}` after line {from} in {lines:#?}"))?;
            from += found + 1;
        }
        Ok(())
    }

    async fn wayland_setup(cx: &mut AsyncApp, title: &str) -> anyhow::Result<Option<Setup>> {
        capture_trace()?;
        let setup = setup(cx, title).await?;
        if !matches!(setup.raw, RawWindowHandle::Wayland(_)) {
            close_window(cx, setup.window);
            return Ok(None);
        }
        Ok(Some(setup))
    }

    fn not_wayland() -> Check {
        Check::Skipped("not a Wayland session (the window is not a Wayland surface)".into())
    }

    /// The native surfaces are subsurfaces of the window surface stacked in
    /// composition order below the overlay, and a native surface reparented
    /// under another is placed above it, as its child; the compositor accepts
    /// every placement and frames keep coming.
    pub async fn wayland_order(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let Some(setup) = wayland_setup(cx, "composition-wayland-order").await? else {
            return Ok(not_wayland());
        };
        let result = async {
            let (base, overlay) = (setup.base, setup.overlay);
            let (native, inner) = (setup.native.id(), setup.inner.id());
            let lines = take_trace();
            in_order(
                &lines,
                &[
                    format!("wayland place {native:?} under {base:?} above {base:?}"),
                    format!("wayland place {inner:?} under {base:?} above {native:?}"),
                    format!("wayland place {overlay:?} under {base:?} above {inner:?}"),
                ],
            )?;
            setup.window.update(cx, |_, window, _| {
                window
                    .enable_window_composition()?
                    .reparent(inner, Some(native))
            })??;
            wait_frames(cx, setup.window, 3).await?;
            let lines = take_trace();
            in_order(
                &lines,
                &[
                    format!("wayland place {native:?} under {base:?} above {base:?}"),
                    format!("wayland place {overlay:?} under {base:?} above {native:?}"),
                ],
            )?;
            in_order(
                &lines,
                &[format!(
                    "wayland place {inner:?} under {native:?} above {native:?}"
                )],
            )?;
            Ok(Check::Passed(
                "natives stacked below the overlay in order, and nested after a reparent".into(),
            ))
        }
        .await;
        close_window(cx, setup.window);
        result
    }

    /// A native subsurface is placed at its element's bounds in logical
    /// pixels, relative to its parent, and follows the element when it moves.
    pub async fn wayland_geometry(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let Some(setup) = wayland_setup(cx, "composition-wayland-geometry").await? else {
            return Ok(not_wayland());
        };
        let result = async {
            let native = setup.native.id();
            let [x, y, w, h] = [NATIVE.0, NATIVE.1, NATIVE.2, NATIVE.3]
                .map(|value| ((value * setup.scale).round() / setup.scale).round() as i32);
            let lines = take_trace();
            in_order(
                &lines,
                &[format!("wayland geometry {native:?} {x},{y} {w}x{h}")],
            )?;
            update(cx, &setup, |composed| composed.native_left = NATIVE.0 + 30.);
            wait_frames(cx, setup.window, 3).await?;
            let moved = (((NATIVE.0 + 30.) * setup.scale).round() / setup.scale).round() as i32;
            let lines = take_trace();
            in_order(
                &lines,
                &[format!("wayland geometry {native:?} {moved},{y} {w}x{h}")],
            )?;
            Ok(Check::Passed(format!(
                "native at {x},{y} {w}x{h}, then {moved},{y}, at scale {}",
                setup.scale
            )))
        }
        .await;
        close_window(cx, setup.window);
        result
    }

    /// The overlay's input region is empty while it draws nothing, and is the
    /// open menu's rectangle (not its shadow) while the menu is open.
    pub async fn wayland_input(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let Some(setup) = wayland_setup(cx, "composition-wayland-input").await? else {
            return Ok(not_wayland());
        };
        let result = async {
            let overlay = setup.overlay;
            let lines = take_trace();
            in_order(&lines, &[format!("wayland input {overlay:?} []")])?;
            update(cx, &setup, |composed| composed.menu_open = true);
            wait_frames(cx, setup.window, 3).await?;
            let expected = [MENU.0, MENU.1, MENU.2, MENU.3].map(|value| value as i32);
            let prefix = format!("wayland input {overlay:?} ");
            let lines = take_trace();
            let line = lines
                .iter()
                .rev()
                .find(|line| line.starts_with(&prefix))
                .with_context(|| format!("no `{prefix}` line in {lines:#?}"))?;
            let region = numbers(&line[prefix.len()..]);
            // The region is rounded out to logical pixels, so at a fractional
            // scale an edge may land a pixel further out.
            if region.len() != 4 || !within_a_pixel(&region, &expected) {
                bail!("the overlay's input region is {line}, expected the menu {expected:?}");
            }
            Ok(Check::Passed(format!(
                "empty while closed, the menu's {region:?} while open"
            )))
        }
        .await;
        close_window(cx, setup.window);
        result
    }

    // -------------------------------------------------------------------- X11

    use x11rb::{
        connection::{Connection as _, RequestConnection as _},
        protocol::{
            shape::{self, ConnectionExt as _},
            xproto::{self, ConnectionExt as _},
        },
    };

    /// An X11 window id from a raw handle.
    fn x_window(raw: &RawWindowHandle) -> Option<u32> {
        match raw {
            RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
            RawWindowHandle::Xlib(handle) => Some(handle.window as u32),
            _ => None,
        }
    }

    fn surface_window(surface: &WindowCompositionSurface) -> anyhow::Result<u32> {
        let handle = surface
            .platform_surface()?
            .platform_handle()?
            .downcast::<RawWindowHandle>()
            .map_err(|_| anyhow::anyhow!("the native surface has no raw window handle"))?;
        x_window(&handle).context("the native surface is not an X11 window")
    }

    /// A composed X11 window, its native child windows and a connection of
    /// the checks' own to the same server.
    struct X11Setup {
        setup: Setup,
        conn: x11rb::rust_connection::RustConnection,
        window: u32,
        native: u32,
        inner: u32,
    }

    async fn x11_setup(cx: &mut AsyncApp, title: &str) -> anyhow::Result<Option<X11Setup>> {
        let setup = setup(cx, title).await?;
        let Some(window) = x_window(&setup.raw) else {
            close_window(cx, setup.window);
            return Ok(None);
        };
        let windows = surface_window(&setup.native).and_then(|native| {
            let (conn, _) = x11rb::connect(None).context("connecting to the X server")?;
            anyhow::Ok((conn, native, surface_window(&setup.inner)?))
        });
        match windows {
            Ok((conn, native, inner)) => Ok(Some(X11Setup {
                setup,
                conn,
                window,
                native,
                inner,
            })),
            Err(error) => {
                close_window(cx, setup.window);
                Err(error)
            }
        }
    }

    fn not_x11() -> Check {
        Check::Skipped("not an X11 session (the window is not an X11 window)".into())
    }

    fn geometry(
        conn: &impl x11rb::connection::Connection,
        window: u32,
    ) -> anyhow::Result<[i32; 4]> {
        let geometry = conn.get_geometry(window)?.reply()?;
        Ok([
            geometry.x.into(),
            geometry.y.into(),
            geometry.width.into(),
            geometry.height.into(),
        ])
    }

    /// The child of `window` the server hit-tests at `(x, y)` in device
    /// pixels, shapes included: `0` for the window itself.
    fn child_at(
        conn: &impl x11rb::connection::Connection,
        window: u32,
        (x, y): (f32, f32),
        scale: f32,
    ) -> anyhow::Result<u32> {
        let reply = conn
            .translate_coordinates(
                window,
                window,
                (x * scale).round() as i16,
                (y * scale).round() as i16,
            )?
            .reply()?;
        Ok(reply.child)
    }

    /// The native surfaces are mapped child windows of the GPUI window,
    /// stacked in composition order, at their elements' bounds in device
    /// pixels.
    pub async fn x11_order_bounds(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let Some(x11) = x11_setup(cx, "composition-x11-order-bounds").await? else {
            return Ok(not_x11());
        };
        let result = (|| {
            let children = x11.conn.query_tree(x11.window)?.reply()?.children;
            let position = |child: u32| {
                children
                    .iter()
                    .position(|candidate| *candidate == child)
                    .with_context(|| format!("window {child:#x} is not a child of the GPUI window"))
            };
            let (native, inner) = (position(x11.native)?, position(x11.inner)?);
            if native > inner {
                bail!(
                    "the second native window (child {inner}) is below the first (child {native})"
                );
            }
            for (window, bounds) in [(x11.native, NATIVE), (x11.inner, INNER)] {
                let attributes = x11.conn.get_window_attributes(window)?.reply()?;
                if attributes.map_state != xproto::MapState::VIEWABLE {
                    bail!(
                        "native window {window:#x} is not viewable: {:?}",
                        attributes.map_state
                    );
                }
                let actual = geometry(&x11.conn, window)?;
                let expected = device(bounds, x11.setup.scale);
                if actual != expected {
                    bail!("native window {window:#x} is at {actual:?}, expected {expected:?}");
                }
            }
            Ok(Check::Passed(format!(
                "children {native} and {inner} in order, at their bounds at scale {}",
                x11.setup.scale
            )))
        })();
        close_window(cx, x11.setup.window);
        result
    }

    /// With the menu open, the menu's rectangle is cut out of the native
    /// window's bounding shape, and the rest of it, under the menu's shadow
    /// included, is not.
    pub async fn x11_cutouts(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let Some(x11) = x11_setup(cx, "composition-x11-cutouts").await? else {
            return Ok(not_x11());
        };
        let result = async {
            update(cx, &x11.setup, |composed| composed.menu_open = true);
            wait_frames(cx, x11.setup.window, 3).await?;
            let rectangles = x11
                .conn
                .shape_get_rectangles(x11.native, shape::SK::BOUNDING)?
                .reply()?
                .rectangles;
            let scale = x11.setup.scale;
            let [nx, ny, _, _] = device(NATIVE, scale);
            // A point in the native window's own coordinates.
            let inside = |(x, y): (f32, f32)| {
                let (x, y) = (
                    (x * scale).round() as i32 - nx,
                    (y * scale).round() as i32 - ny,
                );
                rectangles.iter().any(|rectangle| {
                    x >= rectangle.x.into()
                        && y >= rectangle.y.into()
                        && x < i32::from(rectangle.x) + i32::from(rectangle.width)
                        && y < i32::from(rectangle.y) + i32::from(rectangle.height)
                })
            };
            if inside((MENU.0 + 10., MENU.1 + 10.)) {
                bail!("the menu is not cut out of the native window: {rectangles:?}");
            }
            if !inside((MENU.0 - 6., MENU.1 + 10.)) {
                bail!("the menu's shadow is cut out of the native window: {rectangles:?}");
            }
            if !inside((NATIVE.0 + 10., NATIVE.1 + 10.)) {
                bail!("the native window lost its own area: {rectangles:?}");
            }
            Ok(Check::Passed(format!(
                "menu cut out, shadow kept, in {} rectangles",
                rectangles.len()
            )))
        }
        .await;
        close_window(cx, x11.setup.window);
        result
    }

    /// The server's own hit testing finds the native window over its area,
    /// and the GPUI window where the open menu covers it.
    pub async fn x11_input(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let Some(x11) = x11_setup(cx, "composition-x11-input").await? else {
            return Ok(not_x11());
        };
        let result = async {
            let scale = x11.setup.scale;
            let under_menu = (MENU.0 + 10., MENU.1 + 10.);
            let hit = child_at(&x11.conn, x11.window, under_menu, scale)?;
            if hit != x11.native {
                bail!(
                    "with the menu closed, the native area hits {hit:#x}, not {:#x}",
                    x11.native
                );
            }
            update(cx, &x11.setup, |composed| composed.menu_open = true);
            wait_frames(cx, x11.setup.window, 3).await?;
            let hit = child_at(&x11.conn, x11.window, under_menu, scale)?;
            if hit != x11rb::NONE {
                bail!("with the menu open, a point under it hits child {hit:#x}");
            }
            let beside = child_at(&x11.conn, x11.window, (MENU.0 - 6., MENU.1 + 10.), scale)?;
            if beside != x11.native {
                bail!(
                    "with the menu open, a point beside it hits {beside:#x}, not the native window"
                );
            }
            Ok(Check::Passed(
                "native window hit over its area and beside the menu, the GPUI window under it"
                    .into(),
            ))
        }
        .await;
        close_window(cx, x11.setup.window);
        result
    }

    /// Where a compositing manager blends ARGB windows, the menu above the
    /// native surfaces is drawn into a transparent override-redirect window
    /// over the GPUI window, transient for it, whose input shape is the menu.
    pub async fn x11_overlay(cx: &mut AsyncApp) -> anyhow::Result<Check> {
        let Some(x11) = x11_setup(cx, "composition-x11-overlay").await? else {
            return Ok(not_x11());
        };
        let result = async {
            let screen = &x11.conn.setup().roots[0];
            let root = screen.root;
            let selection = x11.conn.intern_atom(false, b"_NET_WM_CM_S0")?.reply()?.atom;
            let composited = x11.conn.get_selection_owner(selection)?.reply()?.owner != x11rb::NONE
                || x11.conn.extension_information("XWAYLAND")?.is_some();
            if !composited {
                return Ok(Check::Skipped(
                    "no compositing manager: GPUI content is cut out of the native windows instead"
                        .into(),
                ));
            }
            update(cx, &x11.setup, |composed| composed.menu_open = true);
            wait_frames(cx, x11.setup.window, 3).await?;
            let mut overlay = None;
            for child in x11.conn.query_tree(root)?.reply()?.children {
                let attributes = x11.conn.get_window_attributes(child)?.reply()?;
                if !attributes.override_redirect
                    || attributes.map_state != xproto::MapState::VIEWABLE
                {
                    continue;
                }
                let transient = x11
                    .conn
                    .get_property(
                        false,
                        child,
                        xproto::AtomEnum::WM_TRANSIENT_FOR,
                        xproto::AtomEnum::WINDOW,
                        0,
                        1,
                    )?
                    .reply()?;
                if transient.value32().and_then(|mut values| values.next()) == Some(x11.window) {
                    overlay = Some(child);
                }
            }
            let Some(overlay) = overlay else {
                if x11.conn.get_geometry(x11.window)?.reply()?.depth != 32 {
                    return Ok(Check::Skipped(
                        "the window has no ARGB visual, so no overlay window".into(),
                    ));
                }
                bail!("no mapped override-redirect window is transient for the GPUI window");
            };
            let input = x11
                .conn
                .shape_get_rectangles(overlay, shape::SK::INPUT)?
                .reply()?
                .rectangles;
            let expected = device(MENU, x11.setup.scale);
            let matches = input.iter().any(|rectangle| {
                within_a_pixel(
                    &[
                        i32::from(rectangle.x),
                        i32::from(rectangle.y),
                        i32::from(rectangle.width),
                        i32::from(rectangle.height),
                    ],
                    &expected,
                )
            });
            if input.len() != 1 || !matches {
                bail!(
                    "the overlay window's input shape is {input:?}, expected the menu {expected:?}"
                );
            }
            Ok(Check::Passed(format!(
                "overlay window {overlay:#x} takes input at the menu {expected:?}"
            )))
        }
        .await;
        close_window(cx, x11.setup.window);
        result
    }
}
