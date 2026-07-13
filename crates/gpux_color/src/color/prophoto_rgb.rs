/// Encoded ProPhoto RGB channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProPhotoRgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl ProPhotoRgb {
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }
}
