//! Scroll layers on iOS: a scrolled `div` and a `list` draw from tiles, as many frames as a pan
//! and its fling make, and show what the same page shows without layers; a page whose app got a
//! memory warning or went to the background rasterizes its tiles again.
//!
//! The pages are the browser's cards ([`gpux_conformance::scroll_layers`]). Pixels come from the
//! window's wgpu renderer, which renders the frame the window last drew: a composited one.

use crate::ios::request;
use gpux::semantic_query::Query;
use gpux::{AnyWindowHandle, AppContext as _, AsyncApp, WindowOptions};
use gpux_conformance::{
    SceneSpec,
    app_host::{self, SceneFrame},
    report::{Outcome, ResultLine},
    scroll_layers::{
        self, Counters, DIV, LIST, PAN, SCROLLER, same_pixels, scroller_pixels, set_layers,
        timed_scroll,
    },
};
use image::RgbaImage;
use std::time::{Duration, Instant};

/// How many of a pan's frames must come from tiles: the first of a pan and the frames the layer
/// is promoted in need not.
const PAN_COMPOSITED: u64 = 4;

/// How long a pan step waits for the frame it asks for.
const FRAME_LIMIT: Duration = Duration::from_secs(2);

/// How long the testkit's background and foreground take to show in the window.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);

fn result(outcome: Outcome, id: &str, detail: impl Into<String>) -> ResultLine {
    ResultLine::new(outcome, id.to_string(), detail.into())
}

fn pass(id: &str, detail: impl Into<String>) -> ResultLine {
    result(Outcome::Pass, id, detail)
}

fn fail(id: &str, detail: impl Into<String>) -> ResultLine {
    result(Outcome::Fail, id, detail)
}

