//! A virtual list renders a large number of rows or columns of different sizes.
//!
//! Only the visible range is rendered. Every item's size along the list's axis must be known up
//! front.
//!
//! Inspired by [`uniform_list`](crate::uniform_list), from
//! <https://github.com/zed-industries/zed/blob/0ae1603610ab6b265bdfbee7b8dbc23c5ab06edc/crates/gpui/src/elements/uniform_list.rs>
//! Unlike `uniform_list`, each item can have a different size, which suits more complex layouts
//! such as a table with rows of different heights.
//!
//! Moved from RayX's `rayx_components_base::virtual_list` (`VirtualListBase`) at RayX
//! `4ee806e9`.
use std::{
    cell::RefCell,
    cmp,
    ops::{Deref, Range},
    rc::Rc,
};

mod geometry;

use crate::{
    Along, AnyElement, App, AvailableSpace, Axis, Bounds, ContentMask, Context,
    DeferredScrollToItem, Div, Element, ElementId, Entity, GlobalElementId, Half, Hitbox,
    InteractiveElement, IntoElement, IsZero as _, ListSizingBehavior, Overflow, ParentElement,
    Pixels, Point, Render, ScrollBehavior, ScrollContainerContext, ScrollHandle,
    ScrollMutationSource, ScrollStrategy, Size, Stateful, StatefulInteractiveElement,
    StickyRelationIndex, StickyStacking, StickyStateChangedEvent, Style, StyleRefinement, Styled,
    Window, div, point, px, size, sticky_push_offset,
};
pub use geometry::{OrderedExtentIndex, RangeLookupWork};
use geometry::{ParentViewportMetrics, overscanned_range, programmatic_item_target_for_request};

/// Called when a sticky row of a [`VirtualList`] sticks or unsticks.
pub type VirtualStickyStateChangedListener =
    Rc<dyn Fn(&StickyStateChangedEvent, &mut Window, &mut App) + 'static>;

/// The edge a sticky row of a [`VirtualList`] sticks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtualStickyEdge {
    /// The top edge, for a vertical list.
    Top,
    /// The left edge, for a horizontal list.
    Left,
}

impl VirtualStickyEdge {
    /// The axis along which a row sticking to this edge scrolls.
    pub fn axis(self) -> Axis {
        match self {
            Self::Top => Axis::Vertical,
            Self::Left => Axis::Horizontal,
        }
    }
}

/// How one row of a [`VirtualList`] sticks: its edge, stacking relation, inset and stuck style.
#[derive(Clone)]
pub struct VirtualStickyRow {
    /// The edge the row sticks to.
    pub edge: VirtualStickyEdge,
    /// How the row stacks with the other sticky rows.
    pub relation: StickyStacking,
    /// The distance from the edge at which the row sticks.
    pub inset: Pixels,
    /// The row's extent in the sticky stack, when it differs from its item size.
    pub measured_size: Option<Pixels>,
    /// The style applied to the row while it is stuck.
    pub stuck_style: Option<StyleRefinement>,
    /// Called when the row sticks or unsticks.
    pub sticky_state_changed_listener: Option<VirtualStickyStateChangedListener>,
}

impl VirtualStickyRow {
    /// A row that sticks to the top edge.
    pub fn top() -> Self {
        Self::new(VirtualStickyEdge::Top)
    }
    /// A row that sticks to the left edge.
    pub fn left() -> Self {
        Self::new(VirtualStickyEdge::Left)
    }
    /// A level-0 row that sticks to `edge` with no inset.
    pub fn new(edge: VirtualStickyEdge) -> Self {
        Self {
            edge,
            relation: StickyStacking::Level(0),
            inset: px(0.0),
            measured_size: None,
            stuck_style: None,
            sticky_state_changed_listener: None,
        }
    }
    /// Stacks the row at an explicit hierarchy level.
    pub fn level(mut self, level: u16) -> Self {
        self.relation = StickyStacking::Level(level);
        self
    }
    /// Stacks the row in the contiguous keyed run `id`.
    pub fn stack_id(mut self, id: impl Into<ElementId>) -> Self {
        self.relation = StickyStacking::StackId(id.into());
        self
    }
    /// Sets the distance from the edge at which the row sticks.
    pub fn inset(mut self, inset: Pixels) -> Self {
        self.inset = inset;
        self
    }
    /// Overrides the row's extent in the sticky stack.
    pub fn measured_size(mut self, measured_size: Pixels) -> Self {
        self.measured_size = Some(measured_size);
        self
    }
    /// Sets the style applied to the row while it is stuck.
    pub fn when_stuck(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.stuck_style = Some(f(StyleRefinement::default()));
        self
    }
    /// Calls `listener` when the row sticks or unsticks.
    pub fn on_sticky_state_changed(
        mut self,
        listener: impl Fn(&StickyStateChangedEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.sticky_state_changed_listener = Some(Rc::new(listener));
        self
    }
    /// The axis along which the row scrolls.
    pub fn axis(&self) -> Axis {
        self.edge.axis()
    }
}

/// How a [`VirtualList`] should interpret a scroll handle owned by another
/// element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VirtualListExternalScrollBehavior {
    /// Paint items by applying the external handle's offset inside this list.
    ///
    /// Use this when another element owns the visible viewport but the virtual
    /// list is still positioned inside that viewport, such as a horizontally
    /// synchronized data table row.
    #[default]
    SynchronizedOffset,
    /// Let the parent viewport move this list and use the external handle only
    /// to discover which items intersect that viewport.
    ///
    /// Use this when the virtual list is part of normal page layout and must
    /// contribute its full virtual content extent to the parent's scroll size.
    ParentViewport,
}

struct VirtualListScrollHandleState {
    axis: Axis,
    items_count: usize,
    parent_viewport_metrics: Option<ParentViewportMetrics>,
    deferred_parent_viewport_scroll_offset: Option<Pixels>,
    pub deferred_scroll_to_item: Option<DeferredScrollToItem>,
}

/// A scroll handle for [`VirtualList`].
///
/// See also [`ScrollHandle`].
#[derive(Clone)]
pub struct VirtualListScrollHandle {
    state: Rc<RefCell<VirtualListScrollHandleState>>,
    base_handle: ScrollHandle,
}

impl From<ScrollHandle> for VirtualListScrollHandle {
    fn from(handle: ScrollHandle) -> Self {
        let mut this = VirtualListScrollHandle::new();
        this.base_handle = handle;
        this
    }
}

impl AsRef<ScrollHandle> for VirtualListScrollHandle {
    fn as_ref(&self) -> &ScrollHandle {
        &self.base_handle
    }
}

impl Deref for VirtualListScrollHandle {
    type Target = ScrollHandle;

    fn deref(&self) -> &Self::Target {
        &self.base_handle
    }
}

impl VirtualListScrollHandle {
    /// Create a new VirtualListScrollHandle.
    pub fn new() -> Self {
        VirtualListScrollHandle {
            state: Rc::new(RefCell::new(VirtualListScrollHandleState {
                axis: Axis::Vertical,
                items_count: 0,
                parent_viewport_metrics: None,
                deferred_parent_viewport_scroll_offset: None,
                deferred_scroll_to_item: None,
            })),
            base_handle: ScrollHandle::default(),
        }
    }

    /// Get the base scroll handle.
    pub fn base_handle(&self) -> &ScrollHandle {
        &self.base_handle
    }

    /// Scroll to the item at the given index.
    pub fn scroll_to_item(&self, ix: usize, strategy: ScrollStrategy) {
        self.scroll_to_item_with_offset(ix, strategy, 0);
    }

    /// Scroll to the item at the given index, with an additional offset items.
    fn scroll_to_item_with_offset(&self, ix: usize, strategy: ScrollStrategy, offset: usize) {
        let mut state = self.state.borrow_mut();
        state.deferred_scroll_to_item = Some(DeferredScrollToItem {
            item_index: ix,
            strategy,
            offset,
            scroll_strict: false,
        });
    }

    /// Scrolls to the bottom of the list.
    pub fn scroll_to_bottom(&self) {
        let items_count = self.state.borrow().items_count;
        self.scroll_to_item(items_count.saturating_sub(1), ScrollStrategy::Bottom);
    }

    /// The list-local scroll offset along the list's axis that the parent viewport shows, once a
    /// `ParentViewport` list has been laid out.
    pub fn parent_viewport_scroll_offset(&self) -> Option<Pixels> {
        self.state
            .borrow()
            .parent_viewport_metrics
            .map(|metrics| metrics.primary_scroll_offset())
    }

    /// The parent viewport's size along the list's axis, once a `ParentViewport` list has been
    /// laid out.
    pub fn parent_viewport_primary_size(&self) -> Option<Pixels> {
        self.state
            .borrow()
            .parent_viewport_metrics
            .map(|metrics| metrics.viewport_primary_size())
    }

    /// The largest list-local scroll offset the parent viewport can reach, once a
    /// `ParentViewport` list has been laid out.
    pub fn parent_viewport_max_scroll_offset(&self) -> Option<Pixels> {
        self.state
            .borrow()
            .parent_viewport_metrics
            .map(|metrics| metrics.max_primary_scroll_offset())
    }

    /// The parent handle's offset that shows the list-local scroll offset `offset`.
    pub fn parent_viewport_external_offset_for_primary_scroll_offset(
        &self,
        offset: Pixels,
    ) -> Option<Point<Pixels>> {
        self.state
            .borrow()
            .parent_viewport_metrics
            .map(|metrics| metrics.external_offset_for_primary_scroll_offset(offset))
    }

    /// Scrolls the parent viewport to the list-local offset `offset`, or defers the request until
    /// the list has recorded the parent's metrics.
    pub fn set_parent_viewport_scroll_offset(&self, offset: Pixels) {
        let next_offset = self.parent_viewport_external_offset_for_primary_scroll_offset(offset);
        if next_offset.is_none() {
            self.state
                .borrow_mut()
                .deferred_parent_viewport_scroll_offset = Some(offset);
        }
        let Some(next_offset) = next_offset else {
            return;
        };

        self.base_handle
            .request_offset(next_offset, ScrollMutationSource::Programmatic);
    }

