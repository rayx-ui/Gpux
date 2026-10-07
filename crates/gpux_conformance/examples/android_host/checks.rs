//! Android platform checks: startup and the backend the device gave, and the checks that need the
//! operating system (touch through `adb shell input`, and the activity going to the background
//! and coming back), which the testkit performs on request.

use crate::android::{Screens, request, selected};
use gpux::semantic_query::Query;
use gpux::{
    AnyWindowHandle, AppContext as _, AsyncApp, Context, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Window, WindowOptions, div, px, rgb,
};
use gpux_conformance::{
    SceneSpec, Step,
    app_host::{self, SceneFrame},
    evaluate::screen_shows,
    report::{Outcome, ResultLine},
    scene,
};
use std::time::{Duration, Instant};

/// How long a check waits for the testkit's action to take effect.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);

fn pass(id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(Outcome::Pass, id.to_string(), detail.into())
}

fn fail(id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(Outcome::Fail, id.to_string(), detail.into())
}

fn skip(id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(Outcome::Skip, id.to_string(), detail.into())
}

/// Whether the window renders in software (emulators without a host GPU).
fn software_rendered(cx: &mut AsyncApp, window: AnyWindowHandle) -> bool {
    window
        .update(cx, |_, window, _| window.gpu_specs())
        .ok()
        .flatten()
        .is_some_and(|specs| specs.is_software_emulated)
}

async fn open(cx: &mut AsyncApp, scene: &'static SceneSpec) -> anyhow::Result<AnyWindowHandle> {
    let window = cx.open_window(WindowOptions::default(), |window, cx| {
        cx.new(|cx| SceneFrame::new(scene, window, cx))
    })?;
    let window = AnyWindowHandle::from(window);
    app_host::wait_frames(cx, window, 2).await?;
    Ok(window)
}

