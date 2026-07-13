/// Encoded Display P3 channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayP3 {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl DisplayP3 {
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }
}
