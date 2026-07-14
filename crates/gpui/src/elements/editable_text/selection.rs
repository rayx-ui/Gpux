use std::ops::Range;

/// A directional UTF-8 byte selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EditableSelection {
    anchor: usize,
    head: usize,
}

#[allow(missing_docs)]
impl EditableSelection {
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn caret(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    pub fn anchor(&self) -> usize {
        self.anchor
    }

    pub fn head(&self) -> usize {
        self.head
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }

    pub(crate) fn with_head(self, head: usize) -> Self {
        Self { head, ..self }
    }
}
