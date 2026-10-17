/// Events emitted after successful editable-text transactions.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditableTextEvent {
    Changed {
        revision: u64,
        composition_active: bool,
    },
}

/// Emitted after a change to stored formatting that leaves the text (and its revision) as it
/// was. Edits to the text report [`EditableTextEvent::Changed`].
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditableTextFormatEvent {
    Changed { format_revision: u64 },
}
