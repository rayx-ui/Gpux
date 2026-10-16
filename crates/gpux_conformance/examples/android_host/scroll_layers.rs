//! Scroll layers on Android: a scrolled `div` and a `list` draw from tiles, as many frames as a
//! swipe and its fling make, and show on the screen what the same page shows without layers; a
//! page that went to the background and came back rasterizes its tiles again.
//!
//! The pages are the browser's cards ([`gpux_conformance::scroll_layers`]). The screen is read
//! through `adb screencap` ([`Screens`]), as every pixel step of this host is.

use crate::android::{Screens, request};
use crate::checks::{
    ACTION_TIMEOUT, SWIPE_FRAMES, capture_state, close, fail, find, frame_time, open, pass,
    scale_factor, skip, sleep, surface_lost,
};
use gpux::semantic_query::Query;
use gpux::{AnyWindowHandle, AsyncApp};
use gpux_conformance::{
    SceneSpec,
    report::ResultLine,
    scroll_layers::{
        self, Counters, DIV, LIST, PAN, SCROLLER, same_pixels, scroller_pixels, set_layers,
        timed_scroll,
    },
};
use image::RgbaImage;
use std::time::{Duration, Instant};

/// How many frames of a swipe through the system must come from tiles: the finger moves a
/// few times between a slow renderer's frames.
const SWIPE_COMPOSITED_OS: u64 = 2;

/// How long a swipe through the system may take to scroll the page.
const SWIPE_PATIENCE: Duration = Duration::from_secs(120);

/// How long a pan step waits for the frame it asks for at least.
const FRAME_LIMIT_MIN: Duration = Duration::from_secs(2);

/// A frame this slow or slower makes the swipe through the system pointless: `adb` sends the
/// moves its duration has time for, and the app handles them between frames seconds apart.
const SLOW_FRAME: Duration = Duration::from_millis(1500);

/// Milliseconds a pan step waits for its frame: three times the slowest frame time measured so
/// far, so that a software renderer on a loaded runner is waited for rather than raced.
static FRAME_LIMIT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How long a pan step waits for the frame it asks for.
fn frame_limit() -> Duration {
    FRAME_LIMIT_MIN.max(Duration::from_millis(
        FRAME_LIMIT_MS.load(std::sync::atomic::Ordering::Relaxed),
    ))
}