fn close(cx: &mut AsyncApp, window: AnyWindowHandle) {
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

async fn sleep(cx: &mut AsyncApp, duration: Duration) {
    cx.background_executor().timer(duration).await;
}

/// The element `query` matches in the latest frame.
fn find(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    query: &Query,
) -> Option<gpux::DevtoolsElementSnapshot> {
    window
        .update(cx, |_, window, _| {
            query
                .resolve(&window.devtools_frame_snapshot())
                .ok()
                .cloned()
        })
        .ok()
        .flatten()
}

/// The center of the element `query` matches, in the display's physical pixels.
fn physical_center(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    query: &Query,
) -> Option<(i32, i32)> {
    let element = find(cx, window, query)?;
    let scale = window
        .update(cx, |_, window, _| window.scale_factor())
        .ok()?;
    let x = element.bounds.x + element.bounds.width / 2.;
    let y = element.bounds.y + element.bounds.height / 2.;
    Some(((x * scale).round() as i32, (y * scale).round() as i32))
}

/// The scroll offset a scrolling scene's status line reports (`offset <n>`).
fn offset(cx: &mut AsyncApp, window: AnyWindowHandle) -> Option<f32> {
    let status = find(cx, window, &Query::role("status"))?;
    status
        .semantics
        .value?
        .strip_prefix("offset ")?
        .parse()
        .ok()
}

/// Waits until `done` holds, polling every frame, for at most [`ACTION_TIMEOUT`].
async fn wait_for(
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

/// Whether a frame holds anything but transparent black.
fn drew(frame: &image::RgbaImage) -> bool {
    frame.pixels().any(|pixel| pixel.0 != [0, 0, 0, 0])
}

/// The center and color of the first uniform region `scene`'s script probes: a known pixel of the
/// scene to look for on the screen.
fn known_pixel(scene: &SceneSpec) -> Option<(f32, f32, [u8; 4])> {
    (scene.script)().into_iter().find_map(|step| match step {
        Step::ProbeUniform((x, y, width, height), color, _) => {
            Some((x + width / 2., y + height / 2., color))
        }
        _ => None,
    })
}

fn scale_factor(cx: &mut AsyncApp, window: AnyWindowHandle) -> f32 {
    window
        .update(cx, |_, window, _| window.scale_factor())
        .unwrap_or(1.)
}

/// Whether a capture failed because the window's surface is gone (the app is in the background).
fn surface_lost(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("unconfigured")
}

/// Captures the window's frame once, without retrying: whether its surface is there.
fn capture_state(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
) -> Option<anyhow::Result<image::RgbaImage>> {
    window
        .update(cx, |_, window, _| window.render_to_image())
        .ok()
}

/// Startup: a window opens and shows its scene through the backend the device gave (Vulkan, or
/// the GL fallback). The renderer's own readback is reported too: emulators' GL drivers read
/// rendered textures back empty, which is why pixel steps read the screen.
pub async fn startup(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "android-startup";
    let Some(scene) = scene("paint-borders") else {
        return fail(ID, "the catalog has no paint-borders scene");
    };
    let window = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the first window never drew: {error:#}")),
    };
    let backend = window
        .update(cx, |_, window, _| window.gpu_specs())
        .ok()
        .flatten()
        .map(|specs| format!("{} on {}", specs.backend, specs.device_name))
        .unwrap_or_else(|| "an unknown backend".into());
    let readback = match app_host::render_to_image(cx, window).await {
        Ok(frame) if drew(&frame) => {
            format!("renderer readback {}x{}", frame.width(), frame.height())
        }
        Ok(_) => "renderer readback empty on this device".into(),
        Err(error) => format!("no renderer readback: {error:#}"),
    };
    let scale = scale_factor(cx, window);
    let screen = screens.capture(cx, window).await;
    close(cx, window);
    let Some((x, y, color)) = known_pixel(scene) else {
        return fail(ID, "paint-borders probes no uniform region to look for");
    };
    match screen {
        Ok(screen) => match screen_shows(&screen, scale, x, y, color) {
            Ok(()) => pass(
                ID,
                format!(
                    "backend {backend}, showing the scene's border on a {}x{} screen; {readback}",
                    screen.width(),
                    screen.height()
                ),
            ),
            Err(error) => fail(
                ID,
                format!(
                    "backend {backend}, but the screen does not show the scene ({error:#}); {readback}"
                ),
            ),
        },
        Err(error) => fail(
            ID,
            format!("backend {backend}, but no screenshot arrived: {error:#}"),
        ),
    }
}

/// A tap through the operating system clicks the element under it.
async fn touch_tap(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "android-touch-tap";
    let Some(scene) = scene("overlay-anchored") else {
        return fail(ID, "the catalog has no overlay-anchored scene");
    };
    let window = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the scene never drew: {error:#}")),
    };
    let toggle = Query::id("toggle");
    let popover = Query::id("popover");
    let result = match physical_center(cx, window, &toggle) {
        None => fail(ID, "the toggle is not on screen"),
        Some((x, y)) => {
            request(&format!("tap {x} {y}"));
            if wait_for(cx, window, |cx| find(cx, window, &popover).is_some()).await {
                pass(ID, format!("tapped ({x}, {y}) and the popover opened"))
            } else {
                fail(
                    ID,
                    format!("tapped ({x}, {y}) but the popover never opened"),
                )
            }
        }
    };
    close(cx, window);
    result
}

/// A swipe through the operating system scrolls the element under it: a slow one by about its
/// distance, a fast one further, carried by fling momentum after the finger lifts.
async fn touch_swipe(
    cx: &mut AsyncApp,
    id: &str,
    distance: f32,
    duration_ms: u32,
    at_least: f32,
    timing: bool,
) -> ResultLine {
    let Some(scene) = scene("scroll-wheel") else {
        return fail(id, "the catalog has no scroll-wheel scene");
    };
    let window = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(id, format!("the scene never drew: {error:#}")),
    };
    if timing && software_rendered(cx, window) {
        close(cx, window);
        return skip(
            id,
            "the software renderer handles input between slow frames, so a fast swipe arrives as a tap",
        );
    }
    let scale = window
        .update(cx, |_, window, _| window.scale_factor())
        .unwrap_or(1.);
    let result = match find(cx, window, &Query::id("scroller")) {
        None => fail(id, "the scroller is not on screen"),
        Some(scroller) => {
            // From near the scroller's bottom upwards, staying clear of the screen's edges,
            // where system gestures take swipes.
            let x = ((scroller.bounds.x + scroller.bounds.width / 2.) * scale).round() as i32;
            let start = scroller.bounds.y + scroller.bounds.height - 10.;
            let y = (start * scale).round() as i32;
            let to = ((start - distance) * scale).round() as i32;
            request(&format!("swipe {x} {y} {x} {to} {duration_ms}"));
            let moved = wait_for(cx, window, |cx| {
                offset(cx, window).is_some_and(|offset| offset > 0.)
            })
            .await;
            // Let any fling settle before reading the distance.
            let mut last = offset(cx, window).unwrap_or(0.);
            loop {
                sleep(cx, Duration::from_millis(250)).await;
                let now = offset(cx, window).unwrap_or(0.);
                if now == last {
                    break;
                }
                last = now;
            }
            if moved && last >= at_least {
                pass(
                    id,
                    format!("swiped {distance} pixels in {duration_ms} ms and scrolled {last}"),
                )
            } else {
                fail(
                    id,
                    format!(
                        "swiped {distance} pixels in {duration_ms} ms but scrolled {last}, expected at least {at_least}"
                    ),
                )
            }
        }
    };
    close(cx, window);
    result
}

/// Records the window's activation changes, over a white screen with a red block from logical
/// (40, 40) to (160, 120): a known pixel to find on the screen after coming back.
struct Lifecycle {
    changes: Vec<bool>,
    _activation: Subscription,
}

impl Render for Lifecycle {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xffffff)).child(
            div()
                .absolute()
                .top(px(40.))
                .left(px(40.))
                .w(px(120.))
                .h(px(80.))
                .bg(rgb(0xff0000)),
        )
    }
}

