/// CIE XYZ coordinates relative to the D50 white point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XyzD50 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl XyzD50 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}
