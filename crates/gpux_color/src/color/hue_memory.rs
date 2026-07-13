/// Hue memory used to preserve powerless hue values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HueMemory {
    hsv: Option<f64>,
    hsl: Option<f64>,
    oklch: Option<f64>,
}

impl HueMemory {
    /// Empty hue memory.
    pub const EMPTY: Self = Self {
        hsv: None,
        hsl: None,
        oklch: None,
    };

    /// Returns the last meaningful HSV hue in degrees.
    pub const fn hsv(&self) -> Option<f64> {
        self.hsv
    }

    /// Returns the last meaningful HSL hue in degrees.
    pub const fn hsl(&self) -> Option<f64> {
        self.hsl
    }

    /// Returns the last meaningful OKLCH hue in degrees.
    pub const fn oklch(&self) -> Option<f64> {
        self.oklch
    }

    /// Returns a copy with the HSV hue updated when meaningful.
    pub const fn with_hsv(mut self, hue: Option<f64>) -> Self {
        self.hsv = hue;
        self
    }

    /// Returns a copy with the HSL hue updated when meaningful.
    pub const fn with_hsl(mut self, hue: Option<f64>) -> Self {
        self.hsl = hue;
        self
    }

    /// Returns a copy with the OKLCH hue updated when meaningful.
    pub const fn with_oklch(mut self, hue: Option<f64>) -> Self {
        self.oklch = hue;
        self
    }
}
