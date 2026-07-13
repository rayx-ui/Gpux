use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::{hash::Hash, hash::Hasher};

use crate::color::conversion::{encoded_to_xyz, xyz_to_encoded};
use crate::color::conversion_cache::ColorConversionCache;
use crate::color::{
    A98Rgb, ColorComponent, ColorError, ColorSpace, DisplayP3, EncodedColor, GamutStatus,
    HexRepresentation, HslColor, HsvColor, HueMemory, HwbColor, LabColor, LchColor, LinearSrgb,
    MissingComponents, OklabColor, OklchColor, ProPhotoRgb, Rec2020, Srgb, XyzD50, XyzD65,
};

/// Immutable, cheap-clone color with canonical CIE XYZ D65 coordinates.
#[derive(Clone, Debug)]
pub struct Color(Arc<ColorInner>);

impl Default for Color {
    fn default() -> Self {
        Self::transparent_black()
    }
}

#[derive(Debug)]
struct ColorInner {
    xyz_d65: XyzD65,
    alpha: f64,
    source: EncodedColor,
    origin: ColorSpace,
    missing_components: MissingComponents,
    hue_memory: HueMemory,
    cache: ColorConversionCache,
    paint_color: OnceLock<crate::PaintColor>,
    paint_normalizations: AtomicUsize,
}

impl PartialEq for Color {
    fn eq(&self, other: &Self) -> bool {
        self.0.xyz_d65 == other.0.xyz_d65
            && self.0.alpha == other.0.alpha
            && self.0.source == other.0.source
            && self.0.origin == other.0.origin
            && self.0.missing_components == other.0.missing_components
            && self.0.hue_memory == other.0.hue_memory
    }
}

impl Color {
    /// Creates transparent black.
    pub fn transparent_black() -> Self {
        let source = EncodedColor::numeric(ColorSpace::XyzD65, [0.0, 0.0, 0.0], 0.0);
        Self(Arc::new(ColorInner {
            xyz_d65: XyzD65::new(0.0, 0.0, 0.0),
            alpha: 0.0,
            source,
            origin: ColorSpace::XyzD65,
            missing_components: MissingComponents::default(),
            hue_memory: HueMemory::EMPTY,
            cache: ColorConversionCache::new(source),
            paint_color: OnceLock::new(),
            paint_normalizations: AtomicUsize::new(0),
        }))
    }

    pub fn is_transparent(&self) -> bool {
        self.alpha() == 0.0
    }

    pub fn is_opaque(&self) -> bool {
        self.alpha() == 1.0
    }

    pub fn opacity(&self, factor: f32) -> Self {
        self.with_alpha(self.alpha() * f64::from(factor.clamp(0.0, 1.0)))
            .expect("multiplying a valid alpha by a clamped finite factor stays valid")
    }

    /// Fades this handle by replacing it with an immutable value whose alpha is reduced.
    pub fn fade_out(&mut self, factor: f32) {
        *self = self.opacity(1.0 - factor.clamp(0.0, 1.0));
    }

    pub fn blend(&self, other: Self) -> Self {
        let alpha = other.alpha();
        if alpha >= 1.0 {
            return other;
        }
        if alpha <= 0.0 {
            return self.clone();
        }
        let below = self.to_srgb();
        let above = other.to_srgb();
        Self::from_srgb(
            Srgb::new(
                below.r * (1.0 - alpha) + above.r * alpha,
                below.g * (1.0 - alpha) + above.g * alpha,
                below.b * (1.0 - alpha) + above.b * alpha,
            ),
            self.alpha(),
        )
        .expect("blending finite colors produces a finite color")
    }

    /// Evaluates an authored encoded representation into canonical XYZ D65.
    pub fn from_encoded(source: EncodedColor) -> Result<Self, ColorError> {
        let xyz_d65 = encoded_to_xyz(source)?;
        let hue_memory = source_hue_memory(source);
        let origin = source.space();
        let missing_components = MissingComponents::from_components(source.components());
        Ok(Self(Arc::new(ColorInner {
            xyz_d65,
            alpha: source.alpha(),
            source,
            origin,
            missing_components,
            hue_memory,
            cache: ColorConversionCache::new(source),
            paint_color: OnceLock::new(),
            paint_normalizations: AtomicUsize::new(0),
        })))
    }

