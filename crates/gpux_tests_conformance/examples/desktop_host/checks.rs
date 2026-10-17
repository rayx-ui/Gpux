//! Lane D's platform checks (`gpux_tests_conformance::platform_checks`): what a real desktop window
//! must do beyond running catalog scenes.

use crate::{WINDOW_ORIGIN, os};
use anyhow::{Context as _, anyhow, bail};
use gpux::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Bounds, ClipboardItem, Context, EditableText,
    EditableTextState, Entity, IntoElement, MouseButton, MouseDownEvent, Pixels, Point, Render,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, point, prelude::*, px, rgb,
    semantic_query::Query, size,
};
use gpux_tests_conformance::{
    app_host::{render_to_image, wait_frames},
    evaluate,
    probe::{self, DEFAULT_TOLERANCE},
    report::{Outcome, ResultLine},
};
use image::RgbaImage;
use std::{cell::RefCell, rc::Rc, time::Duration};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// Runs the platform check `id`.
pub async fn run(id: &str, cx: &mut AsyncApp) -> ResultLine {
    let result = match id {
        "platform-open-present" => open_present(cx).await,
        "platform-repeated-capture" => repeated_capture(cx).await,
        "platform-resize" => resize(cx).await,
        "platform-scale-change" => scale_change(cx).await,
        "platform-clipboard" => clipboard(cx).await,
        "platform-os-input" => os_input(cx).await,
        "platform-close" => close(cx).await,
        "devtools-hello-snapshot" => devtools(cx, false).await,
        "devtools-tap" => devtools(cx, true).await,
        "screen-reader-macos-voiceover" => crate::screen_reader::voiceover(cx).await,
        "composition-macos-order-bounds" => crate::composition::order_bounds(cx).await,
        "composition-macos-input" => crate::composition::input(cx).await,
        "composition-macos-shadow" => crate::composition::shadow(cx).await,
        "composition-wayland-order" => crate::composition_linux::wayland_order(cx).await,
        "composition-wayland-geometry" => crate::composition_linux::wayland_geometry(cx).await,
        "composition-wayland-input" => crate::composition_linux::wayland_input(cx).await,
        "composition-x11-order-bounds" => crate::composition_linux::x11_order_bounds(cx).await,
        "composition-x11-cutouts" => crate::composition_linux::x11_cutouts(cx).await,
        "composition-x11-input" => crate::composition_linux::x11_input(cx).await,
        "composition-x11-overlay" => crate::composition_linux::x11_overlay(cx).await,
        "composition-windows-order" => crate::composition_windows::order(cx).await,
        "composition-windows-clip" => crate::composition_windows::clip(cx).await,
        "composition-windows-device-loss" => crate::composition_windows::device_loss(cx).await,
        "composition-windows-accelerator" => crate::composition_windows::accelerator(cx).await,
        "composition-windows-resize" => crate::composition_windows::resize(cx).await,
        other => Err(anyhow!("unknown platform check `{other}`")),
    };
    match result {
        Ok(Check::Passed(detail)) => ResultLine::new(Outcome::Pass, id, detail),
        Ok(Check::Skipped(reason)) => ResultLine::new(Outcome::Skip, id, reason),
        Err(error) => ResultLine::new(Outcome::Fail, id, format!("{error:#}")),
    }
}

pub(crate) enum Check {
    Passed(String),
    Skipped(String),
}

/// A red block at (20, 20, 100, 60) and a blue block 10 px in from the bottom-right corner.
struct Blocks;

impl Render for Blocks {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .bg(rgb(0xffffff))
            .child(
                div()
                    .id("block")
                    .absolute()
                    .top(px(20.))
                    .left(px(20.))
                    .w(px(100.))
                    .h(px(60.))
                    .bg(rgb(0xff0000)),
            )
            .child(
                div()
                    .id("corner")
                    .absolute()
                    .bottom(px(10.))
                    .right(px(10.))
                    .w(px(40.))
                    .h(px(30.))
                    .bg(rgb(0x0000ff)),
            )
    }
}

