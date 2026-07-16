use std::{ops::Range, sync::Arc};

use gpux::HighlightStyle;

use super::{TextBufferError, TextBufferSnapshot};

/// One inline paint style associated with a UTF-8 byte range.
#[derive(Clone, Debug, PartialEq)]
pub struct EditableTextStyleRun {
    /// The absolute UTF-8 byte range in the source snapshot.
    pub range: Range<usize>,
    /// The neutral GPUI highlight style.
    pub style: HighlightStyle,
}

impl EditableTextStyleRun {
    /// Creates an inline style run.
    pub fn new(range: Range<usize>, style: HighlightStyle) -> Self {
        Self { range, style }
    }
}

impl From<(Range<usize>, HighlightStyle)> for EditableTextStyleRun {
    fn from((range, style): (Range<usize>, HighlightStyle)) -> Self {
        Self { range, style }
    }
}

/// A cheaply cloned, revision-bound collection of normalized style runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditableTextStyleRuns {
    revision: u64,
    runs: Arc<[EditableTextStyleRun]>,
}

impl EditableTextStyleRuns {
    /// Creates a collection and stably orders runs by range start and end.
    pub fn new(
        revision: u64,
        runs: impl IntoIterator<Item = impl Into<EditableTextStyleRun>>,
    ) -> Self {
        let mut runs = runs.into_iter().map(Into::into).collect::<Vec<_>>();
        runs.sort_by(|left, right| {
            left.range
                .start
                .cmp(&right.range.start)
                .then_with(|| left.range.end.cmp(&right.range.end))
        });
        Self {
            revision,
            runs: runs.into(),
        }
    }

    /// Creates an empty collection for a snapshot revision.
    pub fn empty(revision: u64) -> Self {
        Self::new(revision, std::iter::empty::<EditableTextStyleRun>())
    }

    /// Returns the source snapshot revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the number of style runs.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// Returns whether there are no style runs.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Iterates over the normalized style runs.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &EditableTextStyleRun> {
        self.runs.iter()
    }

    /// Validates this collection against the snapshot and clips it to the range.
    ///
    /// Returned ranges remain absolute UTF-8 byte ranges in the snapshot.
    pub fn clipped_to(
        &self,
        snapshot: &TextBufferSnapshot,
        range: Range<usize>,
    ) -> Result<Vec<EditableTextStyleRun>, TextBufferError> {
        if self.revision != snapshot.revision() {
            return Err(TextBufferError::StaleRevision {
                expected: self.revision,
                actual: snapshot.revision(),
            });
        }
        snapshot.validate_range(&range)?;
        for run in self.runs.iter() {
            snapshot.validate_range(&run.range)?;
        }

        Ok(self
            .runs
            .iter()
            .filter_map(|run| {
                let start = run.range.start.max(range.start);
                let end = run.range.end.min(range.end);
                (start < end).then(|| EditableTextStyleRun::new(start..end, run.style.clone()))
            })
            .collect())
    }
}