    pub fn from_xyz_d65(value: XyzD65, alpha: f64) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([value.x, value.y, value.z])?;
        Self::from_encoded(EncodedColor::from(value, alpha)?)
    }

    pub fn from_xyz_d50(value: XyzD50, alpha: f64) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([value.x, value.y, value.z])?;
        Self::from_encoded(EncodedColor::from(value, alpha)?)
    }

    pub fn from_srgb(value: Srgb, alpha: f64) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([value.r, value.g, value.b])?;
        Self::from_encoded(EncodedColor::from(value, alpha)?)
    }

    pub fn from_display_p3(value: DisplayP3, alpha: f64) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([value.r, value.g, value.b])?;
        Self::from_encoded(EncodedColor::from(value, alpha)?)
    }

    pub fn from_prophoto_rgb(value: ProPhotoRgb, alpha: f64) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([value.r, value.g, value.b])?;
        Self::from_encoded(EncodedColor::from(value, alpha)?)
    }

    /// Creates a color from unbounded linear-light sRGB channels.
    pub fn from_linear_srgb(value: LinearSrgb, alpha: f64) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([value.r, value.g, value.b])?;
        Self::from_encoded(EncodedColor::from(value, alpha)?)
    }

    /// Creates a color from unbounded encoded sRGB channels.
    pub fn from_encoded_srgb(r: f64, g: f64, b: f64, alpha: f64) -> Result<Self, ColorError> {
        Self::from_srgb(Srgb::new(r, g, b), alpha)
    }

    pub fn from_hsv(hue: f64, saturation: f64, value: f64, alpha: f64) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([hue, saturation, value])?;
        Self::from_encoded(EncodedColor::from(
            HsvColor::new(hue, saturation, value),
            alpha,
        )?)
    }

    pub fn from_hsl(
        hue: f64,
        saturation: f64,
        lightness: f64,
        alpha: f64,
    ) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([hue, saturation, lightness])?;
        Self::from_encoded(EncodedColor::from(
            HslColor::new(hue, saturation, lightness),
            alpha,
        )?)
    }

    pub fn from_oklab(l: f64, a: f64, b: f64, alpha: f64) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([l, a, b])?;
        Self::from_encoded(EncodedColor::from(OklabColor::new(l, a, b), alpha)?)
    }

    pub fn from_oklch(
        lightness: f64,
        chroma: f64,
        hue: f64,
        alpha: f64,
    ) -> Result<Self, ColorError> {
        validate_alpha(alpha)?;
        validate_coordinates([lightness, chroma, hue])?;
        Self::from_encoded(EncodedColor::from(
            OklchColor::new(lightness, chroma, hue),
            alpha,
        )?)
    }

    pub fn parse_hex(input: &str, representation: HexRepresentation) -> Result<Self, ColorError> {
        crate::color::hex::parse_hex(input, representation)
    }

    /// Parses bounded numeric CSS Color 4 syntax without palette identifiers.
    pub fn parse_css(input: &str) -> Result<Self, ColorError> {
        crate::color::parser::parse_css(input)
    }

    /// Interpolates two colors using an explicit color space and hue/alpha policy.
    pub fn interpolate(
        &self,
        other: &Self,
        progress: f64,
        options: crate::color::InterpolationOptions,
    ) -> Result<Self, ColorError> {
        crate::color::interpolation::interpolate(self, other, progress, options)
    }

    /// Produces bounded display channels using an explicit loss policy.
    pub fn to_output(
        &self,
        policy: crate::color::ColorOutputPolicy,
    ) -> Result<crate::color::DisplayRgba, ColorError> {
        crate::color::output::to_output(self, policy)
    }

    pub fn to_hex(
        &self,
        policy: crate::color::HexOutputPolicy,
    ) -> Result<crate::color::HexColor, ColorError> {
        crate::color::output::to_hex(self, policy)
    }

    pub fn xyz_d65(&self) -> XyzD65 {
        self.0.xyz_d65
    }
    pub fn alpha(&self) -> f64 {
        self.0.alpha
    }
    pub fn hue_memory(&self) -> HueMemory {
        self.0.hue_memory
    }

    /// Returns the space from which this color was evaluated.
    pub fn origin_space(&self) -> ColorSpace {
        self.0.origin
    }

    /// Returns the authored missing-component markers.
    pub fn missing_components(&self) -> MissingComponents {
        self.0.missing_components
    }

    pub fn to_encoded(&self, space: ColorSpace) -> EncodedColor {
        self.0.cache.get_or_init(space, || {
            xyz_to_encoded(self.0.xyz_d65, self.0.alpha, space, self.0.hue_memory)
        })
    }

    /// Returns the fixed premultiplied linear-light renderer representation.
    pub fn paint_color(&self) -> crate::PaintColor {
        *self.0.paint_color.get_or_init(|| {
            self.0.paint_normalizations.fetch_add(1, Ordering::Relaxed);
            crate::PaintColor::from_color(self)
        })
    }

    pub fn to_srgb(&self) -> Srgb {
        let [r, g, b] = numeric(self.to_encoded(ColorSpace::Srgb));
        Srgb::new(r, g, b)
    }

    pub fn to_linear_srgb(&self) -> LinearSrgb {
        let [r, g, b] = numeric(self.to_encoded(ColorSpace::LinearSrgb));
        LinearSrgb::new(r, g, b)
    }

    pub fn linear_srgb(&self) -> LinearSrgb {
        self.to_linear_srgb()
    }
    pub fn to_encoded_srgb(&self) -> [f64; 3] {
        let v = self.to_srgb();
        [v.r, v.g, v.b]
    }

    pub fn to_hsv(&self) -> HsvColor {
        let [hue_degrees, saturation, value] = numeric(self.to_encoded(ColorSpace::Hsv));
        HsvColor {
            hue_degrees,
            saturation,
            value,
            alpha: self.alpha(),
        }
    }

    pub fn to_hsl(&self) -> HslColor {
        let [hue_degrees, saturation, lightness] = numeric(self.to_encoded(ColorSpace::Hsl));
        HslColor {
            hue_degrees,
            saturation,
            lightness,
            alpha: self.alpha(),
        }
    }

    pub fn to_hwb(&self) -> HwbColor {
        let [hue_degrees, whiteness, blackness] = numeric(self.to_encoded(ColorSpace::Hwb));
        HwbColor::new(hue_degrees, whiteness, blackness)
    }

    pub fn to_lab(&self) -> LabColor {
        let [lightness, a, b] = numeric(self.to_encoded(ColorSpace::Lab));
        LabColor::new(lightness, a, b)
    }

    pub fn to_lch(&self) -> LchColor {
        let [lightness, chroma, hue_degrees] = numeric(self.to_encoded(ColorSpace::Lch));
        LchColor::new(lightness, chroma, hue_degrees)
    }

    pub fn to_oklab(&self) -> OklabColor {
        let [l, a, b] = numeric(self.to_encoded(ColorSpace::Oklab));
        OklabColor {
            l,
            a,
            b,
            alpha: self.alpha(),
        }
    }

    pub fn to_oklch(&self) -> OklchColor {
        let [lightness, chroma, hue_degrees] = numeric(self.to_encoded(ColorSpace::Oklch));
        OklchColor {
            lightness,
            chroma,
            hue_degrees,
            alpha: self.alpha(),
        }
    }

    pub fn to_display_p3(&self) -> DisplayP3 {
        let [r, g, b] = numeric(self.to_encoded(ColorSpace::DisplayP3));
        DisplayP3::new(r, g, b)
    }

    pub fn to_a98_rgb(&self) -> A98Rgb {
        let [r, g, b] = numeric(self.to_encoded(ColorSpace::A98Rgb));
        A98Rgb::new(r, g, b)
    }

    pub fn to_prophoto_rgb(&self) -> ProPhotoRgb {
        let [r, g, b] = numeric(self.to_encoded(ColorSpace::ProPhotoRgb));
        ProPhotoRgb::new(r, g, b)
    }

    pub fn to_rec2020(&self) -> Rec2020 {
        let [r, g, b] = numeric(self.to_encoded(ColorSpace::Rec2020));
        Rec2020::new(r, g, b)
    }

    pub fn to_xyz_d50(&self) -> XyzD50 {
        let [x, y, z] = numeric(self.to_encoded(ColorSpace::XyzD50));
        XyzD50::new(x, y, z)
    }

    pub fn with_alpha(&self, alpha: f64) -> Result<Self, ColorError> {
        Self::from_xyz_d65(self.xyz_d65(), alpha)
    }

    pub fn with_encoded_rgb_channel(&self, index: usize, value: f64) -> Result<Self, ColorError> {
        let mut channels = self.to_encoded_srgb();
        *channels
            .get_mut(index)
            .ok_or(ColorError::InvalidChannelIndex)? = value;
        Self::from_encoded_srgb(channels[0], channels[1], channels[2], self.alpha())
    }

    pub fn with_hsv_channel(&self, index: usize, value: f64) -> Result<Self, ColorError> {
        let mut hsv = self.to_hsv();
        match index {
            0 => hsv.hue_degrees = value.rem_euclid(360.0),
            1 => hsv.saturation = value,
            2 => hsv.value = value,
            3 => return self.with_alpha(value),
            _ => return Err(ColorError::InvalidChannelIndex),
        }
        Self::from_hsv(hsv.hue_degrees, hsv.saturation, hsv.value, self.alpha())
    }

    pub fn with_hsl_channel(&self, index: usize, value: f64) -> Result<Self, ColorError> {
        let mut hsl = self.to_hsl();
        match index {
            0 => hsl.hue_degrees = value.rem_euclid(360.0),
            1 => hsl.saturation = value,
            2 => hsl.lightness = value,
            3 => return self.with_alpha(value),
            _ => return Err(ColorError::InvalidChannelIndex),
        }
        Self::from_hsl(hsl.hue_degrees, hsl.saturation, hsl.lightness, self.alpha())
    }

    pub fn with_oklch_channel(&self, index: usize, value: f64) -> Result<Self, ColorError> {
        let mut oklch = self.to_oklch();
        match index {
            0 => oklch.lightness = value,
            1 => oklch.chroma = value,
            2 => oklch.hue_degrees = value.rem_euclid(360.0),
            3 => return self.with_alpha(value),
            _ => return Err(ColorError::InvalidChannelIndex),
        }
        Self::from_oklch(
            oklch.lightness,
            oklch.chroma,
            oklch.hue_degrees,
            self.alpha(),
        )
    }

    pub fn gamut_status(&self) -> GamutStatus {
        if self.to_linear_srgb().is_in_display_srgb_gamut() {
            GamutStatus::InSrgb
        } else {
            GamutStatus::OutOfSrgb
        }
    }

    pub fn display_gamut_status(&self) -> GamutStatus {
        if self.gamut_status() == GamutStatus::InSrgb {
            GamutStatus::InSrgb
        } else {
            GamutStatus::ClampedForDisplay
        }
    }

    /// Diagnostic counter used to prove one cache initialization under contention.
    #[doc(hidden)]
    pub fn cache_initialization_count(&self, space: ColorSpace) -> usize {
        self.0.cache.initialization_count(space)
    }

    /// Diagnostic counter for all lazy encoded-space conversions.
    #[doc(hidden)]
    pub fn conversion_initialization_count(&self) -> usize {
        self.0.cache.total_initialization_count()
    }

    /// Diagnostic counter for the fixed renderer normalization.
    #[doc(hidden)]
    pub fn paint_color_initialization_count(&self) -> usize {
        self.0.paint_normalizations.load(Ordering::Relaxed)
    }
}

