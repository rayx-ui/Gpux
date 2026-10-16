//! iOS platform checks: startup, wgpu on Metal, CoreText metrics of a bundled font, in-app touch
//! injection, and going to the background and coming back, which the testkit performs on
//! request.

use crate::ios::{request, selected};
use gpux::semantic_query::Query;
use gpux::{
    AnyWindowHandle, AppContext as _, AsyncApp, Context, IntoElement, ParentElement as _,
    PlatformInput, Point, Render, Styled as _, Subscription, TextRun, TouchEvent, TouchId,
    TouchPhase, Window, WindowOptions, div, font, point, px, rgb,
};
use gpux_conformance::{
    SceneSpec,
    app_host::{self, SceneFrame},
    report::{Outcome, ResultLine},
    scene,
};
use std::time::{Duration, Instant};

/// How long a check waits for an action to take effect.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);

/// IBM Plex Sans Regular (bundled by `gpux_fonts`) at 16 px, from the font file itself: 1000
/// units per em, `hhea` ascender 1025 and descender -275, and the advances of `m` (873 units)
/// and `0` (600). "Conformance" spans 6027 units unkerned.
const FONT_SIZE: f32 = 16.;
const ASCENT: f32 = 16.4;
const DESCENT: f32 = 4.4;
const ADVANCE_M: f32 = 13.968;
const ADVANCE_ZERO: f32 = 9.6;
const CONFORMANCE: &str = "Conformance";
const CONFORMANCE_WIDTH: f32 = 96.432;
/// Metrics are exact up to float rounding; shaping may kern.
const METRIC_TOLERANCE: f32 = 0.05;
const SHAPED_TOLERANCE: f32 = 2.;

pub(crate) fn pass(id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(Outcome::Pass, id.to_string(), detail.into())
}

pub(crate) fn fail(id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(Outcome::Fail, id.to_string(), detail.into())
}

fn skip(id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(Outcome::Skip, id.to_string(), detail.into())
}

pub(crate) async fn open(
    cx: &mut AsyncApp,
    scene: &'static SceneSpec,
) -> anyhow::Result<AnyWindowHandle> {
    let window = cx.open_window(WindowOptions::default(), |window, cx| {
        // The checks find elements in the window's accessibility tree.
        gpux_conformance::evaluate::keep_inspecting(window);
        cx.new(|cx| SceneFrame::new(scene, window, cx))
    })?;
    let window = AnyWindowHandle::from(window);
    app_host::wait_frames(cx, window, 2).await?;
    Ok(window)
}