    /// Records the parent viewport's metrics, returning a deferred list-local offset request.
    pub fn update_parent_viewport_metrics(
        &self,
        axis: Axis,
        viewport_bounds: Bounds<Pixels>,
        content_bounds: Bounds<Pixels>,
        content_size: Size<Pixels>,
        external_scroll_offset: Point<Pixels>,
    ) -> Option<Pixels> {
        let mut state = self.state.borrow_mut();
        state.parent_viewport_metrics = Some(ParentViewportMetrics {
            axis,
            viewport_bounds,
            content_bounds,
            content_size,
            external_scroll_offset,
        });
        state.deferred_parent_viewport_scroll_offset.take()
    }

    fn clear_parent_viewport_metrics(&self) {
        let mut state = self.state.borrow_mut();
        state.parent_viewport_metrics = None;
        state.deferred_parent_viewport_scroll_offset = None;
    }
}

impl Default for VirtualListScrollHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a [`VirtualList`] in vertical direction.
///
/// This is like `uniform_list` in GPUI, but support two axis.
///
/// The `item_sizes` is the size of each column,
/// only the `height` is used, `width` is ignored and VirtualList will measure the first item width.
///
/// See also [`h_virtual_list`]
#[inline]
pub fn v_virtual_list<R, V>(
    view: Entity<V>,
    id: impl Into<ElementId>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    f: impl 'static + Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<R>,
) -> VirtualList
where
    R: IntoElement,
    V: Render,
{
    virtual_list(view, id, Axis::Vertical, item_sizes, f)
}

/// Create a [`VirtualList`] in horizontal direction.
///
/// The `item_sizes` is the size of each column,
/// only the `width` is used, `height` is ignored and VirtualList will measure the first item height.
///
/// See also [`v_virtual_list`]
#[inline]
pub fn h_virtual_list<R, V>(
    view: Entity<V>,
    id: impl Into<ElementId>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    f: impl 'static + Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<R>,
) -> VirtualList
where
    R: IntoElement,
    V: Render,
{
    virtual_list(view, id, Axis::Horizontal, item_sizes, f)
}

/// Create a [`VirtualList`] along `axis`.
///
/// The `item_sizes` give each item's size; only the size along `axis` is used.
///
/// See also [`v_virtual_list`] and [`h_virtual_list`].
pub fn virtual_list<R, V>(
    view: Entity<V>,
    id: impl Into<ElementId>,
    axis: Axis,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    f: impl 'static + Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<R>,
) -> VirtualList
where
    R: IntoElement,
    V: Render,
{
    let id: ElementId = id.into();
    // Rendering rows leases the view holding the list as part of drawing it, not as a write
    // (see `fast::layers::lists::render_items`).
    let render_range = move |visible_range, window: &mut Window, cx: &mut App| {
        crate::fast::layers::lists::render_items(window, cx, |window, cx| {
            view.update(cx, |this, cx| {
                f(this, visible_range, window, cx)
                    .into_iter()
                    .map(|component| component.into_any_element())
                    .collect()
            })
        })
    };

    VirtualList {
        id: id.clone(),
        axis,
        base: div().id(id).size_full().overflow_scroll(),
        // Replaced in `request_layout` by the handle the element state keeps, unless the
        // caller tracks one.
        scroll_handle: VirtualListScrollHandle::new(),
        default_handle: true,
        items_count: item_sizes.len(),
        item_sizes,
        render_items: Box::new(render_range),
        sticky_items: VirtualStickyItems::default(),
        sizing_behavior: ListSizingBehavior::default(),
        external_scroll_behavior: None,
        overscan: 0,
        fixed_cross_axis_size: None,
        rendered_range_listener: None,
        #[cfg(feature = "diagnostics")]
        range_lookup_listener: None,
        programmatic_scroll: ScrollBehavior::Instant,
    }
}

/// VirtualList component for rendering a large number of differently sized items.
pub struct VirtualList {
    pub(super) id: ElementId,
    pub(super) axis: Axis,
    base: Stateful<Div>,
    scroll_handle: VirtualListScrollHandle,
    /// Whether the list scrolls with the handle its element state keeps, because the caller
    /// tracks none.
    default_handle: bool,
    pub(super) items_count: usize,
    pub(super) item_sizes: Rc<Vec<Size<Pixels>>>,
    render_items: Box<RenderVirtualItems>,
    pub(super) sticky_items: VirtualStickyItems,
    pub(super) sizing_behavior: ListSizingBehavior,
    pub(super) external_scroll_behavior: Option<VirtualListExternalScrollBehavior>,
    overscan: usize,
    fixed_cross_axis_size: Option<Pixels>,
    rendered_range_listener: Option<RenderedRangeListener>,
    programmatic_scroll: ScrollBehavior,
    #[cfg(feature = "diagnostics")]
    range_lookup_listener: Option<RangeLookupListener>,
}

type RenderedRangeListener = Rc<dyn Fn(Range<usize>, &mut Window, &mut App)>;
type RenderVirtualItems =
    dyn for<'a> Fn(Range<usize>, &'a mut Window, &'a mut App) -> Vec<AnyElement>;

#[cfg(feature = "diagnostics")]
type RangeLookupListener = Rc<dyn Fn(VirtualListRangeLookupDiagnostics)>;

/// Bounded work counters for the latest ordered visible-range lookup.
#[cfg(feature = "diagnostics")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtualListRangeLookupDiagnostics {
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

#[cfg(feature = "diagnostics")]
impl From<RangeLookupWork> for VirtualListRangeLookupDiagnostics {
    fn from(work: RangeLookupWork) -> Self {
        Self {
            items_count: work.items_count,
            linear_probes: work.linear_probes,
            logarithmic_searches: work.logarithmic_searches,
            cache_range_items: work.cache_range_items,
            pinned_items: work.pinned_items,
            rendered_items: work.rendered_items,
        }
    }
}

/// A sticky candidate of a [`VirtualList`]: an item index and how it sticks.
#[derive(Clone)]
pub struct VirtualStickyItem {
    /// The item's index in the list.
    pub visual_index: usize,
    /// The edge the item sticks to.
    pub edge: VirtualStickyEdge,
    /// How the item stacks with the other sticky items.
    pub relation: StickyStacking,
    /// The distance from the edge at which the item sticks.
    pub inset: Pixels,
    /// The item's extent in the sticky stack.
    pub measured_size: Pixels,
    explicit_measured_size: Option<Pixels>,
    /// The style applied to the item while it is stuck.
    pub stuck_style: Option<StyleRefinement>,
    /// Called when the item sticks or unsticks.
    pub sticky_state_changed_listener: Option<VirtualStickyStateChangedListener>,
}

impl VirtualStickyItem {
    /// The candidate at `visual_index` sticking as `row` says, with `measured_size` as its stack
    /// extent unless `row` overrides it.
    pub fn new(visual_index: usize, row: VirtualStickyRow, measured_size: Pixels) -> Self {
        let explicit_measured_size = row.measured_size;
        Self {
            visual_index,
            edge: row.edge,
            relation: row.relation,
            inset: row.inset,
            measured_size: explicit_measured_size.unwrap_or(measured_size),
            explicit_measured_size,
            stuck_style: row.stuck_style,
            sticky_state_changed_listener: row.sticky_state_changed_listener,
        }
    }

    pub(crate) fn axis(&self) -> Axis {
        self.edge.axis()
    }

    fn stack_size(&self, item_extents: &[Pixels]) -> Pixels {
        self.explicit_measured_size
            .or_else(|| item_extents.get(self.visual_index).copied())
            .unwrap_or(self.measured_size)
    }
}

/// A sticky item that is stuck in a [`VirtualList`]'s active stack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveVirtualStickyItem {
    /// The item's index in the list.
    pub visual_index: usize,
    /// The edge the item sticks to.
    pub edge: VirtualStickyEdge,
    /// The item's level in the stack.
    pub level: usize,
    /// The distance from the edge at which the item sticks.
    pub inset: Pixels,
    /// The item's extent in the stack.
    pub measured_size: Pixels,
    /// The item's offset from the edge.
    pub offset: Pixels,
}

impl ActiveVirtualStickyItem {
    fn new(item: &VirtualStickyItem, level: usize, offset: Pixels, measured_size: Pixels) -> Self {
        Self {
            visual_index: item.visual_index,
            edge: item.edge,
            level,
            inset: item.inset,
            measured_size,
            offset,
        }
    }
}

#[derive(Clone, Default)]
struct VirtualStickyRelationIndex {
    relation_index: StickyRelationIndex,
    item_indices: Vec<usize>,
}

impl VirtualStickyRelationIndex {
    fn from_items(items: &[VirtualStickyItem], axis: Axis) -> Self {
        let item_indices = items
            .iter()
            .enumerate()
            .filter_map(|(item_index, item)| (item.axis() == axis).then_some(item_index))
            .collect::<Vec<_>>();
        let relation_index =
            StickyRelationIndex::from_candidates(item_indices.iter().map(|item_index| {
                let item = &items[*item_index];
                (item.visual_index, item.relation.clone())
            }));
        Self {
            relation_index,
            item_indices,
        }
    }

    fn active_item_indices_at_or_before(&self, visual_index: usize) -> Vec<usize> {
        let Some(source) = self.relation_index.source_at_or_before(visual_index) else {
            return Vec::new();
        };
        self.relation_index
            .active_chain(source)
            .into_iter()
            .map(|relation_index| self.item_indices[relation_index])
            .collect()
    }

    fn active_item_indices_for(
        &self,
        visual_index: usize,
        items: &[VirtualStickyItem],
    ) -> Vec<usize> {
        let active = self.active_item_indices_at_or_before(visual_index);
        if active
            .last()
            .is_some_and(|item_index| items[*item_index].visual_index == visual_index)
        {
            active
        } else {
            Vec::new()
        }
    }
}

