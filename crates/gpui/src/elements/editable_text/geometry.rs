use std::ops::Range;

use crate::{Bounds, Pixels, Point, TextAlign, point, px, size};

use super::{TextDisplayPoint, TextLayoutMap};

/// Revision-bound viewport geometry for editable text.
#[derive(Clone, Debug)]
pub struct EditableTextGeometry {
    revision: u64,
    bounds: Bounds<Pixels>,
    scroll_offset: Point<Pixels>,
    alignment_offset: Pixels,
}

/// Resolve the horizontal offset used by editable text paint and interaction
/// geometry for a physical text alignment.
#[doc(hidden)]
pub fn editable_text_alignment_offset(
    alignment: TextAlign,
    viewport_width: Pixels,
    content_width: Pixels,
) -> Pixels {
    match alignment {
        TextAlign::Left => px(0.),
        TextAlign::Center => (viewport_width - content_width) / 2.,
        TextAlign::Right => viewport_width - content_width,
    }
}

#[allow(missing_docs)]
impl EditableTextGeometry {
    pub fn new(
        revision: u64,
        bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        alignment_offset: Pixels,
    ) -> Self {
        Self {
            revision,
            bounds,
            scroll_offset,
            alignment_offset,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    pub fn scroll_offset(&self) -> Point<Pixels> {
        self.scroll_offset
    }

    pub fn alignment_offset(&self) -> Pixels {
        self.alignment_offset
    }

    pub fn index_for_point(&self, map: &TextLayoutMap, position: Point<Pixels>) -> usize {
        if map.visual_line_count() == 0 {
            return 0;
        }
        let content = position
            - self.bounds.origin
            - self.scroll_offset
            - point(self.alignment_offset, px(0.));
        if content.y < px(0.) {
            return 0;
        }
        if content.y >= map.size().height {
            return map
                .visual_row_range(map.visual_line_count() - 1)
                .map_or(0, |range| range.end);
        }
        map.index_for_position(content).unwrap_or(0)
    }

    pub fn bounds_for_range(&self, map: &TextLayoutMap, range: Range<usize>) -> Bounds<Pixels> {
        let origin = self.bounds.origin + self.scroll_offset + point(self.alignment_offset, px(0.));
        let Some(start) = map.buffer_to_display(range.start) else {
            return Bounds::new(origin, size(px(0.), map.line_height()));
        };
        let Some(end) = map.buffer_to_display(range.end) else {
            return Bounds::new(origin, size(px(0.), map.line_height()));
        };
        if range.is_empty() {
            let caret = map
                .caret_bounds(range.start)
                .unwrap_or_else(|| Bounds::new(Point::default(), size(px(0.), map.line_height())));
            return Bounds::new(origin + caret.origin, caret.size);
        }

        let mut left = Pixels::MAX;
        let mut right = -Pixels::MAX;
        for row in start.row..=end.row {
            let Some(row_range) = map.visual_row_range(row) else {
                continue;
            };
            let segment_start = range.start.max(row_range.start).min(row_range.end);
            let segment_end = range.end.min(row_range.end).max(row_range.start);
            let start_column = segment_start.saturating_sub(row_range.start);
            let end_column = segment_end.saturating_sub(row_range.start);
            let start_x = map
                .position_for_display(TextDisplayPoint::new(row, start_column))
                .map_or(px(0.), |position| position.x);
            let end_x = map
                .position_for_display(TextDisplayPoint::new(row, end_column))
                .map_or(start_x, |position| position.x);
            left = left.min(start_x);
            right = right.max(end_x.max(start_x));
        }
        if left == Pixels::MAX {
            left = px(0.);
            right = px(0.);
        }
        let top = map.visual_row(start.row).map_or(px(0.), |row| row.top);
        let bottom = map
            .visual_row(end.row)
            .map_or(top, |row| row.top + row.height);
        Bounds::from_corners(origin + point(left, top), origin + point(right, bottom))
    }
}