fn skip(id: &str, detail: impl Into<String>) -> ResultLine {
    result(Outcome::Skip, id, detail)
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

fn has_renderer(cx: &mut AsyncApp, window: AnyWindowHandle) -> bool {
    window
        .update(cx, |_, window, _| window.gpu_specs())
        .ok()
        .flatten()
        .is_some()
}

fn scale_factor(cx: &mut AsyncApp, window: AnyWindowHandle) -> f32 {
    window
        .update(cx, |_, window, _| window.scale_factor())
        .unwrap_or(1.)
}

/// The first number after `prefix` in the page's status line.
fn status_number(cx: &mut AsyncApp, window: AnyWindowHandle, prefix: &str) -> Option<f32> {
    let value = window
        .update(cx, |_, window, _| {
            Query::role("status")
                .resolve(&window.devtools_frame_snapshot())
                .ok()
                .and_then(|status| status.semantics.value.clone())
        })
        .ok()
        .flatten()?;
    let (_, rest) = value.split_once(prefix)?;
    rest.split(' ').next()?.parse().ok()
}

/// How far the page's container has scrolled: the `div`'s offset, or the list's top row.
fn scrolled(cx: &mut AsyncApp, window: AnyWindowHandle, list: bool) -> f32 {
    status_number(cx, window, if list { " list " } else { "div " }).unwrap_or(0.)
}

/// Turns the wheel through `steps` of [`PAN`] over the page's container, a frame each, and lets
/// the last one settle.
async fn pan(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    steps: std::ops::Range<usize>,
) -> anyhow::Result<()> {
    let at = scroll_layers::pan_position();
    for step in steps {
        scroll_layers::wheel(cx, window, at, PAN[step])?;
        scroll_layers::next_frame(cx, window, FRAME_LIMIT).await;
    }
    for _ in 0..3 {
        scroll_layers::next_frame(cx, window, Duration::from_millis(200)).await;
    }
    Ok(())
}

/// Opens `scene` with layers on or off and scrolls it through `steps` of [`PAN`].
async fn open_scrolled(
    cx: &mut AsyncApp,
    scene: &'static SceneSpec,
    layers: bool,
    steps: std::ops::Range<usize>,
) -> anyhow::Result<AnyWindowHandle> {
    let window = open(cx, scene).await?;
    set_layers(cx, window, layers)?;
    scroll_layers::next_frame(cx, window, FRAME_LIMIT).await;
    pan(cx, window, steps).await?;
    Ok(window)
}

/// What the window shows as its last frame was drawn, and the counters.
async fn shown(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
) -> anyhow::Result<(RgbaImage, Counters)> {
    let counters = scroll_layers::counters(cx, window)?;
    // The frame the pan left, not one drawn again for the capture.
    let frame = window.update(cx, |_, window, _| window.render_to_image())??;
    Ok((frame, counters))
}

/// The page scrolled through [`PAN`] with layers and without them shows the same pixels in the
/// scroller. Returns what the layered window counted.
async fn same_with_and_without(
    cx: &mut AsyncApp,
    scene: &'static SceneSpec,
) -> anyhow::Result<Counters> {
    let layered = open_scrolled(cx, scene, true, 0..PAN.len()).await?;
    let (with, counters) = shown(cx, layered).await?;
    let scale = scale_factor(cx, layered);
    close(cx, layered);
    let plain = open_scrolled(cx, scene, false, 0..PAN.len()).await?;
    let (without, off) = shown(cx, plain).await?;
    close(cx, plain);
    anyhow::ensure!(
        off.composited == 0 && off.tiles_composited == 0,
        "the page without layers composited tiles ({off})"
    );
    same_pixels(&with, &without, scroller_pixels(scale))?;
    Ok(counters)
}

/// Waits until the page stops scrolling (a fling's momentum is spent) and returns how far it went.
async fn rest(cx: &mut AsyncApp, window: AnyWindowHandle, list: bool) -> f32 {
    let mut last = scrolled(cx, window, list);
    loop {
        sleep(cx, Duration::from_millis(300)).await;
        let now = scrolled(cx, window, list);
        if now == last {
            return now;
        }
        last = now;
    }
}

/// Touch pans, as the platform delivers them, scroll the page from tiles: the pan and the fling
/// that follows count as scrolls. Two pans, so the layer is promoted and composited by the second.
async fn touched(
    cx: &mut AsyncApp,
    scene: &'static SceneSpec,
    list: bool,
) -> Result<(f32, Counters), String> {
    let window = open(cx, scene)
        .await
        .map_err(|error| format!("the page never drew: {error:#}"))?;
    set_layers(cx, window, true).map_err(|error| format!("{error:#}"))?;
    scroll_layers::next_frame(cx, window, FRAME_LIMIT).await;
    let (x, y, width, height) = SCROLLER;
    for _ in 0..2 {
        // From near the bottom of the container to near its top, in six frames: fast enough
        // for fling momentum.
        scroll_layers::touch_pan(
            cx,
            window,
            (x + width / 2., y + height - 20.),
            -(height - 40.),
            6,
        )
        .await
        .map_err(|error| format!("{error:#}"))?;
        rest(cx, window, list).await;
    }
    let distance = scrolled(cx, window, list);
    let counters = scroll_layers::counters(cx, window).map_err(|error| format!("{error:#}"))?;
    close(cx, window);
    if distance <= 0. {
        return Err("the touch pans never scrolled the page".into());
    }
    Ok((distance, counters))
}

async fn page(cx: &mut AsyncApp, id: &str, scene: &'static SceneSpec, list: bool) -> ResultLine {
    let probe = match open(cx, scene).await {
        Ok(window) => window,
        Err(error) => return fail(id, format!("the page never drew: {error:#}")),
    };
    let metal = has_renderer(cx, probe);
    close(cx, probe);
    if !metal {
        return skip(
            id,
            "the window has no renderer: this Simulator offers no Metal device, and scroll layers need a renderer with tiles",
        );
    }
    let (distance, touch) = match touched(cx, scene, list).await {
        Ok(touched) => touched,
        Err(reason) => return fail(id, reason),
    };
    if touch.budget != scroll_layers::MOBILE_TILE_BUDGET {
        return fail(
            id,
            format!(
                "the window may keep {} bytes of tiles, not the mobile budget ({touch})",
                touch.budget
            ),
        );
    }
    if touch.composited < PAN_COMPOSITED {
        return fail(
            id,
            format!(
                "touch pans scrolled the page by {distance} but few frames came from tiles ({touch})"
            ),
        );
    }
    match same_with_and_without(cx, scene).await {
        Ok(wheel) if wheel.composited >= PAN_COMPOSITED => pass(
            id,
            format!(
                "touch pans scrolled by {distance} with {touch}; the wheel's pans show the same pixels with and without layers ({wheel})"
            ),
        ),
        Ok(wheel) => fail(id, format!("the pans composited too few frames ({wheel})")),
        Err(error) => fail(id, format!("{error:#}")),
    }
}

/// A scrolled `div`.
pub async fn div(cx: &mut AsyncApp) -> ResultLine {
    page(cx, "scroll-layers-ios-div", &DIV, false).await
}

/// A scrolled `list`.
pub async fn list(cx: &mut AsyncApp) -> ResultLine {
    page(cx, "scroll-layers-ios-list", &LIST, true).await
}

/// How many times the timed scrolls go down and up over the page: 32 frames each.
const TIMED_ROUNDS: usize = 4;

/// What scrolling costs per frame with scroll layers on and off, for the page of each scene: the
/// frame work (drawing and presenting a frame) at its median, 95th percentile and slowest, and
/// the frames that came from tiles. The numbers are of the machine the host runs on, a Simulator
/// included, which shares the Mac's CPU and runs Metal through it. The check holds the counters:
/// frames come from tiles with layers on and none does with them off.
pub async fn timing(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "scroll-layers-ios-timing";
    let mut report = Vec::new();
    for (name, scene) in [("div", &DIV), ("list", &LIST)] {
        for layers in [true, false] {
            let window = match open(cx, scene).await {
                Ok(window) => window,
                Err(error) => return fail(ID, format!("the page never drew: {error:#}")),
            };
            if !has_renderer(cx, window) {
                close(cx, window);
                return skip(
                    ID,
                    "the window has no renderer: this Simulator offers no Metal device",
                );
            }
            if let Err(error) = set_layers(cx, window, layers) {
                return fail(ID, format!("{error:#}"));
            }
            scroll_layers::next_frame(cx, window, FRAME_LIMIT).await;
            let measured = timed_scroll(cx, window, TIMED_ROUNDS, FRAME_LIMIT).await;
            close(cx, window);
            let (work, counters) = match measured {
                Ok(measured) => measured,
                Err(error) => return fail(ID, format!("{error:#}")),
            };
            if layers && counters.composited < PAN_COMPOSITED {
                return fail(
                    ID,
                    format!(
                        "the {name} scrolled with layers on, few frames from tiles ({counters})"
                    ),
                );
            }
            if !layers && (counters.composited != 0 || counters.tiles_composited != 0) {
                return fail(
                    ID,
                    format!("the {name} scrolled with layers off, from tiles ({counters})"),
                );
            }
            report.push(format!(
                "gpux-timing: scroll layers {name} on Metal with layers {}: {work}, {} of {} frames from tiles",
                if layers { "on" } else { "off" },
                counters.composited,
                counters.frames,
            ));
        }
    }
    pass(ID, report.join("; "))
}

/// Whether the window is the active one.
fn active(cx: &mut AsyncApp, window: AnyWindowHandle) -> bool {
    window
        .update(cx, |_, window, _| window.is_window_active())
        .unwrap_or(false)
}

/// What the renderer holds, and how often it has released tiles.
fn renderer(counters: &Counters) -> (u64, u64, u64) {
    counters.renderer.map_or((0, 0, 0), |renderer| {
        (renderer.held, renderer.rasterized, renderer.released)
    })
}

/// A page whose app received a memory warning, and then went to the background and came back,
/// rasterizes its tiles again each time and shows what a page that never lost them shows.
pub async fn resume(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "scroll-layers-ios-resume";
    let probe = match open(cx, &DIV).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the page never drew: {error:#}")),
    };
    let metal = has_renderer(cx, probe);
    close(cx, probe);
    if !metal {
        return skip(
            ID,
            "the window has no renderer: this Simulator offers no Metal device",
        );
    }
    let third = PAN.len() / 3;
    let window = match open_scrolled(cx, &DIV, true, 0..third).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the page never drew: {error:#}")),
    };
    let result = async {
        let before = scroll_layers::counters(cx, window)?;
        let (held, ..) = renderer(&before);
        anyhow::ensure!(
            held > 0 && before.composited > 0,
            "the page held no tiles before the warning ({before})"
        );
        // The system's memory warning reaches the app delegate, which calls this.
        gpui_mobile::ios::ffi::gpui_ios_did_receive_memory_warning(std::ptr::null_mut());
        let warned = scroll_layers::counters(cx, window)?;
        anyhow::ensure!(
            renderer(&warned).0 == 0 && renderer(&warned).2 > renderer(&before).2,
            "the renderer kept its tiles after the warning ({warned})"
        );
        pan(cx, window, third..2 * third).await?;
        let rebuilt = scroll_layers::counters(cx, window)?;
        anyhow::ensure!(
            renderer(&rebuilt).0 > 0 && renderer(&rebuilt).1 > renderer(&before).1,
            "the tiles were not rasterized again after the warning ({rebuilt})"
        );
        // The app goes to the background: its tiles go, and come back with the app.
        request("background");
        let start = Instant::now();
        while active(cx, window) && start.elapsed() < ACTION_TIMEOUT {
            sleep(cx, Duration::from_millis(50)).await;
        }
        anyhow::ensure!(
            !active(cx, window),
            "the window never went to the background"
        );
        // The window deactivates when the app resigns, and gives its tiles back once the app
        // has entered the background.
        let start = Instant::now();
        while renderer(&scroll_layers::counters(cx, window)?).0 > 0
            && start.elapsed() < ACTION_TIMEOUT
        {
            sleep(cx, Duration::from_millis(50)).await;
        }
        let away = scroll_layers::counters(cx, window)?;
        anyhow::ensure!(
            renderer(&away).0 == 0 && renderer(&away).2 > renderer(&rebuilt).2,
            "the renderer kept its tiles in the background ({away})"
        );
        request("foreground");
        let start = Instant::now();
        while !active(cx, window) && start.elapsed() < ACTION_TIMEOUT {
            sleep(cx, Duration::from_millis(50)).await;
        }
        anyhow::ensure!(active(cx, window), "the window never came back");
        pan(cx, window, 2 * third..PAN.len()).await?;
        let (with, after) = shown(cx, window).await?;
        anyhow::ensure!(
            renderer(&after).0 > 0 && renderer(&after).1 > renderer(&away).1,
            "the tiles were not rasterized again after the app came back ({after})"
        );
        let scale = scale_factor(cx, window);
        Ok::<_, anyhow::Error>((with, scale, warned, away, after))
    }
    .await;
    close(cx, window);
    let (with, scale, warned, away, after) = match result {
        Ok(result) => result,
        Err(error) => return fail(ID, format!("{error:#}")),
    };
    let plain = match open_scrolled(cx, &DIV, false, 0..PAN.len()).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("{error:#}")),
    };
    let without = shown(cx, plain).await;
    close(cx, plain);
    match without.map(|(without, _)| same_pixels(&with, &without, scroller_pixels(scale))) {
        Ok(Ok(())) => pass(
            ID,
            format!(
                "tiles released by a memory warning ({warned}) and in the background ({away}), and rebuilt each time ({after}), showing what a page that kept them shows"
            ),
        ),
        Ok(Err(error)) => fail(ID, format!("after resuming the page differs: {error:#}")),
        Err(error) => fail(ID, format!("{error:#}")),
    }
}
