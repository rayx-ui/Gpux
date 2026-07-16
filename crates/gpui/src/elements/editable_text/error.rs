use gpux::Point;
use thiserror::Error;

/// A validation failure produced by the neutral editable-text buffer.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TextBufferError {
    /// The edit or revision-bound data was produced for a different buffer revision.
    #[error("stale text revision: expected {expected}, actual {actual}")]
    StaleRevision {
        /// The revision carried by the operation.
        expected: u64,
        /// The buffer or snapshot revision.
        actual: u64,
    },
    /// The range start is after its end.
    #[error("invalid text range {start}..{end}")]
    InvalidRange {
        /// The invalid range start.
        start: usize,
        /// The invalid range end.
        end: usize,
    },
    /// The byte range extends beyond the buffer.
    #[error("text range {start}..{end} exceeds buffer length {len}")]
    RangeOutOfBounds {
        /// The requested range start.
        start: usize,
        /// The requested range end.
        end: usize,
        /// The current UTF-8 byte length.
        len: usize,
    },
    /// The byte offset extends beyond the buffer.
    #[error("text byte offset {offset} exceeds buffer length {len}")]
    ByteOutOfBounds {
        /// The requested byte offset.
        offset: usize,
        /// The current UTF-8 byte length.
        len: usize,
    },
    /// The byte offset falls inside a UTF-8 scalar value.
    #[error("text byte offset {offset} is not a UTF-8 boundary")]
    NotUtf8Boundary {
        /// The non-boundary byte offset.
        offset: usize,
    },
    /// The row or UTF-8 byte column lies outside its logical line.
    #[error(
        "text point {point:?} is outside {line_count} lines (line byte length: {line_byte_len:?})"
    )]
    PointOutOfBounds {
        /// The requested point. Its x coordinate is the UTF-8 byte column and y is the LF row.
        point: Point<usize>,
        /// The snapshot's logical line count.
        line_count: usize,
        /// The requested row's byte length when that row exists.
        line_byte_len: Option<usize>,
    },
    /// The UTF-16 offset extends beyond the buffer.
    #[error("UTF-16 offset {offset} exceeds UTF-16 length {len}")]
    Utf16OutOfBounds {
        /// The requested UTF-16 code-unit offset.
        offset: usize,
        /// The current UTF-16 code-unit length.
        len: usize,
    },
    /// The UTF-16 offset falls between the surrogate pair of a scalar value.
    #[error("UTF-16 offset {offset} falls inside a scalar value")]
    Utf16InsideScalar {
        /// The invalid UTF-16 code-unit offset.
        offset: usize,
    },
    /// The monotonically increasing revision exhausted the revision counter.
    #[error("text revision overflow")]
    RevisionOverflow,
}
