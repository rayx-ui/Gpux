/// Encoded color spaces supported by the RayX color system.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum ColorSpace {
    /// Encoded sRGB.
    Srgb,
    /// Linear-light sRGB.
    LinearSrgb,
    /// Hue, saturation, and value.
    Hsv,
    /// Hue, saturation, and lightness.
    Hsl,
    /// Hue, whiteness, and blackness.
    Hwb,
    /// CIELAB using a D50 white point.
    Lab,
    /// CIELCH using a D50 white point.
    Lch,
    /// OKLab lightness, a, and b.
    Oklab,
    /// OKLCH lightness, chroma, and hue.
    Oklch,
    /// Encoded Display P3.
    DisplayP3,
    /// Encoded Adobe RGB (1998).
    A98Rgb,
    /// Encoded ProPhoto RGB.
    ProPhotoRgb,
    /// Encoded Rec. 2020 RGB.
    Rec2020,
    /// CIE XYZ using a D65 white point.
    XyzD65,
    /// CIE XYZ using a D50 white point.
    XyzD50,
}

impl ColorSpace {
    /// All DTCG 2025.10 spaces plus the HSV editing space.
    pub const ALL: [Self; 15] = [
        Self::Srgb,
        Self::LinearSrgb,
        Self::Hsv,
        Self::Hsl,
        Self::Hwb,
        Self::Lab,
        Self::Lch,
        Self::Oklab,
        Self::Oklch,
        Self::DisplayP3,
        Self::A98Rgb,
        Self::ProPhotoRgb,
        Self::Rec2020,
        Self::XyzD65,
        Self::XyzD50,
    ];

    pub(crate) const COUNT: usize = Self::ALL.len();

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// Returns the DTCG color-space key, or `None` for editing-only HSV.
    pub const fn dtcg_key(self) -> Option<&'static str> {
        match self {
            Self::Srgb => Some("srgb"),
            Self::LinearSrgb => Some("srgb-linear"),
            Self::Hsl => Some("hsl"),
            Self::Hwb => Some("hwb"),
            Self::Lab => Some("lab"),
            Self::Lch => Some("lch"),
            Self::Oklab => Some("oklab"),
            Self::Oklch => Some("oklch"),
            Self::DisplayP3 => Some("display-p3"),
            Self::A98Rgb => Some("a98-rgb"),
            Self::ProPhotoRgb => Some("prophoto-rgb"),
            Self::Rec2020 => Some("rec2020"),
            Self::XyzD65 => Some("xyz-d65"),
            Self::XyzD50 => Some("xyz-d50"),
            Self::Hsv => None,
        }
    }
}
