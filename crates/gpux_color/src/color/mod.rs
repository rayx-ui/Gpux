//! UI-neutral color-domain APIs.

mod a98_rgb;
mod color_component;
mod color_error;
mod color_space;
mod conversion;
mod conversion_cache;
mod display_p3;
mod encoded_color;
mod gamut_status;
mod hex;
mod hex_fallback;
mod hex_representation;
mod hsl;
mod hsv;
mod hue_memory;
mod hwb;
mod interpolation;
mod linear_srgb;
mod missing_components;
mod oklab;
mod oklch;
mod output;
mod paint;
mod parser;
mod prophoto_rgb;
mod rec2020;
mod srgb;
mod value;
mod xyz_d50;
mod xyz_d65;

pub use color_error::ColorError;
pub use color_space::ColorSpace;
pub use gamut_status::GamutStatus;
pub use hex_fallback::HexFallback;
pub use hex_representation::HexRepresentation;
pub use hsl::HslColor;
pub use hsv::HsvColor;
pub use hue_memory::HueMemory;
pub use hwb::HwbColor;
pub use interpolation::{
    AlphaInterpolationMode, HueInterpolationMethod, InterpolationOptions, InterpolationSpace,
};
pub use linear_srgb::LinearSrgb;
pub use missing_components::MissingComponents;
pub use oklab::OklabColor;
pub use oklch::OklchColor;
pub use output::{
    AlphaEncoding, ColorOutputPolicy, DisplayColorSpace, DisplayRgba, GamutMappingPolicy, HexColor,
    HexOutputPolicy, HexSerialization, QuantizationPolicy,
};
pub use paint::{DEFAULT_PAINT_GAMUT_POLICY, PaintColor, PaintGamutPolicy};
pub use prophoto_rgb::ProPhotoRgb;
pub use rec2020::Rec2020;
pub use srgb::Srgb;
pub use value::Color;
pub use xyz_d50::XyzD50;
pub use xyz_d65::XyzD65;
mod lab;
mod lch;
pub use a98_rgb::A98Rgb;
pub use color_component::ColorComponent;
pub use display_p3::DisplayP3;
pub use encoded_color::{EncodedColor, IntoEncodedColor};
pub use lab::LabColor;
pub use lch::LchColor;