macro_rules! impl_color_from_typed {
    ($type:ty) => {
        impl From<$type> for Color {
            fn from(value: $type) -> Self {
                Self::from_encoded(
                    EncodedColor::from(value, 1.0)
                        .expect("typed color inputs must contain finite components"),
                )
                .expect("typed color inputs must convert to finite XYZ D65")
            }
        }
    };
}

impl_color_from_typed!(Srgb);
impl_color_from_typed!(LinearSrgb);
impl_color_from_typed!(DisplayP3);
impl_color_from_typed!(A98Rgb);
impl_color_from_typed!(ProPhotoRgb);
impl_color_from_typed!(Rec2020);
impl_color_from_typed!(HsvColor);
impl_color_from_typed!(HslColor);
impl_color_from_typed!(HwbColor);
impl_color_from_typed!(LabColor);
impl_color_from_typed!(LchColor);
impl_color_from_typed!(OklabColor);
impl_color_from_typed!(OklchColor);
impl_color_from_typed!(XyzD65);
impl_color_from_typed!(XyzD50);

impl Eq for Color {}

impl Hash for Color {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f64(self.0.xyz_d65.x, state);
        hash_f64(self.0.xyz_d65.y, state);
        hash_f64(self.0.xyz_d65.z, state);
        hash_f64(self.0.alpha, state);
        self.0.origin.hash(state);
        for component in self.0.source.components() {
            match component {
                ColorComponent::Number(value) => {
                    0_u8.hash(state);
                    hash_f64(value, state);
                }
                ColorComponent::None => 1_u8.hash(state),
            }
        }
        hash_f64(self.0.source.alpha(), state);
        self.0
            .source
            .hex_fallback()
            .map(|value| value.bytes())
            .hash(state);
        for index in 0..3 {
            self.0.missing_components.contains(index).hash(state);
        }
        hash_optional_f64(self.0.hue_memory.hsv(), state);
        hash_optional_f64(self.0.hue_memory.hsl(), state);
        hash_optional_f64(self.0.hue_memory.oklch(), state);
    }
}