pub(crate) fn close(cx: &mut AsyncApp, window: AnyWindowHandle) {
    gpux_conformance::evaluate::stop_inspecting(window);
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

async fn sleep(cx: &mut AsyncApp, duration: Duration) {
    cx.background_executor().timer(duration).await;
}

/// The element `query` matches in the latest frame.
pub(crate) fn find(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    query: &Query,
) -> Option<gpux::semantic_query::SemanticNode> {
    window
        .update(cx, |_, window, _| {
            let snapshot = gpux_conformance::evaluate::snapshot(window).ok()?;
            query.resolve(&snapshot).ok().cloned()
        })
        .ok()
        .flatten()
}

/// Waits until `done` holds, polling every frame, for at most [`ACTION_TIMEOUT`].
pub(crate) async fn wait_for(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    mut done: impl FnMut(&mut AsyncApp) -> bool,
) -> bool {
    let start = Instant::now();
    while start.elapsed() < ACTION_TIMEOUT {
        if done(cx) {
            return true;
        }
        if app_host::wait_frames(cx, window, 1).await.is_err() {
            sleep(cx, Duration::from_millis(16)).await;
        }
    }
    done(cx)
}

/// The backend and device the window renders with, or `None` without a renderer.
fn backend(cx: &mut AsyncApp, window: AnyWindowHandle) -> Option<(String, String)> {
    window
        .update(cx, |_, window, _| window.gpu_specs())
        .ok()
        .flatten()
        .map(|specs| (specs.backend, specs.device_name))
}

/// Whether a frame holds anything but transparent black.
fn drew(frame: &image::RgbaImage) -> bool {
    frame.pixels().any(|pixel| pixel.0 != [0, 0, 0, 0])
}

/// Startup: a window opens and its frames arrive.
async fn startup(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "ios-startup";
    let Some(scene) = scene("paint-borders") else {
        return fail(ID, "the catalog has no paint-borders scene");
    };
    match open(cx, scene).await {
        Ok(window) => {
            let detail = match backend(cx, window) {
                Some((backend, device)) => {
                    format!("a window opened and drew through {backend} on {device}")
                }
                None => "a window opened and ran its frames, without a renderer".into(),
            };
            close(cx, window);
            pass(ID, detail)
        }
        Err(error) => fail(ID, format!("the first window never drew: {error:#}")),
    }
}

/// wgpu renders through Metal, and reads its frames back.
async fn metal(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "ios-metal";
    let Some(scene) = scene("paint-borders") else {
        return fail(ID, "the catalog has no paint-borders scene");
    };
    let window = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the window never drew: {error:#}")),
    };
    let result = match backend(cx, window) {
        None => skip(
            ID,
            "the window has no renderer: this Simulator offers no Metal device",
        ),
        Some((backend, device)) if !backend.eq_ignore_ascii_case("metal") => fail(
            ID,
            format!("the window renders through {backend} on {device}, not Metal"),
        ),
        Some((_, device)) => match app_host::render_to_image(cx, window).await {
            Ok(frame) if drew(&frame) => pass(
                ID,
                format!(
                    "wgpu on Metal ({device}), read back {}x{}",
                    frame.width(),
                    frame.height()
                ),
            ),
            Ok(_) => fail(
                ID,
                format!("wgpu on Metal ({device}) read back an empty frame"),
            ),
            Err(error) => fail(
                ID,
                format!("wgpu on Metal ({device}) could not read back: {error:#}"),
            ),
        },
    };
    close(cx, window);
    result
}

fn near(actual: f32, expected: f32, tolerance: f32) -> bool {
    (actual - expected).abs() <= tolerance
}

/// CoreText measures the bundled IBM Plex Sans as its font file declares, and shapes a line to
/// its advances (within kerning).
async fn text_metrics(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "ios-text-metrics";
    let Some(scene) = scene("paint-borders") else {
        return fail(ID, "the catalog has no paint-borders scene");
    };
    let window = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the window never drew: {error:#}")),
    };
    let measured = window.update(cx, |_, window, cx| {
        let plex = font("IBM Plex Sans");
        let system = cx.text_system();
        let id = system.resolve_font(&plex);
        let size = px(FONT_SIZE);
        let advance = |ch| {
            system
                .advance(id, size, ch)
                .map(|advance| f32::from(advance.width))
                .unwrap_or(f32::NAN)
        };
        let line = window.text_system().layout_line(
            CONFORMANCE,
            size,
            px(0.),
            &[TextRun {
                len: CONFORMANCE.len(),
                font: plex,
                color: Default::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );
        (
            system.units_per_em(id),
            f32::from(system.ascent(id, size)),
            f32::from(system.descent(id, size)).abs(),
            advance('m'),
            advance('0'),
            f32::from(line.width),
        )
    });
    close(cx, window);
    let Ok((units, ascent, descent, m, zero, width)) = measured else {
        return fail(ID, "the window closed before measuring");
    };
    let mut wrong = Vec::new();
    if units != 1000 {
        wrong.push(format!(
            "{units} units per em (IBM Plex Sans did not resolve?)"
        ));
    }
    for (name, actual, expected, tolerance) in [
        ("ascent", ascent, ASCENT, METRIC_TOLERANCE),
        ("descent", descent, DESCENT, METRIC_TOLERANCE),
        ("advance of m", m, ADVANCE_M, METRIC_TOLERANCE),
        ("advance of 0", zero, ADVANCE_ZERO, METRIC_TOLERANCE),
        (
            "width of \"Conformance\"",
            width,
            CONFORMANCE_WIDTH,
            SHAPED_TOLERANCE,
        ),
    ] {
        if !near(actual, expected, tolerance) {
            wrong.push(format!(
                "{name} {actual}, expected {expected} ± {tolerance}"
            ));
        }
    }
    if wrong.is_empty() {
        pass(
            ID,
            format!(
                "IBM Plex Sans at {FONT_SIZE} px: ascent {ascent}, descent {descent}, \"{CONFORMANCE}\" {width} px wide"
            ),
        )
    } else {
        fail(ID, wrong.join("; "))
    }
}

fn touch(
    window: &mut Window,
    cx: &mut gpux::App,
    phase: TouchPhase,
    position: Point<gpux::Pixels>,
) {
    window.dispatch_event(
        PlatformInput::Touch(TouchEvent {
            id: TouchId(1),
            phase,
            position,
            predicted_position: None,
            force: None,
            timestamp: None,
        }),
        cx,
    );
}

/// A touch injected into the window taps the element under it.
async fn touch_tap(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "ios-touch-tap";
    let Some(scene) = scene("overlay-anchored") else {
        return fail(ID, "the catalog has no overlay-anchored scene");
    };
    let window = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the scene never drew: {error:#}")),
    };
    let popover = Query::id("popover");
    let result = match find(cx, window, &Query::id("toggle")) {
        None => fail(ID, "the toggle is not on screen"),
        Some(toggle) => {
            let at = point(
                px(toggle.bounds.x + toggle.bounds.width / 2.),
                px(toggle.bounds.y + toggle.bounds.height / 2.),
            );
            window
                .update(cx, |_, window, cx| {
                    touch(window, cx, TouchPhase::Started, at)
                })
                .ok();
            app_host::wait_frames(cx, window, 1).await.ok();
            window
                .update(cx, |_, window, cx| touch(window, cx, TouchPhase::Ended, at))
                .ok();
            if wait_for(cx, window, |cx| find(cx, window, &popover).is_some()).await {
                pass(
                    ID,
                    format!("tapped ({}, {}) and the popover opened", at.x, at.y),
                )
            } else {
                fail(
                    ID,
                    format!("tapped ({}, {}) but the popover never opened", at.x, at.y),
                )
            }
        }
    };
    close(cx, window);
    result
}

/// Records the window's activation changes.
struct Lifecycle {
    changes: Vec<bool>,
    _activation: Subscription,
}

impl Render for Lifecycle {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x2050c0)).child("lifecycle")
    }
}

