/// HSV color coordinates using hue degrees and normalized channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HsvColor {
    /// Hue in degrees.
    pub hue_degrees: f64,
    /// Saturation in `0.0..=1.0`.
    pub saturation: f64,
    /// Value in `0.0..=1.0`.
    pub value: f64,
    /// Alpha in `0.0..=1.0`.
    pub alpha: f64,
}

impl HsvColor {
    pub const fn new(hue_degrees: f64, saturation: f64, value: f64) -> Self {
        Self {
            hue_degrees,
            saturation,
            value,
            alpha: 1.0,
        }
    }
}

pub(crate) fn rgb_to_hsv(r: f64, g: f64, b: f64, alpha: f64, fallback_hue: f64) -> HsvColor {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
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

    let saturation = if max.abs() <= 1.0e-12 || powerless {
        0.0
    } else {
        delta / max
    };

    HsvColor {
        hue_degrees: hue.rem_euclid(360.0),
        saturation,
        value: max,
        alpha,
    }
}

pub(crate) fn hsv_to_rgb(hue_degrees: f64, saturation: f64, value: f64) -> [f64; 3] {
    let hue = hue_degrees.rem_euclid(360.0);
    let chroma = value * saturation;
    let x = chroma * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = value - chroma;

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
