// Neutral virtualization math adapted from gpui-component PR 2677 at c7b6f240 (Apache-2.0).

use std::ops::Range;

use crate::{
    Along, Axis, Bounds, DeferredScrollToItem, Half, Pixels, Point, ScrollStrategy, Size, px,
};

/// Resolves one item into the offset required by a programmatic scroll request.
fn programmatic_item_target(
    axis: Axis,
    scroll_offset: Point<Pixels>,
    bounds: Bounds<Pixels>,
    content_bounds: Bounds<Pixels>,
    strategy: ScrollStrategy,
) -> Point<Pixels> {
    let mut target = scroll_offset;
    match strategy {
        ScrollStrategy::Top => match axis {
            Axis::Horizontal => target.x = content_bounds.left() - bounds.left(),
            Axis::Vertical => target.y = content_bounds.top() - bounds.top(),
        },
        ScrollStrategy::Center => match axis {
            Axis::Horizontal => {
                target.x = content_bounds.left() + content_bounds.size.width.half()
                    - bounds.left()
                    - bounds.size.width.half();
            }
            Axis::Vertical => {
                target.y = content_bounds.top() + content_bounds.size.height.half()
                    - bounds.top()
                    - bounds.size.height.half();
            }
        },
        ScrollStrategy::Bottom => match axis {
            Axis::Horizontal => target.x = content_bounds.right() - bounds.right(),
            Axis::Vertical => target.y = content_bounds.bottom() - bounds.bottom(),
        },
        ScrollStrategy::Nearest => match axis {
            Axis::Horizontal => {
                if bounds.left() + scroll_offset.x < content_bounds.left() {
                    target.x = content_bounds.left() - bounds.left();
                } else if bounds.right() + scroll_offset.x > content_bounds.right() {
                    target.x = content_bounds.right() - bounds.right();
                }
            }
            Axis::Vertical => {
                if bounds.top() + scroll_offset.y < content_bounds.top() {
                    target.y = content_bounds.top() - bounds.top();
                } else if bounds.bottom() + scroll_offset.y > content_bounds.bottom() {
                    target.y = content_bounds.bottom() - bounds.bottom();
                }
            }
        },
    }
    target
}

/// Resolves a deferred item request without accepting an out-of-range target.
pub(super) fn programmatic_item_target_for_request(
    axis: Axis,
    scroll_offset: Point<Pixels>,
    items_bounds: &[Bounds<Pixels>],
    content_bounds: Bounds<Pixels>,
    request: DeferredScrollToItem,
) -> Option<Point<Pixels>> {
    let item_index = request.item_index.checked_add(request.offset)?;
    let bounds = *items_bounds.get(item_index)?;
    Some(programmatic_item_target(
        axis,
        scroll_offset,
        bounds,
        content_bounds,
        request.strategy,
    ))
}

/// Bounded work counters for a variable-extent visible-range lookup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RangeLookupWork {
    /// The number of items the lookup searched.
    pub items_count: usize,
    /// The linear probes the lookup made.
    pub linear_probes: usize,
    /// The binary searches the lookup made.
    pub logarithmic_searches: usize,
    /// The items in the mounted range, overscan included.
    pub cache_range_items: usize,
    /// The active sticky items rendered outside the mounted range.
    pub pinned_items: usize,
    /// The items rendered: the mounted range and the pinned sticky items.
    pub rendered_items: usize,
}

/// Cumulative extent index for variable-size virtual items.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrderedExtentIndex {
    ends: Vec<Pixels>,
}

impl OrderedExtentIndex {
    /// Builds the index from each item's extent, in order.
    pub fn from_extents(extents: impl IntoIterator<Item = Pixels>) -> Self {
        let mut total = px(0.0);
        Self {
            ends: extents
                .into_iter()
                .map(|extent| {
                    total += extent;
                    total
                })
                .collect(),
        }
    }

    /// Rebuilds the index from each item's extent, in order.
    pub fn rebuild(&mut self, extents: impl IntoIterator<Item = Pixels>) {
        *self = Self::from_extents(extents);
    }

