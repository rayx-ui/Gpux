//! Relative pixel probes: assertions about colors at points and regions of a captured image,
//! robust across rasterizers. Expected colors are written in encoded sRGB, the paint contract.

use image::RgbaImage;
use std::fmt;

/// An RGBA color in encoded sRGB, 0–255 per channel.
pub type Rgba = [u8; 4];

/// The default per-channel tolerance of probes and golden comparisons.
pub const DEFAULT_TOLERANCE: u8 = 3;

/// A rectangle of pixels in an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    /// The left edge.
    pub x: u32,
    /// The top edge.
    pub y: u32,
    /// The width.
    pub width: u32,
    /// The height.
    pub height: u32,
}

impl PixelRect {
    /// A rectangle at (`x`, `y`) of `width` × `height` pixels.
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn pixels(self) -> impl Iterator<Item = (u32, u32)> {
        (self.y..self.y + self.height)
            .flat_map(move |y| (self.x..self.x + self.width).map(move |x| (x, y)))
    }

    fn fits(self, image: &RgbaImage) -> Result<(), ProbeError> {
        if self.width == 0
            || self.height == 0
            || self.x + self.width > image.width()
            || self.y + self.height > image.height()
        {
            return Err(ProbeError(format!(
                "region {self:?} is empty or outside the {}x{} image",
                image.width(),
                image.height()
            )));
        }
        Ok(())
    }
}

/// A probe that did not hold, with what was observed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeError(pub String);

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProbeError {}

/// Whether every channel of `actual` lies within `tolerance` of `expected`.
pub fn within(actual: Rgba, expected: Rgba, tolerance: u8) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(actual, expected)| actual.abs_diff(expected) <= tolerance)
}

fn pixel(image: &RgbaImage, x: u32, y: u32) -> Result<Rgba, ProbeError> {
    if x >= image.width() || y >= image.height() {
        return Err(ProbeError(format!(
            "({x}, {y}) is outside the {}x{} image",
            image.width(),
            image.height()
        )));
    }
    Ok(image.get_pixel(x, y).0)
}

/// The pixel at (`x`, `y`) is `expected` within `tolerance` per channel.
pub fn color_at(
    image: &RgbaImage,
    x: u32,
    y: u32,
    expected: Rgba,
    tolerance: u8,
) -> Result<(), ProbeError> {
    let actual = pixel(image, x, y)?;
    if within(actual, expected, tolerance) {
        Ok(())
    } else {
        Err(ProbeError(format!(
            "pixel ({x}, {y}) is {actual:?}, expected {expected:?} ±{tolerance}"
        )))
    }
}

/// Every pixel of `region` is within `tolerance` of its first pixel; returns that color.
pub fn region_uniform(
    image: &RgbaImage,
    region: PixelRect,
    tolerance: u8,
) -> Result<Rgba, ProbeError> {
    region.fits(image)?;
    let first = image.get_pixel(region.x, region.y).0;
    match region
        .pixels()
        .find(|&(x, y)| !within(image.get_pixel(x, y).0, first, tolerance))
    {
        None => Ok(first),
        Some((x, y)) => Err(ProbeError(format!(
            "region {region:?} is not uniform: ({x}, {y}) is {:?}, ({}, {}) is {first:?}",
            image.get_pixel(x, y).0,
            region.x,
            region.y
        ))),
    }
}

/// The largest per-channel difference between two equally sized regions.
fn region_difference(
    image: &RgbaImage,
    a: PixelRect,
    other: &RgbaImage,
    b: PixelRect,
) -> Result<u8, ProbeError> {
    a.fits(image)?;
    b.fits(other)?;
    if (a.width, a.height) != (b.width, b.height) {
        return Err(ProbeError(format!(
            "regions {a:?} and {b:?} have different sizes"
        )));
    }
    Ok(a.pixels()
        .zip(b.pixels())
        .flat_map(|((ax, ay), (bx, by))| {
            let left = image.get_pixel(ax, ay).0;
            let right = other.get_pixel(bx, by).0;
            (0..4).map(move |channel| left[channel].abs_diff(right[channel]))
        })
        .max()
        .unwrap_or(0))
}

/// Regions `a` and `b` of `image` hold the same pixels within `tolerance`.
pub fn regions_equal(
    image: &RgbaImage,
    a: PixelRect,
    b: PixelRect,
    tolerance: u8,
) -> Result<(), ProbeError> {
    let difference = region_difference(image, a, image, b)?;
    if difference <= tolerance {
        Ok(())
    } else {
        Err(ProbeError(format!(
            "regions {a:?} and {b:?} differ by up to {difference} per channel, expected at most {tolerance}"
        )))
    }
}

/// Regions `a` and `b` of `image` differ by more than `tolerance` somewhere.
pub fn regions_differ(
    image: &RgbaImage,
    a: PixelRect,
    b: PixelRect,
    tolerance: u8,
) -> Result<(), ProbeError> {
    let difference = region_difference(image, a, image, b)?;
    if difference > tolerance {
        Ok(())
    } else {
        Err(ProbeError(format!(
            "regions {a:?} and {b:?} differ by at most {difference} per channel, expected more than {tolerance}"
        )))
    }
}

/// `region` changed between `before` and `after` by more than `tolerance` somewhere, as an
/// action is expected to change it.
pub fn changed(
    before: &RgbaImage,
    after: &RgbaImage,
    region: PixelRect,
    tolerance: u8,
) -> Result<(), ProbeError> {
    let difference = region_difference(before, region, after, region)?;
    if difference > tolerance {
        Ok(())
    } else {
        Err(ProbeError(format!(
            "region {region:?} did not change (largest difference {difference}, tolerance {tolerance})"
        )))
    }
}

/// `region` is the same in `before` and `after` within `tolerance`.
pub fn unchanged(
    before: &RgbaImage,
    after: &RgbaImage,
    region: PixelRect,
    tolerance: u8,
) -> Result<(), ProbeError> {
    let difference = region_difference(before, region, after, region)?;
    if difference <= tolerance {
        Ok(())
    } else {
        Err(ProbeError(format!(
            "region {region:?} changed by up to {difference} per channel, expected at most {tolerance}"
        )))
    }
}
