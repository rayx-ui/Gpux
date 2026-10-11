//! Scroll layers on Android: a scrolled `div` and a `list` draw from tiles, as many frames as a
//! swipe and its fling make, and show on the screen what the same page shows without layers; a
//! page that went to the background and came back rasterizes its tiles again.
//!
//! The pages are the browser's cards ([`gpux_conformance::scroll_layers`]). The screen is read
//! through `adb screencap` ([`Screens`]), as every pixel step of this host is.

use crate::android::{Screens, request};
use crate::checks::{
    ACTION_TIMEOUT, SWIPE_FRAMES, capture_state, close, fail, find, frame_time, open, pass,
    scale_factor, skip, sleep, surface_lost, wait_for,
};
use gpux::semantic_query::Query;
use gpux::{AnyWindowHandle, AsyncApp};
use gpux_conformance::{
    SceneSpec,
    report::ResultLine,
    scroll_layers::{
        self, Counters, DIV, LIST, PAN, SCROLLER, same_pixels, scroller_pixels, set_layers,
    },
};
use image::RgbaImage;
use std::time::{Duration, Instant};

/// How many frames of a swipe through the system must come from tiles: the finger moves a
/// few times between a slow renderer's frames.
const SWIPE_COMPOSITED_OS: u64 = 2;

/// How long a pan step waits for the frame it asks for.
const FRAME_LIMIT: Duration = Duration::from_secs(2);

/// How many of a swipe's frames must come from tiles: the first of a pan and the frames the
/// layer is promoted in need not.
const SWIPE_COMPOSITED: u64 = 4;

/// The page's `div` offset, from its status line (`div <n>` or `div <n> list <m>`).
fn div_offset(cx: &mut AsyncApp, window: AnyWindowHandle) -> Option<f32> {
    let status = find(cx, window, &Query::role("status"))?;
    status
        .semantics
        .value?
        .strip_prefix("div ")?
        .split(' ')
        .next()?
        .parse()
        .ok()
}

/// The list's first visible row, from the status line.
fn list_row(cx: &mut AsyncApp, window: AnyWindowHandle) -> Option<usize> {
    let status = find(cx, window, &Query::role("status"))?;
    status
        .semantics
        .value?
        .rsplit_once(" list ")?
        .1
        .parse()
        .ok()
}

/// How far the page's container has scrolled: the `div`'s offset, or the list's top row.
fn scrolled(cx: &mut AsyncApp, window: AnyWindowHandle, list: bool) -> f32 {
    if list {
        list_row(cx, window).unwrap_or(0) as f32
    } else {
        div_offset(cx, window).unwrap_or(0.)
    }
}

/// Opens `scene` with layers on or off and scrolls it through `steps` of [`PAN`] a frame each.
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

/// The screen showing the window, scrolled as `pan` leaves it, and the counters.
async fn shown(
    cx: &mut AsyncApp,
    screens: &Screens,
    window: AnyWindowHandle,
) -> anyhow::Result<(RgbaImage, Counters)> {
    // The frame the pan left on screen, not one drawn again for the capture: a window drawn
    // again from scratch composites no layer.
    let counters = scroll_layers::counters(cx, window)?;
    let screen = screens.fetch(cx).await?;
    Ok((screen, counters))
}

/// The page scrolled through [`PAN`] with layers and without them shows the same pixels in the
/// scroller. Returns what the layered window counted.
async fn same_with_and_without(
    cx: &mut AsyncApp,
    screens: &Screens,
    scene: &'static SceneSpec,
) -> anyhow::Result<Counters> {
    let layered = open_scrolled(cx, scene, true, 0..PAN.len()).await?;
    let (with, counters) = shown(cx, screens, layered).await?;
    let scale = scale_factor(cx, layered);
    close(cx, layered);
    let plain = open_scrolled(cx, scene, false, 0..PAN.len()).await?;
    let (without, off) = shown(cx, screens, plain).await?;
    close(cx, plain);
    anyhow::ensure!(
        off.composited == 0 && off.tiles_composited == 0,
        "the page without layers composited tiles ({off})"
    );
    same_pixels(&with, &without, scroller_pixels(scale))?;
    Ok(counters)
}

