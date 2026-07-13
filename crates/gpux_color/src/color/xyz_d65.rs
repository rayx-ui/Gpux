/// CIE XYZ coordinates relative to the D65 white point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XyzD65 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl XyzD65 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub(crate) fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}