/// Going to the background and coming back: the window deactivates and reactivates, and draws
/// again.
async fn lifecycle(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "ios-lifecycle";
    let window = match cx.open_window(WindowOptions::default(), |window, cx| {
        cx.new(|cx| Lifecycle {
            changes: Vec::new(),
            _activation: cx.observe_window_activation(
                window,
                |this: &mut Lifecycle, window, cx| {
                    this.changes.push(window.is_window_active());
                    cx.notify();
                },
            ),
        })
    }) {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("could not open a window: {error:#}")),
    };
    let handle = AnyWindowHandle::from(window);
    if let Err(error) = app_host::wait_frames(cx, handle, 2).await {
        close(cx, handle);
        return fail(ID, format!("the window never drew: {error:#}"));
    }
    request("background");
    let changes = |cx: &mut AsyncApp| {
        window
            .update(cx, |view, _, _| view.changes.clone())
            .unwrap_or_default()
    };
    // The host asks to come back once the other app took the front. Brought back sooner, on a
    // Simulator still launching that app, it would end up behind it for good.
    let start = Instant::now();
    while start.elapsed() < ACTION_TIMEOUT && !changes(cx).contains(&false) {
        sleep(cx, Duration::from_millis(50)).await;
    }
    request("foreground");
    let start = Instant::now();
    while start.elapsed() < ACTION_TIMEOUT && !changes(cx).ends_with(&[false, true]) {
        sleep(cx, Duration::from_millis(50)).await;
    }
    let seen = changes(cx);
    let result = if !seen.ends_with(&[false, true]) {
        fail(
            ID,
            format!("the window's activation went {seen:?}, not inactive then active"),
        )
    } else {
        match app_host::render_to_image(cx, handle).await {
            Ok(frame) if drew(&frame) => pass(
                ID,
                format!(
                    "deactivated and reactivated, then drew a {}x{} frame",
                    frame.width(),
                    frame.height()
                ),
            ),
            Ok(_) => fail(ID, "came back, but drew an empty frame"),
            Err(error) if error.to_string().contains("has no renderer") => pass(
                ID,
                "deactivated and reactivated (no renderer to read a frame back from)",
            ),
            Err(error) => fail(ID, format!("came back, but could not read back: {error:#}")),
        }
    };
    close(cx, handle);
    result
}

/// The iOS platform checks, in order.
pub async fn platform_checks(cx: &mut AsyncApp) -> Vec<ResultLine> {
    let mut lines = Vec::new();
    if selected("ios-startup") {
        lines.push(startup(cx).await);
    }
    if selected("ios-metal") {
        lines.push(metal(cx).await);
    }
    if selected("ios-text-metrics") {
        lines.push(text_metrics(cx).await);
    }
    if selected("ios-touch-tap") {
        lines.push(touch_tap(cx).await);
    }
    if selected("ios-lifecycle") {
        lines.push(lifecycle(cx).await);
    }
    if selected("composition-ios-order") {
        lines.push(crate::composition::order(cx).await);
    }
    if selected("composition-ios-input") {
        lines.push(crate::composition::input(cx).await);
    }
    if selected("composition-ios-shadow") {
        lines.push(crate::composition::shadow(cx).await);
    }
    if selected("scroll-layers-ios-div") {
        lines.push(crate::scroll_layers::div(cx).await);
    }
    if selected("scroll-layers-ios-list") {
        lines.push(crate::scroll_layers::list(cx).await);
    }
    if selected("scroll-layers-ios-resume") {
        lines.push(crate::scroll_layers::resume(cx).await);
    }
    if selected("scroll-layers-ios-timing") {
        lines.push(crate::scroll_layers::timing(cx).await);
    }
    if selected("platform-view-ios") {
        lines.push(crate::platform_views::platform_view(cx).await);
    }
    if selected("screen-reader-ios-tree") {
        lines.push(crate::screen_reader::tree(cx).await);
    }
    if selected("screen-reader-ios-activate") {
        lines.push(crate::screen_reader::activate(cx).await);
    }
    if selected("devtools-ios") {
        lines.push(crate::devtools::tap(cx).await);
    }
    lines
}