fn options(title: &str, (width, height): (f32, f32)) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(WINDOW_ORIGIN.0), px(WINDOW_ORIGIN.1)),
            size: size(px(width), px(height)),
        })),
        titlebar: Some(TitlebarOptions {
            title: Some(format!("gpux conformance: {title}").into()),
            ..Default::default()
        }),
        focus: true,
        show: true,
        ..Default::default()
    }
}

pub(crate) fn open<V: Render + 'static>(
    cx: &mut AsyncApp,
    title: &str,
    window_size: (f32, f32),
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> anyhow::Result<AnyWindowHandle> {
    let window = cx.open_window(options(title, window_size), |window, cx| {
        // The checks locate elements in the window's accessibility tree.
        evaluate::keep_inspecting(window);
        cx.new(|cx| build(window, cx))
    })?;
    Ok(window.into())
}

pub(crate) fn close_window(cx: &mut AsyncApp, window: AnyWindowHandle) {
    evaluate::stop_inspecting(window);
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

struct Frame {
    image: RgbaImage,
    scale: f32,
    viewport: (f32, f32),
}

/// Renders the window's current frame back from the real surface.
async fn render_frame(cx: &mut AsyncApp, window: AnyWindowHandle) -> anyhow::Result<Frame> {
    wait_frames(cx, window, 2).await?;
    let image = render_to_image(cx, window).await?;
    window.update(cx, |_, window, _| {
        let viewport = window.viewport_size();
        Frame {
            image,
            scale: window.scale_factor(),
            viewport: (f32::from(viewport.width), f32::from(viewport.height)),
        }
    })
}

impl Frame {
    /// The image matches the viewport at the window's scale, within a pixel of rounding.
    fn check_size(&self) -> anyhow::Result<()> {
        let expected = (self.viewport.0 * self.scale, self.viewport.1 * self.scale);
        let actual = (self.image.width() as f32, self.image.height() as f32);
        if (actual.0 - expected.0).abs() > 1. || (actual.1 - expected.1).abs() > 1. {
            bail!(
                "rendered {}x{}, expected the {}x{} viewport at scale {} ({}x{})",
                actual.0,
                actual.1,
                self.viewport.0,
                self.viewport.1,
                self.scale,
                expected.0,
                expected.1
            );
        }
        Ok(())
    }

    fn probe(&self, x: f32, y: f32, color: [u8; 4]) -> anyhow::Result<()> {
        let (px_x, px_y) = evaluate::physical(self.scale, x, y);
        probe::color_at(&self.image, px_x, px_y, color, DEFAULT_TOLERANCE)
            .with_context(|| format!("at logical ({x}, {y}), scale {}", self.scale))?;
        Ok(())
    }
}

fn bounds_of(cx: &mut AsyncApp, window: AnyWindowHandle, id: &str) -> anyhow::Result<[f32; 4]> {
    let snapshot = window.update(cx, |_, window, _| evaluate::snapshot(window))??;
    let bounds = evaluate::locate(&snapshot, &Query::id(id))?.bounds;
    Ok([bounds.x, bounds.y, bounds.width, bounds.height])
}

fn close_enough(actual: [f32; 4], expected: [f32; 4], tolerance: f32) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(actual, expected)| (actual - expected).abs() <= tolerance)
}

async fn open_present(cx: &mut AsyncApp) -> anyhow::Result<Check> {
    let window = open(cx, "open-present", (360., 240.), |_, _| Blocks)?;
    let result = async {
        let frame = render_frame(cx, window).await?;
        frame.check_size()?;
        let block = bounds_of(cx, window, "block")?;
        if !close_enough(block, [20., 20., 100., 60.], 1. / frame.scale) {
            bail!("the block is at {block:?}, expected [20, 20, 100, 60]");
        }
        frame.probe(70., 50., RED)?;
        frame.probe(200., 140., WHITE)?;
        Ok(Check::Passed(format!(
            "{}x{} at scale {}",
            frame.image.width(),
            frame.image.height(),
            frame.scale
        )))
    }
    .await;
    close_window(cx, window);
    result
}

