use crate::color::{
    A98Rgb, ColorComponent, ColorError, ColorSpace, DisplayP3, HexFallback, HslColor, HsvColor,
    HwbColor, LabColor, LchColor, LinearSrgb, OklabColor, OklchColor, ProPhotoRgb, Rec2020, Srgb,
    XyzD50, XyzD65,
};

/// Authored color representation with missing-component and fallback metadata.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EncodedColor {
    space: ColorSpace,
    components: [ColorComponent; 3],
    alpha: f64,
    hex_fallback: Option<HexFallback>,
}

impl EncodedColor {
    pub fn new(
        space: ColorSpace,
        components: [ColorComponent; 3],
        alpha: f64,
    ) -> Result<Self, ColorError> {
        if !components.iter().all(|component| component.is_finite()) {
            return Err(ColorError::InvalidColorCoordinate);
        }
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Err(ColorError::InvalidAlpha);
        }
        Ok(Self {
            space,
            components,
            alpha,
            hex_fallback: None,
        })
    }

    pub fn from<T: IntoEncodedColor>(value: T, alpha: f64) -> Result<Self, ColorError> {
        Self::new(T::SPACE, value.components(), alpha)
    }

    pub const fn space(self) -> ColorSpace {
        self.space
    }
    pub const fn components(self) -> [ColorComponent; 3] {
        self.components
    }
    pub const fn alpha(self) -> f64 {
        self.alpha
    }
    pub const fn hex_fallback(self) -> Option<HexFallback> {
        self.hex_fallback
    }

    pub const fn with_hex_fallback(mut self, fallback: HexFallback) -> Self {
        self.hex_fallback = Some(fallback);
        self
    }

    pub(crate) fn numeric(space: ColorSpace, values: [f64; 3], alpha: f64) -> Self {
        Self {
            space,
            components: values.map(ColorComponent::Number),
            alpha,
            hex_fallback: None,
        }
    }

    pub(crate) fn values(self) -> [f64; 3] {
        self.components.map(ColorComponent::evaluated)
    }
}

/// Conversion into the generic encoded representation.
pub trait IntoEncodedColor {
    const SPACE: ColorSpace;
    fn components(self) -> [ColorComponent; 3];
}

macro_rules! impl_rgb_encoded {
    ($type:ty, $space:expr) => {
        impl IntoEncodedColor for $type {
            const SPACE: ColorSpace = $space;
            fn components(self) -> [ColorComponent; 3] {
                [self.r.into(), self.g.into(), self.b.into()]
            }
        }
    };
}

impl_rgb_encoded!(Srgb, ColorSpace::Srgb);
impl_rgb_encoded!(LinearSrgb, ColorSpace::LinearSrgb);
impl_rgb_encoded!(DisplayP3, ColorSpace::DisplayP3);
impl_rgb_encoded!(A98Rgb, ColorSpace::A98Rgb);
impl_rgb_encoded!(ProPhotoRgb, ColorSpace::ProPhotoRgb);
impl_rgb_encoded!(Rec2020, ColorSpace::Rec2020);

macro_rules! impl_components {
    ($type:ty, $space:expr, $a:ident, $b:ident, $c:ident) => {
        impl IntoEncodedColor for $type {
            const SPACE: ColorSpace = $space;
            fn components(self) -> [ColorComponent; 3] {
                [self.$a.into(), self.$b.into(), self.$c.into()]
            }
        }
    };
}

impl_components!(HsvColor, ColorSpace::Hsv, hue_degrees, saturation, value);
impl_components!(
    HslColor,
    ColorSpace::Hsl,
    hue_degrees,
    saturation,
    lightness
);
impl_components!(HwbColor, ColorSpace::Hwb, hue_degrees, whiteness, blackness);
impl_components!(LabColor, ColorSpace::Lab, lightness, a, b);
impl_components!(LchColor, ColorSpace::Lch, lightness, chroma, hue_degrees);
impl_components!(OklabColor, ColorSpace::Oklab, l, a, b);
impl_components!(
    OklchColor,
    ColorSpace::Oklch,
    lightness,
    chroma,
    hue_degrees
);
impl_components!(XyzD65, ColorSpace::XyzD65, x, y, z);
impl_components!(XyzD50, ColorSpace::XyzD50, x, y, z);
