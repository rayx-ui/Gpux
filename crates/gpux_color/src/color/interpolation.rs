use crate::color::{Color, ColorComponent, ColorError, ColorSpace, EncodedColor};

/// Color space used for interpolation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterpolationSpace {
    Srgb,
    LinearSrgb,
    Hsl,
    Hwb,
    Lab,
    Lch,
    Oklab,
    Oklch,
    DisplayP3,
}

impl InterpolationSpace {
    const fn color_space(self) -> ColorSpace {
        match self {
            Self::Srgb => ColorSpace::Srgb,
            Self::LinearSrgb => ColorSpace::LinearSrgb,
            Self::Hsl => ColorSpace::Hsl,
            Self::Hwb => ColorSpace::Hwb,
            Self::Lab => ColorSpace::Lab,
            Self::Lch => ColorSpace::Lch,
            Self::Oklab => ColorSpace::Oklab,
            Self::Oklch => ColorSpace::Oklch,
            Self::DisplayP3 => ColorSpace::DisplayP3,
        }
    }

    const fn hue_index(self) -> Option<usize> {
        match self {
            Self::Hsl | Self::Hwb => Some(0),
            Self::Lch | Self::Oklch => Some(2),
            _ => None,
        }
    }
}

/// Direction used when interpolating polar hue angles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HueInterpolationMethod {
    Shorter,
    Longer,
    Increasing,
    Decreasing,
}

/// Alpha interpolation behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaInterpolationMode {
    Premultiplied,
    Straight,
}

/// Explicit interpolation configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterpolationOptions {
    space: InterpolationSpace,
    hue_method: HueInterpolationMethod,
    alpha_mode: AlphaInterpolationMode,
}

impl InterpolationOptions {
    pub const fn new(space: InterpolationSpace) -> Self {
        Self {
            space,
            hue_method: HueInterpolationMethod::Shorter,
            alpha_mode: AlphaInterpolationMode::Premultiplied,
        }
    }

    pub const fn hue_method(mut self, method: HueInterpolationMethod) -> Self {
        self.hue_method = method;
        self
    }

    pub const fn alpha_mode(mut self, mode: AlphaInterpolationMode) -> Self {
        self.alpha_mode = mode;
        self
    }
}

pub(crate) fn interpolate(
    left: &Color,
    right: &Color,
    progress: f64,
    options: InterpolationOptions,
) -> Result<Color, ColorError> {
    if !progress.is_finite() {
        return Err(ColorError::InvalidColorCoordinate);
    }
    let space = options.space.color_space();
    let left_encoded = left.to_encoded(space);
    let right_encoded = right.to_encoded(space);
    let mut left_components = left_encoded.components();
    let mut right_components = right_encoded.components();
    let mut result_missing = [false; 3];

    for index in 0..3 {
        match (left_components[index], right_components[index]) {
            (ColorComponent::None, ColorComponent::Number(value)) => {
                left_components[index] = ColorComponent::Number(value);
            }
            (ColorComponent::Number(value), ColorComponent::None) => {
                right_components[index] = ColorComponent::Number(value);
            }
            (ColorComponent::None, ColorComponent::None) => result_missing[index] = true,
            _ => {}
        }
    }

    let left_alpha = left.alpha();
    let right_alpha = right.alpha();
    let alpha = lerp(left_alpha, right_alpha, progress);
    let hue_index = options.space.hue_index();
    let mut result = [ColorComponent::None; 3];
    for index in 0..3 {
        if result_missing[index] {
            continue;
        }
        let mut a = left_components[index].evaluated();
        let mut b = right_components[index].evaluated();
        if Some(index) == hue_index {
            b = adjusted_hue(a, b, options.hue_method);
        } else if options.alpha_mode == AlphaInterpolationMode::Premultiplied {
            a *= left_alpha;
            b *= right_alpha;
        }
        let mut value = lerp(a, b, progress);
        if Some(index) == hue_index {
            value = value.rem_euclid(360.0);
        } else if options.alpha_mode == AlphaInterpolationMode::Premultiplied && alpha != 0.0 {
            value /= alpha;
        }
        result[index] = ColorComponent::Number(value);
    }

    Color::from_encoded(EncodedColor::new(space, result, alpha)?)
}

fn adjusted_hue(left: f64, right: f64, method: HueInterpolationMethod) -> f64 {
    let left = left.rem_euclid(360.0);
    let right = right.rem_euclid(360.0);
    let increasing = (right - left).rem_euclid(360.0);
    let decreasing = increasing - 360.0;
    left + match method {
        HueInterpolationMethod::Increasing => increasing,
        HueInterpolationMethod::Decreasing => decreasing,
        HueInterpolationMethod::Shorter => {
            if increasing <= 180.0 {
                increasing
            } else {
                decreasing
            }
        }
        HueInterpolationMethod::Longer => {
            if increasing < 180.0 {
                decreasing
            } else {
                increasing
            }
        }
    }
}

fn lerp(left: f64, right: f64, progress: f64) -> f64 {
    left + (right - left) * progress
}