/// The sticky candidates of a [`VirtualList`], sorted, with their relation indices.
#[derive(Clone, Default)]
pub struct VirtualStickyItems {
    items: Rc<Vec<VirtualStickyItem>>,
    vertical_relation: Rc<VirtualStickyRelationIndex>,
    horizontal_relation: Rc<VirtualStickyRelationIndex>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StickyRenderLayer {
    level: usize,
    incoming: bool,
}

fn compare_sticky_render_layers(lhs: StickyRenderLayer, rhs: StickyRenderLayer) -> cmp::Ordering {
    rhs.level
        .cmp(&lhs.level)
        .then_with(|| lhs.incoming.cmp(&rhs.incoming))
}

fn push_active_sticky_replacement(
    active_sticky_items: &mut [ActiveVirtualStickyItem],
    retained_depth: usize,
    incoming_start: Pixels,
) {
    let active_stack_end = active_sticky_items
        .last()
        .map(|active| active.offset + active.measured_size)
        .unwrap_or(px(0.));
    let push_offset = sticky_push_offset(active_stack_end, incoming_start);
    for active in active_sticky_items.iter_mut().skip(retained_depth) {
        active.offset += push_offset;
    }
}

impl VirtualStickyItems {
    /// Sorts and deduplicates `items` by index and builds their relation indices.
    pub fn from_items(items: impl IntoIterator<Item = VirtualStickyItem>) -> VirtualStickyItems {
        let mut items = items.into_iter().collect::<Vec<_>>();
        items.sort_by_key(|item| item.visual_index);
        items.dedup_by_key(|item| item.visual_index);
        let vertical_relation = VirtualStickyRelationIndex::from_items(&items, Axis::Vertical);
        let horizontal_relation = VirtualStickyRelationIndex::from_items(&items, Axis::Horizontal);
        Self {
            items: Rc::new(items),
            vertical_relation: Rc::new(vertical_relation),
            horizontal_relation: Rc::new(horizontal_relation),
        }
    }

    /// The candidates, sorted by index.
    pub fn items(&self) -> &[VirtualStickyItem] {
        &self.items
    }

    /// The candidates stuck along `axis` while `visible_range` is shown.
    pub fn active_stack(
        &self,
        visible_range: &Range<usize>,
        axis: Axis,
    ) -> Vec<ActiveVirtualStickyItem> {
        self.active_stack_with_item_extents(visible_range, axis, &[])
    }

    pub(crate) fn active_stack_with_item_extents(
        &self,
        visible_range: &Range<usize>,
        axis: Axis,
        item_extents: &[Pixels],
    ) -> Vec<ActiveVirtualStickyItem> {
        let Some(stack_source_visual_index) = visible_range.start.checked_sub(1) else {
            return Vec::new();
        };

        let mut offset = px(0.0);
        self.relation(axis)
            .active_item_indices_at_or_before(stack_source_visual_index)
            .into_iter()
            .enumerate()
            .map(|(depth, item_ix)| {
                let item = &self.items[item_ix];
                let active_offset = item.inset + offset;
                let stack_size = item.stack_size(item_extents);
                offset = active_offset + stack_size;
                let level = match &item.relation {
                    StickyStacking::Level(level) => *level as usize,
                    StickyStacking::StackId(_) => depth,
                };
                ActiveVirtualStickyItem::new(item, level, active_offset, stack_size)
            })
            .collect()
    }

    pub(crate) fn candidate(&self, visual_index: usize) -> Option<VirtualStickyItem> {
        self.items
            .binary_search_by_key(&visual_index, |item| item.visual_index)
            .ok()
            .map(|ix| self.items[ix].clone())
    }

    pub(crate) fn offset_for_candidate(
        &self,
        candidate: VirtualStickyItem,
        active_stack: &[ActiveVirtualStickyItem],
    ) -> Pixels {
        let parent_visual_index = match &candidate.relation {
            StickyStacking::Level(level) => active_stack
                .iter()
                .rfind(|active| active.edge == candidate.edge && active.level < *level as usize)
                .map(|active| active.visual_index),
            StickyStacking::StackId(_) => {
                let active = self
                    .relation(candidate.axis())
                    .active_item_indices_for(candidate.visual_index, &self.items);
                active
                    .len()
                    .checked_sub(2)
                    .map(|parent| self.items[active[parent]].visual_index)
            }
        };
        let parent_offset = parent_visual_index
            .and_then(|visual_index| {
                active_stack
                    .iter()
                    .find(|active| active.visual_index == visual_index)
            })
            .map(|active| active.offset + active.measured_size)
            .unwrap_or(px(0.0));
        parent_offset + candidate.inset
    }

    fn render_level(&self, candidate: &VirtualStickyItem) -> usize {
        match &candidate.relation {
            StickyStacking::Level(level) => *level as usize,
            StickyStacking::StackId(_) => self
                .relation(candidate.axis())
                .active_item_indices_for(candidate.visual_index, &self.items)
                .len()
                .saturating_sub(1),
        }
    }

    fn next_replacement_after(
        &self,
        visual_index: usize,
        axis: Axis,
    ) -> Option<(&VirtualStickyItem, usize)> {
        let relation = self.relation(axis);
        let source = relation.relation_index.source_at_or_before(visual_index)?;
        let item = &self.items[relation.item_indices[source]];
        if item.visual_index != visual_index {
            return None;
        }
        let (next, retained_depth) = relation.relation_index.next_replacement(source)?;
        let next_item = &self.items[relation.item_indices[next]];
        Some((next_item, retained_depth))
    }

    fn relation(&self, axis: Axis) -> &VirtualStickyRelationIndex {
        match axis {
            Axis::Vertical => &self.vertical_relation,
            Axis::Horizontal => &self.horizontal_relation,
        }
    }
}

impl Styled for VirtualList {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl VirtualList {
    /// Sets the neutral GPUI behavior used for deferred programmatic scrolling.
    pub fn scroll_behavior(mut self, behavior: ScrollBehavior) -> Self {
        self.programmatic_scroll = behavior;
        self
    }

    /// Sets the number of additional items to mount before and after the exact viewport range.
    pub fn overscan(mut self, overscan: usize) -> Self {
        self.overscan = overscan;
        self
    }

    /// Uses an exact cross-axis size instead of rendering the first item to measure it.
    pub fn fixed_cross_axis_size(mut self, size: Pixels) -> Self {
        self.fixed_cross_axis_size = Some(size);
        self
    }

    /// Invokes `listener` once for each prepaint that renders ordinary rows.
    ///
    /// The supplied range is the clamped mounted range, including configured overscan.
    /// Active sticky rows rendered outside that range do not invoke the listener.
    pub fn on_rendered_range(
        mut self,
        listener: impl Fn(Range<usize>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.rendered_range_listener = Some(Rc::new(listener));
        self
    }

    /// Reports bounded work performed by the ordered visible-range lookup.
    #[cfg(feature = "diagnostics")]
    pub fn on_range_lookup(
        mut self,
        listener: impl Fn(VirtualListRangeLookupDiagnostics) + 'static,
    ) -> Self {
        self.range_lookup_listener = Some(Rc::new(listener));
        self
    }

    /// Track the list's own scroll offset with `scroll_handle`.
    pub fn track_scroll(mut self, scroll_handle: &VirtualListScrollHandle) -> Self {
        self.base = self.base.track_scroll(scroll_handle);
        self.scroll_handle = scroll_handle.clone();
        self.default_handle = false;
        self.external_scroll_behavior = None;
        self.scroll_handle.clear_parent_viewport_metrics();
        self
    }

    /// Set the sizing behavior for the list.
    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing_behavior = behavior;
        self
    }

    /// Makes the items at `indices` sticky at level 0, on the start edge of the list's axis.
    pub fn sticky_items(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        let axis = self.axis;
        let sticky_items = indices
            .into_iter()
            .filter(|ix| *ix < self.items_count)
            .map(|ix| {
                let edge = match axis {
                    Axis::Horizontal => VirtualStickyEdge::Left,
                    Axis::Vertical => VirtualStickyEdge::Top,
                };
                let measured_size = self.item_sizes[ix].along(axis);
                VirtualStickyItem::new(ix, VirtualStickyRow::new(edge), measured_size)
            });
        self.sticky_items = VirtualStickyItems::from_items(sticky_items);
        self
    }

    /// Makes each item index sticky as its [`VirtualStickyRow`] says.
    pub fn sticky_rows(
        mut self,
        rows: impl IntoIterator<Item = (usize, VirtualStickyRow)>,
    ) -> Self {
        let axis = self.axis;
        let sticky_items = rows.into_iter().filter_map(|(ix, row)| {
            if ix >= self.items_count {
                return None;
            }

            Some(VirtualStickyItem::new(
                ix,
                row,
                self.item_sizes[ix].along(axis),
            ))
        });
        self.sticky_items = VirtualStickyItems::from_items(sticky_items);
        self
    }

    /// Uses prebuilt sticky candidates.
    pub fn virtual_sticky_items(mut self, sticky_items: VirtualStickyItems) -> Self {
        self.sticky_items = sticky_items;
        self
    }

    /// Track a scroll handle owned by an external element.
    pub fn track_external_scroll(
        mut self,
        scroll_handle: &VirtualListScrollHandle,
        behavior: VirtualListExternalScrollBehavior,
    ) -> Self {
        self.base = match behavior {
            VirtualListExternalScrollBehavior::SynchronizedOffset => {
                div().id(self.id.clone()).size_full()
            }
            VirtualListExternalScrollBehavior::ParentViewport => match self.axis {
                Axis::Horizontal => div().id(self.id.clone()).h_full(),
                Axis::Vertical => div().id(self.id.clone()).w_full(),
            },
        };
        self.scroll_handle = scroll_handle.clone();
        self.default_handle = false;
        self.external_scroll_behavior = Some(behavior);
        if behavior != VirtualListExternalScrollBehavior::ParentViewport {
            self.scroll_handle.clear_parent_viewport_metrics();
        }
        self
    }

    fn uses_parent_viewport_scroll(&self) -> bool {
        self.external_scroll_behavior == Some(VirtualListExternalScrollBehavior::ParentViewport)
    }

