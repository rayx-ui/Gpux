/// CIELAB coordinates relative to D50.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabColor {
    pub lightness: f64,
    pub a: f64,
    pub b: f64,
}

impl LabColor {
    pub const fn new(lightness: f64, a: f64, b: f64) -> Self {
        Self { lightness, a, b }
    }
}
