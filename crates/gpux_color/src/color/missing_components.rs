use crate::color::ColorComponent;

/// Compact authored `none` markers for a three-component color.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MissingComponents(u8);

impl MissingComponents {
    pub(crate) fn from_components(components: [ColorComponent; 3]) -> Self {
        let mut bits = 0;
        for (index, component) in components.into_iter().enumerate() {
            if component == ColorComponent::None {
                bits |= 1 << index;
            }
        }
        Self(bits)
    }

    /// Returns whether the authored component at `index` was `none`.
    pub const fn contains(self, index: usize) -> bool {
        index < 3 && self.0 & (1 << index) != 0
    }

    /// Returns whether no components were authored as `none`.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}