/// Waits until the page stops scrolling (a fling's momentum is spent) and returns how far it
/// went.
async fn rest(cx: &mut AsyncApp, window: AnyWindowHandle, list: bool, frame: Duration) -> f32 {
    let mut last = scrolled(cx, window, list);
    loop {
        sleep(cx, Duration::from_millis(250).max(frame * 3)).await;
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
    let frame = frame_time(cx, window).await;
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
        rest(cx, window, list, frame).await;
    }
    let distance = scrolled(cx, window, list);
    let counters = scroll_layers::counters(cx, window).map_err(|error| format!("{error:#}"))?;
    close(cx, window);
    if distance <= 0. {
        return Err("the touch pans never scrolled the page".into());
    }
    Ok((distance, counters))
}

/// A swipe through the operating system (`adb shell input swipe`) scrolls the page from tiles
/// too. `adb` injects each event once the app handled the one before and sends only the moves
/// its duration has time for, so the swipe lasts [`SWIPE_FRAMES`] frames at least: a swipe
/// shorter than a frame is a tap, as `android-touch-fling` finds on a software renderer.
async fn swiped(
    cx: &mut AsyncApp,
    scene: &'static SceneSpec,
    list: bool,
) -> Result<(f32, Counters), String> {
    let window = open(cx, scene)
        .await
        .map_err(|error| format!("the page never drew: {error:#}"))?;
    set_layers(cx, window, true).map_err(|error| format!("{error:#}"))?;
    scroll_layers::next_frame(cx, window, FRAME_LIMIT).await;
    let frame = frame_time(cx, window).await;
    let scale = scale_factor(cx, window);
    let (x, y, width, height) = SCROLLER;
    let from_x = ((x + width / 2.) * scale).round() as i32;
    let from_y = ((y + height - 20.) * scale).round() as i32;
    let to_y = ((y + 20.) * scale).round() as i32;
    let duration = ((frame * SWIPE_FRAMES).as_millis() as u32).max(600);
    // Two swipes, so the layer is promoted and composited by the second.
    for _ in 0..2 {
        request(&format!(
            "swipe {from_x} {from_y} {from_x} {to_y} {duration}"
        ));
        let before = scrolled(cx, window, list);
        let moved = wait_for(cx, window, |cx| scrolled(cx, window, list) > before).await;
        sleep(cx, Duration::from_millis(u64::from(duration) + 300)).await;
        if !moved {
            close(cx, window);
            return Err("the swipe never scrolled the page".into());
        }
    }
    let distance = rest(cx, window, list, frame).await;
    let counters = scroll_layers::counters(cx, window).map_err(|error| format!("{error:#}"))?;
    close(cx, window);
    Ok((distance, counters))
}

async fn page(
    cx: &mut AsyncApp,
    screens: &Screens,
    id: &str,
    scene: &'static SceneSpec,
    list: bool,
) -> ResultLine {
    let (distance, touch) = match touched(cx, scene, list).await {
        Ok(touched) => touched,
        Err(reason) => return fail(id, reason),
    };
    if touch.composited < SWIPE_COMPOSITED {
        return fail(
            id,
            format!(
                "touch pans scrolled the page by {distance} but few frames came from tiles ({touch})"
            ),
        );
    }
    let swipe = match swiped(cx, scene, list).await {
        Ok((_, counters)) if counters.composited < SWIPE_COMPOSITED_OS => {
            return fail(
                id,
                format!("a swipe came from tiles in few frames ({counters})"),
            );
        }
        Ok(swipe) => swipe,
        Err(reason) => return fail(id, reason),
    };
    match same_with_and_without(cx, screens, scene).await {
        Ok(wheel) if wheel.composited >= SWIPE_COMPOSITED => pass(
            id,
            format!(
                "touch pans scrolled by {distance} with {touch}; a swipe through the system scrolled by {} with {}; the wheel's pans show the same pixels with and without layers ({wheel})",
                swipe.0, swipe.1
            ),
        ),
        Ok(wheel) => fail(id, format!("the pans composited too few frames ({wheel})")),
        Err(error) => fail(id, format!("{error:#}")),
    }
}

