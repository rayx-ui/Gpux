/// Linear-light sRGB channels stored as `f64`.
///
/// Channels are intentionally not clamped so OKLab and OKLCH editing can
/// preserve out-of-display-gamut colors until a display or serialization
/// boundary is reached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearSrgb {
    /// Red channel.
    pub r: f64,
    /// Green channel.
    pub g: f64,
    /// Blue channel.
    pub b: f64,
}

impl LinearSrgb {
    /// Linear black.
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0);

    /// Creates a new linear sRGB value without clamping channels.
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    /// Returns `true` when all channels are finite.
    pub fn is_finite(&self) -> bool {
        self.r.is_finite() && self.g.is_finite() && self.b.is_finite()
    }

    /// Returns `true` when all channels are finite and displayable in sRGB.
    pub fn is_in_display_srgb_gamut(&self) -> bool {
        self.is_finite()
            && (0.0..=1.0).contains(&self.r)
            && (0.0..=1.0).contains(&self.g)
            && (0.0..=1.0).contains(&self.b)
    }

    /// Converts an encoded sRGB channel to linear light.
    pub fn encoded_channel_to_linear(channel: f64) -> f64 {
        let sign = channel.signum();
        let channel = channel.abs();
        sign * if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }

    /// Converts a linear-light sRGB channel to encoded sRGB.
    pub fn linear_channel_to_encoded(channel: f64) -> f64 {
        let sign = channel.signum();
        let channel = channel.abs();
        sign * if channel <= 0.003_130_8 {
            channel * 12.92
        } else {
            1.055 * channel.powf(1.0 / 2.4) - 0.055
        }
    }

    /// Creates a linear value from encoded sRGB channels in `0.0..=1.0`.
    pub fn from_encoded_srgb(r: f64, g: f64, b: f64) -> Self {
        Self::new(
            Self::encoded_channel_to_linear(r),
            Self::encoded_channel_to_linear(g),
            Self::encoded_channel_to_linear(b),
        )
    }

    /// Converts this value to encoded sRGB channels without clamping.
    pub fn to_encoded_srgb(self) -> [f64; 3] {
        [
            Self::linear_channel_to_encoded(self.r),
            Self::linear_channel_to_encoded(self.g),
            Self::linear_channel_to_encoded(self.b),
        ]
    }
}
