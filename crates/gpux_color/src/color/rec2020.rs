/// Encoded Rec. 2020 channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rec2020 {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl Rec2020 {
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }
}
