/// Display sRGB gamut status for a color value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GamutStatus {
    /// The color is directly displayable in sRGB.
    InSrgb,
    /// At least one linear sRGB channel is outside the displayable range.
    OutOfSrgb,
    /// A preview or serialization boundary clamped the color for display.
    ClampedForDisplay,
}
