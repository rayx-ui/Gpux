/// HWB color coordinates using hue degrees and normalized channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HwbColor {
    pub hue_degrees: f64,
    pub whiteness: f64,
    pub blackness: f64,
}

impl HwbColor {
    pub const fn new(hue_degrees: f64, whiteness: f64, blackness: f64) -> Self {
        Self {
            hue_degrees,
            whiteness,
            blackness,
        }
    }
}
