/// HSL color coordinates using hue degrees and normalized channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HslColor {
    /// Hue in degrees.
    pub hue_degrees: f64,
    /// Saturation in `0.0..=1.0`.
    pub saturation: f64,
    /// Lightness in `0.0..=1.0`.
    pub lightness: f64,
    /// Alpha in `0.0..=1.0`.
    pub alpha: f64,
}

impl HslColor {
    pub const fn new(hue_degrees: f64, saturation: f64, lightness: f64) -> Self {
        Self {
            hue_degrees,
            saturation,
            lightness,
            alpha: 1.0,
        }
    }
}

pub(crate) fn rgb_to_hsl(r: f64, g: f64, b: f64, alpha: f64, fallback_hue: f64) -> HslColor {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let lightness = (max + min) / 2.0;
    let powerless = delta.abs() <= 1.0e-12;

    let hue = if powerless {
        fallback_hue
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };

    let denominator = 1.0 - (2.0 * lightness - 1.0).abs();
    let saturation = if powerless || denominator.abs() <= 1.0e-12 {
        0.0
    } else {
        delta / denominator
    };

    HslColor {
        hue_degrees: hue.rem_euclid(360.0),
        saturation,
        lightness,
        alpha,
    }
}

pub(crate) fn hsl_to_rgb(hue_degrees: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    let hue = hue_degrees.rem_euclid(360.0);
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = lightness - chroma / 2.0;

    let [r1, g1, b1] = match hue {
        h if h < 60.0 => [chroma, x, 0.0],
        h if h < 120.0 => [x, chroma, 0.0],
        h if h < 180.0 => [0.0, chroma, x],
        h if h < 240.0 => [0.0, x, chroma],
        h if h < 300.0 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    };

    [r1 + m, g1 + m, b1 + m]
}