fn source_hue_memory(source: EncodedColor) -> HueMemory {
    let values = source.values();
    match source.space() {
        ColorSpace::Hsv
            if source.components()[0] != ColorComponent::None
                && values[1] != 0.0
                && values[2] != 0.0 =>
        {
            HueMemory::EMPTY.with_hsv(Some(values[0].rem_euclid(360.0)))
        }
        ColorSpace::Hsl if source.components()[0] != ColorComponent::None && values[1] != 0.0 => {
            HueMemory::EMPTY.with_hsl(Some(values[0].rem_euclid(360.0)))
        }
        ColorSpace::Oklch if source.components()[2] != ColorComponent::None && values[1] != 0.0 => {
            HueMemory::EMPTY.with_oklch(Some(values[2].rem_euclid(360.0)))
        }
        _ => HueMemory::EMPTY,
    }
}

fn numeric(value: EncodedColor) -> [f64; 3] {
    value.values()
}
fn validate_alpha(alpha: f64) -> Result<(), ColorError> {
    if alpha.is_finite() && (0.0..=1.0).contains(&alpha) {
        Ok(())
    } else {
        Err(ColorError::InvalidAlpha)
    }
}
fn validate_coordinates(values: [f64; 3]) -> Result<(), ColorError> {
    if values.into_iter().all(f64::is_finite) {
        Ok(())
    } else {
        Err(ColorError::InvalidColorCoordinate)
    }
}

fn hash_f64<H: Hasher>(value: f64, state: &mut H) {
    if value == 0.0 {
        0_u64.hash(state);
    } else {
        value.to_bits().hash(state);
    }
}

fn hash_optional_f64<H: Hasher>(value: Option<f64>, state: &mut H) {
    match value {
        Some(value) => {
            1_u8.hash(state);
            hash_f64(value, state);
        }
        None => 0_u8.hash(state),
    }
}
