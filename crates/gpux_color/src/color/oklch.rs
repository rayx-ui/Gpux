use crate::color::OklabColor;

const POWERLESS_CHROMA_EPSILON: f64 = 1e-7;

/// OKLCH color coordinates plus alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OklchColor {
    /// Perceptual lightness.
    pub lightness: f64,
    /// Perceptual chroma.
    pub chroma: f64,
    /// Hue in degrees.
    pub hue_degrees: f64,
    /// Alpha in `0.0..=1.0`.
    pub alpha: f64,
}

impl OklchColor {
    pub const fn new(lightness: f64, chroma: f64, hue_degrees: f64) -> Self {
        Self {
            lightness,
            chroma,
            hue_degrees,
            alpha: 1.0,
        }
    }
}

pub(crate) fn oklab_to_oklch(oklab: OklabColor, fallback_hue: f64) -> OklchColor {
    let chroma = (oklab.a * oklab.a + oklab.b * oklab.b).sqrt();
    let hue = if chroma <= POWERLESS_CHROMA_EPSILON {
        fallback_hue
    } else {
        oklab.b.atan2(oklab.a).to_degrees().rem_euclid(360.0)
    };

    OklchColor {
        lightness: oklab.l,
        chroma,
        hue_degrees: hue,
        alpha: oklab.alpha,
    }
}

pub(crate) fn oklch_to_oklab(
    lightness: f64,
    chroma: f64,
    hue_degrees: f64,
    alpha: f64,
) -> OklabColor {
    let hue = hue_degrees.to_radians();
    OklabColor {
        l: lightness,
        a: chroma * hue.cos(),
        b: chroma * hue.sin(),
        alpha,
    }
}
