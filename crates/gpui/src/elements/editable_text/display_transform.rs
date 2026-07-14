use std::sync::Arc;

use crate::SharedString;
use unicode_segmentation::UnicodeSegmentation as _;

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
}

impl EditableTextDisplayTransform {
    /// Replaces every non-newline grapheme with the supplied display text.
    pub fn per_grapheme(replacement: impl Into<SharedString>) -> Self {
        Self::PerGrapheme(replacement.into())
    }

    pub(crate) fn project(&self, source: &str) -> Option<EditableTextProjection> {
        let Self::PerGrapheme(replacement) = self else {
            return None;
        };
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
}

#[derive(Clone, Debug)]
pub(crate) struct EditableTextProjection {
    text: SharedString,
    boundaries: Arc<[(usize, usize)]>,
}

impl EditableTextProjection {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn buffer_to_display(&self, offset: usize) -> usize {
        match self
            .boundaries
            .binary_search_by_key(&offset, |(buffer, _)| *buffer)
        {
            Ok(index) => self.boundaries[index].1,
            Err(index) => self.boundaries[index.saturating_sub(1)].1,
        }
    }

    pub(crate) fn display_to_buffer(&self, offset: usize) -> usize {
        match self
            .boundaries
            .binary_search_by_key(&offset, |(_, display)| *display)
        {
            Ok(index) => self.boundaries[index].0,
            Err(index) => self.boundaries[index.saturating_sub(1)].0,
        }
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
}
