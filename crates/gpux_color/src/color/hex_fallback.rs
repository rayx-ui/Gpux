/// Optional six-digit sRGB fallback carried by an encoded color.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HexFallback([u8; 3]);

impl HexFallback {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b])
    }

    pub const fn bytes(self) -> [u8; 3] {
        self.0
    }
}
