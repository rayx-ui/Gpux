use std::ops::Range;

/// An atomic UTF-8 byte-range replacement for a specific buffer revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    expected_revision: u64,
    range: Range<usize>,
    replacement: String,
}

impl TextEdit {
    /// Creates a replacement that is valid only for the expected revision.
    pub fn new(
        expected_revision: u64,
        range: Range<usize>,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            expected_revision,
            range,
            replacement: replacement.into(),
        }
    }

    /// Returns the buffer revision this edit expects.
    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    /// Returns the UTF-8 byte range replaced by this edit.
    pub fn range(&self) -> &Range<usize> {
        &self.range
    }

    /// Returns the replacement UTF-8 text.
    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    pub(super) fn into_parts(self) -> (u64, Range<usize>, String) {
        (self.expected_revision, self.range, self.replacement)
    }
}
