/// Controls whether editable text accepts logical line breaks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditableTextLineMode {
    /// Newlines are normalized to spaces and the viewport remains one visual row.
    #[default]
    SingleLine,
    /// Newlines are preserved and the viewport can display multiple visual rows.
    MultiLine,
}