    fn scroll_to_deferred_item(
        &self,
        scroll_offset: Point<Pixels>,
        items_bounds: &[Bounds<Pixels>],
        content_bounds: &Bounds<Pixels>,
        scroll_to_item: DeferredScrollToItem,
    ) -> Point<Pixels> {
        let Some(target_offset) = programmatic_item_target_for_request(
            self.axis,
            scroll_offset,
            items_bounds,
            *content_bounds,
            scroll_to_item,
        ) else {
            return scroll_offset;
        };

        let behavior = self.programmatic_scroll;
        self.scroll_handle
            .base_handle()
            .scroll_to(target_offset, behavior);
        if behavior == ScrollBehavior::Instant {
            target_offset
        } else {
            scroll_offset
        }
    }

    fn scroll_to_deferred_parent_viewport_item(
        &self,
        scroll_offset: Point<Pixels>,
        items_bounds: &[Bounds<Pixels>],
        viewport_bounds: Bounds<Pixels>,
        scroll_to_item: DeferredScrollToItem,
    ) -> Point<Pixels> {
        let Some(bounds) = items_bounds
            .get(scroll_to_item.item_index + scroll_to_item.offset)
            .cloned()
        else {
            return scroll_offset;
        };

        let mut next_offset = scroll_offset;
        match self.axis {
            Axis::Horizontal => {
                let item_doc_left = bounds.left() - scroll_offset.x;
                let item_doc_right = bounds.right() - scroll_offset.x;
                next_offset.x = match scroll_to_item.strategy {
                    ScrollStrategy::Top => viewport_bounds.left() - item_doc_left,
                    ScrollStrategy::Center => {
                        viewport_bounds.left() + viewport_bounds.size.width.half()
                            - item_doc_left
                            - bounds.size.width.half()
                    }
                    ScrollStrategy::Bottom => viewport_bounds.right() - item_doc_right,
                    ScrollStrategy::Nearest => {
                        if bounds.left() < viewport_bounds.left() {
                            viewport_bounds.left() - item_doc_left
                        } else if bounds.right() > viewport_bounds.right() {
                            viewport_bounds.right() - item_doc_right
                        } else {
                            scroll_offset.x
                        }
                    }
                };
            }
            Axis::Vertical => {
                let item_doc_top = bounds.top() - scroll_offset.y;
                let item_doc_bottom = bounds.bottom() - scroll_offset.y;
                next_offset.y = match scroll_to_item.strategy {
                    ScrollStrategy::Top => viewport_bounds.top() - item_doc_top,
                    ScrollStrategy::Center => {
                        viewport_bounds.top() + viewport_bounds.size.height.half()
                            - item_doc_top
                            - bounds.size.height.half()
                    }
                    ScrollStrategy::Bottom => viewport_bounds.bottom() - item_doc_bottom,
                    ScrollStrategy::Nearest => {
                        if bounds.top() < viewport_bounds.top() {
                            viewport_bounds.top() - item_doc_top
                        } else if bounds.bottom() > viewport_bounds.bottom() {
                            viewport_bounds.bottom() - item_doc_bottom
                        } else {
                            scroll_offset.y
                        }
                    }
                };
            }
        }

        self.scroll_handle
            .base_handle()
            .request_offset(next_offset, ScrollMutationSource::Deferred);
        next_offset
    }

    /// Ref from: https://github.com/zed-industries/zed/blob/83f9f9d9e3f5914392cab9a09e3472711a1d7b38/crates/gpui/src/elements/uniform_list.rs#L660
    fn measure_item(
        &self,
        list_width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Size<Pixels> {
        if self.items_count == 0 {
            return Size::default();
        }

        let item_ix = 0;
        let mut items = (self.render_items)(item_ix..item_ix + 1, window, cx);
        let Some(mut item_to_measure) = items.pop() else {
            return Size::default();
        };
        let available_space = size(
            list_width.map_or(AvailableSpace::MinContent, |width| {
                AvailableSpace::Definite(width)
            }),
            AvailableSpace::MinContent,
        );
        item_to_measure.layout_as_root(available_space, window, cx)
    }
}

/// The scroll handle a [`VirtualList`] that tracks none keeps in its element state.
struct DefaultScrollHandle(VirtualListScrollHandle);

/// Frame state used by the [`VirtualList`].
pub struct VirtualListFrameState {
    /// Visible items to be painted: the ordinary rows, then the sticky rows
    /// from `sticky_start`, drawn above the rows (and above a list layer).
    items: Vec<AnyElement>,
    sticky_start: usize,
    size_layout: ItemSizeLayout,
    scroll_context: Option<ScrollContainerContext>,
}

#[derive(Default, Clone)]
struct ItemSizeLayout {
    items_sizes: Rc<Vec<Size<Pixels>>>,
    axis: Option<Axis>,
    gap: Pixels,
    items_count: usize,
    content_size: Size<Pixels>,
    sizes: Vec<Pixels>,
    origins: Vec<Pixels>,
    extent_index: OrderedExtentIndex,
    last_layout_bounds: Bounds<Pixels>,
}

impl IntoElement for VirtualList {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for VirtualList {
    type RequestLayoutState = VirtualListFrameState;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&crate::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        if self.default_handle
            && let Some(global_id) = global_id
        {
            // One handle for the element's lifetime: a fresh handle tracked on every render
            // would take new metrics on every frame and notify its owner to draw again.
            let handle =
                window.with_element_state(global_id, |state: Option<DefaultScrollHandle>, _| {
                    let state = state
                        .unwrap_or_else(|| DefaultScrollHandle(VirtualListScrollHandle::new()));
                    (state.0.clone(), state)
                });
            self.base.interactivity().tracked_scroll_handle = Some(handle.base_handle.clone());
            self.scroll_handle = handle;
        }
        let rem_size = window.rem_size();
        let font_size = window.text_style().font_size.to_pixels(rem_size);
        let mut size_layout = ItemSizeLayout::default();
        let parent_viewport_scroll = self.uses_parent_viewport_scroll();
        let longest_item_size = if parent_viewport_scroll {
            Size::default()
        } else {
            self.fixed_cross_axis_size.map_or_else(
                || {
                    if !self.uses_parent_viewport_scroll() {
                        // A frame that only scrolls the list's layer keeps what it measured.
                        crate::fast::layers::lists::measure_item(
                            window,
                            cx,
                            global_id,
                            |window, cx| self.measure_item(None, window, cx),
                        )
                    } else {
                        self.measure_item(None, window, cx)
                    }
                },
                |cross_axis_size| match self.axis {
                    Axis::Horizontal => size(px(0.), cross_axis_size),
                    Axis::Vertical => size(cross_axis_size, px(0.)),
                },
            )
        };

        let layout_id = self.base.interactivity().request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| {
                let Some(global_id) = global_id else {
                    panic!("VirtualList layout requires its global element id");
                };
                size_layout = window.with_element_state(
                    global_id,
                    |state: Option<ItemSizeLayout>, _window| {
                        let mut state = state.unwrap_or_default();

                        // Including the gap between items for calculate the item size
                        let gap = style
                            .gap
                            .along(self.axis)
                            .to_pixels(font_size.into(), rem_size);

                        if state.items_sizes != self.item_sizes
                            || state.axis != Some(self.axis)
                            || state.gap != gap
                            || state.items_count != self.items_count
                        {
                            state.items_sizes = self.item_sizes.clone();
                            state.axis = Some(self.axis);
                            state.gap = gap;
                            state.items_count = self.items_count;
                            // Prepare each item's size by axis
                            state.sizes = self
                                .item_sizes
                                .iter()
                                .enumerate()
                                .map(|(i, size)| {
                                    let size = size.along(self.axis);
                                    if i + 1 == self.items_count {
                                        size
                                    } else {
                                        size + gap
                                    }
                                })
                                .collect::<Vec<_>>();

                            // Prepare each item's origin by axis
                            state.origins = state
                                .sizes
                                .iter()
                                .scan(px(0.), |cumulative, size| match self.axis {
                                    Axis::Horizontal => {
                                        let x = *cumulative;
                                        *cumulative += *size;
                                        Some(x)
                                    }
                                    Axis::Vertical => {
                                        let y = *cumulative;
                                        *cumulative += *size;
                                        Some(y)
                                    }
                                })
                                .collect::<Vec<_>>();
                            state.extent_index.rebuild(state.sizes.iter().copied());
                        }

                        state.content_size = if self.axis == Axis::Horizontal {
                            Size {
                                width: state.extent_index.total(),
                                height: longest_item_size.height,
                            }
                        } else {
                            Size {
                                width: longest_item_size.width,
                                height: state.extent_index.total(),
                            }
                        };

                        (state.clone(), state)
                    },
                );

                let axis = self.axis;
                if parent_viewport_scroll {
                    // A list in its parent's flow takes its whole content extent along its axis.
                    // It lays itself out around a spacer of that extent rather than as a
                    // measured leaf: when view retention keeps the view holding the list without
                    // building it, a kept measured leaf other than text has no closure and is
                    // sized empty, while a kept node keeps its style. The spacer gives the list
                    // the intrinsic size the leaf measured, with padding and borders outside it.
                    let extent = size_layout.content_size.along(axis);
                    let mut spacer = Style {
                        flex_shrink: 0.,
                        ..Style::default()
                    };
                    spacer.size = match axis {
                        Axis::Horizontal => size(extent.into(), px(0.).into()),
                        Axis::Vertical => size(px(0.).into(), extent.into()),
                    };
                    crate::fast::layout_key::push_layout_key(window, None);
                    let spacer = window.request_layout(spacer, None, cx);
                    crate::fast::layout_key::pop_layout_key(window);
                    return window.with_text_style(style.text_style().cloned(), |window| {
                        window.request_layout(style, Some(spacer), cx)
                    });
                }
                match self.sizing_behavior {
                    ListSizingBehavior::Infer => {
                        window.with_text_style(style.text_style().cloned(), |window| {
                            let size_layout = size_layout.clone();

                            window.request_measured_layout(style, {
                                move |known_dimensions, available_space, _, _| {
                                    let mut size = Size::default();
                                    size.width = known_dimensions.width.unwrap_or(
                                        match available_space.width {
                                            AvailableSpace::Definite(x) => x,
                                            AvailableSpace::MinContent
                                            | AvailableSpace::MaxContent => {
                                                size_layout.content_size.width
                                            }
                                        },
                                    );
                                    size.height = known_dimensions.height.unwrap_or(
                                        match available_space.height {
                                            AvailableSpace::Definite(x) => x,
                                            AvailableSpace::MinContent
                                            | AvailableSpace::MaxContent => {
                                                size_layout.content_size.height
                                            }
                                        },
                                    );
                                    size
                                }
                            })
                        })
                    }
                    ListSizingBehavior::Auto => window
                        .with_text_style(style.text_style().cloned(), |window| {
                            window.request_layout(style, None, cx)
                        }),
                }
            },
        );

