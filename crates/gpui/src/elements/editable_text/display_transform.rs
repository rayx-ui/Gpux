use std::{ops::Range, sync::Arc};

use crate::SharedString;
use unicode_segmentation::UnicodeSegmentation as _;

use super::{TextBuffer, TextBufferError};

/// A form-agnostic visual transform for editable text.
///
/// The underlying buffer remains authoritative. The transform only supplies
/// display text and coordinate mapping to the editable-text element.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum EditableTextDisplayTransform {
    /// Paint the buffer without transforming it.
    #[default]
    Identity,
    /// Replace every non-newline grapheme with the supplied display text.
    PerGrapheme(SharedString),
    /// Paint an immutable, validated projection of the current source.
    Projected(EditableTextDisplayProjection),
}

/// A validated source-to-display projection with private coordinate boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditableTextDisplayProjection {
    source: SharedString,
    projection: EditableTextProjection,
}

impl EditableTextDisplayTransform {
    /// Replaces every non-newline grapheme with the supplied display text.
    pub fn per_grapheme(replacement: impl Into<SharedString>) -> Self {
        Self::PerGrapheme(replacement.into())
    }

    /// Creates a neutral display projection that omits the supplied UTF-8 byte ranges.
    pub fn omitting_ranges(
        source: impl AsRef<str>,
        ranges: impl IntoIterator<Item = Range<usize>>,
    ) -> Result<Self, TextBufferError> {
        let source = source.as_ref();
        let snapshot = TextBuffer::new(source).snapshot();
        let mut ranges = ranges.into_iter().collect::<Vec<_>>();
        ranges.sort_by_key(|range| (range.start, range.end));
        let mut normalized: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            snapshot.validate_range(&range)?;
            if range.is_empty() {
                continue;
            }
            if let Some(last) = normalized.last_mut()
                && range.start <= last.end
            {
                last.end = last.end.max(range.end);
                continue;
            }
            normalized.push(range);
        }

        let mut display = String::with_capacity(source.len());
        let mut boundaries = vec![(0, 0)];
        let mut cursor = 0;
        for range in normalized {
            append_projected_segment(source, cursor..range.start, &mut display, &mut boundaries);
            boundaries.push((range.end, display.len()));
            cursor = range.end;
        }
        append_projected_segment(source, cursor..source.len(), &mut display, &mut boundaries);
        Ok(Self::Projected(EditableTextDisplayProjection {
            source: SharedString::from(source.to_owned()),
            projection: EditableTextProjection {
                text: display.into(),
                boundaries: boundaries.into(),
            },
        }))
    }

    pub(crate) fn project(&self, source: &str) -> Option<EditableTextProjection> {
        match self {
            Self::Identity => None,
            Self::PerGrapheme(replacement) => {
                let mut text = String::new();
                let mut boundaries = Vec::new();
                boundaries.push((0, 0));
                for (buffer_start, grapheme) in source.grapheme_indices(true) {
                    if grapheme == "\n" {
                        text.push('\n');
                    } else {
                        text.push_str(replacement);
                    }
                    boundaries.push((buffer_start + grapheme.len(), text.len()));
                }
                Some(EditableTextProjection {
                    text: text.into(),
                    boundaries: boundaries.into(),
                })
            }
            Self::Projected(projected) if projected.source.as_ref() == source => {
                Some(projected.projection.clone())
            }
            Self::Projected(_) => None,
        }
    }
}

fn append_projected_segment(
    source: &str,
    range: Range<usize>,
    display: &mut String,
    boundaries: &mut Vec<(usize, usize)>,
) {
    for (relative_start, character) in source[range.clone()].char_indices() {
        display.push(character);
        boundaries.push((
            range.start + relative_start + character.len_utf8(),
            display.len(),
        ));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditableTextProjection {
    text: SharedString,
    boundaries: Arc<[(usize, usize)]>,
}

impl EditableTextProjection {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn buffer_to_display(&self, offset: usize) -> usize {
        let index = self
            .boundaries
            .partition_point(|(buffer, _)| *buffer <= offset)
            .saturating_sub(1);
        self.boundaries[index].1
    }

    pub(crate) fn display_to_buffer(&self, offset: usize) -> usize {
        let index = self
            .boundaries
            .partition_point(|(_, display)| *display <= offset)
            .saturating_sub(1);
        self.boundaries[index].0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_grapheme_projection_maps_unicode_boundaries_without_raw_text() {
        let source = "aë👨‍👩‍👧‍👦z";
        let projection = EditableTextDisplayTransform::per_grapheme("•")
            .project(source)
            .expect("per-grapheme projection");

        assert_eq!(projection.text(), "••••");
        assert!(!projection.text().contains(source));
        for (buffer, display) in [(0, 0), (1, 3), (3, 6), (28, 9), (29, 12)] {
            assert_eq!(projection.buffer_to_display(buffer), display);
            assert_eq!(projection.display_to_buffer(display), buffer);
        }
    }

    #[test]
    fn per_grapheme_projection_preserves_newline_boundaries() {
        let projection = EditableTextDisplayTransform::per_grapheme("*")
            .project("ab\n中")
            .expect("per-grapheme projection");

        assert_eq!(projection.text(), "**\n*");
        assert_eq!(projection.buffer_to_display(3), 3);
        assert_eq!(projection.display_to_buffer(3), 3);
    }

    #[test]
    fn omitted_range_projection_collapses_to_the_following_visible_boundary() {
        let source = "start\none\ntwo\nend";
        let start = source.find("one").unwrap();
        let end = source.find("end").unwrap();
        let transform = EditableTextDisplayTransform::omitting_ranges(source, [start..end])
            .expect("line-aligned omission is valid");
        let projection = transform
            .project(source)
            .expect("matching source projection");

        assert_eq!(projection.text(), "start\nend");
        assert_eq!(projection.buffer_to_display(start + 2), start);
        assert_eq!(projection.display_to_buffer(start), end);
    }
}