/// A scrolled `div`.
pub async fn div(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    page(cx, screens, "scroll-layers-android-div", &DIV, false).await
}

/// A scrolled `list`.
pub async fn list(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    page(cx, screens, "scroll-layers-android-list", &LIST, true).await
}

/// A page that goes to the background (the system takes its surface and the app gives its tiles
/// back) and comes back rasterizes its tiles again and shows what a page that never left shows.
pub async fn resume(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "scroll-layers-android-resume";
    let half = PAN.len() / 2;
    let window = match open_scrolled(cx, &DIV, true, 0..half).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the page never drew: {error:#}")),
    };
    let before = match scroll_layers::counters(cx, window) {
        Ok(counters) => counters,
        Err(error) => return fail(ID, format!("{error:#}")),
    };
    let Some(held) = before.renderer.map(|renderer| renderer.held) else {
        close(cx, window);
        return skip(ID, "this build reports no renderer counters");
    };
    if held == 0 || before.composited == 0 {
        close(cx, window);
        return fail(
            ID,
            format!("the page held no tiles before going away ({before})"),
        );
    }
    request("background");
    let start = Instant::now();
    let mut lost = false;
    while start.elapsed() < ACTION_TIMEOUT {
        if let Some(Err(error)) = capture_state(cx, window)
            && surface_lost(&error)
        {
            lost = true;
            break;
        }
        sleep(cx, Duration::from_millis(50)).await;
    }
    if !lost {
        close(cx, window);
        return fail(ID, "the surface was never taken away in the background");
    }
    let away = match scroll_layers::counters(cx, window) {
        Ok(counters) => counters,
        Err(error) => return fail(ID, format!("{error:#}")),
    };
    let released = away.renderer.is_some_and(|renderer| {
        renderer.held == 0 && renderer.released > before.renderer.map_or(0, |r| r.released)
    });
    request("foreground");
    let start = Instant::now();
    let mut asked = Instant::now();
    let mut recovered = false;
    while start.elapsed() < ACTION_TIMEOUT {
        if let Some(Ok(_)) = capture_state(cx, window) {
            recovered = true;
            break;
        }
        if asked.elapsed() >= crate::android::REQUEST_REPEAT {
            request("foreground");
            asked = Instant::now();
        }
        sleep(cx, Duration::from_millis(50)).await;
    }
    if !recovered {
        close(cx, window);
        return fail(ID, "the surface never came back after the app returned");
    }
    // Back in front the page scrolls on, from tiles it rasterized again.
    let result = async {
        pan(cx, window, half..PAN.len()).await?;
        let (with, after) = shown(cx, screens, window).await?;
        let scale = scale_factor(cx, window);
        Ok::<_, anyhow::Error>((with, after, scale))
    }
    .await;
    close(cx, window);
    let (with, after, scale) = match result {
        Ok(result) => result,
        Err(error) => return fail(ID, format!("{error:#}")),
    };
    let rebuilt = after.renderer.is_some_and(|renderer| {
        renderer.held > 0 && renderer.rasterized > before.renderer.map_or(0, |r| r.rasterized)
    });
    if !released {
        return fail(
            ID,
            format!("the renderer still held tiles in the background ({away})"),
        );
    }
    if !rebuilt || after.composited <= before.composited {
        return fail(
            ID,
            format!("the tiles were not rebuilt after the app came back ({after})"),
        );
    }
    let plain = match open_scrolled(cx, &DIV, false, 0..PAN.len()).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("{error:#}")),
    };
    let without = shown(cx, screens, plain).await;
    close(cx, plain);
    match without.map(|(without, _)| same_pixels(&with, &without, scroller_pixels(scale))) {
        Ok(Ok(())) => pass(
            ID,
            format!(
                "tiles released in the background ({away}) and rebuilt after it ({after}), showing what a page that stayed shows"
            ),
        ),
        Ok(Err(error)) => fail(ID, format!("after resuming the page differs: {error:#}")),
        Err(error) => fail(ID, format!("{error:#}")),
    }
}