/// The center of the lifecycle window's red block, and its color.
const BLOCK: (f32, f32, [u8; 4]) = (100., 80., [255, 0, 0, 255]);

/// Going to the background and coming back: the system takes the window's surface away, the
/// window deactivates and reactivates, the surface comes back, and the screen shows the window's
/// block again.
async fn lifecycle(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "android-lifecycle";
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
    // In the background the system takes the surface away; capture then reports it unconfigured.
    let start = Instant::now();
    let mut lost = false;
    while start.elapsed() < ACTION_TIMEOUT {
        if let Some(Err(error)) = capture_state(cx, handle)
            && surface_lost(&error)
        {
            lost = true;
            break;
        }
        sleep(cx, Duration::from_millis(50)).await;
    }
    let changes = |cx: &mut AsyncApp| {
        window
            .update(cx, |view, _, _| view.changes.clone())
            .unwrap_or_default()
    };
    let start = Instant::now();
    while start.elapsed() < ACTION_TIMEOUT && !changes(cx).ends_with(&[false, true]) {
        sleep(cx, Duration::from_millis(50)).await;
    }
    let seen = changes(cx);
    // Back in front, the surface comes back: capture works again.
    let start = Instant::now();
    let mut recovered = false;
    while start.elapsed() < ACTION_TIMEOUT {
        if let Some(Ok(_)) = capture_state(cx, handle) {
            recovered = true;
            break;
        }
        sleep(cx, Duration::from_millis(50)).await;
    }
    let result = if !lost {
        fail(ID, "the surface was never taken away in the background")
    } else if !seen.ends_with(&[false, true]) {
        fail(
            ID,
            format!("the window's activation went {seen:?}, not inactive then active"),
        )
    } else if !recovered {
        fail(ID, "the surface never came back after the app returned")
    } else {
        let scale = scale_factor(cx, handle);
        let (x, y, color) = BLOCK;
        match screens.capture(cx, handle).await {
            Ok(screen) => match screen_shows(&screen, scale, x, y, color) {
                Ok(()) => pass(
                    ID,
                    format!(
                        "lost and recovered the surface, deactivated and reactivated, then showed \
                         the window's block on a {}x{} screen",
                        screen.width(),
                        screen.height()
                    ),
                ),
                Err(error) => fail(
                    ID,
                    format!("came back, but the screen does not show the window ({error:#})"),
                ),
            },
            Err(error) => fail(
                ID,
                format!("came back, but no screenshot arrived: {error:#}"),
            ),
        }
    };
    close(cx, handle);
    result
}

/// The checks that need the operating system, in order.
pub async fn os_checks(cx: &mut AsyncApp, screens: &Screens) -> Vec<ResultLine> {
    let mut lines = Vec::new();
    if selected("android-touch-tap") {
        lines.push(touch_tap(cx).await);
    }
    // Slowly, the content follows the finger; fast, momentum carries it further.
    if selected("android-touch-pan") {
        lines.push(touch_swipe(cx, "android-touch-pan", 40., 600, 20., false).await);
    }
    // `adb shell input` injects each event only after the app handled the one before, and a
    // software renderer handles input between frames that take hundreds of milliseconds: a fast
    // swipe then arrives as a tap. Like lane B's software profiles, it asserts no timing there.
    if selected("android-touch-fling") {
        lines.push(touch_swipe(cx, "android-touch-fling", 80., 120, 100., true).await);
    }
    if selected("android-lifecycle") {
        lines.push(lifecycle(cx, screens).await);
    }
    lines
}
