use crate::color::{Color, ColorError};
use std::fmt;

/// Bounded display color space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayColorSpace {
    Srgb,
    DisplayP3,
}

/// Gamut handling applied at a bounded output boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GamutMappingPolicy {
    PreserveOrError,
    Clip,
    CssColor4,
}

/// Output quantization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantizationPolicy {
    Float,
    Uint8,
}

/// Output alpha representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaEncoding {
    Straight,
    Premultiplied,
}

/// Complete explicit policy for lossy display output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorOutputPolicy {
    pub target: DisplayColorSpace,
    pub gamut_mapping: GamutMappingPolicy,
    pub quantization: QuantizationPolicy,
    pub alpha: AlphaEncoding,
}

impl ColorOutputPolicy {
    pub const fn new(
        target: DisplayColorSpace,
        gamut_mapping: GamutMappingPolicy,
        quantization: QuantizationPolicy,
        alpha: AlphaEncoding,
    ) -> Self {
        Self {
            target,
            gamut_mapping,
            quantization,
            alpha,
        }
    }
}

/// Neutral display-ready RGBA channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayRgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
    bytes: Option<[u8; 4]>,
}

impl DisplayRgba {
    pub const fn bytes(self) -> Option<[u8; 4]> {
        self.bytes
    }
}

/// Explicit hexadecimal serialization layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HexSerialization {
    Rgb,
    Rgba,
    Argb,
}

/// Explicit policy for sRGB hexadecimal serialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HexOutputPolicy {
    pub gamut_mapping: GamutMappingPolicy,
    pub serialization: HexSerialization,
}

impl HexOutputPolicy {
    pub const fn new(gamut_mapping: GamutMappingPolicy, serialization: HexSerialization) -> Self {
        Self {
            gamut_mapping,
            serialization,
        }
    }
}

/// Serialized lowercase hexadecimal color.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HexColor(String);

impl HexColor {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for HexColor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) fn to_output(
    color: &Color,
    policy: ColorOutputPolicy,
) -> Result<DisplayRgba, ColorError> {
    let mut channels = target_channels(color, policy.target);
    if !in_gamut(channels) {
        channels = match policy.gamut_mapping {
            GamutMappingPolicy::PreserveOrError => return Err(ColorError::OutOfGamut),
            GamutMappingPolicy::Clip => channels.map(|value| value.clamp(0.0, 1.0)),
            GamutMappingPolicy::CssColor4 => css_gamut_map(color, policy.target),
        };
    }
    let alpha = color.alpha();
    if policy.alpha == AlphaEncoding::Premultiplied {
        channels = channels.map(|value| value * alpha);
    }
    let bytes = (policy.quantization == QuantizationPolicy::Uint8).then(|| {
        [
            to_u8(channels[0]),
            to_u8(channels[1]),
            to_u8(channels[2]),
            to_u8(alpha),
        ]
    });
    Ok(DisplayRgba {
        r: channels[0],
        g: channels[1],
        b: channels[2],
        a: alpha,
        bytes,
    })
}

pub(crate) fn to_hex(color: &Color, policy: HexOutputPolicy) -> Result<HexColor, ColorError> {
    let output = to_output(
        color,
        ColorOutputPolicy::new(
            DisplayColorSpace::Srgb,
            policy.gamut_mapping,
            QuantizationPolicy::Uint8,
            AlphaEncoding::Straight,
        ),
    )?;
    let [r, g, b, a] = output.bytes.ok_or(ColorError::InvalidEncodedColor)?;
    let text = match policy.serialization {
        HexSerialization::Rgb => format!("#{r:02x}{g:02x}{b:02x}"),
        HexSerialization::Rgba => format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
        HexSerialization::Argb => format!("#{a:02x}{r:02x}{g:02x}{b:02x}"),
    };
    Ok(HexColor(text))
}

fn target_channels(color: &Color, target: DisplayColorSpace) -> [f64; 3] {
    match target {
        DisplayColorSpace::Srgb => {
            let value = color.to_srgb();
            [value.r, value.g, value.b]
        }
        DisplayColorSpace::DisplayP3 => {
            let value = color.to_display_p3();
            [value.r, value.g, value.b]
        }
    }
}

fn css_gamut_map(color: &Color, target: DisplayColorSpace) -> [f64; 3] {
    let oklch = color.to_oklch();
    let mut low = 0.0;
    let mut high = oklch.chroma.max(0.0);
    let mut best = [0.0; 3];
    for _ in 0..24 {
        let chroma = (low + high) * 0.5;
        let candidate =
            Color::from_oklch(oklch.lightness, chroma, oklch.hue_degrees, color.alpha());
        let Ok(candidate) = candidate else { break };
        let channels = target_channels(&candidate, target);
        if in_gamut(channels) {
            best = channels;
            low = chroma;
        } else {
            high = chroma;
        }
    }
    best.map(|value| value.clamp(0.0, 1.0))
}

fn in_gamut(channels: [f64; 3]) -> bool {
    channels
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
}

fn to_u8(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
