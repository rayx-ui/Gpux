use std::ops::Range;

use crate::{Bounds, Pixels, Point, ShapedLine, point};

/// Revision-bound geometry for a shaped editable-text line.
#[derive(Clone, Debug)]
pub struct EditableTextGeometry {
    revision: u64,
    bounds: Bounds<Pixels>,
    line: ShapedLine,
}

#[allow(missing_docs)]
impl EditableTextGeometry {
    pub fn new(revision: u64, bounds: Bounds<Pixels>, line: ShapedLine) -> Self {
        Self {
            revision,
            bounds,
            line,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    pub fn index_for_point(&self, position: Point<Pixels>) -> usize {
        if self.line.text.is_empty() || position.y < self.bounds.top() {
            return 0;
        }
        if position.y > self.bounds.bottom() {
            return self.line.len();
        }
        self.line
            .closest_index_for_x(position.x - self.bounds.left())
    }

    pub fn bounds_for_range(&self, range: Range<usize>) -> Bounds<Pixels> {
        Bounds::from_corners(
            point(
                self.bounds.left() + self.line.x_for_index(range.start),
                self.bounds.top(),
            ),
            point(
                self.bounds.left() + self.line.x_for_index(range.end),
                self.bounds.bottom(),
            ),
        )
    }
}
