/// Encoded Adobe RGB (1998) channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct A98Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl A98Rgb {
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }
}
