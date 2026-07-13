/// CIELCH coordinates relative to D50.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LchColor {
    pub lightness: f64,
    pub chroma: f64,
    pub hue_degrees: f64,
}

impl LchColor {
    pub const fn new(lightness: f64, chroma: f64, hue_degrees: f64) -> Self {
        Self {
            lightness,
            chroma,
            hue_degrees,
        }
    }
}
