//! Checks shared by every host: expectations against the window's AccessKit tree (read as a
//! [`SemanticSnapshot`] while the host inspects the window), and pixel steps against a captured
//! frame.

use crate::{
    probe::{self, DEFAULT_TOLERANCE, PixelRect},
    scene::{Expect, LogicalRect, Step},
};
use anyhow::{Context as _, bail};
use gpux::{
    A11yInspection, Pixels, Point, Window, point, px,
    semantic_query::{self, Query, SemanticNode, SemanticSnapshot},
};
use image::RgbaImage;
use std::{
    collections::HashMap,
    f64::consts::{PI, TAU},
};

/// The center of a node's bounds in window pixels.
pub fn center(element: &SemanticNode) -> Point<Pixels> {
    let bounds = element.bounds;
    point(
        px(bounds.x + bounds.width / 2.),
        px(bounds.y + bounds.height / 2.),
    )
}

/// Starts inspecting a scene's window, so it builds its accessibility tree (generic containers
/// for elements with an id and no role included) for the host's queries. Hold the guard while
/// the scene is shown.
pub fn inspect(window: &mut Window) -> A11yInspection {
    window.begin_a11y_inspection()
}

std::thread_local! {
    /// The windows hosts keep inspecting, with their guards ([`keep_inspecting`]).
    static INSPECTIONS: std::cell::RefCell<Vec<(gpux::AnyWindowHandle, A11yInspection)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Keeps `window` inspected until [`stop_inspecting`], for hosts that open their own windows
/// and query them over many frames. Calling it again changes nothing.
pub fn keep_inspecting(window: &mut Window) {
    let handle = window.window_handle();
    INSPECTIONS.with(|inspections| {
        let mut inspections = inspections.borrow_mut();
        if !inspections.iter().any(|(kept, _)| *kept == handle) {
            inspections.push((handle, window.begin_a11y_inspection()));
        }
    });
}

/// Stops the inspection [`keep_inspecting`] started for `handle`.
pub fn stop_inspecting(handle: gpux::AnyWindowHandle) {
    INSPECTIONS.with(|inspections| inspections.borrow_mut().retain(|(kept, _)| *kept != handle));
}

/// The tree a window built last, as queries read it.
pub fn snapshot(window: &Window) -> anyhow::Result<SemanticSnapshot> {
    SemanticSnapshot::of(window).ok_or_else(|| {
        anyhow::anyhow!(
            "the window builds no accessibility tree: inspection has not started, or \
             accessibility is disabled"
        )
    })
}

/// The one node `query` identifies in `snapshot`.
pub fn locate(snapshot: &SemanticSnapshot, query: &Query) -> anyhow::Result<SemanticNode> {
    Ok(query.resolve(snapshot)?.clone())
}

/// Checks an expectation against the window's accessibility tree. `min_tolerance` widens bounds
/// tolerances to at least one device pixel on hosts whose scale snaps layout to fractions of a
/// logical pixel. [`Expect::Accessible`] reads the same tree; the test host also checks that the
/// platform adapter receives it.
pub fn expect(
    snapshot: &SemanticSnapshot,
    expect: &Expect,
    min_tolerance: f32,
) -> anyhow::Result<()> {
    match expect {
        Expect::Exists(query) => {
            query.resolve(snapshot)?;
        }
        Expect::Missing(query) => {
            let count = query.matches(snapshot).len();
            if count != 0 {
                bail!("{count} nodes match `{query}`, expected none");
            }
        }
        Expect::Count(query, expected) => {
            let count = query.matches(snapshot).len();
            if count != *expected {
                bail!("{count} nodes match `{query}`, expected {expected}");
            }
        }
        Expect::AtLeast(query, expected) => {
            let count = query.matches(snapshot).len();
            if count < *expected {
                bail!("{count} nodes match `{query}`, expected at least {expected}");
            }
        }
        Expect::Focused(query) => {
            let element = query.resolve(snapshot)?;
            if !semantic_query::focus_is_within(snapshot, element) {
                bail!(
                    "`{query}` does not hold focus: {}",
                    semantic_query::describe_node(element)
                );
            }
        }
        Expect::Bounds(query, (x, y, width, height), tolerance) => {
            let tolerance = tolerance.max(min_tolerance);
            let bounds = query.resolve(snapshot)?.bounds;
            let actual = [bounds.x, bounds.y, bounds.width, bounds.height];
            let expected = [*x, *y, *width, *height];
            if actual
                .iter()
                .zip(expected)
                .any(|(actual, expected)| (actual - expected).abs() > tolerance)
            {
                bail!("`{query}` has bounds {actual:?}, expected {expected:?} ±{tolerance}");
            }
        }
        Expect::Size(query, width, height, tolerance) => {
            let tolerance = tolerance.max(min_tolerance);
            let bounds = query.resolve(snapshot)?.bounds;
            if (bounds.width - width).abs() > tolerance
                || (bounds.height - height).abs() > tolerance
            {
                bail!(
                    "`{query}` is {}x{}, expected {width}x{height} ±{tolerance}",
                    bounds.width,
                    bounds.height
                );
            }
        }
        Expect::Accessible(label) => {
            if !snapshot
                .nodes
                .iter()
                .any(|node| node.name.as_deref() == Some(label.as_str()))
            {
                let names: Vec<&str> = snapshot
                    .nodes
                    .iter()
                    .filter_map(|node| node.name.as_deref())
                    .collect();
                bail!("no accessibility node is named {label:?}; names: {names:?}");
            }
        }
    }
    Ok(())
}

/// A logical point in physical pixels at `scale`.
pub fn physical(scale: f32, x: f32, y: f32) -> (u32, u32) {
    (
        (x * scale).floor().max(0.) as u32,
        (y * scale).floor().max(0.) as u32,
    )
}

/// A logical rectangle in physical pixels at `scale`.
pub fn physical_rect(scale: f32, (x, y, width, height): LogicalRect) -> PixelRect {
    let (left, top) = physical(scale, x, y);
    PixelRect::new(
        left,
        top,
        ((width * scale).round() as u32).max(1),
        ((height * scale).round() as u32).max(1),
    )
}

/// How far 16-bit (RGB565) color moves a channel: rounding to 5 bits lands red and blue up to
/// 4 levels away (green, with 6 bits, up to 2).
pub const RGB565_ROUNDING: u8 = 4;

/// Whether every pixel of `region` in `frame` is a 16-bit RGB565 color widened to 8 bits per
/// channel, as on screens that render at 16-bit depth (the GL surfaces of Android emulators).
/// Screens widen by repeating the high bits or by rounding to the nearest level; both count.
/// Pixels outside the frame are ignored.
pub fn shows_rgb565(frame: &RgbaImage, region: PixelRect) -> bool {
    fn widened(value: u8, bits: u32) -> bool {
        let max = (1 << bits) - 1;
        let value = u32::from(value);
        let narrow = (value * max + 127) / 255;
        let repeated = (narrow << (8 - bits)) | (narrow >> (2 * bits - 8));
        let rounded = (narrow * 255 + max / 2) / max;
        value == repeated || value == rounded
    }
    let right = (region.x + region.width).min(frame.width());
    let bottom = (region.y + region.height).min(frame.height());
    (region.y..bottom).all(|y| {
        (region.x..right).all(|x| {
            let pixel = frame.get_pixel(x, y);
            widened(pixel[0], 5) && widened(pixel[1], 6) && widened(pixel[2], 5)
        })
    })
}

/// Checks a pixel step against `frame`, rendered at `scale`. Captures go into `captures`.
/// Golden steps are host-specific and not handled here.
pub fn pixel_step(
    step: &Step,
    frame: &RgbaImage,
    scale: f32,
    captures: &mut HashMap<&'static str, RgbaImage>,
) -> anyhow::Result<()> {
    pixel_step_within(step, frame, scale, captures, 0)
}

/// Checks that a screenshot of a whole screen shows `color` at the logical point (`x`, `y`) of a
/// window at the screen's origin rendered at `scale`: a known pixel of the app, which system bars
/// or an empty screen cannot pass for. Where the screen shows 16-bit color around the point, the
/// probe allows its rounding ([`RGB565_ROUNDING`]).
pub fn screen_shows(
    frame: &RgbaImage,
    scale: f32,
    x: f32,
    y: f32,
    color: probe::Rgba,
) -> anyhow::Result<()> {
    let (px, py) = physical(scale, x, y);
    let around = PixelRect::new(px.saturating_sub(4), py.saturating_sub(4), 9, 9);
    let tolerance = if shows_rgb565(frame, around) {
        RGB565_ROUNDING.max(DEFAULT_TOLERANCE)
    } else {
        DEFAULT_TOLERANCE
    };
    probe::color_at(frame, px, py, color, tolerance)?;
    Ok(())
}

/// [`pixel_step`] with color comparisons allowing at least `color_tolerance` per channel: the
/// rounding of a frame shown at a lower color depth ([`RGB565_ROUNDING`]).
pub fn pixel_step_within(
    step: &Step,
    frame: &RgbaImage,
    scale: f32,
    captures: &mut HashMap<&'static str, RgbaImage>,
    color_tolerance: u8,
) -> anyhow::Result<()> {
    match step {
        Step::Probe(x, y, color, tolerance) => {
            let (x, y) = physical(scale, *x, *y);
            probe::color_at(frame, x, y, *color, (*tolerance).max(color_tolerance))?;
        }
        Step::ProbeUniform(rect, color, tolerance) => {
            let region = physical_rect(scale, *rect);
            let tolerance = (*tolerance).max(color_tolerance);
            let actual = probe::region_uniform(frame, region, tolerance)?;
            if !probe::within(actual, *color, tolerance) {
                bail!("region {region:?} is uniformly {actual:?}, expected {color:?}");
            }
        }
        Step::ProbeDiffer(a, b, tolerance) => {
            probe::regions_differ(
                frame,
                physical_rect(scale, *a),
                physical_rect(scale, *b),
                *tolerance,
            )?;
        }
        Step::Capture(name) => {
            captures.insert(name, frame.clone());
        }
        Step::EvenRing(..) => bail!("a ring step reads its element's bounds: see `even_ring_step`"),
        Step::ChangedSince(name, rect) => {
            let Some(before) = captures.get(name) else {
                bail!("no capture named {name:?}");
            };
            probe::changed(
                before,
                frame,
                physical_rect(scale, *rect),
                DEFAULT_TOLERANCE,
            )?;
        }
        other => bail!("{other:?} is not a pixel step"),
    }
    Ok(())
}

/// A rectangle in device pixels: `(x, y, width, height)`.
pub type DeviceRect = (f32, f32, f32, f32);

/// The angular bins [`ring_evenness`] sums a ring's energy into.
pub const RING_BINS: usize = 48;

/// How far from a ring border's centre line [`ring_evenness`] collects pixels, in device pixels
/// at `scale`: half the 1 px border plus two pixels of anti-aliasing on either side.
pub fn ring_reach(scale: f32) -> f32 {
    scale / 2. + 2.
}

/// The linear-light luminance (Rec. 709 weights) of an encoded sRGB pixel.
fn luminance(pixel: probe::Rgba) -> f64 {
    fn linear(value: u8) -> f64 {
        let value = f64::from(value) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(pixel[0]) + 0.7152 * linear(pixel[1]) + 0.0722 * linear(pixel[2])
}

/// How unevenly the 1 px border of a `rounded_full` element is drawn around its circumference
/// (RayX's beading metric). `bounds` is the element's border box in device pixels of `frame`,
/// rendered at `scale`.
///
/// Every pixel within [`ring_reach`] of the border's centre line (a circle half a logical pixel
/// inside the element's edge) adds the linear-light luminance it shows above the backdrop, the
/// pixel at the ring's centre, to one of [`RING_BINS`] angular bins. The result is the bins'
/// coefficient of variation (standard deviation over mean): an evenly anti-aliased ring puts the
/// same energy in every bin whatever its sub-pixel phase, while a beaded one (alternately bright
/// and dark runs along the curve) spreads them. Fails when the ring adds no energy over the
/// backdrop, since an undrawn ring is never even, or reaches outside the frame.
pub fn ring_evenness(frame: &RgbaImage, bounds: DeviceRect, scale: f32) -> anyhow::Result<f64> {
    let (x, y, width, height) = bounds;
    let center = (f64::from(x + width / 2.), f64::from(y + height / 2.));
    let radius = f64::from(width / 2. - scale / 2.);
    let reach = f64::from(ring_reach(scale));
    let left = (center.0 - radius - reach).floor();
    let top = (center.1 - radius - reach).floor();
    let right = (center.0 + radius + reach).ceil();
    let bottom = (center.1 + radius + reach).ceil();
    if left < 0.
        || top < 0.
        || right >= f64::from(frame.width())
        || bottom >= f64::from(frame.height())
    {
        bail!(
            "the ring of device bounds {bounds:?} reaches outside the {}x{} frame",
            frame.width(),
            frame.height()
        );
    }
    let backdrop = luminance(
        frame
            .get_pixel(center.0.floor() as u32, center.1.floor() as u32)
            .0,
    );
    let mut energy = [0f64; RING_BINS];
    for row in top as u32..=bottom as u32 {
        for column in left as u32..=right as u32 {
            let dx = f64::from(column) + 0.5 - center.0;
            let dy = f64::from(row) + 0.5 - center.1;
            if (dx.hypot(dy) - radius).abs() > reach {
                continue;
            }
            let bin = ((dy.atan2(dx) + PI) / TAU * RING_BINS as f64) as usize % RING_BINS;
            energy[bin] += (luminance(frame.get_pixel(column, row).0) - backdrop).max(0.);
        }
    }
    let mean = energy.iter().sum::<f64>() / RING_BINS as f64;
    if mean <= 0. {
        bail!("the ring adds no light over its backdrop: an undrawn ring is never even");
    }
    let variance = energy
        .iter()
        .map(|energy| (energy - mean).powi(2))
        .sum::<f64>()
        / RING_BINS as f64;
    Ok(variance.sqrt() / mean)
}

/// The limit of a [`Step::EvenRing`] at `scale`: `at_1x` below scale 2, `at_2x` from scale 2
/// up, where the border spans more device pixels and anti-aliases more evenly.
pub fn ring_limit(scale: f32, at_1x: f32, at_2x: f32) -> f32 {
    if scale < 2. { at_1x } else { at_2x }
}

/// The ring of the element with device-pixel `bounds` is even: its [`ring_evenness`] is at most
/// `limit`. Returns the measured coefficient.
pub fn even_ring(
    frame: &RgbaImage,
    bounds: DeviceRect,
    scale: f32,
    limit: f32,
) -> anyhow::Result<f64> {
    let coefficient = ring_evenness(frame, bounds, scale)?;
    if coefficient > f64::from(limit) {
        bail!(
            "the ring beads: its per-angle energy varies by {coefficient:.3} (coefficient of \
             variation), above the limit {limit} at scale {scale}"
        );
    }
    Ok(coefficient)
}

/// Checks a [`Step::EvenRing`] against `frame`, rendered at `scale`, locating its element in
/// `snapshot` as [`Expect::Bounds`] does.
pub fn even_ring_step(
    snapshot: &SemanticSnapshot,
    step: &Step,
    frame: &RgbaImage,
    scale: f32,
) -> anyhow::Result<()> {
    let Step::EvenRing(query, at_1x, at_2x) = step else {
        bail!("{step:?} is not a ring step");
    };
    let bounds = query.resolve(snapshot)?.bounds;
    let device = (
        bounds.x * scale,
        bounds.y * scale,
        bounds.width * scale,
        bounds.height * scale,
    );
    even_ring(frame, device, scale, ring_limit(scale, *at_1x, *at_2x))
        .with_context(|| format!("`{query}`"))?;
    Ok(())
}
