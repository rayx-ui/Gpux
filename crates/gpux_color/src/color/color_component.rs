/// One authored encoded color component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorComponent {
    /// A numeric component, preserving its exact `f64` bits.
    Number(f64),
    /// The authored `none` marker.
    None,
}

impl ColorComponent {
    pub(crate) fn evaluated(self) -> f64 {
        match self {
            Self::Number(value) => value,
            Self::None => 0.0,
        }
    }

    pub(crate) fn is_finite(self) -> bool {
        matches!(self, Self::None) || matches!(self, Self::Number(value) if value.is_finite())
    }
}

impl From<f64> for ColorComponent {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}