/// Notes a measured frame time for [`frame_limit`] and says whether frames are [`SLOW_FRAME`].
fn note_frame_time(frame: Duration) -> bool {
    FRAME_LIMIT_MS.fetch_max(
        (frame * 3).as_millis() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    frame >= SLOW_FRAME
}

/// How many of a swipe's frames must come from tiles: the first of a pan and the frames the
/// layer is promoted in need not.
const SWIPE_COMPOSITED: u64 = 4;

/// The page's `div` offset, from its status line (`div <n>` or `div <n> list <m>`).
fn div_offset(cx: &mut AsyncApp, window: AnyWindowHandle) -> Option<f32> {
    let status = find(cx, window, &Query::role("status"))?;
    status
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
    status.value?.rsplit_once(" list ")?.1.parse().ok()
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
    scroll_layers::next_frame(cx, window, frame_limit()).await;
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
        scroll_layers::next_frame(cx, window, frame_limit()).await;
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
    scroll_layers::next_frame(cx, window, frame_limit()).await;
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
    scroll_layers::next_frame(cx, window, frame_limit()).await;
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
        // A software renderer on a large screen draws a frame in seconds, and the swipe lasts
        // eight of them.
        let begun = Instant::now();
        let mut moved = false;
        while begun.elapsed() < SWIPE_PATIENCE {
            if scrolled(cx, window, list) > before {
                moved = true;
                break;
            }
            sleep(cx, Duration::from_millis(100)).await;
        }
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

/// Opens the page once to read the backend it renders through.
async fn probe_backend(
    cx: &mut AsyncApp,
    scene: &'static SceneSpec,
) -> Result<(String, bool), String> {
    let window = open(cx, scene)
        .await
        .map_err(|error| format!("the page never drew: {error:#}"))?;
    let backend = backend_of(cx, window);
    let slow = note_frame_time(frame_time(cx, window).await);
    close(cx, window);
    backend.map(|backend| (backend, slow))
}

async fn page(
    cx: &mut AsyncApp,
    screens: &Screens,
    id: &str,
    scene: &'static SceneSpec,
    list: bool,
) -> ResultLine {
    let (backend, slow) = match probe_backend(cx, scene).await {
        Ok(probed) => probed,
        Err(error) => return fail(id, error),
    };
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
    if touch.composited < SWIPE_COMPOSITED {
        return fail(
            id,
            format!(
                "touch pans scrolled the page by {distance} but few frames came from tiles ({touch})"
            ),
        );
    }
    // Where a frame takes seconds the swipe is a tap (as in `android-touch-fling`), and the
    // app does not recover from a swipe that long: the touch pans above are the check there.
    let swipe = if slow {
        None
    } else {
        match swiped(cx, scene, list).await {
            Ok((_, counters)) if counters.composited < SWIPE_COMPOSITED_OS => {
                return fail(
                    id,
                    format!("a swipe came from tiles in few frames ({counters})"),
                );
            }
            Ok(swipe) => Some(swipe),
            Err(reason) => return fail(id, reason),
        }
    };
    let swipe = swipe.map_or_else(
        || {
            "a swipe through the system was skipped: this renderer's frames take seconds"
                .to_string()
        },
        |swipe| {
            format!(
                "a swipe through the system scrolled by {} with {}",
                swipe.0, swipe.1
            )
        },
    );
    match same_with_and_without(cx, screens, scene).await {
        Ok(wheel) if wheel.composited >= SWIPE_COMPOSITED => pass(
            id,
            format!(
                "on {backend}: touch pans scrolled by {distance} with {touch}; {swipe}; the wheel's pans show the same pixels with and without layers ({wheel})"
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

/// What the renderer holds and how often it released its tiles, as the counters say.
fn tiles(counters: &Counters) -> (u64, u64, u64) {
    counters
        .renderer
        .map_or((0, 0, 0), |r| (r.held, r.rasterized, r.released))
}

/// Waits until the renderer released its tiles once more than `released` times, and returns the
/// counters and whether the window still had its surface then. Polls every 50 ms for
/// [`ACTION_TIMEOUT`].
async fn released_after(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    released: u64,
) -> Option<(Counters, bool)> {
    let start = Instant::now();
    while start.elapsed() < ACTION_TIMEOUT {
        let counters = scroll_layers::counters(cx, window).ok()?;
        if tiles(&counters).2 > released {
            let surface = matches!(capture_state(cx, window), Some(Ok(_)));
            return Some((counters, surface));
        }
        sleep(cx, Duration::from_millis(50)).await;
    }
    None
}

/// Waits until the window's surface is gone.
async fn surface_gone(cx: &mut AsyncApp, window: AnyWindowHandle) -> bool {
    let start = Instant::now();
    while start.elapsed() < ACTION_TIMEOUT {
        if let Some(Err(error)) = capture_state(cx, window)
            && surface_lost(&error)
        {
            return true;
        }
        sleep(cx, Duration::from_millis(50)).await;
    }
    false
}

/// A page whose app is trimmed by the system, goes to the background (the activity pauses, and the
/// system takes its surface) and comes back gives its tiles back each time, rasterizes them again
/// and shows what a page that never lost them shows.
///
/// The releases are told apart where a device allows: `onTrimMemory` through `am send-trim-memory`
/// (the host's activity is `GpuxNativeActivity`), and the pause the home key brings (the surface
/// is still there while the system stops the activity). The loss of the surface comes with the
/// system's own trim as the app is hidden, so what the check shows of it is an empty cache once
/// the surface is gone; `term_window`'s own release is by inspection.
pub async fn resume(cx: &mut AsyncApp, screens: &Screens) -> ResultLine {
    const ID: &str = "scroll-layers-android-resume";
    let third = PAN.len() / 3;
    let window = match open_scrolled(cx, &DIV, true, 0..third).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("the page never drew: {error:#}")),
    };
    let backend = match backend_of(cx, window) {
        Ok(backend) => backend,
        Err(error) => {
            close(cx, window);
            return fail(ID, error);
        }
    };
    let result = resume_phases(cx, window, third).await;
    let phases = match result {
        Ok(phases) => phases,
        Err(error) => {
            close(cx, window);
            return fail(ID, format!("{error:#}"));
        }
    };
    let scale = scale_factor(cx, window);
    let shown = shown_settled(cx, screens, window, scale).await;
    close(cx, window);
    let with = match shown {
        Ok(with) => with,
        Err(error) => return fail(ID, format!("{error:#}")),
    };
    let plain = match open_scrolled(cx, &DIV, false, 0..PAN.len()).await {
        Ok(window) => window,
        Err(error) => return fail(ID, format!("{error:#}")),
    };
    let without = shown_plain(cx, screens, plain).await;
    close(cx, plain);
    match without.map(|without| same_pixels(&with, &without, scroller_pixels(scale))) {
        Ok(Ok(())) => pass(
            ID,
            format!("on {backend}: {phases}; showing what a page that kept its tiles shows"),
        ),
        Ok(Err(error)) => fail(ID, format!("after resuming the page differs: {error:#}")),
        Err(error) => fail(ID, format!("{error:#}")),
    }
}

/// [`shown`] for a window that just came back to the front: the system fades and scales it in
/// over the launcher, so the screen is captured again until two captures in a row are the same
/// and the page shows (the left margin of the scroller's first row is no longer white).
async fn shown_settled(
    cx: &mut AsyncApp,
    screens: &Screens,
    window: AnyWindowHandle,
    scale: f32,
) -> anyhow::Result<RgbaImage> {
    let (x, y, ..) = SCROLLER;
    let (px, py) = (((x + 2.) * scale) as u32, ((y + 5.) * scale) as u32);
    let begun = Instant::now();
    let mut previous: Option<RgbaImage> = None;
    loop {
        let (frame, _) = shown(cx, screens, window).await?;
        let showing = frame
            .get_pixel_checked(px, py)
            .is_some_and(|pixel| pixel.0[..3].iter().any(|channel| *channel < 250));
        if showing && previous.as_ref() == Some(&frame) {
            return Ok(frame);
        }
        anyhow::ensure!(
            begun.elapsed() < ACTION_TIMEOUT,
            "the screen never settled on the page after the app came back"
        );
        previous = Some(frame);
        sleep(cx, Duration::from_millis(500)).await;
    }
}

async fn shown_plain(
    cx: &mut AsyncApp,
    screens: &Screens,
    window: AnyWindowHandle,
) -> anyhow::Result<RgbaImage> {
    Ok(shown(cx, screens, window).await?.0)
}

/// The three releases and the rebuilds between them; says what each did.
async fn resume_phases(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    third: usize,
) -> anyhow::Result<String> {
    let start = scroll_layers::counters(cx, window)?;
    anyhow::ensure!(
        start.renderer.is_some(),
        "this build reports no renderer counters"
    );
    anyhow::ensure!(
        tiles(&start).0 > 0 && start.composited > 0,
        "the page held no tiles before the trim ({start})"
    );

    // The system trims the app's memory.
    request("trim-memory RUNNING_LOW");
    let (trimmed, _) = released_after(cx, window, tiles(&start).2)
        .await
        .ok_or_else(|| anyhow::anyhow!("onTrimMemory never released the tiles ({start})"))?;
    anyhow::ensure!(
        tiles(&trimmed).0 == 0,
        "the renderer kept tiles after onTrimMemory ({trimmed})"
    );
    pan(cx, window, third..2 * third).await?;
    let rebuilt = scroll_layers::counters(cx, window)?;
    anyhow::ensure!(
        tiles(&rebuilt).0 > 0 && tiles(&rebuilt).1 > tiles(&start).1,
        "the tiles were not rasterized again after onTrimMemory ({rebuilt})"
    );

    // The app goes to the background: the activity pauses first, and the system takes the surface
    // once it has stopped it.
    request("background");
    let (paused, surface_present) = released_after(cx, window, tiles(&rebuilt).2)
        .await
        .ok_or_else(|| anyhow::anyhow!("the pause never released the tiles ({rebuilt})"))?;
    anyhow::ensure!(
        surface_present,
        "the tiles went only with the surface, not with the pause ({paused})"
    );
    anyhow::ensure!(
        tiles(&paused).0 == 0,
        "the renderer kept tiles after the pause ({paused})"
    );
    anyhow::ensure!(
        surface_gone(cx, window).await,
        "the surface was never taken away in the background"
    );
    let away = scroll_layers::counters(cx, window)?;
    // A device sends `onTrimMemory` as it hides and stops the app, so the surface loss's own
    // release (`term_window`) cannot be told from them here; the renderer holds nothing once the
    // surface is gone, whichever released it.
    anyhow::ensure!(
        tiles(&away).0 == 0,
        "the renderer held tiles without a surface ({away})"
    );
    request("foreground");
    let begun = Instant::now();
    let mut asked = Instant::now();
    loop {
        if let Some(Ok(_)) = capture_state(cx, window) {
            break;
        }
        anyhow::ensure!(
            begun.elapsed() < ACTION_TIMEOUT,
            "the surface never came back after the app returned"
        );
        if asked.elapsed() >= crate::android::REQUEST_REPEAT {
            request("foreground");
            asked = Instant::now();
        }
        sleep(cx, Duration::from_millis(50)).await;
    }
    // Back in front the page scrolls on, from tiles it rasterized again.
    pan(cx, window, 2 * third..PAN.len()).await?;
    let after = scroll_layers::counters(cx, window)?;
    anyhow::ensure!(
        tiles(&after).0 > 0
            && tiles(&after).1 > tiles(&away).1
            && after.composited > start.composited,
        "the tiles were not rasterized again after the app came back ({after})"
    );
    Ok(format!(
        "tiles released by onTrimMemory ({} released, none held), rebuilt ({} rasterized), released by the pause with the surface still there, released with it, and rebuilt after the app came back ({after})",
        tiles(&trimmed).2,
        tiles(&rebuilt).1,
    ))
}

/// How many times the timed scrolls go down and up over the page: 32 frames each.
const TIMED_ROUNDS: usize = 4;

/// What scrolling costs per frame with scroll layers on and off, for the page of `scene`: the
/// frame work (drawing and presenting a frame) at its median, 95th percentile and slowest, and
/// the frames that came from tiles. The numbers are of the device the host runs on; an emulator
/// draws in software, so what they say of a phone they do not. The check holds the counters:
/// frames come from tiles with layers on and none does with them off.
pub async fn timing(cx: &mut AsyncApp) -> ResultLine {
    const ID: &str = "scroll-layers-android-timing";
    // Where a frame takes seconds (a software renderer on a loaded runner) the 512 frames of the
    // four timed scrolls take the better part of an hour and their times say nothing.
    match probe_backend(cx, &DIV).await {
        Ok((_, true)) => {
            return skip(
                ID,
                "frames take seconds on this renderer: timing them says nothing and takes an hour",
            );
        }
        Ok(_) => {}
        Err(error) => return fail(ID, error),
    }
    let mut report = Vec::new();
    for (name, scene) in [("div", &DIV), ("list", &LIST)] {
        for layers in [true, false] {
            let window = match open(cx, scene).await {
                Ok(window) => window,
                Err(error) => return fail(ID, format!("the page never drew: {error:#}")),
            };
            let backend = match backend_of(cx, window) {
                Ok(backend) => backend,
                Err(error) => return fail(ID, error),
            };
            if let Err(error) = set_layers(cx, window, layers) {
                return fail(ID, format!("{error:#}"));
            }
            scroll_layers::next_frame(cx, window, frame_limit()).await;
            let measured = timed_scroll(cx, window, TIMED_ROUNDS, frame_limit()).await;
            close(cx, window);
            let (work, counters) = match measured {
                Ok(measured) => measured,
                Err(error) => return fail(ID, format!("{error:#}")),
            };
            if layers && counters.composited < SWIPE_COMPOSITED {
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
                "gpux-timing: scroll layers {name} on {backend} with layers {}: {work}, {} of {} frames from tiles",
                if layers { "on" } else { "off" },
                counters.composited,
                counters.frames,
            ));
        }
    }
    pass(ID, report.join("; "))
}

/// The backend the window renders through: `vulkan` or `gl`. A host asked for the emulator's
/// software Vulkan (`debug.gpux.software_vulkan`) must render through it.
fn backend_of(cx: &mut AsyncApp, window: AnyWindowHandle) -> Result<String, String> {
    let backend = window
        .update(cx, |_, window, _| window.gpu_specs())
        .ok()
        .flatten()
        .map(|specs| specs.backend.to_ascii_lowercase())
        .unwrap_or_default();
    let asked = crate::android::property("debug.gpux.software_vulkan").as_deref() == Some("1");
    if asked && !backend.contains("vulkan") {
        return Err(format!(
            "the host asked for Vulkan but the window renders through {backend:?}"
        ));
    }
    Ok(backend)
}
