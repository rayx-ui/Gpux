use ropey::Rope;

use super::{TextBufferError, TextBufferSnapshot, TextEdit};

/// A revisioned UTF-8 text buffer backed privately by a copy-on-write Rope.
#[derive(Clone, Debug)]
pub struct TextBuffer {
    rope: Rope,
    revision: u64,
}

impl TextBuffer {
    /// Creates a buffer at revision zero.
    pub fn new(text: impl AsRef<str>) -> Self {
        Self {
            rope: Rope::from(text.as_ref()),
            revision: 0,
        }
    }

    /// Returns the current revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the current UTF-8 byte length.
    pub fn len(&self) -> usize {
        self.rope.len()
    }

    /// Returns whether the buffer contains no bytes.
    pub fn is_empty(&self) -> bool {
        self.rope.len() == 0
    }

    /// Returns a stable immutable snapshot of the current text and revision.
    pub fn snapshot(&self) -> TextBufferSnapshot {
        TextBufferSnapshot::new(self.rope.clone(), self.revision)
    }

    /// Applies one revision-checked edit atomically and returns the new snapshot.
    ///
    /// Revision, range, and UTF-8 boundary validation completes before a cloned
    /// Rope is changed. The buffer is replaced only after the edit succeeds.
    pub fn apply(&mut self, edit: TextEdit) -> Result<TextBufferSnapshot, TextBufferError> {
        let (expected_revision, range, replacement) = edit.into_parts();
        if expected_revision != self.revision {
            return Err(TextBufferError::StaleRevision {
                expected: expected_revision,
                actual: self.revision,
            });
        }

        self.snapshot().validate_range(&range)?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(TextBufferError::RevisionOverflow)?;
        let mut next_rope = self.rope.clone();
        next_rope.remove(range.clone());
        next_rope.insert(range.start, &replacement);

        self.rope = next_rope;
        self.revision = next_revision;
        Ok(self.snapshot())
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new("")
    }
}

impl From<&str> for TextBuffer {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for TextBuffer {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}