        (
            layout_id,
            VirtualListFrameState {
                items: Vec::new(),
                sticky_start: 0,
                size_layout,
                scroll_context: None,
            },
        )
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&crate::InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        layout.items.clear();
        layout.sticky_start = 0;
        layout.scroll_context = None;
        layout.size_layout.last_layout_bounds = bounds;
        let parent_viewport_scroll = self.uses_parent_viewport_scroll();
        // A list in its parent's flow takes the parent's viewport from the content mask. While
        // the parent's scroll layer paints its content the mask is the region the layer paints,
        // the viewport and its overscan; the viewport is what the list's metrics and range
        // describe, and the rows it shows, as without a layer.
        let parent_viewport_bounds =
            crate::fast::layers::paint::unlayered_content_mask(window).bounds;

        let style = self
            .base
            .interactivity()
            .compute_style(global_id, None, window, cx);
        let border_widths = style.border_widths.to_pixels(window.rem_size());
        let paddings = style
            .padding
            .to_pixels(bounds.size.into(), window.rem_size());

        let content_bounds = Bounds::from_corners(
            bounds.origin
                + point(
                    border_widths.left + paddings.left,
                    border_widths.top + paddings.top,
                ),
            bounds.bottom_right()
                - point(
                    border_widths.right + paddings.right,
                    border_widths.bottom + paddings.bottom,
                ),
        );

        let axis = self.axis;
        if parent_viewport_scroll {
            match axis {
                Axis::Horizontal => {
                    layout.size_layout.content_size.height = content_bounds.size.height;
                }
                Axis::Vertical => {
                    layout.size_layout.content_size.width = content_bounds.size.width;
                }
            }
        }
        let item_sizes = &layout.size_layout.sizes;
        let item_origins = &layout.size_layout.origins;
        let extent_index = &layout.size_layout.extent_index;

        let mut scroll_state = self.scroll_handle.state.borrow_mut();
        scroll_state.axis = axis;
        scroll_state.items_count = self.items_count;
        let deferred_scroll_to_item = scroll_state.deferred_scroll_to_item.take();
        drop(scroll_state);

        let mut scroll_offset = self.scroll_handle.offset();
        let deferred_parent_viewport_scroll_offset = if parent_viewport_scroll {
            self.scroll_handle.update_parent_viewport_metrics(
                axis,
                parent_viewport_bounds,
                content_bounds,
                layout.size_layout.content_size,
                scroll_offset,
            )
        } else {
            self.scroll_handle.clear_parent_viewport_metrics();
            None
        };

        if let Some(scroll_to_item) = deferred_scroll_to_item {
            let items_bounds = item_origins
                .iter()
                .enumerate()
                .map(|(i, &origin)| {
                    let item_size = item_sizes[i];

                    Bounds {
                        origin: match self.axis {
                            Axis::Horizontal => point(content_bounds.left() + origin, px(0.)),
                            Axis::Vertical => point(px(0.), content_bounds.top() + origin),
                        },
                        size: match self.axis {
                            Axis::Horizontal => size(item_size, content_bounds.size.height),
                            Axis::Vertical => size(content_bounds.size.width, item_size),
                        },
                    }
                })
                .collect::<Vec<_>>();
            scroll_offset = if parent_viewport_scroll {
                self.scroll_to_deferred_parent_viewport_item(
                    scroll_offset,
                    &items_bounds,
                    parent_viewport_bounds,
                    scroll_to_item,
                )
            } else {
                self.scroll_to_deferred_item(
                    scroll_offset,
                    &items_bounds,
                    &content_bounds,
                    scroll_to_item,
                )
            };
        } else if parent_viewport_scroll
            && let Some(offset) = deferred_parent_viewport_scroll_offset
        {
            self.scroll_handle.set_parent_viewport_scroll_offset(offset);
            scroll_offset = self.scroll_handle.offset();
        }

        if !parent_viewport_scroll {
            scroll_offset = scroll_offset
                .max(&point(
                    content_bounds.size.width - layout.size_layout.content_size.width,
                    content_bounds.size.height - layout.size_layout.content_size.height,
                ))
                .min(&point(px(0.), px(0.)));
            if scroll_offset != self.scroll_handle.offset() {
                self.scroll_handle
                    .base_handle()
                    .request_offset(scroll_offset, ScrollMutationSource::LayoutClamp);
            }
        }

