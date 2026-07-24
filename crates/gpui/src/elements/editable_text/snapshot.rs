use std::ops::Range;

use gpux::Point;
use ropey::{LineType, Rope};

use super::TextBufferError;

/// An immutable, cheaply cloned view of a text buffer at one revision.
#[derive(Clone, Debug)]
pub struct TextBufferSnapshot {
    rope: Rope,
    revision: u64,
}

impl TextBufferSnapshot {
    pub(super) fn new(rope: Rope, revision: u64) -> Self {
        Self { rope, revision }
    }

    /// Returns the source buffer revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the UTF-8 byte length.
    pub fn len(&self) -> usize {
        self.rope.len()
    }

    /// Returns whether the snapshot contains no bytes.
    pub fn is_empty(&self) -> bool {
        self.rope.len() == 0
    }

    /// Returns the number of LF-delimited logical lines.
    ///
    /// Empty text has one line, and text ending in LF has an empty final line.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines(LineType::LF)
    }

    pub(crate) fn logical_line_range(&self, line: usize) -> Result<Range<usize>, TextBufferError> {
        let line_count = self.line_count();
        if line >= line_count {
            return Err(TextBufferError::PointOutOfBounds {
                point: Point::new(0, line),
                line_count,
                line_byte_len: None,
            });
        }

        let start = self.rope.line_to_byte_idx(line, LineType::LF);
        let end = if line + 1 < line_count {
            self.rope.line_to_byte_idx(line + 1, LineType::LF) - 1
        } else {
            self.len()
        };
        Ok(start..end)
    }

    /// Materializes the snapshot as UTF-8 text without exposing Rope storage.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Materializes a validated UTF-8 byte range.
    pub fn slice(&self, range: Range<usize>) -> Result<String, TextBufferError> {
        self.validate_range(&range)?;
        Ok(self.rope.slice(range).to_string())
    }

    /// Converts a UTF-8 byte offset to a GPUI point.
    ///
    /// Point x is the UTF-8 byte column and point y is the zero-based LF row.
    pub fn byte_to_point(&self, offset: usize) -> Result<Point<usize>, TextBufferError> {
        self.validate_byte_offset(offset)?;
        let row = self.rope.byte_to_line_idx(offset, LineType::LF);
        let line_start = self.rope.line_to_byte_idx(row, LineType::LF);
        Ok(Point::new(offset - line_start, row))
    }

    /// Converts a GPUI point to a UTF-8 byte offset.
    ///
    /// Point x is the UTF-8 byte column and point y is the zero-based LF row.
    /// Line columns exclude LF while retaining a preceding CR in CRLF text.
    pub fn point_to_byte(&self, point: Point<usize>) -> Result<usize, TextBufferError> {
        let line_count = self.line_count();
        if point.y >= line_count {
            return Err(TextBufferError::PointOutOfBounds {
                point,
                line_count,
                line_byte_len: None,
            });
        }

        let line_start = self.rope.line_to_byte_idx(point.y, LineType::LF);
        let line_end = if point.y + 1 < line_count {
            self.rope.line_to_byte_idx(point.y + 1, LineType::LF) - 1
        } else {
            self.len()
        };
        let line_byte_len = line_end - line_start;
        if point.x > line_byte_len {
            return Err(TextBufferError::PointOutOfBounds {
                point,
                line_count,
                line_byte_len: Some(line_byte_len),
            });
        }

        let offset = line_start + point.x;
        self.validate_byte_offset(offset)?;
        Ok(offset)
    }

    /// Converts a validated UTF-8 byte offset to a UTF-16 code-unit offset.
    pub fn byte_to_utf16(&self, offset: usize) -> Result<usize, TextBufferError> {
        self.validate_byte_offset(offset)?;
        Ok(self.rope.byte_to_utf16_idx(offset))
    }

    /// Converts a UTF-16 code-unit offset to a UTF-8 byte offset.
    ///
    /// Offsets inside a surrogate pair are rejected instead of clipped.
    pub fn utf16_to_byte(&self, offset: usize) -> Result<usize, TextBufferError> {
        let len = self.rope.len_utf16();
        if offset > len {
            return Err(TextBufferError::Utf16OutOfBounds { offset, len });
        }

        let byte = self.rope.utf16_to_byte_idx(offset);
        if self.rope.byte_to_utf16_idx(byte) != offset {
            return Err(TextBufferError::Utf16InsideScalar { offset });
        }
        Ok(byte)
    }

    pub(super) fn validate_range(&self, range: &Range<usize>) -> Result<(), TextBufferError> {
        if range.start > range.end {
            return Err(TextBufferError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }
        if range.end > self.len() {
            return Err(TextBufferError::RangeOutOfBounds {
                start: range.start,
                end: range.end,
                len: self.len(),
            });
        }
        self.validate_byte_offset(range.start)?;
        self.validate_byte_offset(range.end)
    }

    fn validate_byte_offset(&self, offset: usize) -> Result<(), TextBufferError> {
        if offset > self.len() {
            return Err(TextBufferError::ByteOutOfBounds {
                offset,
                len: self.len(),
            });
        }
        if !self.rope.is_char_boundary(offset) {
            return Err(TextBufferError::NotUtf8Boundary { offset });
        }
        Ok(())
    }
}

impl PartialEq for TextBufferSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision && self.rope == other.rope
    }
}

impl Eq for TextBufferSnapshot {}