/// More captures than any swapchain has images: a capture that kept its surface texture
/// would starve the surface before the last one.
async fn repeated_capture(cx: &mut AsyncApp) -> anyhow::Result<Check> {
    const CAPTURES: usize = 8;
    let window = open(cx, "repeated-capture", (360., 240.), |_, _| Blocks)?;
    let result = async {
        for capture in 1..=CAPTURES {
            let frame = render_frame(cx, window)
                .await
                .with_context(|| format!("capture {capture} of {CAPTURES}"))?;
            frame.check_size()?;
            frame.probe(70., 50., RED)?;
        }
        Ok(Check::Passed(format!("{CAPTURES} captures")))
    }
    .await;
    close_window(cx, window);
    result
}

async fn resize(cx: &mut AsyncApp) -> anyhow::Result<Check> {
    let window = open(cx, "resize", (360., 240.), |_, _| Blocks)?;
    let result = async {
        wait_frames(cx, window, 2).await?;
        window.update(cx, |_, window, _| window.resize(size(px(480.), px(320.))))?;
        let mut viewport = (0., 0.);
        for _ in 0..100 {
            wait_frames(cx, window, 1).await?;
            viewport = window.update(cx, |_, window, _| {
                let size = window.viewport_size();
                (f32::from(size.width), f32::from(size.height))
            })?;
            if (viewport.0 - 480.).abs() < 1. && (viewport.1 - 320.).abs() < 1. {
                break;
            }
        }
        if (viewport.0 - 480.).abs() >= 1. || (viewport.1 - 320.).abs() >= 1. {
            bail!("the viewport stayed {viewport:?} after resizing to 480x320");
        }
        let frame = render_frame(cx, window).await?;
        frame.check_size()?;
        let corner = bounds_of(cx, window, "corner")?;
        let expected = [viewport.0 - 50., viewport.1 - 40., 40., 30.];
        if !close_enough(corner, expected, 1. / frame.scale) {
            bail!("the corner block is at {corner:?}, expected {expected:?}");
        }
        frame.probe(viewport.0 - 30., viewport.1 - 25., BLUE)?;
        Ok(Check::Passed(format!(
            "{}x{} at scale {}",
            frame.image.width(),
            frame.image.height(),
            frame.scale
        )))
    }
    .await;
    close_window(cx, window);
    result
}

async fn wait_for_scale(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    scale: f32,
) -> anyhow::Result<()> {
    for _ in 0..100 {
        wait_frames(cx, window, 1).await?;
        let current = window.update(cx, |_, window, _| window.scale_factor())?;
        if (current - scale).abs() < 0.01 {
            return Ok(());
        }
    }
    bail!("the window never reached scale {scale}")
}

async fn scale_change(cx: &mut AsyncApp) -> anyhow::Result<Check> {
    if let Err(reason) = os::scale_change_supported() {
        return Ok(Check::Skipped(reason));
    }
    let window = open(cx, "scale-change", (360., 240.), |_, _| Blocks)?;
    let result = async {
        wait_frames(cx, window, 2).await?;
        let (native, original) = window.update(cx, |_, window, _| {
            (os::native(window), window.scale_factor())
        })?;
        let native = native.map_err(|reason| anyhow!(reason))?;
        let target = if original >= 2. { 1. } else { 2. };
        os::change_scale(&native, original, target).map_err(|reason| anyhow!(reason))?;
        wait_for_scale(cx, window, target).await?;
        let frame = render_frame(cx, window).await?;
        frame.check_size()?;
        frame.probe(70., 50., RED)?;
        os::change_scale(&native, target, original).map_err(|reason| anyhow!(reason))?;
        wait_for_scale(cx, window, original).await?;
        let restored = render_frame(cx, window).await?;
        restored.check_size()?;
        restored.probe(70., 50., RED)?;
        Ok(Check::Passed(format!(
            "scale {original} -> {target} -> {original}"
        )))
    }
    .await;
    close_window(cx, window);
    result
}

