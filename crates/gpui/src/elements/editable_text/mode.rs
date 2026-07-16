/// Neutral interaction modes for editable text.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditableTextMode {
    #[default]
    Editable,
    ReadOnly,
    Disabled,
}

#[allow(missing_docs)]
impl EditableTextMode {
    pub fn permits_mutation(self) -> bool {
        matches!(self, Self::Editable)
    }

    pub fn permits_focus(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}
