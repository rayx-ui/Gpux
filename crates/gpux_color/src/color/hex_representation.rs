/// Hex alpha-channel ordering for eight-digit color values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HexRepresentation {
    /// `#RRGGBBAA` or `#RGBA`.
    Rgba,
    /// `#AARRGGBB`.
    Argb,
}