async fn clipboard(cx: &mut AsyncApp) -> anyhow::Result<Check> {
    let text = format!("gpux conformance ✓ {}", std::process::id());
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())));
    let executor = cx.background_executor().clone();
    let mut read = None;
    for _ in 0..50 {
        executor.timer(Duration::from_millis(20)).await;
        // The asynchronous read asks the platform each time; the synchronous one may answer
        // from a cache that has not seen this write yet (Wayland).
        let task = cx.update(|cx| cx.read_from_clipboard_async());
        read = task.await.ok().flatten().and_then(|item| item.text());
        if read.as_deref() == Some(text.as_str()) {
            return Ok(Check::Passed(format!(
                "{} characters",
                text.chars().count()
            )));
        }
    }
    bail!("wrote {text:?} to the clipboard and read back {read:?}")
}

/// A target that records primary-button presses and a text field.
struct InputTarget {
    presses: Rc<RefCell<Vec<Point<Pixels>>>>,
    field: Entity<EditableTextState>,
}

impl Render for InputTarget {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let presses = self.presses.clone();
        div()
            .size_full()
            .relative()
            .bg(rgb(0xffffff))
            .child(
                div()
                    .id("target")
                    .absolute()
                    .top(px(20.))
                    .left(px(20.))
                    .w(px(120.))
                    .h(px(50.))
                    .bg(rgb(0x808080))
                    .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, _| {
                        presses.borrow_mut().push(event.position);
                    }),
            )
            .child(
                div()
                    .id("field")
                    .absolute()
                    .top(px(100.))
                    .left(px(20.))
                    .w(px(240.))
                    .h(px(30.))
                    .child(EditableText::new(&self.field)),
            )
    }
}

async fn os_input(cx: &mut AsyncApp) -> anyhow::Result<Check> {
    if let Err(reason) = os::os_input_available() {
        return Ok(Check::Skipped(reason));
    }
    let optional = std::env::var("GPUX_OS_INPUT").is_ok_and(|value| value == "optional");
    let presses = Rc::new(RefCell::new(Vec::new()));
    let field = Rc::new(RefCell::new(None::<Entity<EditableTextState>>));
    let window = {
        let presses = presses.clone();
        let field = field.clone();
        open(cx, "os-input", (360., 240.), move |_, cx| {
            let state = cx.new(EditableTextState::new);
            *field.borrow_mut() = Some(state.clone());
            InputTarget {
                presses,
                field: state,
            }
        })?
    };
    let field = field.borrow().clone().expect("built with the window");
    let result = async {
        wait_frames(cx, window, 2).await?;
        let delivered = async {
            window
                .update(cx, |_, window, _| os::bring_to_front(window))?
                .map_err(|reason| anyhow!("could not bring the window to the front: {reason}"))?;
            wait_frames(cx, window, 2).await?;
            // Click the target's center through the OS.
            let target = bounds_of(cx, window, "target")?;
            let center = point(
                px(target[0] + target[2] / 2.),
                px(target[1] + target[3] / 2.),
            );
            window
                .update(cx, |_, window, _| os::click(window, center))?
                .map_err(|reason| anyhow!(reason))?;
            let executor = cx.background_executor().clone();
            for _ in 0..100 {
                if !presses.borrow().is_empty() {
                    break;
                }
                executor.timer(Duration::from_millis(20)).await;
            }
            let press = presses
                .borrow()
                .first()
                .copied()
                .ok_or_else(|| anyhow!("the OS click never reached the window"))?;
            let error = (f32::from(press.x) - f32::from(center.x))
                .abs()
                .max((f32::from(press.y) - f32::from(center.y)).abs());
            if error > 2. {
                bail!("the OS click arrived at {press:?}, expected {center:?}");
            }
            // Focus the field with an OS click and type into it through the OS.
            let field_bounds = bounds_of(cx, window, "field")?;
            let field_center = point(
                px(field_bounds[0] + 10.),
                px(field_bounds[1] + field_bounds[3] / 2.),
            );
            window
                .update(cx, |_, window, _| os::click(window, field_center))?
                .map_err(|reason| anyhow!(reason))?;
            wait_frames(cx, window, 2).await?;
            window
                .update(cx, |_, window, _| os::type_letters(window, "gpux"))?
                .map_err(|reason| anyhow!(reason))?;
            let mut text = String::new();
            for _ in 0..100 {
                text = cx.read_entity(&field, |state, _| state.text());
                if text == "gpux" {
                    break;
                }
                executor.timer(Duration::from_millis(20)).await;
            }
            if text != "gpux" {
                bail!("typed \"gpux\" through the OS and the field reads {text:?}");
            }
            anyhow::Ok(())
        }
        .await;
        match delivered {
            Ok(()) => Ok(Check::Passed("click and typing reached GPUI".into())),
            Err(error) if optional => Ok(Check::Skipped(format!(
                "the session delivers no synthetic input (GPUX_OS_INPUT=optional): {error:#}"
            ))),
            Err(error) => Err(error),
        }
    }
    .await;
    os::park_cursor();
    close_window(cx, window);
    result
}

