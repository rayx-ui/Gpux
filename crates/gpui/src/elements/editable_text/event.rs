/// Events emitted after successful editable-text transactions.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditableTextEvent {
    Changed { revision: u64 },
}
