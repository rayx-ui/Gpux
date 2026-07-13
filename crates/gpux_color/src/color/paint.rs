use crate::{Color, LinearSrgb};

/// Fixed renderer gamut policy used by [`Color::paint_color`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaintGamutPolicy {
    /// Clamp unbounded linear-light sRGB channels before premultiplication.
    ClipLinearSrgb,
}

/// The sole renderer normalization policy.
pub const DEFAULT_PAINT_GAMUT_POLICY: PaintGamutPolicy = PaintGamutPolicy::ClipLinearSrgb;

/// Compact renderer paint: premultiplied linear-light sRGB.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct PaintColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl PaintColor {
    pub const BYTE_SIZE: usize = 16;
    pub const GAMUT_POLICY: PaintGamutPolicy = DEFAULT_PAINT_GAMUT_POLICY;

    pub(crate) fn from_color(color: &Color) -> Self {
        let LinearSrgb { r, g, b } = color.to_linear_srgb();
        let alpha = unit(color.alpha());
        Self {
            r: (unit(r) * alpha) as f32,
            g: (unit(g) * alpha) as f32,
            b: (unit(b) * alpha) as f32,
            a: alpha as f32,
        }
    }
}

fn unit(value: f64) -> f64 {
    let value = value.clamp(0.0, 1.0);
    if value == 0.0 { 0.0 } else { value }
}
