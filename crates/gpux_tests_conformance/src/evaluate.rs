//! Checks shared by every host: element-tree expectations against a devtools snapshot, and
//! pixel steps against a captured frame.

use crate::{
    probe::{self, DEFAULT_TOLERANCE, PixelRect},
    scene::{Expect, LogicalRect, Step},
};
use anyhow::bail;
use gpux::{
    DevtoolsElementSnapshot, DevtoolsFrameSnapshot, Pixels, Point, point, px,
    semantic_query::{self, Query},
};
use image::RgbaImage;
use std::collections::HashMap;

/// The center of an element's window-local bounds.
pub fn center(element: &DevtoolsElementSnapshot) -> Point<Pixels> {
    let bounds = element.bounds;
    point(
        px(bounds.x + bounds.width / 2.),
        px(bounds.y + bounds.height / 2.),
    )
}

/// The one element `query` identifies in `snapshot`.
pub fn locate(
    snapshot: &DevtoolsFrameSnapshot,
    query: &Query,
) -> anyhow::Result<DevtoolsElementSnapshot> {
    Ok(query.resolve(snapshot)?.clone())
}

/// Checks an element-tree expectation. `min_tolerance` widens bounds tolerances to at least
/// one device pixel on hosts whose scale snaps layout to fractions of a logical pixel.
/// [`Expect::Accessible`] needs the platform's accessibility tree, so hosts check it themselves.
pub fn expect(
    snapshot: &DevtoolsFrameSnapshot,
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
                bail!("{count} elements match `{query}`, expected none");
            }
        }
        Expect::Count(query, expected) => {
            let count = query.matches(snapshot).len();
            if count != *expected {
                bail!("{count} elements match `{query}`, expected {expected}");
            }
        }
        Expect::AtLeast(query, expected) => {
            let count = query.matches(snapshot).len();
            if count < *expected {
                bail!("{count} elements match `{query}`, expected at least {expected}");
            }
        }
        Expect::Focused(query) => {
            let element = query.resolve(snapshot)?;
            if !semantic_query::focus_is_within(snapshot, element) {
                bail!(
                    "`{query}` does not hold focus: {}",
                    semantic_query::describe_element(element)
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
        Expect::Accessible(_) => {
            bail!("accessibility expectations are checked by the host's own accessibility tree")
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
