use std::fmt;

/// Typed errors returned by color parsing, validation, and editing APIs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorError {
    /// Alpha must be finite and in the `0.0..=1.0` range.
    InvalidAlpha,
    /// Encoded RGB channels must be finite and in the `0.0..=1.0` range.
    InvalidRgbChannel,
    /// Channel index is outside the supported range.
    InvalidChannelIndex,
    /// A color coordinate must be finite.
    InvalidColorCoordinate,
    /// Hex color text is invalid.
    InvalidHex,
    /// The encoded representation has invalid structural data.
    InvalidEncodedColor,
    /// CSS color syntax is invalid at the byte offset.
    InvalidCssSyntax { offset: usize },
    /// The selected bounded output cannot preserve the color.
    OutOfGamut,
}

impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAlpha => f.write_str("alpha must be finite and in the 0.0..=1.0 range"),
            Self::InvalidRgbChannel => {
                f.write_str("encoded RGB channel must be finite and in the 0.0..=1.0 range")
            }
            Self::InvalidChannelIndex => {
                f.write_str("channel index is outside the supported range")
            }
            Self::InvalidColorCoordinate => f.write_str("color coordinate must be finite"),
            Self::InvalidHex => f.write_str("hex color text is invalid"),
            Self::InvalidEncodedColor => f.write_str("encoded color structure is invalid"),
            Self::InvalidCssSyntax { offset } => {
                write!(f, "CSS color syntax is invalid at byte offset {offset}")
            }
            Self::OutOfGamut => f.write_str("color is outside the selected output gamut"),
        }
    }
}

impl std::error::Error for ColorError {}
