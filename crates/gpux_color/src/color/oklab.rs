use crate::color::LinearSrgb;

/// OKLab color coordinates plus alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OklabColor {
    /// Perceptual lightness.
    pub l: f64,
    /// Green-red opponent channel.
    pub a: f64,
    /// Blue-yellow opponent channel.
    pub b: f64,
    /// Alpha in `0.0..=1.0`.
    pub alpha: f64,
}

impl OklabColor {
    pub const fn new(l: f64, a: f64, b: f64) -> Self {
        Self {
            l,
            a,
            b,
            alpha: 1.0,
        }
    }
}

pub(crate) fn linear_srgb_to_oklab(linear: LinearSrgb, alpha: f64) -> OklabColor {
    let l = 0.412_221_470_8 * linear.r + 0.536_332_536_3 * linear.g + 0.051_445_992_9 * linear.b;
    let m = 0.211_903_498_2 * linear.r + 0.680_699_545_1 * linear.g + 0.107_396_956_6 * linear.b;
    let s = 0.088_302_461_9 * linear.r + 0.281_718_837_6 * linear.g + 0.629_978_700_5 * linear.b;

    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();

    OklabColor {
        l: 0.210_454_255_3 * l_ + 0.793_617_785 * m_ - 0.004_072_046_8 * s_,
        a: 1.977_998_495_1 * l_ - 2.428_592_205 * m_ + 0.450_593_709_9 * s_,
        b: 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766 * s_,
        alpha,
    }
}

pub(crate) fn oklab_to_linear_srgb(l: f64, a: f64, b: f64) -> LinearSrgb {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    LinearSrgb::new(
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s,
    )
}