async fn close(cx: &mut AsyncApp) -> anyhow::Result<Check> {
    let window = open(cx, "close", (360., 240.), |_, _| Blocks)?;
    wait_frames(cx, window, 2).await?;
    let id = window.window_id();
    // The root view lives only as long as the window holds it; a weak handle sees it go.
    let root = window.update(cx, |root, _, _| root.downgrade())?;
    window.update(cx, |_, window, _| window.remove_window())?;
    let executor = cx.background_executor().clone();
    let is_open = |cx: &mut AsyncApp| {
        cx.update(|cx: &mut App| cx.windows().iter().any(|w| w.window_id() == id))
    };
    for _ in 0..100 {
        executor.timer(Duration::from_millis(20)).await;
        if !is_open(cx) && root.upgrade().is_none() {
            return Ok(Check::Passed(
                "removed, and its root view released, without panics".into(),
            ));
        }
    }
    if is_open(cx) {
        bail!("the closed window is still open");
    }
    bail!("the closed window's root view is still alive: something leaks it")
}

/// Connects the app to a sidecar on loopback, opens [`devtools::ProbeWindow`] and has the
/// sidecar's probe read `hello` and a snapshot, then (with `tap`) click the button and wait for
/// its effect in the window's tree.
async fn devtools(cx: &mut AsyncApp, tap: bool) -> anyhow::Result<Check> {
    use gpux_tests_conformance::devtools;
    let server = gpux_devtools_mcp::apps::Server::start(0, None)?;
    let url = format!("ws://{}", server.address);
    let window = open(cx, "devtools", (400., 240.), |_, _| {
        devtools::ProbeWindow::new()
    })?;
    cx.update(|cx| devtools::connect(cx, url));
    let registry = server.registry.clone();
    let (sender, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let patience = Duration::from_secs(20);
        let probed = if tap {
            gpux_devtools_mcp::probe::press_button_until(
                &registry,
                devtools::BUTTON,
                "click",
                devtools::TAPPED,
                patience,
            )
        } else {
            gpux_devtools_mcp::probe::tap_button(&registry, devtools::BUTTON, patience)
        };
        sender.send(probed).ok();
    });
    let probed = loop {
        match result.try_recv() {
            Ok(probed) => break probed,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                cx.background_executor()
                    .timer(Duration::from_millis(20))
                    .await
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => bail!("the probe thread died"),
        }
    };
    cx.update(devtools::disconnect);
    close_window(cx, window);
    let probed = probed.map_err(|error| anyhow!(error))?;
    if probed.app.app_id != devtools::APP_ID {
        bail!("hello named `{}`", probed.app.app_id);
    }
    let window_id = window.window_id().as_u64();
    if !probed
        .app
        .windows
        .iter()
        .any(|window| window.id == window_id)
    {
        bail!(
            "hello did not list the window {window_id}: {:?}",
            probed.app.windows
        );
    }
    Ok(Check::Passed(if tap {
        format!(
            "the click at ref {} showed `{}`",
            probed.reference,
            devtools::TAPPED
        )
    } else {
        format!(
            "hello from {} with {} window(s); the snapshot has {} lines",
            probed.app.app_id,
            probed.app.windows.len(),
            probed.snapshot.lines().count()
        )
    }))
}