        self.base.interactivity().prepaint(
            global_id,
            inspector_id,
            bounds,
            layout.size_layout.content_size,
            window,
            cx,
            |_style, _, hitbox, _positioned_bounds, _sticky_state, window, cx| {
                if self.items_count == 0 {
                    if let Some(listener) = &self.rendered_range_listener {
                        publish_rendered_range(listener, 0..0, window, cx);
                    }
                } else {
                    if !parent_viewport_scroll {
                        let min_scroll_offset = content_bounds.size.along(self.axis)
                            - layout.size_layout.content_size.along(self.axis);

                        let is_scrolled = !scroll_offset.along(self.axis).is_zero();
                        if is_scrolled {
                            match self.axis {
                                Axis::Horizontal if scroll_offset.x < min_scroll_offset => {
                                    scroll_offset.x = min_scroll_offset;
                                    self.scroll_handle.base_handle().request_offset(
                                        scroll_offset,
                                        ScrollMutationSource::LayoutClamp,
                                    );
                                }
                                Axis::Vertical if scroll_offset.y < min_scroll_offset => {
                                    scroll_offset.y = min_scroll_offset;
                                    self.scroll_handle.base_handle().request_offset(
                                        scroll_offset,
                                        ScrollMutationSource::LayoutClamp,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    // A list that scrolls itself, or by a handle it shares with a sibling
                    // (`SynchronizedOffset`), places its rows at an offset on whole device
                    // pixels while scroll layers snap, as `uniform_list` does, so rows painted
                    // into its layer and rows drawn without one land alike. Such a list has a
                    // list layer; one in its parent's flow has its parent's.
                    let own_scroll = !parent_viewport_scroll;
                    if own_scroll {
                        scroll_offset =
                            crate::fast::layers::lists::snap_item_offset(window, scroll_offset);
                    }

                    let (visible_range, range_lookup_work) = match self.axis {
                        Axis::Horizontal => {
                            if parent_viewport_scroll {
                                let viewport_bounds =
                                    parent_viewport_bounds.intersect(&content_bounds);
                                extent_index.visible_range(
                                    viewport_bounds.left() - content_bounds.left(),
                                    viewport_bounds.right() - content_bounds.left(),
                                    self.items_count,
                                )
                            } else {
                                extent_index.visible_range(
                                    -(scroll_offset.x + paddings.left),
                                    -scroll_offset.x + content_bounds.size.width,
                                    self.items_count,
                                )
                            }
                        }
                        Axis::Vertical => {
                            if parent_viewport_scroll {
                                let viewport_bounds =
                                    parent_viewport_bounds.intersect(&content_bounds);
                                extent_index.visible_range(
                                    viewport_bounds.top() - content_bounds.top(),
                                    viewport_bounds.bottom() - content_bounds.top(),
                                    self.items_count,
                                )
                            } else {
                                extent_index.visible_range(
                                    -(scroll_offset.y + paddings.top),
                                    -scroll_offset.y + content_bounds.size.height,
                                    self.items_count,
                                )
                            }
                        }
                    };
                    #[cfg(feature = "diagnostics")]
                    let mut range_lookup_work = range_lookup_work;
                    #[cfg(not(feature = "diagnostics"))]
                    let _ = range_lookup_work;

                    let rendered_range = overscanned_item_range(
                        visible_range.clone(),
                        self.overscan,
                        self.items_count,
                    );
                    let mut active_sticky_items = self.sticky_items.active_stack_with_item_extents(
                        &visible_range,
                        self.axis,
                        item_sizes,
                    );
                    if let Some((next_candidate, retained_depth)) =
                        active_sticky_items.last().and_then(|active| {
                            self.sticky_items
                                .next_replacement_after(active.visual_index, self.axis)
                        })
                    {
                        let incoming_start = match self.axis {
                            Axis::Horizontal if parent_viewport_scroll => {
                                content_bounds.left() + item_origins[next_candidate.visual_index]
                                    - parent_viewport_bounds.left()
                            }
                            Axis::Horizontal => {
                                item_origins[next_candidate.visual_index] + scroll_offset.x
                            }
                            Axis::Vertical if parent_viewport_scroll => {
                                content_bounds.top() + item_origins[next_candidate.visual_index]
                                    - parent_viewport_bounds.top()
                            }
                            Axis::Vertical => {
                                item_origins[next_candidate.visual_index] + scroll_offset.y
                            }
                        };
                        push_active_sticky_replacement(
                            &mut active_sticky_items,
                            retained_depth,
                            incoming_start,
                        );
                    }
                    #[cfg(feature = "diagnostics")]
                    {
                        range_lookup_work.cache_range_items = rendered_range.len();
                        range_lookup_work.pinned_items = active_sticky_items
                            .iter()
                            .filter(|item| !rendered_range.contains(&item.visual_index))
                            .count();
                        range_lookup_work.rendered_items =
                            range_lookup_work.cache_range_items + range_lookup_work.pinned_items;
                        if let Some(listener) = &self.range_lookup_listener {
                            listener(range_lookup_work.into());
                        }
                    }
                    let mut sticky_render_items =
                        Vec::<(AnyElement, usize, StickyRenderLayer)>::new();
                    for candidate in active_sticky_items.iter().copied() {
                        let ix = candidate.visual_index;
                        let mut items = (self.render_items)(ix..ix + 1, window, cx);
                        if let Some(item) = items.pop() {
                            sticky_render_items.push((
                                item,
                                ix,
                                StickyRenderLayer {
                                    level: candidate.level,
                                    incoming: false,
                                },
                            ));
                        }
                    }

                    if let Some(listener) = &self.rendered_range_listener {
                        publish_rendered_range(listener, rendered_range.clone(), window, cx);
                    }

                    // A list that is its own scroll container composites its ordinary rows from
                    // a list layer along its axis, and draws its sticky candidates in view (the
                    // active stack among them) above it every frame; a list scrolled by another
                    // element calls no hook.
                    // The layer holds no sticky candidate anywhere in its reach.
                    let candidates = self.sticky_items.items();
                    let is_candidate = |ix: usize| {
                        candidates
                            .binary_search_by_key(&ix, |item| item.visual_index)
                            .is_ok()
                    };
                    if self.external_scroll_behavior
                        == Some(VirtualListExternalScrollBehavior::SynchronizedOffset)
                        && let Some(global_id) = global_id
                    {
                        // Scrolled by its driver's handle: the list's layer is keyed by the list.
                        crate::fast::layers::invalidate::painted_synchronized(
                            window,
                            global_id,
                            &self.scroll_handle.base_handle(),
                        );
                    }
                    // A list scrolled by a sibling's handle has no scroll container of its own:
                    // its rows' hitboxes are clipped to its bounds as they are prepainted, and
                    // what they paint is not. A layer clips both to its viewport, the clip of
                    // what holds the list, so it shows what the frame without it shows only
                    // while that clip lies within the list's bounds, as a table's body is
                    // clipped. Otherwise the list is counted ineligible.
                    let synchronized = self.external_scroll_behavior
                        == Some(VirtualListExternalScrollBehavior::SynchronizedOffset);
                    let clipped_to_bounds = !synchronized || {
                        let mask = window.content_mask().bounds;
                        mask.intersect(&bounds) == mask
                    };
                    let fast_rows = if own_scroll {
                        crate::fast::layers::lists::begin_virtual_list(
                            window,
                            cx,
                            global_id,
                            self.axis,
                            content_bounds,
                            scroll_offset,
                            item_origins,
                            item_sizes,
                            extent_index,
                            &rendered_range,
                            &is_candidate,
                            clipped_to_bounds,
                        )
                    } else {
                        crate::fast::layers::lists::Rows::unlayered()
                    };
                    let fast_indices =
                        crate::fast::layers::lists::row_indices(&fast_rows, rendered_range.clone());
                    let rendered_visible_items = crate::fast::layers::lists::render_rows(
                        &fast_rows,
                        rendered_range.clone(),
                        |range| (self.render_items)(range, window, cx).into_iter().collect(),
                    )
                    .into_iter()
                    .zip(fast_indices);
                    let sticky_items = self.sticky_items.items();
                    let first_sticky_item = sticky_items
                        .partition_point(|item| item.visual_index < rendered_range.start);
                    if fast_rows.layered() {
                        // The layer renders no sticky candidate: the incoming ones in view are
                        // rendered here, as the rows rendered without a layer would bring them.
                        for candidate in sticky_items[first_sticky_item..]
                            .iter()
                            .take_while(|item| item.visual_index < rendered_range.end)
                        {
                            let ix = candidate.visual_index;
                            if active_sticky_items
                                .iter()
                                .any(|active| active.visual_index == ix)
                            {
                                continue;
                            }
                            if let Some(item) = (self.render_items)(ix..ix + 1, window, cx).pop() {
                                sticky_render_items.push((
                                    item,
                                    ix,
                                    StickyRenderLayer {
                                        level: self.sticky_items.render_level(candidate),
                                        incoming: true,
                                    },
                                ));
                            }
                        }
                    }
                    let mut sticky_candidates = sticky_items[first_sticky_item..].iter().peekable();
                    let mut rendered_items = Vec::<(AnyElement, usize)>::new();
                    for (item, ix) in rendered_visible_items {
                        if let Some(candidate) = sticky_candidates
                            .next_if(|candidate| candidate.visual_index == ix)
                            .cloned()
                        {
                            if !active_sticky_items
                                .iter()
                                .any(|active| active.visual_index == ix)
                            {
                                sticky_render_items.push((
                                    item,
                                    ix,
                                    StickyRenderLayer {
                                        level: self.sticky_items.render_level(&candidate),
                                        incoming: true,
                                    },
                                ));
                            }
                        } else {
                            rendered_items.push((item, ix));
                        }
                    }
                    sticky_render_items
                        .sort_by(|lhs, rhs| compare_sticky_render_layers(lhs.2, rhs.2));
                    let sticky_start = rendered_items.len();
                    rendered_items.extend(
                        sticky_render_items
                            .into_iter()
                            .map(|(item, ix, _)| (item, ix)),
                    );
                    let overflow = match self.axis {
                        Axis::Horizontal => point(Overflow::Scroll, Overflow::Hidden),
                        Axis::Vertical => point(Overflow::Hidden, Overflow::Scroll),
                    };
                    let scroll_context = if parent_viewport_scroll {
                        ScrollContainerContext {
                            scrollport_bounds: parent_viewport_bounds,
                            content_bounds: Bounds::new(
                                content_bounds.origin,
                                layout.size_layout.content_size,
                            ),
                            scroll_offset,
                            overflow,
                        }
                    } else {
                        ScrollContainerContext {
                            scrollport_bounds: content_bounds,
                            content_bounds: Bounds::new(
                                content_bounds.origin + scroll_offset,
                                layout.size_layout.content_size,
                            ),
                            scroll_offset,
                            overflow,
                        }
                    };
                    layout.scroll_context = Some(scroll_context);

                    let content_mask = ContentMask::rectangular(bounds);
                    layout.sticky_start = sticky_start;
                    let mut fast_rows = Some(fast_rows);
                    window.with_scroll_container_context(scroll_context, |window| {
                        window.with_content_mask(Some(content_mask), |window| {
                            for (position, (item, ix)) in rendered_items.into_iter().enumerate() {
                                // The sticky rows are prepainted after the ordinary rows are
                                // done, outside a list layer, so their hitboxes are hit first.
                                if position == sticky_start
                                    && let Some(rows) = fast_rows.take()
                                {
                                    crate::fast::layers::lists::end_rows(window, cx, rows);
                                }
                                let item_origin = match self.axis {
                                    Axis::Horizontal => {
                                        if parent_viewport_scroll {
                                            content_bounds.origin + point(item_origins[ix], px(0.))
                                        } else {
                                            content_bounds.origin
                                                + point(
                                                    item_origins[ix] + scroll_offset.x,
                                                    scroll_offset.y,
                                                )
                                        }
                                    }
                                    Axis::Vertical => {
                                        if parent_viewport_scroll {
                                            content_bounds.origin + point(px(0.), item_origins[ix])
                                        } else {
                                            content_bounds.origin
                                                + point(
                                                    scroll_offset.x,
                                                    item_origins[ix] + scroll_offset.y,
                                                )
                                        }
                                    }
                                };

                                let active_offset = active_sticky_items
                                    .iter()
                                    .find(|item| item.visual_index == ix)
                                    .map(|item| item.offset);
                                let sticky_candidate = self.sticky_items.candidate(ix);
                                let sticky_offset = active_offset.or_else(|| {
                                    sticky_candidate.as_ref().map(|candidate| {
                                        self.sticky_items.offset_for_candidate(
                                            candidate.clone(),
                                            &active_sticky_items,
                                        )
                                    })
                                });
                                let available_space = match self.axis {
                                    Axis::Horizontal => size(
                                        AvailableSpace::Definite(item_sizes[ix]),
                                        AvailableSpace::Definite(content_bounds.size.height),
                                    ),
                                    Axis::Vertical => size(
                                        AvailableSpace::Definite(content_bounds.size.width),
                                        AvailableSpace::Definite(item_sizes[ix]),
                                    ),
                                };
                                let is_sticky = sticky_offset.is_some();
                                let mut item = if is_sticky {
                                    wrap_virtual_item_root(
                                        item,
                                        ix,
                                        self.axis,
                                        item_sizes[ix],
                                        content_bounds.size,
                                        sticky_offset,
                                        sticky_candidate,
                                    )
                                } else {
                                    item
                                };

                                if own_scroll {
                                    crate::fast::layout_key::layout_as_list_item(
                                        &mut item,
                                        ix,
                                        available_space,
                                        window,
                                        cx,
                                    );
                                } else {
                                    item.layout_as_root(available_space, window, cx);
                                }
                                if own_scroll && !is_sticky {
                                    crate::fast::layers::lists::prepaint_row(
                                        window,
                                        cx,
                                        ix,
                                        |window, cx| item.prepaint_at(item_origin, window, cx),
                                    );
                                } else {
                                    item.prepaint_at(item_origin, window, cx);
                                }
                                layout.items.push(item);
                            }
                            if let Some(rows) = fast_rows.take() {
                                crate::fast::layers::lists::end_rows(window, cx, rows);
                            }
                        });
                    });
                }

                hitbox
            },
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&crate::InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.base.interactivity().paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            |_, window, cx| {
                let own_scroll = self.external_scroll_behavior
                    != Some(VirtualListExternalScrollBehavior::ParentViewport);
                let mut paint_items = |window: &mut Window, cx: &mut App| {
                    if !own_scroll {
                        for item in &mut layout.items {
                            item.paint(window, cx);
                        }
                        return;
                    }
                    let (rows, sticky) = layout.items.split_at_mut(layout.sticky_start);
                    crate::fast::layers::lists::begin_paint_rows(window, cx, global_id);
                    for item in rows {
                        crate::fast::layers::lists::paint_row(window, cx, None, |window, cx| {
                            item.paint(window, cx)
                        });
                    }
                    crate::fast::layers::lists::end_paint_rows(window, cx, global_id);
                    // Above the rows, and above the layer composited from them.
                    for item in sticky {
                        item.paint(window, cx);
                    }
                };

                if let Some(scroll_context) = layout.scroll_context {
                    window.with_scroll_container_context(scroll_context, |window| {
                        paint_items(window, cx);
                    });
                } else {
                    paint_items(window, cx);
                }
            },
        )
    }
}

/// Calls the rendered-range listener as part of drawing the view holding the list, as its rows
/// are rendered (`fast::layers::lists::render_items`): the listener commonly records the range
/// in that view, and gpux counts a view updated without a notification as changed, so the views
/// that read it, such as the one holding a wrapper around the list, would be built again on every
/// frame that scrolled the list to another range. With scroll layers off it is called as it is
/// today.
fn publish_rendered_range(
    listener: &RenderedRangeListener,
    range: Range<usize>,
    window: &mut Window,
    cx: &mut App,
) {
    crate::fast::layers::lists::render_items(window, cx, |window, cx| listener(range, window, cx));
}

fn overscanned_item_range(
    exact_range: Range<usize>,
    overscan: usize,
    items_count: usize,
) -> Range<usize> {
    overscanned_range(exact_range, overscan, items_count)
}

fn wrap_virtual_item_root(
    item: AnyElement,
    visual_index: usize,
    axis: Axis,
    primary_size: Pixels,
    content_size: Size<Pixels>,
    sticky_offset: Option<Pixels>,
    sticky_candidate: Option<VirtualStickyItem>,
) -> AnyElement {
    let root = match axis {
        Axis::Horizontal => div()
            .id(("virtual-list-row-root", visual_index))
            .w(primary_size)
            .h(content_size.height),
        Axis::Vertical => div()
            .id(("virtual-list-row-root", visual_index))
            .w(content_size.width)
            .h(primary_size),
    };
    // A sticky row is rendered through this wrapper sized from the actual laid-out content
    // bounds. Stretch its direct child so entity-backed rows retain that cross-axis extent.
    let root = root.flex().flex_col().items_stretch();
    let root = match (axis, sticky_offset) {
        (Axis::Horizontal, Some(offset)) => root.sticky().left(offset),
        (Axis::Vertical, Some(offset)) => root.sticky().top(offset),
        (_, None) => root,
    };
    let root = if let Some(stuck_style) = sticky_candidate
        .as_ref()
        .and_then(|candidate| candidate.stuck_style.clone())
    {
        root.when_stuck(|_| stuck_style)
    } else {
        root
    };
    let root = if let Some(listener) =
        sticky_candidate.and_then(|candidate| candidate.sticky_state_changed_listener)
    {
        root.on_sticky_state_changed(move |event, window, cx| {
            listener(event, window, cx);
        })
    } else {
        root
    };
    root.child(item).into_any_element()
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::{
        Axis, Bounds, DeferredScrollToItem, ScrollHandle, ScrollStrategy, StickyStacking, point,
        px, size,
    };

    use super::{
        OrderedExtentIndex, StickyRenderLayer, VirtualListScrollHandle, VirtualStickyEdge,
        VirtualStickyItem, VirtualStickyItems, VirtualStickyRow, compare_sticky_render_layers,
        overscanned_item_range, programmatic_item_target_for_request,
        push_active_sticky_replacement,
    };

    #[test]
    fn sticky_metadata_preserves_edge_level_and_inset() {
        let top = VirtualStickyRow::top().level(2).inset(px(8.0));
        assert_eq!(top.edge, VirtualStickyEdge::Top);
        assert_eq!(top.axis(), Axis::Vertical);
        assert_eq!(top.relation, StickyStacking::Level(2));
        assert_eq!(top.inset, px(8.0));
        assert_eq!(top.measured_size, None);

        let left = VirtualStickyRow::left()
            .level(1)
            .inset(px(4.0))
            .measured_size(px(24.0));
        assert_eq!(left.edge, VirtualStickyEdge::Left);
        assert_eq!(left.axis(), Axis::Horizontal);
        assert_eq!(left.relation, StickyStacking::Level(1));
        assert_eq!(left.inset, px(4.0));
        assert_eq!(left.measured_size, Some(px(24.0)));
    }

    #[test]
    fn virtual_sticky_item_preserves_policy_metadata_and_measured_size() {
        let item = VirtualStickyItem::new(
            7,
            VirtualStickyRow::top().level(3).inset(px(12.0)),
            px(28.0),
        );

        assert_eq!(item.visual_index, 7);
        assert_eq!(item.edge, VirtualStickyEdge::Top);
        assert_eq!(item.axis(), Axis::Vertical);
        assert_eq!(item.relation, StickyStacking::Level(3));
        assert_eq!(item.inset, px(12.0));
        assert_eq!(item.measured_size, px(28.0));
    }

    #[test]
    fn sticky_render_layers_keep_parents_above_children_and_incoming_above_same_level() {
        let mut layers = vec![
            (
                "active parent",
                StickyRenderLayer {
                    level: 0,
                    incoming: false,
                },
            ),
            (
                "active child",
                StickyRenderLayer {
                    level: 1,
                    incoming: false,
                },
            ),
            (
                "incoming child",
                StickyRenderLayer {
                    level: 1,
                    incoming: true,
                },
            ),
            (
                "incoming parent",
                StickyRenderLayer {
                    level: 0,
                    incoming: true,
                },
            ),
        ];

        layers.sort_by(|lhs, rhs| compare_sticky_render_layers(lhs.1, rhs.1));

        assert_eq!(
            layers
                .into_iter()
                .map(|(label, _)| label)
                .collect::<Vec<_>>(),
            vec![
                "active child",
                "incoming child",
                "active parent",
                "incoming parent"
            ]
        );
    }

    #[test]
    fn active_sticky_stack_returns_no_rows_without_prior_candidates() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(4, VirtualStickyRow::top(), px(20.0)),
            VirtualStickyItem::new(8, VirtualStickyRow::top(), px(20.0)),
        ]);

        assert!(
            sticky_items
                .active_stack(&(0..4), Axis::Vertical)
                .is_empty()
        );
    }

    #[test]
    fn active_sticky_stack_returns_flat_prior_candidate() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::top().inset(px(2.0)), px(20.0)),
            VirtualStickyItem::new(8, VirtualStickyRow::top().inset(px(2.0)), px(20.0)),
        ]);