    /// The sum of every extent.
    pub fn total(&self) -> Pixels {
        self.ends.last().copied().unwrap_or(px(0.0))
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// Whether the index has no items.
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The start of the item at `index`: the sum of the extents before it.
    pub fn origin(&self, index: usize) -> Pixels {
        index
            .checked_sub(1)
            .and_then(|previous| self.ends.get(previous).copied())
            .unwrap_or(px(0.0))
    }

    /// The items that intersect `viewport_start..viewport_end`, with two items of lookahead
    /// past the end, and the work the lookup did.
    #[inline]
    pub fn visible_range(
        &self,
        viewport_start: Pixels,
        viewport_end: Pixels,
        items_count: usize,
    ) -> (Range<usize>, RangeLookupWork) {
        self.visible_range_with_lookahead(viewport_start, viewport_end, items_count, 2)
    }

    /// The items that intersect `viewport_start..viewport_end`, with `lookahead` items past the
    /// end, and the work the lookup did.
    #[inline]
    pub fn visible_range_with_lookahead(
        &self,
        viewport_start: Pixels,
        viewport_end: Pixels,
        items_count: usize,
        lookahead: usize,
    ) -> (Range<usize>, RangeLookupWork) {
        let items_count = items_count.min(self.ends.len());
        let mut work = RangeLookupWork {
            items_count,
            ..Default::default()
        };
        if items_count == 0 || viewport_end <= px(0.0) || viewport_start >= viewport_end {
            return (0..0, work);
        }
        let viewport_start = viewport_start.max(px(0.0));
        let ends = &self.ends[..items_count];
        let first = ends.partition_point(|end| *end <= viewport_start);
        work.logarithmic_searches += 1;
        if first == items_count {
            return (items_count..items_count, work);
        }
        let boundary = ends.partition_point(|end| *end <= viewport_end);
        work.logarithmic_searches += 1;
        let last = if boundary == items_count {
            items_count
        } else {
            boundary.saturating_add(lookahead).min(items_count)
        };
        (first..last, work)
    }
}

/// Adds configured item overscan to an exact mounted range.
#[inline]
pub(super) fn overscanned_range(exact: Range<usize>, overscan: usize, len: usize) -> Range<usize> {
    exact.start.saturating_sub(overscan)..exact.end.saturating_add(overscan).min(len)
}

/// Mapping between a parent-owned viewport and list-local scroll coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ParentViewportMetrics {
    pub axis: Axis,
    pub viewport_bounds: Bounds<Pixels>,
    pub content_bounds: Bounds<Pixels>,
    pub content_size: Size<Pixels>,
    pub external_scroll_offset: Point<Pixels>,
}

impl ParentViewportMetrics {
    pub fn primary_scroll_offset(self) -> Pixels {
        (self.viewport_start() - self.content_start())
            .clamp(px(0.0), self.max_primary_scroll_offset())
    }
    pub fn viewport_primary_size(self) -> Pixels {
        self.viewport_bounds.size.along(self.axis)
    }
    pub fn max_primary_scroll_offset(self) -> Pixels {
        px(
            (self.content_size.along(self.axis) - self.viewport_primary_size())
                .as_f32()
                .max(0.0),
        )
    }
    pub fn external_offset_for_primary_scroll_offset(self, offset: Pixels) -> Point<Pixels> {
        let offset = offset.clamp(px(0.0), self.max_primary_scroll_offset());
        let mut next = self.external_scroll_offset;
        let document_start = self.content_start() - self.external_scroll_offset.along(self.axis);
        let primary = self.viewport_start() - document_start - offset;
        match self.axis {
            Axis::Horizontal => next.x = primary,
            Axis::Vertical => next.y = primary,
        }
        next
    }
    fn viewport_start(self) -> Pixels {
        match self.axis {
            Axis::Horizontal => self.viewport_bounds.left(),
            Axis::Vertical => self.viewport_bounds.top(),
        }
    }
    fn content_start(self) -> Pixels {
        match self.axis {
            Axis::Horizontal => self.content_bounds.left(),
            Axis::Vertical => self.content_bounds.top(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, point, size};

    #[test]
    fn scroll_virtual_tree_lookup_stays_bounded_at_scale() {
        for len in [1_000, 10_000, 100_000] {
            let index = OrderedExtentIndex::from_extents(
                (0..len).map(|index| px(if index % 7 == 0 { 13.0 } else { 9.0 })),
            );
            let end = index.total();
            let (range, work) = index.visible_range(end - px(180.0), end, len);
            assert!(!range.is_empty());
            assert_eq!(work.linear_probes, 0);
            assert_eq!(work.logarithmic_searches, 2);
            assert!(overscanned_range(range.clone(), 3, len).len() <= range.len() + 6);
        }
    }

    #[test]
    fn scroll_virtual_tree_parent_viewport_maps_list_local_offset() {
        let metrics = ParentViewportMetrics {
            axis: Axis::Vertical,
            viewport_bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(300.0), px(100.0))),
            content_bounds: Bounds::new(point(px(0.0), px(-50.0)), size(px(300.0), px(1_000.0))),
            content_size: size(px(300.0), px(1_000.0)),
            external_scroll_offset: point(px(0.0), px(-250.0)),
        };
        assert_eq!(metrics.primary_scroll_offset(), px(50.0));
        assert_eq!(
            metrics
                .external_offset_for_primary_scroll_offset(px(120.0))
                .y,
            px(-320.0)
        );
    }

    #[test]
    fn accessibility_virtual_scroll_target_rejects_invalid_index() {
        let request = DeferredScrollToItem {
            item_index: usize::MAX,
            offset: 1,
            strategy: ScrollStrategy::Nearest,
            scroll_strict: false,
        };
        assert_eq!(
            programmatic_item_target_for_request(
                Axis::Vertical,
                point(px(0.0), px(0.0)),
                &[],
                Bounds::new(point(px(0.0), px(0.0)), size(px(100.0), px(100.0))),
                request,
            ),
            None
        );
    }
}