        let active = sticky_items.active_stack(&(5..12), Axis::Vertical);

        assert_eq!(active.len(), 1);
        assert_eq!(active[0].visual_index, 0);
        assert_eq!(active[0].level, 0);
        assert_eq!(active[0].offset, px(2.0));
    }

    #[test]
    fn active_sticky_stack_replaces_same_level_candidates() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::top(), px(20.0)),
            VirtualStickyItem::new(6, VirtualStickyRow::top(), px(20.0)),
            VirtualStickyItem::new(12, VirtualStickyRow::top(), px(20.0)),
        ]);

        let active = sticky_items.active_stack(&(10..16), Axis::Vertical);

        assert_eq!(active.len(), 1);
        assert_eq!(active[0].visual_index, 6);
    }

    #[test]
    fn active_sticky_stack_orders_nested_candidates_by_level_with_cumulative_offsets() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::top().level(0), px(20.0)),
            VirtualStickyItem::new(3, VirtualStickyRow::top().level(1), px(12.0)),
            VirtualStickyItem::new(5, VirtualStickyRow::top().level(2), px(8.0)),
        ]);

        let active = sticky_items.active_stack(&(7..14), Axis::Vertical);

        assert_eq!(
            active
                .iter()
                .map(|item| (item.visual_index, item.level, item.offset))
                .collect::<Vec<_>>(),
            vec![(0, 0, px(0.0)), (3, 1, px(20.0)), (5, 2, px(32.0))]
        );
    }

    #[test]
    fn active_sticky_stack_uses_virtual_item_extents_for_default_offsets() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::top().level(0), px(38.0)),
            VirtualStickyItem::new(2, VirtualStickyRow::top().level(1), px(38.0)),
        ]);
        let item_extents = vec![px(40.0); 4];

        let active =
            sticky_items.active_stack_with_item_extents(&(3..4), Axis::Vertical, &item_extents);

        assert_eq!(
            active
                .iter()
                .map(|item| (
                    item.visual_index,
                    item.level,
                    item.offset,
                    item.measured_size
                ))
                .collect::<Vec<_>>(),
            vec![(0, 0, px(0.0), px(40.0)), (2, 1, px(40.0), px(40.0))]
        );
    }

    #[test]
    fn base_completion_virtual_list_preserves_explicit_sticky_extent() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(
                0,
                VirtualStickyRow::top().level(0).measured_size(px(38.0)),
                px(38.0),
            ),
            VirtualStickyItem::new(2, VirtualStickyRow::top().level(1), px(38.0)),
        ]);
        let item_extents = vec![px(40.0); 4];

        let active =
            sticky_items.active_stack_with_item_extents(&(3..4), Axis::Vertical, &item_extents);

        assert_eq!(
            active
                .iter()
                .map(|item| (
                    item.visual_index,
                    item.level,
                    item.offset,
                    item.measured_size
                ))
                .collect::<Vec<_>>(),
            vec![(0, 0, px(0.0), px(38.0)), (2, 1, px(38.0), px(40.0))]
        );
    }

    #[test]
    fn active_sticky_stack_replaces_nested_same_level_and_drops_old_descendants() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::top().level(0), px(20.0)),
            VirtualStickyItem::new(3, VirtualStickyRow::top().level(1), px(12.0)),
            VirtualStickyItem::new(5, VirtualStickyRow::top().level(2), px(8.0)),
            VirtualStickyItem::new(9, VirtualStickyRow::top().level(1), px(16.0)),
        ]);

        let active = sticky_items.active_stack(&(12..18), Axis::Vertical);

        assert_eq!(
            active
                .iter()
                .map(|item| (item.visual_index, item.level, item.offset))
                .collect::<Vec<_>>(),
            vec![(0, 0, px(0.0)), (9, 1, px(20.0))]
        );
    }

    #[test]
    fn active_sticky_stack_uses_shared_relation_predecessor_index() {
        let sticky_items = VirtualStickyItems::from_items(
            (0..1000).map(|ix| VirtualStickyItem::new(ix, VirtualStickyRow::top(), px(20.0))),
        );

        assert_eq!(
            sticky_items
                .vertical_relation
                .relation_index
                .source_at_or_before(999),
            Some(999)
        );
        assert_eq!(
            sticky_items
                .vertical_relation
                .relation_index
                .active_chain(999),
            vec![999]
        );

        let active = sticky_items.active_stack(&(1000..1001), Axis::Vertical);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].visual_index, 999);
    }

    #[test]
    fn base_completion_virtual_list_keeps_keyed_sticky_identity_stable() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(
                0,
                VirtualStickyRow::top().stack_id("keyed_sticky-a"),
                px(20.0),
            ),
            VirtualStickyItem::new(
                1,
                VirtualStickyRow::top().stack_id("keyed_sticky-a"),
                px(12.0),
            ),
            VirtualStickyItem::new(
                4,
                VirtualStickyRow::top().stack_id("keyed_sticky-b"),
                px(18.0),
            ),
            VirtualStickyItem::new(
                5,
                VirtualStickyRow::top().stack_id("keyed_sticky-b"),
                px(14.0),
            ),
            VirtualStickyItem::new(
                8,
                VirtualStickyRow::top().stack_id("keyed_sticky-a"),
                px(16.0),
            ),
        ]);

        let active = |range| {
            sticky_items
                .active_stack(&range, Axis::Vertical)
                .into_iter()
                .map(|item| (item.visual_index, item.level, item.offset))
                .collect::<Vec<_>>()
        };
        assert_eq!(active(3..4), vec![(0, 0, px(0.0)), (1, 1, px(20.0))]);
        assert_eq!(active(7..8), vec![(4, 0, px(0.0)), (5, 1, px(18.0))]);
        assert_eq!(active(9..10), vec![(8, 0, px(0.0))]);
    }

    #[test]
    fn horizontal_keyed_stack_is_pushed_by_the_incoming_run_width() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::left().stack_id("a"), px(20.0)),
            VirtualStickyItem::new(1, VirtualStickyRow::left().stack_id("a"), px(12.0)),
            VirtualStickyItem::new(4, VirtualStickyRow::left().stack_id("b"), px(18.0)),
        ]);
        let mut active = sticky_items.active_stack(&(3..4), Axis::Horizontal);

        push_active_sticky_replacement(&mut active, 0, px(27.0));

        assert_eq!(active[0].offset, px(-5.0));
        assert_eq!(active[1].offset, px(15.0));
        assert_eq!(active[1].offset + active[1].measured_size, px(27.0));
    }

    #[test]
    fn horizontal_flat_sticky_level_is_pushed_by_the_incoming_header() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::left(), px(20.0)),
            VirtualStickyItem::new(4, VirtualStickyRow::left(), px(18.0)),
        ]);
        let mut active = sticky_items.active_stack(&(3..4), Axis::Horizontal);
        let (incoming, retained_depth) = sticky_items
            .next_replacement_after(0, Axis::Horizontal)
            .expect("the next flat header replaces the active header");

        push_active_sticky_replacement(&mut active, retained_depth, px(12.0));

        assert_eq!(incoming.visual_index, 4);
        assert_eq!(active[0].offset, px(-8.0));
    }

    #[test]
    fn flat_sticky_level_replacement_pushes_the_outgoing_header() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::top(), px(20.0)),
            VirtualStickyItem::new(4, VirtualStickyRow::top(), px(18.0)),
        ]);
        let mut active = sticky_items.active_stack(&(3..4), Axis::Vertical);
        let (incoming, retained_depth) = sticky_items
            .next_replacement_after(0, Axis::Vertical)
            .expect("the next flat header replaces the active header");

        push_active_sticky_replacement(&mut active, retained_depth, px(12.0));

        assert_eq!(incoming.visual_index, 4);
        assert_eq!(active[0].offset, px(-8.0));
    }

    #[test]
    fn same_sticky_level_replacement_pushes_only_the_outgoing_suffix() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::top().level(0), px(20.0)),
            VirtualStickyItem::new(2, VirtualStickyRow::top().level(1), px(12.0)),
            VirtualStickyItem::new(4, VirtualStickyRow::top().level(2), px(10.0)),
            VirtualStickyItem::new(8, VirtualStickyRow::top().level(1), px(14.0)),
        ]);
        let mut active = sticky_items.active_stack(&(7..8), Axis::Vertical);
        let (incoming, retained_depth) = sticky_items
            .next_replacement_after(4, Axis::Vertical)
            .expect("the next level-one header replaces the active child suffix");

        push_active_sticky_replacement(&mut active, retained_depth, px(27.0));

        assert_eq!(incoming.visual_index, 8);
        assert_eq!(retained_depth, 1);
        assert_eq!(active[0].offset, px(0.0));
        assert_eq!(active[1].offset, px(5.0));
        assert_eq!(active[2].offset, px(17.0));
        assert_eq!(active[2].offset + active[2].measured_size, px(27.0));
    }

    #[test]
    fn keyed_sticky_10000_candidates_use_the_shared_relation_index() {
        let sticky_items = VirtualStickyItems::from_items((0..10_000).map(|visual_index| {
            VirtualStickyItem::new(
                visual_index,
                VirtualStickyRow::top().stack_id("keyed_sticky-large"),
                px(1.0),
            )
        }));

        assert_eq!(
            sticky_items
                .vertical_relation
                .relation_index
                .active_chain(9_999)
                .len(),
            10_000
        );
        let active = sticky_items.active_stack(&(10_000..10_001), Axis::Vertical);
        assert_eq!(active.len(), 10_000);
        assert_eq!(active.last().map(|item| item.offset), Some(px(9_999.0)));
    }

    #[test]
    fn virtual_sticky_items_clone_shares_immutable_metadata() {
        let sticky_items = VirtualStickyItems::from_items([
            VirtualStickyItem::new(0, VirtualStickyRow::top(), px(20.0)),
            VirtualStickyItem::new(2, VirtualStickyRow::left(), px(16.0)),
        ]);
        let cloned = sticky_items.clone();

        assert!(Rc::ptr_eq(&sticky_items.items, &cloned.items));
        assert!(Rc::ptr_eq(
            &sticky_items.vertical_relation,
            &cloned.vertical_relation
        ));
        assert!(Rc::ptr_eq(
            &sticky_items.horizontal_relation,
            &cloned.horizontal_relation
        ));
    }

    #[test]
    fn visible_item_range_returns_empty_when_viewport_does_not_intersect_content() {
        let item_sizes = vec![px(10.), px(10.), px(10.)];
        let index = OrderedExtentIndex::from_extents(item_sizes);

        assert_eq!(index.visible_range(px(-30.), px(-1.), 3).0, 0..0);
        assert_eq!(index.visible_range(px(30.), px(40.), 3).0, 3..3);
    }

    #[test]
    fn base_completion_virtual_list_derives_variable_extent_parent_viewport_range() {
        let item_sizes = vec![px(10.); 10];
        let index = OrderedExtentIndex::from_extents(item_sizes);

        assert_eq!(index.visible_range(px(25.), px(45.), 10).0, 2..6);
    }

    #[test]
    fn base_completion_virtual_list_applies_bounded_overscan() {
        assert_eq!(overscanned_item_range(4..7, 0, 10), 4..7);
        assert_eq!(overscanned_item_range(4..7, 2, 10), 2..9);
        assert_eq!(overscanned_item_range(0..2, 4, 10), 0..6);
        assert_eq!(overscanned_item_range(8..10, 4, 10), 4..10);
    }

    #[test]
    fn base_completion_virtual_list_converts_parent_viewport_offsets_without_spacer_state() {
        let page_scroll = ScrollHandle::new();
        let handle = VirtualListScrollHandle::from(page_scroll.clone());

        handle.update_parent_viewport_metrics(
            Axis::Vertical,
            Bounds::new(point(px(0.0), px(0.0)), size(px(300.0), px(100.0))),
            Bounds::new(point(px(0.0), px(-50.0)), size(px(300.0), px(1000.0))),
            size(px(300.0), px(1000.0)),
            point(px(0.0), px(-250.0)),
        );

        assert_eq!(handle.parent_viewport_scroll_offset(), Some(px(50.0)));
        assert_eq!(handle.parent_viewport_primary_size(), Some(px(100.0)));
        assert_eq!(handle.parent_viewport_max_scroll_offset(), Some(px(900.0)));

        let requested = handle
            .state
            .borrow()
            .parent_viewport_metrics
            .unwrap()
            .external_offset_for_primary_scroll_offset(px(120.0));
        assert_eq!(requested.y, px(-320.0));

        handle.set_parent_viewport_scroll_offset(px(120.0));
        assert_eq!(page_scroll.offset().y, px(0.0));
    }

    #[test]
    fn base_completion_virtual_list_resolves_programmatic_targets_from_measured_geometry() {
        let item_bounds = [Bounds::new(
            point(px(0.0), px(200.0)),
            size(px(100.0), px(20.0)),
        )];
        let content_bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(100.0), px(100.0)));
        let request = |strategy| DeferredScrollToItem {
            item_index: 0,
            offset: 0,
            strategy,
            scroll_strict: false,
        };

        assert_eq!(
            programmatic_item_target_for_request(
                Axis::Vertical,
                point(px(0.0), px(0.0)),
                &item_bounds,
                content_bounds,
                request(ScrollStrategy::Top),
            )
            .map(|point| point.y),
            Some(px(-200.0))
        );
        assert_eq!(
            programmatic_item_target_for_request(
                Axis::Vertical,
                point(px(0.0), px(0.0)),
                &item_bounds,
                content_bounds,
                request(ScrollStrategy::Center),
            )
            .map(|point| point.y),
            Some(px(-160.0))
        );
        assert_eq!(
            programmatic_item_target_for_request(
                Axis::Vertical,
                point(px(0.0), px(0.0)),
                &item_bounds,
                content_bounds,
                request(ScrollStrategy::Bottom),
            )
            .map(|point| point.y),
            Some(px(-120.0))
        );
        assert_eq!(
            programmatic_item_target_for_request(
                Axis::Vertical,
                point(px(0.0), px(0.0)),
                &item_bounds,
                content_bounds,
                DeferredScrollToItem {
                    item_index: 1,
                    ..request(ScrollStrategy::Top)
                },
            ),
            None
        );
    }
}
