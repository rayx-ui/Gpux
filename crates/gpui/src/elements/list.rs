//! A list element that can be used to render a large number of differently sized elements
//! efficiently. Clients of this API need to ensure that elements outside of the scrolled
//! area do not change their height for this element to function correctly. If your elements
//! do change height, notify the list element via [`ListState::splice`] or [`ListState::reset`].
//! In order to minimize re-renders, this element's state is stored intrusively
//! on your own views, so that your code can coordinate directly with the list element's cached state.
//!
//! If all of your elements are the same height, see [`crate::UniformList`] for a simpler API

use super::scroll_activity::{ProgrammaticScrollFrame, ScrollActivity};
use crate::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, DispatchPhase, Edges, Element, EntityId,
    FocusHandle, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, Instant, IntoElement,
    Overflow, Pixels, Point, ScrollBehavior, ScrollContainerContext, ScrollDelta, ScrollWheelEvent,
    Size, StickyRelationIndex, StickyStacking, Style, StyleRefinement, Styled, Window, point, px,
    size, sticky_push_offset,
};
use collections::VecDeque;
use refineable::Refineable as _;
use std::{cell::RefCell, ops::Range, rc::Rc};
use sum_tree::{Bias, Dimensions, SumTree};

type RenderItemFn = dyn FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static;

/// Construct a new list element
pub fn list(
    state: ListState,
    render_item: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
) -> List {
    List {
        state,
        render_item: Box::new(render_item),
        style: StyleRefinement::default(),
        sizing_behavior: ListSizingBehavior::default(),
    }
}

/// A list element
pub struct List {
    state: ListState,
    render_item: Box<RenderItemFn>,
    style: StyleRefinement,
    sizing_behavior: ListSizingBehavior,
}

impl List {
    /// Set the sizing behavior for the list.
    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing_behavior = behavior;
        self
    }
}

/// The list state that views must hold on behalf of the list element.
#[derive(Clone)]
pub struct ListState(Rc<RefCell<StateInner>>);

impl std::fmt::Debug for ListState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ListState")
    }
}

struct StateInner {
    last_layout_bounds: Option<Bounds<Pixels>>,
    last_padding: Option<Edges<Pixels>>,
    items: SumTree<ListItem>,
    logical_scroll_top: Option<ListOffset>,
    alignment: ListAlignment,
    overdraw: Pixels,
    reset: bool,
    #[allow(clippy::type_complexity)]
    scroll_handler: Option<Box<dyn FnMut(&ListScrollEvent, &mut Window, &mut App)>>,
    scrollbar_drag_start_height: Option<Pixels>,
    scrollbar_drag_start_items: Option<SumTree<ListItem>>,
    measuring_behavior: ListMeasuringBehavior,
    item_size_hint: Option<Size<Pixels>>,
    pending_scroll: Option<PendingScroll>,
    pending_programmatic_scroll: Option<PendingProgrammaticScroll>,
    follow_state: FollowState,
    sticky_items: StickyItems,
    activity: ScrollActivity,
}

#[derive(Clone, Copy, Debug)]
enum PendingProgrammaticScroll {
    Logical(ListOffset, ScrollBehavior),
    Scrollbar(Point<Pixels>, ScrollBehavior),
}

/// A sticky candidate in a low-level list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StickyItem {
    /// The candidate's item index.
    pub index: usize,
    /// The candidate's canonical stacking relation.
    pub stacking: StickyStacking,
}

impl StickyItem {
    /// Creates a sticky candidate with its canonical stacking relation.
    pub fn new(index: usize, stacking: StickyStacking) -> Self {
        Self { index, stacking }
    }
}

impl From<usize> for StickyItem {
    fn from(index: usize) -> Self {
        Self::new(index, StickyStacking::Level(0))
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StickyItems {
    candidates: Vec<StickyItem>,
    relation: StickyRelationIndex,
}

impl StickyItems {
    pub(crate) fn new<I>(items: I, item_count: usize) -> Self
    where
        I: IntoIterator,
        I::Item: Into<StickyItem>,
    {
        let mut candidates = items
            .into_iter()
            .map(Into::into)
            .filter(|item: &StickyItem| item.index < item_count)
            .collect::<Vec<_>>();
        // Stable sorting makes the first declaration win for duplicate indices.
        candidates.sort_by_key(|item| item.index);
        candidates.dedup_by_key(|item| item.index);
        let relation = StickyRelationIndex::from_candidates(
            candidates
                .iter()
                .map(|item| (item.index, item.stacking.clone())),
        );
        Self {
            candidates,
            relation,
        }
    }

    pub(crate) fn active_item_indices_before(&self, item_index: usize) -> Vec<usize> {
        item_index
            .checked_sub(1)
            .and_then(|previous| self.relation.source_at_or_before(previous))
            .map(|source| {
                self.relation
                    .active_chain(source)
                    .into_iter()
                    .map(|position| self.candidates[position].index)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn predecessor_item_indices(&self, item_index: usize) -> Vec<usize> {
        let Ok(position) = self
            .candidates
            .binary_search_by_key(&item_index, |item| item.index)
        else {
            return Vec::new();
        };
        let mut chain = self.relation.active_chain(position);
        chain.pop();
        chain
            .into_iter()
            .map(|position| self.candidates[position].index)
            .collect()
    }

    pub(crate) fn contains(&self, item_index: usize) -> bool {
        self.candidates
            .binary_search_by_key(&item_index, |item| item.index)
            .is_ok()
    }

    pub(crate) fn next_replacement_after(&self, item_index: usize) -> Option<(usize, usize)> {
        let position = self
            .candidates
            .binary_search_by_key(&item_index, |item| item.index)
            .ok()?;
        let (next, retained_depth) = self.relation.next_replacement(position)?;
        Some((self.candidates[next].index, retained_depth))
    }
}

/// Deferred scroll adjustment applied after the scroll-top item has been remeasured.
///
/// An absolute pending scroll preserves the same pixel offset into the item, which keeps
/// visible text stable while content is appended to or removed from that item. A
/// proportional pending scroll preserves the same fractional position within the item,
/// which is useful when the whole list is being resized and each item scales similarly.
#[derive(Clone)]
enum PendingScroll {
    /// Preserve the same pixel offset into the item after it is remeasured.
    Absolute { item_ix: usize, offset: Pixels },
    /// Preserve the same fractional offset into the item after it is remeasured.
    Proportional(PendingScrollFraction),
}

/// Keeps track of a fractional scroll position within an item for restoration
/// after remeasurement.
#[derive(Clone)]
struct PendingScrollFraction {
    /// The index of the item to scroll within.
    item_ix: usize,
    /// Fractional offset (0.0 to 1.0) within the item's height.
    fraction: f32,
}

/// Determines how remeasurement preserves the scroll position when the scroll-top item
/// changes height.
enum ScrollAnchor {
    /// Preserve the same pixel offset into the scroll-top item.
    Absolute,
    /// Preserve the same fractional position within the scroll-top item.
    Proportional,
}

/// Controls whether the list automatically follows new content at the end.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FollowMode {
    /// Normal scrolling — no automatic following.
    #[default]
    Normal,
    /// The list should auto-scroll along with the tail, when scrolled to bottom.
    Tail,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum FollowState {
    #[default]
    Normal,
    Tail {
        is_following: bool,
    },
}

impl FollowState {
    fn is_following(&self) -> bool {
        matches!(self, FollowState::Tail { is_following: true })
    }

    fn has_stopped_following(&self) -> bool {
        matches!(
            self,
            FollowState::Tail {
                is_following: false
            }
        )
    }

    fn start_following(&mut self) {
        if let FollowState::Tail {
            is_following: false,
        } = self
        {
            *self = FollowState::Tail { is_following: true };
        }
    }

    fn stop_following(&mut self) {
        if let FollowState::Tail { is_following: true } = self {
            *self = FollowState::Tail {
                is_following: false,
            };
        }
    }
}

/// Whether the list is scrolling from top to bottom or bottom to top.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListAlignment {
    /// The list is scrolling from top to bottom, like most lists.
    Top,
    /// The list is scrolling from bottom to top, like a chat log.
    Bottom,
}

/// A scroll event that has been converted to be in terms of the list's items.
pub struct ListScrollEvent {
    /// The range of items currently visible in the list, after applying the scroll event.
    pub visible_range: Range<usize>,

    /// The number of items that are currently visible in the list, after applying the scroll event.
    pub count: usize,

    /// Whether the list has been scrolled.
    pub is_scrolled: bool,

    /// Whether the list is currently in follow-tail mode (auto-scrolling to end).
    pub is_following_tail: bool,
}

/// The sizing behavior to apply during layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ListSizingBehavior {
    /// The list should calculate its size based on the size of its items.
    Infer,
    /// The list should not calculate a fixed size.
    #[default]
    Auto,
}

/// The measuring behavior to apply during layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ListMeasuringBehavior {
    /// Measure all items in the list.
    /// Note: This can be expensive for the first frame in a large list.
    Measure(bool),
    /// Only measure visible items
    #[default]
    Visible,
}

impl ListMeasuringBehavior {
    fn reset(&mut self) {
        match self {
            ListMeasuringBehavior::Measure(has_measured) => *has_measured = false,
            ListMeasuringBehavior::Visible => {}
        }
    }
}

/// The horizontal sizing behavior to apply during layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ListHorizontalSizingBehavior {
    /// List items' width can never exceed the width of the list.
    #[default]
    FitList,
    /// List items' width may go over the width of the list, if any item is wider.
    Unconstrained,
}

struct LayoutItemsResponse {
    max_item_width: Pixels,
    scroll_top: ListOffset,
    item_layouts: VecDeque<ItemLayout>,
    scroll_context: Option<ScrollContainerContext>,
}

struct ItemLayout {
    index: usize,
    element: AnyElement,
    size: Size<Pixels>,
    sticky_stack_offset: Option<Pixels>,
    sticky_push_offset: Pixels,
}

/// Frame state used by the [List] element after layout.
pub struct ListPrepaintState {
    hitbox: Hitbox,
    layout: LayoutItemsResponse,
}

#[derive(Clone)]
enum ListItem {
    Unmeasured {
        size_hint: Option<Size<Pixels>>,
        focus_handle: Option<FocusHandle>,
    },
    Measured {
        size: Size<Pixels>,
        focus_handle: Option<FocusHandle>,
    },
}

impl ListItem {
    fn size(&self) -> Option<Size<Pixels>> {
        if let ListItem::Measured { size, .. } = self {
            Some(*size)
        } else {
            None
        }
    }

    fn size_hint(&self) -> Option<Size<Pixels>> {
        match self {
            ListItem::Measured { size, .. } => Some(*size),
            ListItem::Unmeasured { size_hint, .. } => *size_hint,
        }
    }

    fn focus_handle(&self) -> Option<FocusHandle> {
        match self {
            ListItem::Unmeasured { focus_handle, .. } | ListItem::Measured { focus_handle, .. } => {
                focus_handle.clone()
            }
        }
    }

    fn contains_focused(&self, window: &Window, cx: &App) -> bool {
        match self {
            ListItem::Unmeasured { focus_handle, .. } | ListItem::Measured { focus_handle, .. } => {
                focus_handle
                    .as_ref()
                    .is_some_and(|handle| handle.contains_focused(window, cx))
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ListItemSummary {
    count: usize,
    rendered_count: usize,
    unrendered_count: usize,
    height: Pixels,
    has_focus_handles: bool,
    has_unknown_height: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Count(usize);

#[derive(Clone, Debug, Default)]
struct Height(Pixels);

impl ListState {
    /// Construct a new list state, for storage on a view.
    ///
    /// The overdraw parameter controls how much extra space is rendered
    /// above and below the visible area. Elements within this area will
    /// be measured even though they are not visible. This can help ensure
    /// that the list doesn't flicker or pop in when scrolling.
    pub fn new(item_count: usize, alignment: ListAlignment, overdraw: Pixels) -> Self {
        let this = Self(Rc::new(RefCell::new(StateInner {
            last_layout_bounds: None,
            last_padding: None,
            items: SumTree::default(),
            logical_scroll_top: None,
            alignment,
            overdraw,
            scroll_handler: None,
            reset: false,
            scrollbar_drag_start_height: None,
            scrollbar_drag_start_items: None,
            measuring_behavior: ListMeasuringBehavior::default(),
            item_size_hint: None,
            pending_scroll: None,
            pending_programmatic_scroll: None,
            follow_state: FollowState::default(),
            sticky_items: StickyItems::default(),
            activity: ScrollActivity::default(),
        })));
        this.splice(0..0, item_count);
        this
    }

    /// Set the list to measure all items in the list in the first layout phase.
    ///
    /// This is useful for ensuring that the scrollbar size is correct instead of based on only rendered elements.
    pub fn measure_all(self) -> Self {
        self.0.borrow_mut().measuring_behavior = ListMeasuringBehavior::Measure(false);
        self
    }

    /// Set the default height hint used by unmeasured items.
    ///
    /// Hints keep scroll metrics stable for large lists whose rows are measured lazily. Visible
    /// items are still rendered and measured normally, replacing the hint with their actual size.
    pub fn with_item_height_hint(self, height: Pixels) -> Self {
        self.set_item_height_hint(height);
        self
    }

    /// Sets the items that may contain sticky positioned descendants.
    ///
    /// Plain indices map to [`StickyStacking::Level(0)`].
    pub fn set_sticky_items<I>(&self, items: I)
    where
        I: IntoIterator,
        I::Item: Into<StickyItem>,
    {
        let mut state = self.0.borrow_mut();
        let item_count = state.items.summary().count;
        state.sticky_items = StickyItems::new(items, item_count);
    }

    /// Builder-style variant of [`Self::set_sticky_items`].
    pub fn with_sticky_items<I>(self, items: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<StickyItem>,
    {
        self.set_sticky_items(items);
        self
    }

    /// Update the default height hint used by unmeasured items.
    pub fn set_item_height_hint(&self, height: Pixels) {
        let state = &mut *self.0.borrow_mut();
        let hint = Some(size(px(0.), height.max(px(1.))));
        state.item_size_hint = hint;

        let new_items = SumTree::from_iter(
            state.items.iter().map(|item| match item {
                ListItem::Measured { size, focus_handle } => ListItem::Measured {
                    size: *size,
                    focus_handle: focus_handle.clone(),
                },
                ListItem::Unmeasured { focus_handle, .. } => ListItem::Unmeasured {
                    size_hint: hint,
                    focus_handle: focus_handle.clone(),
                },
            }),
            (),
        );
        state.items = new_items;
    }

    /// Pre-populate every unmeasured item with a uniform height hint so the scrollbar thumb
    /// is correctly sized from the first frame, without measuring all items up front.
    ///
    /// As items are actually rendered their real heights replace the hint, so the scrollbar
    /// converges to the exact size over time. This is a cheaper alternative to [`Self::measure_all`]
    /// for lists where items have roughly uniform heights (e.g. table rows).
    pub fn with_uniform_item_height(self, height: Pixels) -> Self {
        self.apply_uniform_item_height(height);
        self
    }
    /// Reset this instantiation of the list state.
    ///
    /// Note that this will cause scroll events to be dropped until the next paint.
    pub fn reset(&self, element_count: usize) {
        let old_count = {
            let state = &mut *self.0.borrow_mut();
            state.reset = true;
            state.measuring_behavior.reset();
            state.activity.stop();
            state.pending_programmatic_scroll = None;
            state.logical_scroll_top = None;
            state.pending_scroll = None;
            state.scrollbar_drag_start_height = None;
            state.scrollbar_drag_start_items = None;
            state.items.summary().count
        };

        self.splice(0..old_count, element_count);
    }

    /// Reset the list to `element_count` items, pre-populating every item with a
    /// uniform height hint so the scrollbar thumb is correctly sized from the first
    /// frame even for off-screen items.
    pub fn reset_with_uniform_height(&self, element_count: usize, height: Pixels) {
        self.reset(element_count);
        self.apply_uniform_item_height(height);
    }

    fn apply_uniform_item_height(&self, height: Pixels) {
        let size_hint = Size {
            width: px(0.),
            height,
        };
        let mut state = self.0.borrow_mut();
        let new_items = state
            .items
            .iter()
            .map(|item| ListItem::Unmeasured {
                size_hint: Some(item.size_hint().unwrap_or(size_hint)),
                focus_handle: item.focus_handle(),
            })
            .collect::<Vec<_>>();
        let mut tree = SumTree::default();
        tree.extend(new_items, ());
        state.items = tree;
    }

    /// Remeasure all items while preserving proportional scroll position.
    ///
    /// Use this when item heights may have changed (e.g., font size changes)
    /// but the number and identity of items remains the same.
    pub fn remeasure(&self) {
        let count = self.item_count();
        self.remeasure_items_with_scroll_anchor(0..count, ScrollAnchor::Proportional);
    }

    /// Mark items in `range` as needing remeasurement while preserving
    /// the current scroll position. Unlike [`Self::splice`], this does
    /// not change the number of items or blow away `logical_scroll_top`.
    ///
    /// Use this when an item's content has changed and its rendered
    /// height may be different (e.g., streaming text, tool results
    /// loading), but the item itself still exists at the same index.
    pub fn remeasure_items(&self, range: Range<usize>) {
        self.remeasure_items_with_scroll_anchor(range, ScrollAnchor::Absolute);
    }

    fn remeasure_items_with_scroll_anchor(&self, range: Range<usize>, scroll_anchor: ScrollAnchor) {
        let state = &mut *self.0.borrow_mut();

        if let Some(scroll_top) = state.logical_scroll_top {
            if range.contains(&scroll_top.item_ix) {
                state.pending_scroll = match scroll_anchor {
                    ScrollAnchor::Absolute => Some(PendingScroll::Absolute {
                        item_ix: scroll_top.item_ix,
                        offset: scroll_top.offset_in_item,
                    }),
                    ScrollAnchor::Proportional => {
                        // If the scroll-top item falls within the remeasured range,
                        // store a fractional offset so the layout can restore the
                        // proportional scroll position after the item is re-rendered
                        // at its new height.
                        let mut cursor = state.items.cursor::<Count>(());
                        cursor.seek(&Count(scroll_top.item_ix), Bias::Right);

                        cursor
                            .item()
                            .and_then(|item| {
                                item.size().map(|size| {
                                    let fraction = if size.height.0 > 0.0 {
                                        (scroll_top.offset_in_item.0 / size.height.0)
                                            .clamp(0.0, 1.0)
                                    } else {
                                        0.0
                                    };

                                    PendingScroll::Proportional(PendingScrollFraction {
                                        item_ix: scroll_top.item_ix,
                                        fraction,
                                    })
                                })
                            })
                            .or_else(|| state.pending_scroll.clone())
                    }
                };
            }
        }

        // Rebuild the tree, replacing items in the range with
        // Unmeasured copies that keep their focus handles.
        let new_items = {
            let mut cursor = state.items.cursor::<Count>(());
            let mut new_items = cursor.slice(&Count(range.start), Bias::Right);
            let invalidated = cursor.slice(&Count(range.end), Bias::Right);
            new_items.extend(
                invalidated.iter().map(|item| ListItem::Unmeasured {
                    size_hint: item.size_hint(),
                    focus_handle: item.focus_handle(),
                }),
                (),
            );
            new_items.append(cursor.suffix(), ());
            new_items
        };
        state.items = new_items;
        state.measuring_behavior.reset();
    }

    /// The number of items in this list.
    pub fn item_count(&self) -> usize {
        self.0.borrow().items.summary().count
    }

    /// Whether the list is scrolled to the end, or `None` if the list is
    /// not scrollable or the total content height is not yet known.
    pub fn is_scrolled_to_end(&self) -> Option<bool> {
        let state = self.0.borrow();
        let bounds = state.last_layout_bounds?;
        let summary = state.items.summary();
        if summary.has_unknown_height {
            return None;
        }
        let padding = state.last_padding.unwrap_or_default();
        let content_height = summary.height + padding.top + padding.bottom;
        let scroll_max = (content_height - bounds.size.height).max(px(0.));
        if scroll_max <= px(0.) {
            return None;
        }
        let scroll_top = state.scroll_top(&state.logical_scroll_top());
        Some(scroll_top >= scroll_max)
    }

    /// Try to read the number of items without panicking if the state is already borrowed.
    pub fn try_item_count(&self) -> Option<usize> {
        self.0
            .try_borrow()
            .ok()
            .map(|state| state.items.summary().count)
    }

    /// Inform the list state that the items in `old_range` have been replaced
    /// by `count` new items that must be recalculated.
    pub fn splice(&self, old_range: Range<usize>, count: usize) {
        self.splice_focusable(old_range, (0..count).map(|_| None))
    }

    /// Register with the list state that the items in `old_range` have been replaced
    /// by new items. As opposed to [`Self::splice`], this method allows an iterator of optional focus handles
    /// to be supplied to properly integrate with items in the list that can be focused. If a focused item
    /// is scrolled out of view, the list will continue to render it to allow keyboard interaction.
    pub fn splice_focusable(
        &self,
        old_range: Range<usize>,
        focus_handles: impl IntoIterator<Item = Option<FocusHandle>>,
    ) {
        let state = &mut *self.0.borrow_mut();

        let mut old_items = state.items.cursor::<Count>(());
        let mut new_items = old_items.slice(&Count(old_range.start), Bias::Right);
        old_items.seek_forward(&Count(old_range.end), Bias::Right);

        let mut spliced_count = 0;
        new_items.extend(
            focus_handles.into_iter().map(|focus_handle| {
                spliced_count += 1;
                ListItem::Unmeasured {
                    size_hint: state.item_size_hint,
                    focus_handle,
                }
            }),
            (),
        );
        new_items.append(old_items.suffix(), ());
        drop(old_items);
        state.items = new_items;

        if let Some(ListOffset {
            item_ix,
            offset_in_item,
        }) = state.logical_scroll_top.as_mut()
        {
            if old_range.contains(item_ix) {
                *item_ix = old_range.start;
                *offset_in_item = px(0.);
            } else if old_range.end <= *item_ix {
                *item_ix = *item_ix - (old_range.end - old_range.start) + spliced_count;
            }
        }
    }

    /// Set a handler that will be called when the list is scrolled.
    pub fn set_scroll_handler(
        &self,
        handler: impl FnMut(&ListScrollEvent, &mut Window, &mut App) + 'static,
    ) {
        self.0.borrow_mut().scroll_handler = Some(Box::new(handler))
    }

    /// Try to set the scroll handler without panicking if the state is already borrowed.
    pub fn try_set_scroll_handler(
        &self,
        handler: impl FnMut(&ListScrollEvent, &mut Window, &mut App) + 'static,
    ) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.scroll_handler = Some(Box::new(handler));
        true
    }

    /// Get the current scroll offset, in terms of the list's items.
    pub fn logical_scroll_top(&self) -> ListOffset {
        self.0.borrow().logical_scroll_top()
    }

    /// Try to get the current logical scroll offset without panicking if the state is already borrowed.
    pub fn try_logical_scroll_top(&self) -> Option<ListOffset> {
        self.0
            .try_borrow()
            .ok()
            .map(|state| state.logical_scroll_top())
    }

    /// Scroll the list by the given offset
    pub fn scroll_by(&self, distance: Pixels) {
        if distance == px(0.) {
            return;
        }

        let current_offset = self.logical_scroll_top();
        let state = &mut *self.0.borrow_mut();
        state.activity.stop();
        state.pending_programmatic_scroll = None;

        if distance < px(0.) {
            state.follow_state.stop_following();
        }

        let mut cursor = state.items.cursor::<ListItemSummary>(());
        cursor.seek(&Count(current_offset.item_ix), Bias::Right);

        let start_pixel_offset = cursor.start().height + current_offset.offset_in_item;
        let new_pixel_offset = (start_pixel_offset + distance).max(px(0.));
        if new_pixel_offset > start_pixel_offset {
            cursor.seek_forward(&Height(new_pixel_offset), Bias::Right);
        } else {
            cursor.seek(&Height(new_pixel_offset), Bias::Right);
        }

        let scroll_top = ListOffset {
            item_ix: cursor.start().count,
            offset_in_item: new_pixel_offset - cursor.start().height,
        };
        drop(cursor);
        state.rebase_pending_scroll(scroll_top);
        state.logical_scroll_top = Some(scroll_top);
    }

    /// Scroll the list to the very end (past the last item).
    ///
    /// Unlike [`scroll_to_reveal_item`], this uses the total item count as the
    /// anchor, so the list's layout pass will walk backwards from the end and
    /// always show the bottom of the last item — even when that item is still
    /// growing (e.g. during streaming).
    pub fn scroll_to_end(&self) {
        let state = &mut *self.0.borrow_mut();
        state.activity.stop();
        state.pending_programmatic_scroll = None;
        let item_count = state.items.summary().count;
        state.logical_scroll_top = Some(ListOffset {
            item_ix: item_count,
            offset_in_item: px(0.),
        });
    }

    /// Try to scroll the list to the end without panicking if the state is already borrowed.
    pub fn try_scroll_to_end(&self) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.activity.stop();
        state.pending_programmatic_scroll = None;
        let item_count = state.items.summary().count;
        state.pending_scroll = None;
        state.logical_scroll_top = Some(ListOffset {
            item_ix: item_count,
            offset_in_item: px(0.),
        });
        true
    }

    /// Set the follow mode for the list. In `Tail` mode, the list
    /// will auto-scroll to the end and re-engage after the user
    /// scrolls back to the bottom. In `Normal` mode, no automatic
    /// following occurs.
    pub fn set_follow_mode(&self, mode: FollowMode) {
        let state = &mut *self.0.borrow_mut();
        state.activity.stop();
        state.pending_programmatic_scroll = None;

        match mode {
            FollowMode::Normal => {
                state.follow_state = FollowState::Normal;
            }
            FollowMode::Tail => {
                state.follow_state = FollowState::Tail { is_following: true };
                if matches!(mode, FollowMode::Tail) {
                    let item_count = state.items.summary().count;
                    state.logical_scroll_top = Some(ListOffset {
                        item_ix: item_count,
                        offset_in_item: px(0.),
                    });
                }
            }
        }
    }

    /// Pause tail-following, freezing the list at its current scroll
    /// position. Unlike [`Self::set_follow_mode`] with [`FollowMode::Normal`],
    /// this keeps the list in `Tail` mode, so it will resume following
    /// automatically once the view returns to the bottom. No-op when the list
    /// isn't currently following.
    ///
    /// Useful when something other than the user grows an item (e.g. zooming a
    /// diagram) and the current position should stay put rather than snapping
    /// to the end.
    pub fn pause_following_tail(&self) {
        self.0.borrow_mut().follow_state.stop_following();
    }

    /// Returns whether the list is currently actively following the
    /// tail (snapping to the end on each layout).
    pub fn is_following_tail(&self) -> bool {
        matches!(
            self.0.borrow().follow_state,
            FollowState::Tail { is_following: true }
        )
    }

    /// Scroll the list to the given offset
    pub fn scroll_to(&self, mut scroll_top: ListOffset) {
        let state = &mut *self.0.borrow_mut();
        state.activity.stop();
        state.pending_programmatic_scroll = None;
        let item_count = state.items.summary().count;
        if scroll_top.item_ix >= item_count {
            scroll_top.item_ix = item_count;
            scroll_top.offset_in_item = px(0.);
        }

        if scroll_top.item_ix < item_count {
            state.follow_state.stop_following();
        }

        state.rebase_pending_scroll(scroll_top);
        state.logical_scroll_top = Some(scroll_top);
    }

    /// Try to scroll the list without panicking if the state is already borrowed.
    pub fn try_scroll_to(&self, mut scroll_top: ListOffset) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.activity.stop();
        state.pending_programmatic_scroll = None;
        let item_count = state.items.summary().count;
        if scroll_top.item_ix >= item_count {
            scroll_top.item_ix = item_count;
            scroll_top.offset_in_item = px(0.);
        }

        state.logical_scroll_top = Some(scroll_top);
        true
    }

    /// Queue an explicit programmatic logical offset with its final behavior.
    ///
    /// Smooth requests retain the logical target until its prefix geometry is known, then reuse
    /// the shared [`ScrollActivity`] driver. Instant and zero-duration requests preserve the
    /// existing exact logical-offset behavior.
    pub fn scroll_to_with_behavior(&self, mut scroll_top: ListOffset, behavior: ScrollBehavior) {
        let state = &mut *self.0.borrow_mut();
        state.activity.stop();
        let item_count = state.items.summary().count;
        if scroll_top.item_ix >= item_count {
            scroll_top.item_ix = item_count;
            scroll_top.offset_in_item = px(0.);
        }

        if scroll_top.item_ix < item_count {
            state.follow_state.stop_following();
        }

        if matches!(behavior, ScrollBehavior::Instant)
            || matches!(behavior, ScrollBehavior::Smooth(duration) if duration.is_zero())
        {
            if let Some((target, maximum)) = state.resolve_programmatic_offset(scroll_top) {
                let current = state.scroll_top(&state.logical_scroll_top());
                state.activity.start_programmatic_at(
                    Point::new(px(0.), current),
                    Point::new(px(0.), target),
                    Point::new(px(0.), maximum),
                    behavior,
                    crate::Instant::now(),
                );
                state.pending_programmatic_scroll = None;
                state.logical_scroll_top = Some(scroll_top);
                return;
            }
        }
        state.pending_programmatic_scroll =
            Some(PendingProgrammaticScroll::Logical(scroll_top, behavior));
    }

    /// Try to queue an explicit programmatic logical offset without panicking on re-entrancy.
    pub fn try_scroll_to_with_behavior(
        &self,
        mut scroll_top: ListOffset,
        behavior: ScrollBehavior,
    ) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.activity.stop();
        let item_count = state.items.summary().count;
        if scroll_top.item_ix >= item_count {
            scroll_top.item_ix = item_count;
            scroll_top.offset_in_item = px(0.);
        }

        if scroll_top.item_ix < item_count {
            state.follow_state.stop_following();
        }

        if matches!(behavior, ScrollBehavior::Instant)
            || matches!(behavior, ScrollBehavior::Smooth(duration) if duration.is_zero())
        {
            if let Some((target, maximum)) = state.resolve_programmatic_offset(scroll_top) {
                let current = state.scroll_top(&state.logical_scroll_top());
                state.activity.start_programmatic_at(
                    Point::new(px(0.), current),
                    Point::new(px(0.), target),
                    Point::new(px(0.), maximum),
                    behavior,
                    crate::Instant::now(),
                );
                state.pending_programmatic_scroll = None;
                state.logical_scroll_top = Some(scroll_top);
                return true;
            }
        }
        state.pending_programmatic_scroll =
            Some(PendingProgrammaticScroll::Logical(scroll_top, behavior));
        true
    }

    /// Advance this owner's active programmatic driver without relying on a platform frame pump.
    /// Generation `1` is the first request on a fresh owner.
    #[cfg(any(test, feature = "test-support"))]
    pub fn advance_programmatic_scroll_for_test(
        &self,
        generation: u64,
        timestamp: Instant,
        current_view: EntityId,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        apply_list_programmatic_frame(self, generation, timestamp, current_view, window, cx)
            .is_some()
    }

    /// Scroll the list to the given item, such that the item is fully visible.
    pub fn scroll_to_reveal_item(&self, ix: usize) {
        let state = &mut *self.0.borrow_mut();
        state.activity.stop();
        state.pending_programmatic_scroll = None;

        let mut scroll_top = state.logical_scroll_top();
        let height = state
            .last_layout_bounds
            .map_or(px(0.), |bounds| bounds.size.height);
        let padding = state.last_padding.unwrap_or_default();

        if ix <= scroll_top.item_ix {
            scroll_top.item_ix = ix;
            scroll_top.offset_in_item = px(0.);
        } else {
            let mut cursor = state.items.cursor::<ListItemSummary>(());
            cursor.seek(&Count(ix + 1), Bias::Right);
            let bottom = cursor.start().height + padding.top;
            let goal_top = px(0.).max(bottom - height + padding.bottom);

            cursor.seek(&Height(goal_top), Bias::Left);
            let start_ix = cursor.start().count;
            let start_item_top = cursor.start().height;

            if start_ix >= scroll_top.item_ix {
                scroll_top.item_ix = start_ix;
                scroll_top.offset_in_item = goal_top - start_item_top;
            }
        }

        state.rebase_pending_scroll(scroll_top);
        state.logical_scroll_top = Some(scroll_top);
    }

    /// Get the bounds for the given item in window coordinates, if it's
    /// been rendered.
    pub fn bounds_for_item(&self, ix: usize) -> Option<Bounds<Pixels>> {
        let state = &*self.0.borrow();

        let bounds = state.last_layout_bounds.unwrap_or_default();
        let scroll_top = state.logical_scroll_top();
        if ix < scroll_top.item_ix {
            return None;
        }

        let mut cursor = state.items.cursor::<Dimensions<Count, Height>>(());
        cursor.seek(&Count(scroll_top.item_ix), Bias::Right);

        let scroll_top = cursor.start().1.0 + scroll_top.offset_in_item;

        cursor.seek_forward(&Count(ix), Bias::Right);
        if let Some(&ListItem::Measured { size, .. }) = cursor.item() {
            let &Dimensions(Count(count), Height(top), _) = cursor.start();
            if count == ix {
                let top = bounds.top() + top - scroll_top;
                return Some(Bounds::from_corners(
                    point(bounds.left(), top),
                    point(bounds.right(), top + size.height),
                ));
            }
        }
        None
    }

    /// Try to get the bounds for a rendered item without panicking if the state is already borrowed.
    pub fn try_bounds_for_item(&self, ix: usize) -> Option<Bounds<Pixels>> {
        let Ok(state) = self.0.try_borrow() else {
            return None;
        };

        let bounds = state.last_layout_bounds.unwrap_or_default();
        let scroll_top = state.logical_scroll_top();
        if ix < scroll_top.item_ix {
            return None;
        }

        let mut cursor = state.items.cursor::<Dimensions<Count, Height>>(());
        cursor.seek(&Count(scroll_top.item_ix), Bias::Right);

        let scroll_top = cursor.start().1.0 + scroll_top.offset_in_item;

        cursor.seek_forward(&Count(ix), Bias::Right);
        if let Some(&ListItem::Measured { size, .. }) = cursor.item() {
            let &Dimensions(Count(count), Height(top), _) = cursor.start();
            if count == ix {
                let top = bounds.top() + top - scroll_top;
                return Some(Bounds::from_corners(
                    point(bounds.left(), top),
                    point(bounds.right(), top + size.height),
                ));
            }
        }
        None
    }

    /// Call this method when the user starts dragging the scrollbar.
    ///
    /// This will prevent the height reported to the scrollbar from changing during the drag
    /// as items in the overdraw get measured, and help offset scroll position changes accordingly.
    pub fn scrollbar_drag_started(&self) {
        let mut state = self.0.borrow_mut();
        state.scrollbar_drag_start_height = Some(state.items.summary().height);
        state.scrollbar_drag_start_items = Some(state.items.clone());
    }

    /// Called when the user stops dragging the scrollbar.
    ///
    /// See `scrollbar_drag_started`.
    pub fn scrollbar_drag_ended(&self) {
        let mut state = self.0.borrow_mut();
        state.scrollbar_drag_start_height.take();
        state.scrollbar_drag_start_items.take();
    }

    /// Returns `true` if the scrollbar is currently being dragged.
    ///
    /// This is set between [`scrollbar_drag_started`](Self::scrollbar_drag_started)
    /// and [`scrollbar_drag_ended`](Self::scrollbar_drag_ended) calls. Useful for
    /// consumers that need to distinguish scrollbar drags from wheel/trackpad scrolls,
    /// e.g. to suppress auto-scroll behavior during manual positioning.
    pub fn is_scrollbar_dragging(&self) -> bool {
        self.0.borrow().scrollbar_drag_start_height.is_some()
    }

    /// Set the offset from the scrollbar
    pub fn request_offset_from_scrollbar(&self, point: Point<Pixels>) {
        let mut state = self.0.borrow_mut();
        state.activity.stop();
        state.pending_programmatic_scroll = None;
        state.request_offset_from_scrollbar(point);
    }

    /// Queue a scrollbar-derived physical offset through this list's shared programmatic driver.
    pub fn request_offset_from_scrollbar_with_behavior(
        &self,
        point: Point<Pixels>,
        behavior: ScrollBehavior,
    ) {
        let mut state = self.0.borrow_mut();
        state.activity.stop();
        if matches!(behavior, ScrollBehavior::Instant)
            || matches!(behavior, ScrollBehavior::Smooth(duration) if duration.is_zero())
        {
            if let Some((target, maximum)) = state.resolve_scrollbar_programmatic_offset(point) {
                let current = state.scroll_top(&state.logical_scroll_top());
                state.activity.start_programmatic_at(
                    Point::new(px(0.), current),
                    Point::new(px(0.), target),
                    Point::new(px(0.), maximum),
                    behavior,
                    crate::Instant::now(),
                );
                state.pending_programmatic_scroll = None;
                state.request_offset_from_scrollbar(Point::new(px(0.), -target));
                return;
            }
        }
        state.pending_programmatic_scroll =
            Some(PendingProgrammaticScroll::Scrollbar(point, behavior));
    }

    /// Try to queue a scrollbar-derived physical offset without panicking on re-entrancy.
    pub fn try_request_offset_from_scrollbar_with_behavior(
        &self,
        point: Point<Pixels>,
        behavior: ScrollBehavior,
    ) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.activity.stop();
        if matches!(behavior, ScrollBehavior::Instant)
            || matches!(behavior, ScrollBehavior::Smooth(duration) if duration.is_zero())
        {
            if let Some((target, maximum)) = state.resolve_scrollbar_programmatic_offset(point) {
                let current = state.scroll_top(&state.logical_scroll_top());
                state.activity.start_programmatic_at(
                    Point::new(px(0.), current),
                    Point::new(px(0.), target),
                    Point::new(px(0.), maximum),
                    behavior,
                    crate::Instant::now(),
                );
                state.pending_programmatic_scroll = None;
                state.request_offset_from_scrollbar(Point::new(px(0.), -target));
                return true;
            }
        }
        state.pending_programmatic_scroll =
            Some(PendingProgrammaticScroll::Scrollbar(point, behavior));
        true
    }

    /// Try to set the scrollbar-derived offset without panicking if the state is already borrowed.
    pub fn try_request_offset_from_scrollbar(&self, point: Point<Pixels>) -> bool {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return false;
        };
        state.activity.stop();
        state.pending_programmatic_scroll = None;
        state.request_offset_from_scrollbar(point);
        true
    }

    /// Returns the maximum scroll offset according to the items we have measured.
    /// This value remains constant while dragging to prevent the scrollbar from moving away unexpectedly.
    pub fn max_offset_for_scrollbar(&self) -> Point<Pixels> {
        let state = self.0.borrow();
        point(Pixels::ZERO, state.max_scroll_offset())
    }

    /// Try to get the maximum scrollbar offset without panicking if the state is already borrowed.
    pub fn try_max_offset_for_scrollbar(&self) -> Option<Point<Pixels>> {
        self.0
            .try_borrow()
            .ok()
            .map(|state| point(Pixels::ZERO, state.max_scroll_offset()))
    }

    /// Returns the current scroll offset adjusted for the scrollbar.
    ///
    /// The returned offset has a negative `y` component representing
    /// how far the content has scrolled.
    pub fn scroll_px_offset_for_scrollbar(&self) -> Point<Pixels> {
        let state = &self.0.borrow();

        if state.logical_scroll_top.is_none() && state.alignment == ListAlignment::Bottom {
            return Point::new(px(0.), -state.max_scroll_offset());
        }

        let logical_scroll_top = state.logical_scroll_top();

        let items = state
            .scrollbar_drag_start_items
            .as_ref()
            .unwrap_or(&state.items);
        let mut cursor = items.cursor::<ListItemSummary>(());
        let summary: ListItemSummary =
            cursor.summary(&Count(logical_scroll_top.item_ix), Bias::Right);
        let offset = summary.height + logical_scroll_top.offset_in_item;

        Point::new(px(0.), -offset)
    }

    /// Try to get the current scrollbar-adjusted pixel offset without panicking if the state is already borrowed.
    pub fn try_scroll_px_offset_for_scrollbar(&self) -> Option<Point<Pixels>> {
        let Ok(state) = self.0.try_borrow() else {
            return None;
        };

        if state.logical_scroll_top.is_none() && state.alignment == ListAlignment::Bottom {
            return Some(Point::new(px(0.), -state.max_scroll_offset()));
        }

        let logical_scroll_top = state.logical_scroll_top();

        let items = state
            .scrollbar_drag_start_items
            .as_ref()
            .unwrap_or(&state.items);
        let mut cursor = items.cursor::<ListItemSummary>(());
        let summary: ListItemSummary =
            cursor.summary(&Count(logical_scroll_top.item_ix), Bias::Right);
        let offset = summary.height + logical_scroll_top.offset_in_item;

        Some(Point::new(px(0.), -offset))
    }

    /// Return the bounds of the viewport in pixels.
    pub fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().last_layout_bounds.unwrap_or_default()
    }

    /// Try to get the viewport bounds without panicking if the state is already borrowed.
    pub fn try_viewport_bounds(&self) -> Option<Bounds<Pixels>> {
        self.0
            .try_borrow()
            .ok()
            .map(|state| state.last_layout_bounds.unwrap_or_default())
    }

    /// Try to read follow-tail state without panicking if the state is already borrowed.
    pub fn try_is_following_tail(&self) -> Option<bool> {
        self.0
            .try_borrow()
            .ok()
            .map(|state| matches!(state.follow_state, FollowState::Tail { is_following: true }))
    }

    /// Returns whether the item is entirely above the viewport, or `None` if
    /// the list has not measured enough layout to know.
    ///
    /// A zero-height viewport still yields a definitive answer: callers may
    /// size sibling UI based on this query (potentially squeezing the list
    /// itself to zero height), so returning `None` in that case would make
    /// the answer oscillate from frame to frame.
    pub fn item_is_above_viewport(&self, ix: usize) -> Option<bool> {
        let viewport_bounds = self.0.borrow().last_layout_bounds?;

        let scroll_top = self.logical_scroll_top();
        if ix < scroll_top.item_ix {
            // Rows before the logical scroll top have no item bounds, but
            // their position relative to the viewport is known from scroll state.
            return Some(true);
        }

        let item_bounds = self.bounds_for_item(ix)?;
        Some(item_bounds.bottom() <= viewport_bounds.top())
    }

    /// Returns whether the item is entirely below the viewport, or `None` if
    /// the list has not measured enough layout to know.
    ///
    /// See [`Self::item_is_above_viewport`] for why a zero-height viewport
    /// still yields a definitive answer.
    pub fn item_is_below_viewport(&self, ix: usize) -> Option<bool> {
        let viewport_bounds = self.0.borrow().last_layout_bounds?;

        let scroll_top = self.logical_scroll_top();
        if ix < scroll_top.item_ix {
            // Rows before the logical scroll top have no item bounds, but
            // their position relative to the viewport is known from scroll state.
            return Some(false);
        }

        let item_bounds = self.bounds_for_item(ix)?;
        Some(item_bounds.top() >= viewport_bounds.bottom())
    }
}

impl StateInner {
    /// Re-anchor a pending scroll adjustment from a remeasure onto a newly set
    /// scroll position, so it clamps to the remeasured item's new height on
    /// the next layout instead of reverting the scroll.
    fn rebase_pending_scroll(&mut self, scroll_top: ListOffset) {
        let Some(pending) = self.pending_scroll.take() else {
            return;
        };
        if scroll_top.item_ix >= self.items.summary().count {
            return;
        }

        self.pending_scroll = match pending {
            PendingScroll::Absolute { .. } => Some(PendingScroll::Absolute {
                item_ix: scroll_top.item_ix,
                offset: scroll_top.offset_in_item,
            }),
            PendingScroll::Proportional(_) => {
                let mut cursor = self.items.cursor::<Count>(());
                cursor.seek(&Count(scroll_top.item_ix), Bias::Right);
                cursor
                    .item()
                    .and_then(|item| item.size_hint())
                    .filter(|size| size.height.0 > 0.0)
                    .map(|size| {
                        PendingScroll::Proportional(PendingScrollFraction {
                            item_ix: scroll_top.item_ix,
                            fraction: (scroll_top.offset_in_item.0 / size.height.0).clamp(0.0, 1.0),
                        })
                    })
            }
        };
    }

    fn max_scroll_offset(&self) -> Pixels {
        let bounds = self.last_layout_bounds.unwrap_or_default();
        let height = self
            .scrollbar_drag_start_height
            .unwrap_or_else(|| self.items.summary().height);
        (height - bounds.size.height).max(px(0.))
    }

    fn resolve_programmatic_offset(&self, target: ListOffset) -> Option<(Pixels, Pixels)> {
        let bounds = self.last_layout_bounds?;
        let padding = self.last_padding.unwrap_or_default();
        let total = self.items.summary();
        if total.has_unknown_height {
            return None;
        }

        let mut cursor = self.items.cursor::<ListItemSummary>(());
        let prefix: ListItemSummary = cursor.summary(&Count(target.item_ix), Bias::Right);
        if prefix.has_unknown_height {
            return None;
        }
        let offset_in_item = self
            .items
            .iter()
            .nth(target.item_ix)
            .and_then(ListItem::size)
            .map_or(Pixels::ZERO, |size| {
                target.offset_in_item.clamp(px(0.), size.height)
            });
        let target = prefix.height + offset_in_item;
        let maximum =
            (total.height + padding.top + padding.bottom - bounds.size.height).max(px(0.));
        Some((target.clamp(px(0.), maximum), maximum))
    }

    fn visible_range(
        items: &SumTree<ListItem>,
        height: Pixels,
        scroll_top: &ListOffset,
    ) -> Range<usize> {
        let mut cursor = items.cursor::<ListItemSummary>(());
        cursor.seek(&Count(scroll_top.item_ix), Bias::Right);
        let start_y = cursor.start().height + scroll_top.offset_in_item;
        cursor.seek_forward(&Height(start_y + height), Bias::Left);
        scroll_top.item_ix..cursor.start().count + 1
    }

    fn scroll(
        &mut self,
        scroll_top: &ListOffset,
        height: Pixels,
        delta: Point<Pixels>,
        current_view: EntityId,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        // Drop scroll events after a reset, since we can't calculate
        // the new logical scroll top without the item heights
        if self.reset {
            return false;
        }

        let previous_scroll_top = self.scroll_top(scroll_top);
        let padding = self.last_padding.unwrap_or_default();
        let scroll_max =
            (self.items.summary().height + padding.top + padding.bottom - height).max(px(0.));
        let desired_scroll_top = (self.scroll_top(scroll_top) - delta.y).max(px(0.));
        let new_scroll_top = desired_scroll_top.min(scroll_max);
        let should_anchor_to_end =
            delta.y < px(0.) && scroll_max > px(0.) && desired_scroll_top >= scroll_max - px(1.);

        if self.alignment == ListAlignment::Bottom && new_scroll_top == scroll_max {
            self.pending_scroll = None;
            self.logical_scroll_top = None;
        } else if should_anchor_to_end {
            self.logical_scroll_top = Some(ListOffset {
                item_ix: self.items.summary().count,
                offset_in_item: px(0.),
            });
        } else {
            let (start, ..) =
                self.items
                    .find::<ListItemSummary, _>((), &Height(new_scroll_top), Bias::Right);
            let scroll_top = ListOffset {
                item_ix: start.count,
                offset_in_item: new_scroll_top - start.height,
            };
            // The user's scroll supersedes the position stashed by a
            // remeasure; re-anchor the pending adjustment so it doesn't revert
            // this scroll on the next layout.
            self.rebase_pending_scroll(scroll_top);
            self.logical_scroll_top = Some(scroll_top);
        }

        if delta.y > px(0.) {
            self.follow_state.stop_following();
        }

        let current_scroll_top = self.logical_scroll_top();
        let advanced = self.scroll_top(&current_scroll_top) != previous_scroll_top;
        if let Some(handler) = self.scroll_handler.as_mut() {
            let visible_range = Self::visible_range(&self.items, height, &current_scroll_top);
            handler(
                &ListScrollEvent {
                    visible_range,
                    count: self.items.summary().count,
                    is_scrolled: self.logical_scroll_top.is_some(),
                    is_following_tail: matches!(
                        self.follow_state,
                        FollowState::Tail { is_following: true }
                    ),
                },
                window,
                cx,
            );
        }

        cx.notify(current_view);
        advanced
    }

    fn logical_scroll_top(&self) -> ListOffset {
        self.logical_scroll_top
            .unwrap_or_else(|| match self.alignment {
                ListAlignment::Top => ListOffset {
                    item_ix: 0,
                    offset_in_item: px(0.),
                },
                ListAlignment::Bottom => ListOffset {
                    item_ix: self.items.summary().count,
                    offset_in_item: px(0.),
                },
            })
    }

    fn scroll_top(&self, logical_scroll_top: &ListOffset) -> Pixels {
        let (start, ..) = self.items.find::<ListItemSummary, _>(
            (),
            &Count(logical_scroll_top.item_ix),
            Bias::Right,
        );
        start.height + logical_scroll_top.offset_in_item
    }

    fn item_top(&self, item_ix: usize) -> Pixels {
        let (start, ..) = self
            .items
            .find::<ListItemSummary, _>((), &Count(item_ix), Bias::Right);
        start.height
    }

    fn item_height(&self, item_ix: usize) -> Option<Pixels> {
        let mut cursor = self.items.cursor::<Count>(());
        cursor.seek(&Count(item_ix), Bias::Right);
        cursor
            .item()
            .and_then(|item| item.size_hint().map(|size| size.height))
    }

    fn active_sticky_item_indices(&self, scroll_top: &ListOffset) -> Vec<usize> {
        self.sticky_items
            .active_item_indices_before(scroll_top.item_ix)
    }

    fn sticky_target_offset(&self, item_ix: usize) -> Pixels {
        self.sticky_items
            .predecessor_item_indices(item_ix)
            .into_iter()
            .fold(Pixels::ZERO, |offset, predecessor| {
                offset + self.item_height(predecessor).unwrap_or(Pixels::ZERO)
            })
    }

    fn layout_all_items(
        &mut self,
        available_width: Pixels,
        render_item: &mut RenderItemFn,
        window: &mut Window,
        cx: &mut App,
    ) {
        match &mut self.measuring_behavior {
            ListMeasuringBehavior::Visible => {
                return;
            }
            ListMeasuringBehavior::Measure(has_measured) => {
                if *has_measured {
                    return;
                }
                *has_measured = true;
            }
        }

        let mut cursor = self.items.cursor::<Count>(());
        let available_item_space = size(
            AvailableSpace::Definite(available_width),
            AvailableSpace::MinContent,
        );

        let mut measured_items = Vec::default();

        for (ix, item) in cursor.enumerate() {
            let size = item.size().or_else(|| item.size_hint()).unwrap_or_else(|| {
                let mut element = render_item(ix, window, cx);
                element.layout_as_root(available_item_space, window, cx)
            });

            measured_items.push(ListItem::Measured {
                size,
                focus_handle: item.focus_handle(),
            });
        }

        self.items = SumTree::from_iter(measured_items, ());
    }

    fn layout_items(
        &mut self,
        available_width: Option<Pixels>,
        available_height: Pixels,
        padding: &Edges<Pixels>,
        render_item: &mut RenderItemFn,
        window: &mut Window,
        cx: &mut App,
    ) -> LayoutItemsResponse {
        let old_items = self.items.clone();
        let mut measured_items = VecDeque::new();
        let mut item_layouts = VecDeque::new();
        let mut rendered_height = padding.top;
        let mut max_item_width = px(0.);
        let mut scroll_top = self.logical_scroll_top();

        if self.follow_state.is_following() {
            scroll_top = ListOffset {
                item_ix: self.items.summary().count,
                offset_in_item: px(0.),
            };
            self.logical_scroll_top = Some(scroll_top);
        }

        let mut rendered_focused_item = false;

        let available_item_space = size(
            available_width.map_or(AvailableSpace::MaxContent, |width| {
                AvailableSpace::Definite(width)
            }),
            AvailableSpace::MinContent,
        );

        let mut cursor = old_items.cursor::<Count>(());

        // Render items after the scroll top, including those in the trailing overdraw
        cursor.seek(&Count(scroll_top.item_ix), Bias::Right);
        for (ix, item) in cursor.by_ref().enumerate() {
            let visible_height = rendered_height - scroll_top.offset_in_item;
            if visible_height >= available_height + self.overdraw {
                break;
            }

            // Use the previously cached height and focus handle if available
            let mut size = item.size();

            // If we're within the visible area or the height wasn't cached, render and measure the item's element
            if visible_height < available_height || size.is_none() {
                let item_index = scroll_top.item_ix + ix;
                let mut element = render_item(item_index, window, cx);
                let element_size = element.layout_as_root(available_item_space, window, cx);
                size = Some(element_size);

                // If there's a pending scroll adjustment for the scroll-top
                // item, apply it.
                if ix == 0 {
                    if let Some(pending_scroll) = self.pending_scroll.take() {
                        match pending_scroll {
                            PendingScroll::Absolute { item_ix, offset }
                                if item_ix == scroll_top.item_ix =>
                            {
                                scroll_top.offset_in_item = offset.min(element_size.height);
                                self.logical_scroll_top = Some(scroll_top);
                            }
                            PendingScroll::Proportional(pending_scroll)
                                if pending_scroll.item_ix == scroll_top.item_ix =>
                            {
                                // Ensuring proportional scroll position is
                                // maintained after re-measuring.
                                scroll_top.offset_in_item =
                                    Pixels(pending_scroll.fraction * element_size.height.0);
                                self.logical_scroll_top = Some(scroll_top);
                            }
                            _ => {}
                        }
                    }
                }

                if visible_height < available_height {
                    item_layouts.push_back(ItemLayout {
                        index: item_index,
                        element,
                        size: element_size,
                        sticky_stack_offset: None,
                        sticky_push_offset: px(0.),
                    });
                    if item.contains_focused(window, cx) {
                        rendered_focused_item = true;
                    }
                }
            }

            let size = size.unwrap();
            rendered_height += size.height;
            max_item_width = max_item_width.max(size.width);
            measured_items.push_back(ListItem::Measured {
                size,
                focus_handle: item.focus_handle(),
            });
        }
        rendered_height += padding.bottom;

        // Prepare to start walking upward from the item at the scroll top.
        cursor.seek(&Count(scroll_top.item_ix), Bias::Right);

        // If the rendered items do not fill the visible region, then adjust
        // the scroll top upward.
        if rendered_height - scroll_top.offset_in_item < available_height {
            while rendered_height < available_height {
                cursor.prev();
                if let Some(item) = cursor.item() {
                    let item_index = cursor.start().0;
                    let mut element = render_item(item_index, window, cx);
                    let element_size = element.layout_as_root(available_item_space, window, cx);
                    let focus_handle = item.focus_handle();
                    rendered_height += element_size.height;
                    measured_items.push_front(ListItem::Measured {
                        size: element_size,
                        focus_handle,
                    });
                    item_layouts.push_front(ItemLayout {
                        index: item_index,
                        element,
                        size: element_size,
                        sticky_stack_offset: None,
                        sticky_push_offset: px(0.),
                    });
                    if item.contains_focused(window, cx) {
                        rendered_focused_item = true;
                    }
                } else {
                    break;
                }
            }

            scroll_top = ListOffset {
                item_ix: cursor.start().0,
                offset_in_item: rendered_height - available_height,
            };

            match self.alignment {
                ListAlignment::Top => {
                    scroll_top.offset_in_item = scroll_top.offset_in_item.max(px(0.));
                    self.logical_scroll_top = Some(scroll_top);
                }
                ListAlignment::Bottom => {
                    scroll_top = ListOffset {
                        item_ix: cursor.start().0,
                        offset_in_item: rendered_height - available_height,
                    };
                    self.logical_scroll_top = None;
                }
            };
        }

        // Measure items in the leading overdraw
        let mut leading_overdraw = scroll_top.offset_in_item;
        while leading_overdraw < self.overdraw {
            cursor.prev();
            if let Some(item) = cursor.item() {
                let size = if let ListItem::Measured { size, .. } = item {
                    *size
                } else {
                    let mut element = render_item(cursor.start().0, window, cx);
                    element.layout_as_root(available_item_space, window, cx)
                };

                leading_overdraw += size.height;
                measured_items.push_front(ListItem::Measured {
                    size,
                    focus_handle: item.focus_handle(),
                });
            } else {
                break;
            }
        }

        let measured_range = cursor.start().0..(cursor.start().0 + measured_items.len());
        let mut cursor = old_items.cursor::<Count>(());
        let mut new_items = cursor.slice(&Count(measured_range.start), Bias::Right);
        new_items.extend(measured_items, ());
        cursor.seek(&Count(measured_range.end), Bias::Right);
        new_items.append(cursor.suffix(), ());
        self.items = new_items;

        // If follow_tail mode is on but the user scrolled away
        // (is_following is false), check whether the current scroll
        // position has returned to the bottom.
        if self.follow_state.has_stopped_following() {
            let padding = self.last_padding.unwrap_or_default();
            let total_height = self.items.summary().height + padding.top + padding.bottom;
            let scroll_offset = self.scroll_top(&scroll_top);
            if scroll_offset + available_height >= total_height - px(1.0) {
                self.follow_state.start_following();
            }
        }

        // If none of the visible items are focused, check if an off-screen item is focused
        // and include it to be rendered after the visible items so keyboard interaction continues
        // to work for it.
        if !rendered_focused_item {
            let mut cursor = self
                .items
                .filter::<_, Count>((), |summary| summary.has_focus_handles);
            cursor.next();
            while let Some(item) = cursor.item() {
                if item.contains_focused(window, cx) {
                    let item_index = cursor.start().0;
                    let mut element = render_item(cursor.start().0, window, cx);
                    let element_size = element.layout_as_root(available_item_space, window, cx);
                    item_layouts.push_back(ItemLayout {
                        index: item_index,
                        element,
                        size: element_size,
                        sticky_stack_offset: None,
                        sticky_push_offset: px(0.),
                    });
                    break;
                }
                cursor.next();
            }
        }

        let active_sticky_items = self.active_sticky_item_indices(&scroll_top);
        let mut active_sticky_stack_offset = Pixels::ZERO;
        for &item_index in &active_sticky_items {
            let stack_offset = active_sticky_stack_offset;
            let item_height;
            if let Some(item) = item_layouts
                .iter_mut()
                .find(|item| item.index == item_index)
            {
                item.sticky_stack_offset = Some(stack_offset);
                item_height = item.size.height;
            } else {
                let mut element = render_item(item_index, window, cx);
                let element_size = element.layout_as_root(available_item_space, window, cx);
                item_layouts.push_back(ItemLayout {
                    index: item_index,
                    element,
                    size: element_size,
                    sticky_stack_offset: Some(stack_offset),
                    sticky_push_offset: px(0.),
                });
                item_height = element_size.height;
            }

            active_sticky_stack_offset += item_height;
        }

        let scroll_top_pixels = self.scroll_top(&scroll_top);
        if let Some((next_candidate, retained_depth)) = active_sticky_items
            .last()
            .and_then(|item_index| self.sticky_items.next_replacement_after(*item_index))
        {
            let incoming_start = self.item_top(next_candidate) - scroll_top_pixels;
            let push_offset = sticky_push_offset(active_sticky_stack_offset, incoming_start);
            for item_index in active_sticky_items.iter().skip(retained_depth) {
                if let Some(item) = item_layouts
                    .iter_mut()
                    .find(|item| item.index == *item_index)
                {
                    item.sticky_stack_offset =
                        item.sticky_stack_offset.map(|offset| offset + push_offset);
                    item.sticky_push_offset = push_offset;
                }
            }
        }

        let mut ordinary_items = VecDeque::new();
        let mut active_sticky_layouts = VecDeque::new();
        let mut incoming_sticky_items = VecDeque::new();
        while let Some(mut item) = item_layouts.pop_front() {
            let incoming_sticky_item =
                item.index >= scroll_top.item_ix && self.sticky_items.contains(item.index);
            if incoming_sticky_item {
                let target_offset = self.sticky_target_offset(item.index);
                let normal_offset = self.item_top(item.index) - scroll_top_pixels;
                if normal_offset < target_offset {
                    item.sticky_stack_offset = Some(target_offset);
                }
                incoming_sticky_items.push_back(item);
            } else if item.sticky_stack_offset.is_some() {
                active_sticky_layouts.push_front(item);
            } else {
                ordinary_items.push_back(item);
            }
        }
        ordinary_items.append(&mut active_sticky_layouts);
        ordinary_items.append(&mut incoming_sticky_items);
        item_layouts = ordinary_items;

        LayoutItemsResponse {
            max_item_width,
            scroll_top,
            item_layouts,
            scroll_context: None,
        }
    }

    fn prepaint_items(
        &mut self,
        bounds: Bounds<Pixels>,
        padding: Edges<Pixels>,
        autoscroll: bool,
        render_item: &mut RenderItemFn,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<LayoutItemsResponse, ListOffset> {
        window.transact(|window| {
            match self.measuring_behavior {
                ListMeasuringBehavior::Measure(has_measured) if !has_measured => {
                    self.layout_all_items(bounds.size.width, render_item, window, cx);
                }
                _ => {}
            }

            let mut layout_response = self.layout_items(
                Some(bounds.size.width),
                bounds.size.height,
                &padding,
                render_item,
                window,
                cx,
            );

            // Avoid honoring autoscroll requests from elements other than our children.
            window.take_autoscroll();

            // Only paint the visible items, if there is actually any space for them (taking padding into account)
            if bounds.size.height > padding.top + padding.bottom {
                let scroll_top = self.scroll_top(&layout_response.scroll_top);
                let scroll_offset = point(px(0.), -scroll_top);
                let content_size = size(
                    bounds.size.width,
                    self.items.summary().height + padding.top + padding.bottom,
                );
                let scrollport_bounds = bounds.intersect(&window.content_mask().bounds);
                let scroll_context = ScrollContainerContext {
                    scrollport_bounds,
                    content_bounds: Bounds::new(bounds.origin + scroll_offset, content_size),
                    scroll_offset,
                    overflow: point(Overflow::Hidden, Overflow::Scroll),
                };
                layout_response.scroll_context = Some(scroll_context);

                window.with_scroll_container_context(scroll_context, |window| {
                    for item in &mut layout_response.item_layouts {
                        let item_origin = point(
                            bounds.origin.x,
                            bounds.origin.y
                                + padding.top
                                + item
                                    .sticky_stack_offset
                                    .unwrap_or_else(|| self.item_top(item.index) - scroll_top),
                        );
                        window.with_content_mask(
                            Some(ContentMask::rectangular(bounds)),
                            |window| {
                                if item.sticky_stack_offset.is_some() {
                                    window.insert_hitbox(
                                        Bounds::new(item_origin, item.size),
                                        HitboxBehavior::BlockMouseExceptScroll,
                                    );
                                }
                                if item.sticky_push_offset < px(0.) {
                                    let mut item_scroll_context = scroll_context;
                                    item_scroll_context.scrollport_bounds.origin.y +=
                                        item.sticky_push_offset;
                                    window.with_scroll_container_context(
                                        item_scroll_context,
                                        |window| {
                                            item.element.prepaint_at(item_origin, window, cx);
                                        },
                                    );
                                } else {
                                    item.element.prepaint_at(item_origin, window, cx);
                                }
                            },
                        );

                        if let Some(autoscroll_bounds) = window.take_autoscroll()
                            && autoscroll
                        {
                            if autoscroll_bounds.top() < bounds.top() {
                                let mut item_ix = item.index;
                                let mut offset_in_item = autoscroll_bounds.top() - item_origin.y;

                                // The requested top can sit above this item's own
                                // top. Walk into earlier items so the offset stays
                                // non-negative and no blank space appears above the
                                // list.
                                if offset_in_item < Pixels::ZERO {
                                    let mut cursor = self.items.cursor::<Count>(());
                                    cursor.seek(&Count(item_ix), Bias::Right);
                                    while offset_in_item < Pixels::ZERO {
                                        cursor.prev();
                                        let Some(prev_item) = cursor.item() else {
                                            offset_in_item = Pixels::ZERO;
                                            break;
                                        };
                                        let size = prev_item.size().unwrap_or_else(|| {
                                            let mut element =
                                                render_item(cursor.start().0, window, cx);
                                            let item_available_size = size(
                                                bounds.size.width.into(),
                                                AvailableSpace::MinContent,
                                            );
                                            element.layout_as_root(item_available_size, window, cx)
                                        });
                                        item_ix = cursor.start().0;
                                        offset_in_item += size.height;
                                    }
                                }

                                return Err(ListOffset {
                                    item_ix,
                                    offset_in_item,
                                });
                            } else if autoscroll_bounds.bottom() > bounds.bottom() {
                                let mut cursor = self.items.cursor::<Count>(());
                                cursor.seek(&Count(item.index), Bias::Right);
                                let mut height = bounds.size.height - padding.top - padding.bottom;

                                // Account for the height of the element down until the autoscroll bottom.
                                height -= autoscroll_bounds.bottom() - item_origin.y;

                                // Keep decreasing the scroll top until we fill all the available space.
                                while height > Pixels::ZERO {
                                    cursor.prev();
                                    let Some(item) = cursor.item() else { break };

                                    let size = item.size().unwrap_or_else(|| {
                                        let mut item = render_item(cursor.start().0, window, cx);
                                        let item_available_size = size(
                                            bounds.size.width.into(),
                                            AvailableSpace::MinContent,
                                        );
                                        item.layout_as_root(item_available_size, window, cx)
                                    });
                                    height -= size.height;
                                }

                                return Err(ListOffset {
                                    item_ix: cursor.start().0,
                                    offset_in_item: if height < Pixels::ZERO {
                                        -height
                                    } else {
                                        Pixels::ZERO
                                    },
                                });
                            }
                        }
                    }
                    Ok(())
                })?;
            } else {
                layout_response.item_layouts.clear();
            }

            Ok(layout_response)
        })
    }

    // Scrollbar support

    fn request_offset_from_scrollbar(&mut self, point: Point<Pixels>) {
        let Some(bounds) = self.last_layout_bounds else {
            return;
        };
        let height = bounds.size.height;

        let padding = self.last_padding.unwrap_or_default();
        // Scrollbar drag positions are computed from the content height
        // captured at drag start, so map them back using the same height.
        let content_height = self
            .scrollbar_drag_start_height
            .unwrap_or_else(|| self.items.summary().height);
        let scroll_max = (content_height + padding.top + padding.bottom - height).max(px(0.));
        let new_scroll_top = (-point.y).max(px(0.)).min(scroll_max);

        // If content grew during the drag, the frozen bottom is below the
        // live bottom. Treat dragging to the frozen end as resuming tail follow.
        let dragged_to_end =
            scroll_max > px(0.) && new_scroll_top >= (scroll_max - px(1.0)).max(px(0.));
        if dragged_to_end {
            if matches!(self.follow_state, FollowState::Tail { .. }) {
                self.follow_state = FollowState::Tail { is_following: true };
            }

            let item_count = self.items.summary().count;
            self.pending_scroll = None;
            self.logical_scroll_top = if self.alignment == ListAlignment::Bottom {
                None
            } else {
                Some(ListOffset {
                    item_ix: item_count,
                    offset_in_item: px(0.),
                })
            };
            return;
        }

        self.follow_state.stop_following();

        if self.alignment == ListAlignment::Bottom && new_scroll_top == scroll_max {
            self.pending_scroll = None;
            self.logical_scroll_top = None;
        } else {
            let items = self
                .scrollbar_drag_start_items
                .as_ref()
                .unwrap_or(&self.items);
            let (start, _, _) =
                items.find::<ListItemSummary, _>((), &Height(new_scroll_top), Bias::Right);

            let scroll_top = ListOffset {
                item_ix: start.count,
                offset_in_item: new_scroll_top - start.height,
            };
            self.rebase_pending_scroll(scroll_top);
            self.logical_scroll_top = Some(scroll_top);
        }
    }

    fn resolve_scrollbar_programmatic_offset(
        &mut self,
        point: Point<Pixels>,
    ) -> Option<(Pixels, Pixels)> {
        let bounds = self.last_layout_bounds?;
        let padding = self.last_padding.unwrap_or_default();
        let maximum = (self.items.summary().height + padding.top + padding.bottom
            - bounds.size.height)
            .max(px(0.));
        let target = (-point.y).clamp(px(0.), maximum);

        if maximum == px(0.) || target < (maximum - px(1.)).max(px(0.)) {
            self.follow_state.stop_following();
        }

        Some((target, maximum))
    }
}

impl std::fmt::Debug for ListItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unmeasured { .. } => write!(f, "Unrendered"),
            Self::Measured { size, .. } => f.debug_struct("Rendered").field("size", size).finish(),
        }
    }
}

/// An offset into the list's items, in terms of the item index and the number
/// of pixels off the top left of the item.
#[derive(Debug, Clone, Copy, Default)]
pub struct ListOffset {
    /// The index of an item in the list
    pub item_ix: usize,
    /// The number of pixels to offset from the item index.
    pub offset_in_item: Pixels,
}

impl Element for List {
    type RequestLayoutState = ();
    type PrepaintState = ListPrepaintState;

    fn id(&self) -> Option<crate::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        let layout_id = match self.sizing_behavior {
            ListSizingBehavior::Infer => {
                let mut style = Style::default();
                style.overflow.y = Overflow::Scroll;
                style.refine(&self.style);
                window.with_text_style(style.text_style().cloned(), |window| {
                    let Some((max_element_width, total_height)) =
                        self.state.0.try_borrow_mut().ok().map(|mut state| {
                            let available_height =
                                if let Some(last_bounds) = state.last_layout_bounds {
                                    last_bounds.size.height
                                } else {
                                    // If we don't have the last layout bounds (first render),
                                    // we might just use the overdraw value as the available height to layout enough items.
                                    state.overdraw
                                };
                            let padding = style.padding.to_pixels(
                                state.last_layout_bounds.unwrap_or_default().size.into(),
                                window.rem_size(),
                            );

                            let layout_response = state.layout_items(
                                None,
                                available_height,
                                &padding,
                                &mut self.render_item,
                                window,
                                cx,
                            );
                            let summary = state.items.summary();
                            (layout_response.max_item_width, summary.height)
                        })
                    else {
                        return window.request_layout(style, None, cx);
                    };

                    window.request_measured_layout(
                        style,
                        move |known_dimensions, available_space, _window, _cx| {
                            let width =
                                known_dimensions
                                    .width
                                    .unwrap_or(match available_space.width {
                                        AvailableSpace::Definite(x) => x,
                                        AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                                            max_element_width
                                        }
                                    });
                            let height = match available_space.height {
                                AvailableSpace::Definite(height) => total_height.min(height),
                                AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                                    total_height
                                }
                            };
                            size(width, height)
                        },
                    )
                })
            }
            ListSizingBehavior::Auto => {
                let mut style = Style::default();
                style.refine(&self.style);
                window.with_text_style(style.text_style().cloned(), |window| {
                    window.request_layout(style, None, cx)
                })
            }
        };
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> ListPrepaintState {
        let mut style = Style::default();
        style.refine(&self.style);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        let Ok(mut state) = self.state.0.try_borrow_mut() else {
            return ListPrepaintState {
                hitbox,
                layout: LayoutItemsResponse {
                    max_item_width: Pixels::ZERO,
                    scroll_top: ListOffset::default(),
                    item_layouts: VecDeque::new(),
                    scroll_context: None,
                },
            };
        };
        state.reset = false;

        // If the width of the list has changed, invalidate all cached item heights
        if state
            .last_layout_bounds
            .is_none_or(|last_bounds| last_bounds.size.width != bounds.size.width)
        {
            let new_items = SumTree::from_iter(
                state.items.iter().map(|item| ListItem::Unmeasured {
                    size_hint: state.item_size_hint,
                    focus_handle: item.focus_handle(),
                }),
                (),
            );

            state.items = new_items;
            state.measuring_behavior.reset();
        }

        let padding = style
            .padding
            .to_pixels(bounds.size.into(), window.rem_size());
        let layout =
            match state.prepaint_items(bounds, padding, true, &mut self.render_item, window, cx) {
                Ok(layout) => layout,
                Err(autoscroll_request) => {
                    state.logical_scroll_top = Some(autoscroll_request);
                    state
                        .prepaint_items(bounds, padding, false, &mut self.render_item, window, cx)
                        .unwrap()
                }
            };

        state.last_layout_bounds = Some(bounds);
        state.last_padding = Some(padding);
        let programmatic_frame = state.pending_programmatic_scroll.and_then(|request| {
            let (target, maximum, behavior) = match request {
                PendingProgrammaticScroll::Logical(target, behavior) => {
                    let (target, maximum) = state.resolve_programmatic_offset(target)?;
                    (target, maximum, behavior)
                }
                PendingProgrammaticScroll::Scrollbar(target, behavior) => {
                    let (target, maximum) = state.resolve_scrollbar_programmatic_offset(target)?;
                    (target, maximum, behavior)
                }
            };
            state.pending_programmatic_scroll = None;
            let current = state.scroll_top(&state.logical_scroll_top());
            state.activity.start_programmatic_at(
                point(px(0.), current),
                point(px(0.), target),
                point(px(0.), maximum),
                behavior,
                crate::Instant::now(),
            )
        });
        drop(state);
        if let Some(frame) = programmatic_frame {
            schedule_list_programmatic_scroll(
                self.state.clone(),
                frame.generation,
                (!frame.request_next_frame).then_some(frame),
                window.current_view(),
                window,
            );
            window.refresh();
        }
        ListPrepaintState { hitbox, layout }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<crate::Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current_view = window.current_view();
        let mut paint_items = |window: &mut Window, cx: &mut App| {
            window.with_content_mask(Some(ContentMask::rectangular(bounds)), |window| {
                for item in &mut prepaint.layout.item_layouts {
                    item.element.paint(window, cx);
                }
            });
        };
        if let Some(scroll_context) = prepaint.layout.scroll_context {
            window.with_scroll_container_context(scroll_context, |window| {
                paint_items(window, cx);
            });
        } else {
            paint_items(window, cx);
        }

        // Register the scroll listener before painting children so that, in
        // the bubble phase (which runs in reverse registration order),
        // children's scroll-wheel handlers run first and can stop propagation
        // to prevent the list from scrolling. This matches the ordering of
        // div-based scroll containers.
        let list_state = self.state.clone();
        let height = bounds.size.height;
        let scroll_top = prepaint.layout.scroll_top;
        let hitbox_id = prepaint.hitbox.id;
        let mut accumulated_scroll_delta = ScrollDelta::default();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox_id.should_handle_scroll(window) {
                accumulated_scroll_delta = accumulated_scroll_delta.coalesce(event.delta);
                let pixel_delta = accumulated_scroll_delta.pixel_delta(px(20.));
                let mut advanced = false;
                if let Ok(mut state) = list_state.0.try_borrow_mut() {
                    // Wheel, trackpad, and recognized touch scrolling replace any
                    // programmatic driver.
                    state.pending_programmatic_scroll = None;
                    state.activity.stop();
                    advanced =
                        state.scroll(&scroll_top, height, pixel_delta, current_view, window, cx);
                }
                // At the scroll edge the unconsumed delta keeps bubbling, so an
                // enclosing scroll owner continues the gesture.
                if advanced {
                    cx.stop_propagation();
                }
            }
        });
    }
}

fn schedule_list_programmatic_scroll(
    list_state: ListState,
    generation: u64,
    first_frame: Option<ProgrammaticScrollFrame>,
    current_view: EntityId,
    window: &Window,
) {
    window.on_next_frame(move |window, cx| {
        let frame = if let Some(frame) = first_frame {
            Some(frame)
        } else {
            apply_list_programmatic_frame(
                &list_state,
                generation,
                crate::Instant::now(),
                current_view,
                window,
                cx,
            )
        };
        let Some(frame) = frame else {
            return;
        };

        let advanced = if first_frame.is_some()
            && let Ok(mut state) = list_state.0.try_borrow_mut()
        {
            let Some(bounds) = state.last_layout_bounds else {
                state.activity.stop();
                return;
            };
            let scroll_top = state.logical_scroll_top();
            let current = state.scroll_top(&scroll_top);
            state.scroll(
                &scroll_top,
                bounds.size.height,
                point(px(0.), current - frame.offset.y),
                current_view,
                window,
                cx,
            )
        } else {
            false
        };

        if frame.request_next_frame {
            schedule_list_programmatic_scroll(list_state, generation, None, current_view, window);
        }
        if advanced || frame.request_next_frame {
            window.refresh();
        }
    });
}

fn apply_list_programmatic_frame(
    list_state: &ListState,
    generation: u64,
    timestamp: Instant,
    current_view: EntityId,
    window: &mut Window,
    cx: &mut App,
) -> Option<ProgrammaticScrollFrame> {
    let mut state = list_state.0.try_borrow_mut().ok()?;
    let Some(bounds) = state.last_layout_bounds else {
        state.activity.stop();
        return None;
    };
    let padding = state.last_padding.unwrap_or_default();
    let maximum = (state.items.summary().height + padding.top + padding.bottom
        - bounds.size.height)
        .max(px(0.));
    let frame =
        state
            .activity
            .next_programmatic_at(generation, timestamp, point(px(0.), maximum))?;
    let scroll_top = state.logical_scroll_top();
    let current = state.scroll_top(&scroll_top);
    state.scroll(
        &scroll_top,
        bounds.size.height,
        point(px(0.), current - frame.offset.y),
        current_view,
        window,
        cx,
    );
    Some(frame)
}

impl IntoElement for List {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for List {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl sum_tree::Item for ListItem {
    type Summary = ListItemSummary;

    fn summary(&self, _: ()) -> Self::Summary {
        match self {
            ListItem::Unmeasured {
                size_hint,
                focus_handle,
            } => ListItemSummary {
                count: 1,
                rendered_count: 0,
                unrendered_count: 1,
                height: if let Some(size) = size_hint {
                    size.height
                } else {
                    px(0.)
                },
                has_focus_handles: focus_handle.is_some(),
                has_unknown_height: size_hint.is_none(),
            },
            ListItem::Measured {
                size, focus_handle, ..
            } => ListItemSummary {
                count: 1,
                rendered_count: 1,
                unrendered_count: 0,
                height: size.height,
                has_focus_handles: focus_handle.is_some(),
                has_unknown_height: false,
            },
        }
    }
}

impl sum_tree::ContextLessSummary for ListItemSummary {
    fn zero() -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &Self) {
        self.count += summary.count;
        self.rendered_count += summary.rendered_count;
        self.unrendered_count += summary.unrendered_count;
        self.height += summary.height;
        self.has_focus_handles |= summary.has_focus_handles;
        self.has_unknown_height |= summary.has_unknown_height;
    }
}

impl<'a> sum_tree::Dimension<'a, ListItemSummary> for Count {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a ListItemSummary, _: ()) {
        self.0 += summary.count;
    }
}

impl<'a> sum_tree::Dimension<'a, ListItemSummary> for Height {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a ListItemSummary, _: ()) {
        self.0 += summary.height;
    }
}

impl sum_tree::SeekTarget<'_, ListItemSummary, ListItemSummary> for Count {
    fn cmp(&self, other: &ListItemSummary, _: ()) -> std::cmp::Ordering {
        self.0.partial_cmp(&other.count).unwrap()
    }
}

impl sum_tree::SeekTarget<'_, ListItemSummary, ListItemSummary> for Height {
    fn cmp(&self, other: &ListItemSummary, _: ()) -> std::cmp::Ordering {
        self.0.partial_cmp(&other.height).unwrap()
    }
}

#[cfg(test)]
mod test {

    use super::StickyItems;
    use gpux::{MouseButton, ScrollDelta, ScrollWheelEvent};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use crate::{
        self as gpux, AppContext, Bounds, Context, Element, FollowMode, InteractiveElement,
        IntoElement, ListState, Point, Render, StatefulInteractiveElement, StickyItem,
        StickyStacking, Styled, TestApp, TestAppContext, Window, canvas, div, list, point, px,
        size,
    };

    #[test]
    fn keyed_sticky_item_uses_contiguous_stack_runs() {
        let state = ListState::new(40, crate::ListAlignment::Top, px(0.)).with_sticky_items([
            StickyItem::new(0, StickyStacking::StackId("a".into())),
            StickyItem::new(8, StickyStacking::StackId("a".into())),
            StickyItem::new(16, StickyStacking::StackId("b".into())),
            StickyItem::new(24, StickyStacking::StackId("a".into())),
        ]);

        let stack = state
            .0
            .borrow()
            .active_sticky_item_indices(&gpux::ListOffset {
                item_ix: 10,
                offset_in_item: px(0.),
            });
        assert_eq!(stack, vec![0, 8]);

        let stack = state
            .0
            .borrow()
            .active_sticky_item_indices(&gpux::ListOffset {
                item_ix: 20,
                offset_in_item: px(0.),
            });
        assert_eq!(stack, vec![16]);

        let stack = state
            .0
            .borrow()
            .active_sticky_item_indices(&gpux::ListOffset {
                item_ix: 30,
                offset_in_item: px(0.),
            });
        assert_eq!(stack, vec![24]);
    }

    #[test]
    fn keyed_sticky_item_filters_invalid_indices_and_keeps_first_duplicate() {
        let sticky_items = StickyItems::new(
            [
                StickyItem::new(2, StickyStacking::StackId("first".into())),
                StickyItem::new(2, StickyStacking::StackId("duplicate".into())),
                StickyItem::new(8, StickyStacking::StackId("out-of-range".into())),
                StickyItem::new(0, StickyStacking::Level(0)),
            ],
            4,
        );

        assert_eq!(sticky_items.candidates.len(), 2);
        assert_eq!(sticky_items.candidates[0].index, 0);
        assert_eq!(sticky_items.candidates[1].index, 2);
        assert_eq!(
            sticky_items.candidates[1].stacking,
            StickyStacking::StackId("first".into())
        );
    }

    #[test]
    fn keyed_sticky_item_target_uses_variable_predecessor_extents() {
        let state = ListState::new(12, crate::ListAlignment::Top, px(0.))
            .with_item_height_hint(px(34.))
            .with_sticky_items([
                StickyItem::new(0, StickyStacking::StackId("a".into())),
                StickyItem::new(1, StickyStacking::StackId("a".into())),
                StickyItem::new(8, StickyStacking::StackId("b".into())),
            ]);

        assert_eq!(state.0.borrow().sticky_target_offset(1), px(34.));
        assert_eq!(state.0.borrow().sticky_target_offset(8), px(0.));
    }

    #[test]
    fn keyed_sticky_item_blocks_clicks_to_overlapped_list_rows() {
        struct StickyListView {
            state: ListState,
            clicks: Rc<RefCell<Vec<usize>>>,
        }

        impl Render for StickyListView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let clicks = self.clicks.clone();
                list(self.state.clone(), move |ix, _, _| {
                    let clicks = clicks.clone();
                    let row = div()
                        .id(("row", ix))
                        .w(px(100.))
                        .h(px(20.))
                        .on_click(move |_, _, _| clicks.borrow_mut().push(ix));
                    if ix < 2 {
                        row.sticky().top(px(0.)).into_any()
                    } else {
                        row.into_any()
                    }
                })
                .w(px(100.))
                .h(px(100.))
            }
        }

        let state = ListState::new(20, crate::ListAlignment::Top, px(0.))
            .with_item_height_hint(px(20.))
            .with_sticky_items([
                StickyItem::new(0, StickyStacking::StackId("a".into())),
                StickyItem::new(1, StickyStacking::StackId("a".into())),
            ]);
        state.scroll_to(gpux::ListOffset {
            item_ix: 4,
            offset_in_item: px(0.),
        });
        let clicks = Rc::new(RefCell::new(Vec::new()));
        let view_clicks = clicks.clone();
        let mut app = TestApp::new();
        let mut window = app.open_window(move |_, _| StickyListView {
            state,
            clicks: view_clicks,
        });

        window.draw();
        window.simulate_click(point(px(50.), px(30.)), MouseButton::Left);

        assert_eq!(&*clicks.borrow(), &[1]);
    }

    #[test]
    fn pushed_sticky_list_child_hit_tests_below_its_retained_parent() {
        struct StickyListView {
            state: ListState,
            clicks: Rc<RefCell<Vec<usize>>>,
        }

        impl Render for StickyListView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let clicks = self.clicks.clone();
                list(self.state.clone(), move |ix, _, _| {
                    let clicks = clicks.clone();
                    let row = div()
                        .id(("hierarchy-row", ix))
                        .w(px(100.))
                        .h(px(20.))
                        .on_click(move |_, _, _| clicks.borrow_mut().push(ix));
                    match ix {
                        0 | 24 => row.sticky().top(px(0.)).into_any(),
                        8 | 16 => row.sticky().top(px(20.)).into_any(),
                        _ => row.into_any(),
                    }
                })
                .w(px(100.))
                .h(px(100.))
            }
        }

        let state = ListState::new(40, crate::ListAlignment::Top, px(0.))
            .with_item_height_hint(px(20.))
            .with_sticky_items([
                StickyItem::new(0, StickyStacking::Level(0)),
                StickyItem::new(8, StickyStacking::Level(1)),
                StickyItem::new(16, StickyStacking::Level(1)),
                StickyItem::new(24, StickyStacking::Level(0)),
            ]);
        state.scroll_to(gpux::ListOffset {
            item_ix: 15,
            offset_in_item: px(10.),
        });
        let clicks = Rc::new(RefCell::new(Vec::new()));
        let mut app = TestApp::new();
        let mut window = app.open_window(move |_, _| StickyListView { state, clicks });

        window.draw();
        window.update(|_, window, _| {
            let parent_bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(20.)));
            let top_hitbox_id = window
                .rendered_frame
                .hit_test(point(px(50.), px(5.)))
                .ids
                .first()
                .copied()
                .expect("overlapping list stickies should insert hitboxes");
            let top_hitbox = window
                .rendered_frame
                .hitboxes
                .iter()
                .find(|hitbox| hitbox.id == top_hitbox_id)
                .expect("top list hitbox should be retained");
            assert_eq!(parent_bounds, top_hitbox.bounds);
        });
    }

    #[test]
    fn active_sticky_item_indices_stack_nested_levels() {
        let state = ListState::new(40, crate::ListAlignment::Top, px(0.)).with_sticky_items([
            StickyItem::new(0, StickyStacking::Level(0)),
            StickyItem::new(8, StickyStacking::Level(1)),
            StickyItem::new(16, StickyStacking::Level(1)),
            StickyItem::new(24, StickyStacking::Level(0)),
            StickyItem::new(32, StickyStacking::Level(1)),
        ]);

        let stack = state
            .0
            .borrow()
            .active_sticky_item_indices(&gpux::ListOffset {
                item_ix: 10,
                offset_in_item: px(0.),
            });
        assert_eq!(stack, vec![0, 8]);

        let stack = state
            .0
            .borrow()
            .active_sticky_item_indices(&gpux::ListOffset {
                item_ix: 18,
                offset_in_item: px(0.),
            });
        assert_eq!(stack, vec![0, 16]);
    }

    #[test]
    fn active_sticky_item_indices_replace_parent_and_drop_descendants() {
        let state = ListState::new(48, crate::ListAlignment::Top, px(0.)).with_sticky_items([
            StickyItem::new(0, StickyStacking::Level(0)),
            StickyItem::new(8, StickyStacking::Level(1)),
            StickyItem::new(16, StickyStacking::Level(2)),
            StickyItem::new(24, StickyStacking::Level(0)),
            StickyItem::new(32, StickyStacking::Level(1)),
        ]);

        let stack = state
            .0
            .borrow()
            .active_sticky_item_indices(&gpux::ListOffset {
                item_ix: 26,
                offset_in_item: px(0.),
            });
        assert_eq!(stack, vec![24]);

        let stack = state
            .0
            .borrow()
            .active_sticky_item_indices(&gpux::ListOffset {
                item_ix: 34,
                offset_in_item: px(0.),
            });
        assert_eq!(stack, vec![24, 32]);
    }

    #[test]
    fn sticky_replacement_retains_only_unchanged_list_ancestors() {
        let sticky_items = StickyItems::new(
            [
                StickyItem::new(0, StickyStacking::Level(0)),
                StickyItem::new(8, StickyStacking::Level(1)),
                StickyItem::new(16, StickyStacking::Level(2)),
                StickyItem::new(24, StickyStacking::Level(1)),
                StickyItem::new(32, StickyStacking::Level(0)),
            ],
            40,
        );

        assert_eq!(sticky_items.next_replacement_after(16), Some((24, 1)));
        assert_eq!(sticky_items.next_replacement_after(24), Some((32, 0)));
    }

    #[test]
    fn rendered_sticky_row_uses_list_scroll_context() {
        struct StickyListView {
            state: ListState,
        }

        impl Render for StickyListView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.state.clone(), |ix, _, _| {
                    let row = div().w(px(100.)).h(px(20.));
                    if ix == 3 {
                        row.sticky()
                            .top(px(0.))
                            .debug_selector(|| "list-sticky-row".into())
                            .into_any()
                    } else {
                        row.into_any()
                    }
                })
                .w(px(100.))
                .h(px(100.))
            }
        }

        let mut test_app = TestAppContext::single();
        let state =
            ListState::new(20, crate::ListAlignment::Top, px(0.)).with_item_height_hint(px(20.));
        state.scroll_to(gpux::ListOffset {
            item_ix: 3,
            offset_in_item: px(10.),
        });
        let window = test_app.add_window(move |_, _| StickyListView { state });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let sticky_bounds = test_app
            .update_window(any_window, |_, window, _| {
                window
                    .rendered_frame
                    .debug_bounds
                    .get("list-sticky-row")
                    .copied()
            })
            .unwrap();

        assert_eq!(
            Some(Bounds::new(Point::default(), size(px(100.), px(20.)))),
            sticky_bounds
        );
    }

    #[test]
    fn sticky_candidate_row_remains_rendered_in_list() {
        struct StickyCandidateListView {
            state: ListState,
        }

        impl Render for StickyCandidateListView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.state.clone(), |ix, _, _| {
                    let row = div().w(px(100.)).h(px(20.));
                    if ix == 0 {
                        row.sticky()
                            .top(px(0.))
                            .debug_selector(|| "list-sticky-candidate".into())
                            .into_any()
                    } else {
                        row.into_any()
                    }
                })
                .w(px(100.))
                .h(px(100.))
            }
        }

        let mut test_app = TestAppContext::single();
        let state =
            ListState::new(20, crate::ListAlignment::Top, px(0.)).with_item_height_hint(px(20.));
        state.set_sticky_items([0]);
        state.scroll_to(gpux::ListOffset {
            item_ix: 3,
            offset_in_item: px(10.),
        });
        let window = test_app.add_window(move |_, _| StickyCandidateListView { state });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let sticky_bounds = test_app
            .update_window(any_window, |_, window, _| {
                window
                    .rendered_frame
                    .debug_bounds
                    .get("list-sticky-candidate")
                    .copied()
            })
            .unwrap();

        assert_eq!(
            Some(Bounds::new(Point::default(), size(px(100.), px(20.)))),
            sticky_bounds
        );
    }

    #[test]
    fn nested_sticky_candidate_rows_stack_in_list() {
        struct NestedStickyCandidateListView {
            state: ListState,
        }

        impl Render for NestedStickyCandidateListView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.state.clone(), |ix, _, _| {
                    let row = div().w(px(100.)).h(px(20.));
                    match ix {
                        0 => row
                            .sticky()
                            .top(px(0.))
                            .debug_selector(|| "list-sticky-parent".into())
                            .into_any(),
                        8 => row
                            .sticky()
                            .top(px(20.))
                            .debug_selector(|| "list-sticky-child".into())
                            .into_any(),
                        _ => row.into_any(),
                    }
                })
                .w(px(100.))
                .h(px(100.))
            }
        }

        let mut test_app = TestAppContext::single();
        let state =
            ListState::new(20, crate::ListAlignment::Top, px(0.)).with_item_height_hint(px(20.));
        state.set_sticky_items([
            StickyItem::new(0, StickyStacking::Level(0)),
            StickyItem::new(8, StickyStacking::Level(1)),
        ]);
        state.scroll_to(gpux::ListOffset {
            item_ix: 12,
            offset_in_item: px(0.),
        });
        let window = test_app.add_window(move |_, _| NestedStickyCandidateListView { state });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();

        let (parent_bounds, child_bounds) = test_app
            .update_window(any_window, |_, window, _| {
                (
                    window
                        .rendered_frame
                        .debug_bounds
                        .get("list-sticky-parent")
                        .copied(),
                    window
                        .rendered_frame
                        .debug_bounds
                        .get("list-sticky-child")
                        .copied(),
                )
            })
            .unwrap();

        assert_eq!(
            Some(Bounds::new(Point::default(), size(px(100.), px(20.)))),
            parent_bounds
        );
        assert_eq!(
            Some(Bounds::new(point(px(0.), px(20.)), size(px(100.), px(20.)))),
            child_bounds
        );
    }

    #[gpux::test]
    fn test_autoscroll_above_item_top_renders_items_above(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.));
        state.scroll_to(gpux::ListOffset {
            item_ix: 2,
            offset_in_item: px(0.),
        });

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |ix, _, _| {
                    if ix == 2 {
                        // Request an autoscroll whose top sits 30px above item 2's
                        // own top, mimicking a scroll-margin overshoot.
                        canvas(
                            |bounds, window, _| {
                                window.request_autoscroll(Bounds::from_corners(
                                    point(bounds.left(), bounds.top() - px(30.)),
                                    point(bounds.right(), bounds.top() + px(5.)),
                                ));
                            },
                            |_, _, _, _| {},
                        )
                        .h(px(20.))
                        .w_full()
                        .into_any()
                    } else {
                        div().h(px(20.)).w_full().into_any()
                    }
                })
                .w_full()
                .h_full()
            }
        }

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(60.)), |_, cx| {
            cx.new(|_| TestView(state.clone())).into_any_element()
        });

        // 30px above item 2's top, with 20px items, lands 10px into item 0.
        let scroll_top = state.logical_scroll_top();
        assert!(
            scroll_top.offset_in_item >= px(0.),
            "offset_in_item must never be negative (would leave blank space above), got {:?}",
            scroll_top.offset_in_item,
        );
        assert_eq!(scroll_top.item_ix, 0);
        assert_eq!(scroll_top.offset_in_item, px(10.));
    }

    #[gpux::test]
    fn test_reset_after_paint_before_scroll(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.));

        // Ensure that the list is scrolled to the top
        state.scroll_to(gpux::ListOffset {
            item_ix: 0,
            offset_in_item: px(0.0),
        });

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(10.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        // Paint
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, cx| {
            cx.new(|_| TestView(state.clone())).into_any_element()
        });

        // Reset
        state.reset(5);

        // And then receive a scroll event _before_ the next paint
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(1.), px(1.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-500.))),
            ..Default::default()
        });

        // Scroll position should stay at the top of the list
        assert_eq!(state.logical_scroll_top().item_ix, 0);
        assert_eq!(state.logical_scroll_top().offset_in_item, px(0.));
    }

    #[gpux::test]
    fn test_scroll_by_positive_and_negative_distance(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(20.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        // Paint
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(100.)), |_, cx| {
            cx.new(|_| TestView(state.clone())).into_any_element()
        });

        // Test positive distance: start at item 1, move down 30px
        state.scroll_by(px(30.));

        // Should move to item 2
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 1);
        assert_eq!(offset.offset_in_item, px(10.));

        // Test negative distance: start at item 2, move up 30px
        state.scroll_by(px(-30.));

        // Should move back to item 1
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 0);
        assert_eq!(offset.offset_in_item, px(0.));

        // Test zero distance
        state.scroll_by(px(0.));
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 0);
        assert_eq!(offset.offset_in_item, px(0.));
    }

    #[gpux::test]
    fn test_child_scroll_handler_can_stop_list_scroll(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.));
        let child_saw_event = Rc::new(Cell::new(false));

        struct TestView {
            state: ListState,
            child_saw_event: Rc<Cell<bool>>,
        }
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let child_saw_event = self.child_saw_event.clone();
                list(self.state.clone(), move |_, _, _| {
                    let child_saw_event = child_saw_event.clone();
                    div()
                        .h(px(20.))
                        .w_full()
                        .on_scroll_wheel(move |_, _, cx| {
                            child_saw_event.set(true);
                            cx.stop_propagation();
                        })
                        .into_any()
                })
                .w_full()
                .h_full()
            }
        }

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(100.)), |_, cx| {
            cx.new(|_| TestView {
                state: state.clone(),
                child_saw_event: child_saw_event.clone(),
            })
            .into_any_element()
        });

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(10.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-30.))),
            ..Default::default()
        });

        assert!(
            child_saw_event.get(),
            "the child's scroll-wheel handler should run"
        );
        // The child stopped propagation, so the list must not have scrolled.
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 0);
        assert_eq!(offset.offset_in_item, px(0.));
    }

    struct TestListView(ListState);
    impl Render for TestListView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            list(self.0.clone(), |_, _, _| {
                div().h(px(20.)).w_full().into_any()
            })
            .w_full()
            .h_full()
        }
    }

    #[gpux::test]
    fn test_item_viewport_queries_return_none_before_layout(_cx: &mut TestAppContext) {
        let state = ListState::new(5, crate::ListAlignment::Top, px(10.)).measure_all();

        assert_eq!(state.item_is_above_viewport(0), None);
        assert_eq!(state.item_is_below_viewport(0), None);
    }

    #[gpux::test]
    fn test_item_viewport_queries_before_logical_scroll_top(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.)).measure_all();

        state.scroll_to(gpux::ListOffset {
            item_ix: 2,
            offset_in_item: px(0.),
        });
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, cx| {
            cx.new(|_| TestListView(state.clone())).into_any_element()
        });

        assert_eq!(state.item_is_above_viewport(1), Some(true));
        assert_eq!(state.item_is_below_viewport(1), Some(false));
    }

    #[gpux::test]
    fn test_item_viewport_queries_measured_item_inside_viewport(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.)).measure_all();

        state.scroll_to(gpux::ListOffset {
            item_ix: 2,
            offset_in_item: px(0.),
        });
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, cx| {
            cx.new(|_| TestListView(state.clone())).into_any_element()
        });

        assert_eq!(state.item_is_above_viewport(2), Some(false));
        assert_eq!(state.item_is_below_viewport(2), Some(false));
    }

    #[gpux::test]
    fn test_item_viewport_queries_measured_item_above_viewport(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.)).measure_all();

        state.scroll_to(gpux::ListOffset {
            item_ix: 2,
            offset_in_item: px(20.),
        });
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, cx| {
            cx.new(|_| TestListView(state.clone())).into_any_element()
        });

        assert_eq!(state.item_is_above_viewport(2), Some(true));
        assert_eq!(state.item_is_below_viewport(2), Some(false));
    }

    #[gpux::test]
    fn test_item_viewport_queries_measured_item_below_viewport(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.)).measure_all();

        state.scroll_to(gpux::ListOffset {
            item_ix: 2,
            offset_in_item: px(0.),
        });
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, cx| {
            cx.new(|_| TestListView(state.clone())).into_any_element()
        });

        assert_eq!(state.item_is_above_viewport(3), Some(false));
        assert_eq!(state.item_is_below_viewport(3), Some(true));
    }

    #[gpux::test]
    fn test_item_viewport_queries_remain_stable_with_zero_height_viewport(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.)).measure_all();

        state.scroll_to(gpux::ListOffset {
            item_ix: 2,
            offset_in_item: px(0.),
        });
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, cx| {
            cx.new(|_| TestListView(state.clone())).into_any_element()
        });

        assert_eq!(state.item_is_above_viewport(3), Some(false));
        assert_eq!(state.item_is_below_viewport(3), Some(true));

        // Squeeze the list to zero height, e.g. because a sibling element
        // (sized based on the queries above) consumed all the space. The
        // answers must remain definitive rather than becoming `None`,
        // otherwise the sibling's size can oscillate between frames.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(0.)), |_, cx| {
            cx.new(|_| TestListView(state.clone())).into_any_element()
        });

        assert_eq!(state.item_is_above_viewport(1), Some(true));
        assert_eq!(state.item_is_below_viewport(1), Some(false));
        assert_eq!(state.item_is_above_viewport(3), Some(false));
        assert_eq!(state.item_is_below_viewport(3), Some(true));
    }

    #[gpux::test]
    fn test_item_viewport_queries_after_scroll_to_end_before_layout(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(5, crate::ListAlignment::Top, px(10.)).measure_all();

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, cx| {
            cx.new(|_| TestListView(state.clone())).into_any_element()
        });

        state.scroll_to_end();

        assert_eq!(state.logical_scroll_top().item_ix, state.item_count());
        assert_eq!(state.item_is_above_viewport(0), Some(true));
        assert_eq!(state.item_is_below_viewport(0), Some(false));
    }

    #[gpux::test]
    fn test_measure_all_after_width_change(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(10, crate::ListAlignment::Top, px(0.)).measure_all();

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        // First draw at width 100: all 10 items measured (total 500px).
        // Viewport is 200px, so max scroll offset should be 300px.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert_eq!(state.max_offset_for_scrollbar().y, px(300.));

        // Second draw at a different width: items get invalidated.
        // Without the fix, max_offset would drop because unmeasured items
        // contribute 0 height.
        cx.draw(point(px(0.), px(0.)), size(px(200.), px(200.)), |_, _| {
            view.into_any_element()
        });
        assert_eq!(state.max_offset_for_scrollbar().y, px(300.));
    }

    #[gpux::test]
    fn test_remeasure(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // Create a list with 10 items, each 100px tall. We'll keep a reference
        // to the item height so we can later change the height and assert how
        // `ListState` handles it.
        let item_height = Rc::new(Cell::new(100usize));
        let state = ListState::new(10, crate::ListAlignment::Top, px(10.));

        struct TestView {
            state: ListState,
            item_height: Rc<Cell<usize>>,
        }

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let height = self.item_height.get();
                list(self.state.clone(), move |_, _, _| {
                    div().h(px(height as f32)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let state_clone = state.clone();
        let item_height_clone = item_height.clone();
        let view = cx.update(|_, cx| {
            cx.new(|_| TestView {
                state: state_clone,
                item_height: item_height_clone,
            })
        });

        // Simulate scrolling 40px inside the element with index 2. Since the
        // original item height is 100px, this equates to 40% inside the item.
        state.scroll_to(gpux::ListOffset {
            item_ix: 2,
            offset_in_item: px(40.),
        });

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 2);
        assert_eq!(offset.offset_in_item, px(40.));

        // Update the `item_height` to be 50px instead of 100px so we can assert
        // that the scroll position is proportionally preserved, that is,
        // instead of 40px from the top of item 2, it should be 20px, since the
        // item's height has been halved.
        item_height.set(50);
        state.remeasure();

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 2);
        assert_eq!(offset.offset_in_item, px(20.));
    }

    #[gpux::test]
    fn test_remeasure_item_preserves_scroll_offset(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let item_height = Rc::new(Cell::new(100usize));
        let state = ListState::new(20, crate::ListAlignment::Top, px(10.));

        struct TestView {
            state: ListState,
            item_height: Rc<Cell<usize>>,
        }

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let height = self.item_height.get();
                list(self.state.clone(), move |index, _, _| {
                    let height = if index == 5 { height } else { 100 };
                    div().h(px(height as f32)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let state_clone = state.clone();
        let item_height_clone = item_height.clone();
        let view = cx.update(|_, cx| {
            cx.new(|_| TestView {
                state: state_clone,
                item_height: item_height_clone,
            })
        });

        state.scroll_to(gpux::ListOffset {
            item_ix: 5,
            offset_in_item: px(40.),
        });

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        item_height.set(200);
        state.remeasure_items(5..6);

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 5);
        assert_eq!(offset.offset_in_item, px(40.));
    }

    #[gpux::test]
    fn test_remeasure_then_scroll_does_not_revert_scroll_position(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state = ListState::new(20, crate::ListAlignment::Top, px(10.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(100.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = {
            let state = state.clone();
            cx.update(|_, cx| cx.new(|_| TestView(state)))
        };

        state.scroll_to(gpux::ListOffset {
            item_ix: 5,
            offset_in_item: px(40.),
        });

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        state.remeasure_items(5..6);

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-30.))),
            ..Default::default()
        });

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 5);
        assert_eq!(offset.offset_in_item, px(70.));

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 5);
        assert_eq!(
            offset.offset_in_item,
            px(70.),
            "scrolling after a remeasure should not be reverted by the stale pending scroll"
        );
    }

    #[gpux::test]
    fn test_scroll_after_remeasure_clamps_to_shrunk_item_height(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let item_height = Rc::new(Cell::new(100usize));
        let state = ListState::new(20, crate::ListAlignment::Top, px(10.));

        struct TestView {
            state: ListState,
            item_height: Rc<Cell<usize>>,
        }

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let height = self.item_height.get();
                list(self.state.clone(), move |index, _, _| {
                    let height = if index == 5 { height } else { 100 };
                    div().h(px(height as f32)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = {
            let state = state.clone();
            let item_height = item_height.clone();
            cx.update(|_, cx| cx.new(|_| TestView { state, item_height }))
        };

        state.scroll_to(gpux::ListOffset {
            item_ix: 5,
            offset_in_item: px(40.),
        });

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        // Item 5 shrinks from 100px to 50px and is remeasured...
        item_height.set(50);
        state.remeasure_items(5..6);

        // ...and then the user scrolls down by 30px before the next frame,
        // landing at offset 70.
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-30.))),
            ..Default::default()
        });

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });

        // The rebased pending scroll clamps the user's offset to the item's
        // new height instead of leaving it pointing past the end of the item.
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 5);
        assert_eq!(offset.offset_in_item, px(50.));
    }

    #[gpux::test]
    fn test_follow_tail_stays_at_bottom_as_items_grow(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 10 items, each 50px tall → 500px total content, 200px viewport.
        // With follow-tail on, the list should always show the bottom.
        let item_height = Rc::new(Cell::new(50usize));
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.));

        struct TestView {
            state: ListState,
            item_height: Rc<Cell<usize>>,
        }
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let height = self.item_height.get();
                list(self.state.clone(), move |_, _, _| {
                    div().h(px(height as f32)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let state_clone = state.clone();
        let item_height_clone = item_height.clone();
        let view = cx.update(|_, cx| {
            cx.new(|_| TestView {
                state: state_clone,
                item_height: item_height_clone,
            })
        });

        state.set_follow_mode(FollowMode::Tail);

        // First paint — items are 50px, total 500px, viewport 200px.
        // Follow-tail should anchor to the end.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        // The scroll should be at the bottom: the last visible items fill the
        // 200px viewport from the end of 500px of content (offset 300px).
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 6);
        assert_eq!(offset.offset_in_item, px(0.));
        assert!(state.is_following_tail());

        // Simulate items growing (e.g. streaming content makes each item taller).
        // 10 items × 80px = 800px total.
        item_height.set(80);
        state.remeasure();

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });

        // After growth, follow-tail should have re-anchored to the new end.
        // 800px total − 200px viewport = 600px offset → item 7 at offset 40px,
        // but follow-tail anchors to item_count (10), and layout walks back to
        // fill 200px, landing at item 7 (7 × 80 = 560, 800 − 560 = 240 > 200,
        // so item 8: 8 × 80 = 640, 800 − 640 = 160 < 200 → keeps walking →
        // item 7: offset = 800 − 200 = 600, item_ix = 600/80 = 7, remainder 40).
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 7);
        assert_eq!(offset.offset_in_item, px(40.));
        assert!(state.is_following_tail());
    }

    #[gpux::test]
    fn test_pause_following_tail_reengages_when_still_at_bottom(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 10 items × 50px = 500px total, 200px viewport.
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));
        state.set_follow_mode(FollowMode::Tail);

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert!(state.is_following_tail());

        // Pausing while the view is still at the bottom (e.g. a no-op zoom)
        // must not strand follow-tail: the next layout has to re-engage so the
        // invariant "at the bottom + new content => visible" is preserved.
        state.pause_following_tail();
        assert!(!state.is_following_tail());

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });
        assert!(
            state.is_following_tail(),
            "pausing while at the bottom must re-engage follow-tail on the next layout"
        );
    }

    #[gpux::test]
    fn test_pause_following_tail_freezes_off_bottom(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 10 items, 200px viewport. Item height is adjustable to simulate a
        // diagram block growing/shrinking on zoom.
        let item_height = Rc::new(Cell::new(50usize));
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.));

        struct TestView {
            state: ListState,
            item_height: Rc<Cell<usize>>,
        }
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let height = self.item_height.get();
                list(self.state.clone(), move |_, _, _| {
                    div().h(px(height as f32)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| {
            cx.new(|_| TestView {
                state: state.clone(),
                item_height: item_height.clone(),
            })
        });
        state.set_follow_mode(FollowMode::Tail);

        // At the bottom: 500px content, 200px viewport → top at item 6.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert_eq!(state.logical_scroll_top().item_ix, 6);
        assert!(state.is_following_tail());

        // Pause, then grow items (a zoom-in that pushes content below the fold).
        // The frozen top must stay put rather than snapping to the new end, and
        // following stays paused since we're no longer at the bottom.
        state.pause_following_tail();
        item_height.set(80);
        state.remeasure();
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 6);
        assert_eq!(offset.offset_in_item, px(0.));
        assert!(
            !state.is_following_tail(),
            "a paused list must not re-engage while the frozen top is off the bottom"
        );

        // Shrink back (zoom-out) so the frozen top once again reaches the
        // bottom: follow-tail must re-engage on its own.
        item_height.set(50);
        state.remeasure();
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });
        assert!(
            state.is_following_tail(),
            "returning to the bottom must restore follow-tail"
        );
    }

    #[gpux::test]
    fn test_follow_tail_disengages_on_user_scroll(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 10 items × 50px = 500px total, 200px viewport.
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        state.set_follow_mode(FollowMode::Tail);

        // Paint with follow-tail — scroll anchored to the bottom.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, cx| {
            cx.new(|_| TestView(state.clone())).into_any_element()
        });
        assert!(state.is_following_tail());

        // Simulate the user scrolling up.
        // This should disengage follow-tail.
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(100.))),
            ..Default::default()
        });

        assert!(
            !state.is_following_tail(),
            "follow-tail should disengage when the user scrolls toward the start"
        );
    }

    #[gpux::test]
    fn test_follow_tail_disengages_on_scrollbar_reposition(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 10 items × 50px = 500px total, 200px viewport.
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.)).measure_all();

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        state.set_follow_mode(FollowMode::Tail);

        // Paint with follow-tail — scroll anchored to the bottom.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert!(state.is_following_tail());

        // Simulate the scrollbar moving the viewport to the middle.
        state.request_offset_from_scrollbar(point(px(0.), px(-150.)));

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 3);
        assert_eq!(offset.offset_in_item, px(0.));
        assert!(
            !state.is_following_tail(),
            "follow-tail should disengage when the scrollbar manually repositions the list"
        );

        // A subsequent draw should preserve the user's manual position instead
        // of snapping back to the end.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 3);
        assert_eq!(offset.offset_in_item, px(0.));
    }

    #[gpux::test]
    fn test_scrollbar_drag_with_growing_content(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let last_item_height = Rc::new(Cell::new(50usize));
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.)).measure_all();

        struct TestView {
            state: ListState,
            last_item_height: Rc<Cell<usize>>,
        }
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let last_item_height = self.last_item_height.clone();
                list(self.state.clone(), move |index, _, _| {
                    let height = if index == 9 {
                        last_item_height.get()
                    } else {
                        50
                    };
                    div().h(px(height as f32)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| {
            cx.new(|_| TestView {
                state: state.clone(),
                last_item_height: last_item_height.clone(),
            })
        });

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        state.scrollbar_drag_started();

        state.request_offset_from_scrollbar(point(px(0.), px(-150.)));
        let scrollbar_offset_before_growth = state.scroll_px_offset_for_scrollbar();

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 3);
        assert_eq!(offset.offset_in_item, px(0.));

        last_item_height.set(550);
        state.remeasure_items(9..10);
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        assert_eq!(state.max_offset_for_scrollbar().y, px(300.));
        assert_eq!(
            state.scroll_px_offset_for_scrollbar(),
            scrollbar_offset_before_growth
        );

        state.request_offset_from_scrollbar(point(px(0.), px(-150.)));
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 3);
        assert_eq!(offset.offset_in_item, px(0.));
    }

    #[gpux::test]
    fn test_scrollbar_drag_with_estimated_item_heights_uses_drag_snapshot(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state =
            ListState::new(30, crate::ListAlignment::Top, px(0.)).with_item_height_hint(px(100.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |index, _, _| {
                    let height = match index {
                        11 | 12 => 400.,
                        _ => 40.,
                    };
                    div().h(px(height)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        state.scrollbar_drag_started();
        state.request_offset_from_scrollbar(point(px(0.), px(-1200.)));
        let offset_before_measurement = state.logical_scroll_top();
        assert_eq!(offset_before_measurement.item_ix, 15);
        assert_eq!(offset_before_measurement.offset_in_item, px(0.));

        state.0.borrow_mut().overdraw = px(700.);
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        assert_eq!(
            state.scroll_px_offset_for_scrollbar(),
            point(px(0.), px(-1200.))
        );

        state.request_offset_from_scrollbar(point(px(0.), px(-1200.)));
        let offset_after_measurement = state.logical_scroll_top();
        assert_eq!(offset_after_measurement.item_ix, 15);
        assert_eq!(offset_after_measurement.offset_in_item, px(0.));

        state.scrollbar_drag_ended();
    }

    #[gpux::test]
    fn test_scrollbar_drag_to_estimated_bottom_anchors_to_list_end(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let state =
            ListState::new(30, crate::ListAlignment::Top, px(0.)).with_item_height_hint(px(100.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |index, _, _| {
                    let height = match index {
                        28 | 29 => 400.,
                        _ => 40.,
                    };
                    div().h(px(height)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        state.scrollbar_drag_started();
        state.request_offset_from_scrollbar(point(px(0.), -state.max_offset_for_scrollbar().y));

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, state.item_count());
        assert_eq!(offset.offset_in_item, px(0.));

        state.scrollbar_drag_ended();
    }

    #[gpux::test]
    fn test_set_follow_tail_snaps_to_bottom(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 10 items × 50px = 500px total, 200px viewport.
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        // Scroll to the middle of the list (item 3).
        state.scroll_to(gpux::ListOffset {
            item_ix: 3,
            offset_in_item: px(0.),
        });

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 3);
        assert_eq!(offset.offset_in_item, px(0.));
        assert!(!state.is_following_tail());

        // Enable follow-tail — this should immediately snap the scroll anchor
        // to the end, like the user just sent a prompt.
        state.set_follow_mode(FollowMode::Tail);

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });

        // After paint, scroll should be at the bottom.
        // 500px total − 200px viewport = 300px offset → item 6, offset 0.
        let offset = state.logical_scroll_top();
        assert_eq!(offset.item_ix, 6);
        assert_eq!(offset.offset_in_item, px(0.));
        assert!(state.is_following_tail());
    }

    #[gpux::test]
    fn test_bottom_aligned_scrollbar_offset_at_end(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        const ITEMS: usize = 10;
        const ITEM_SIZE: f32 = 50.0;

        let state = ListState::new(
            ITEMS,
            crate::ListAlignment::Bottom,
            px(ITEMS as f32 * ITEM_SIZE),
        );

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(ITEM_SIZE)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(100.)), |_, cx| {
            cx.new(|_| TestView(state.clone())).into_any_element()
        });

        // Bottom-aligned lists start pinned to the end: logical_scroll_top returns
        // item_ix == item_count, meaning no explicit scroll position has been set.
        assert_eq!(state.logical_scroll_top().item_ix, ITEMS);

        let max_offset = state.max_offset_for_scrollbar();
        let scroll_offset = state.scroll_px_offset_for_scrollbar();

        assert_eq!(
            -scroll_offset.y, max_offset.y,
            "scrollbar offset ({}) should equal max offset ({}) when list is pinned to bottom",
            -scroll_offset.y, max_offset.y,
        );
    }

    /// When the user scrolls away from the bottom during follow_tail,
    /// follow_tail suspends. If they scroll back to the bottom, the
    /// next paint should re-engage follow_tail using fresh measurements.
    #[gpux::test]
    fn test_follow_tail_reengages_when_scrolled_back_to_bottom(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 10 items × 50px = 500px total, 200px viewport.
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        state.set_follow_mode(FollowMode::Tail);

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert!(state.is_following_tail());

        // Scroll up — follow_tail should suspend (not fully disengage).
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(50.))),
            ..Default::default()
        });
        assert!(!state.is_following_tail());

        // Scroll back down to the bottom.
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            ..Default::default()
        });

        // After a paint, follow_tail should re-engage because the
        // layout confirmed we're at the true bottom.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert!(
            state.is_following_tail(),
            "follow_tail should re-engage after scrolling back to the bottom"
        );
    }

    /// When an item is spliced to unmeasured (0px) while follow_tail
    /// is suspended, the re-engagement check should still work correctly
    #[gpux::test]
    fn test_follow_tail_reengagement_not_fooled_by_unmeasured_items(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 20 items × 50px = 1000px total, 200px viewport, 1000px
        // overdraw so all items get measured during the follow_tail
        // paint (matching realistic production settings).
        let state = ListState::new(20, crate::ListAlignment::Top, px(1000.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        state.set_follow_mode(FollowMode::Tail);

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert!(state.is_following_tail());

        // Scroll up a meaningful amount — suspends follow_tail.
        // 20 items × 50px = 1000px. viewport 200px. scroll_max = 800px.
        // Scrolling up 200px puts us at 600px, clearly not at bottom.
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(200.))),
            ..Default::default()
        });
        assert!(!state.is_following_tail());

        // Invalidate the last item (simulates EntryUpdated calling
        // remeasure_items). This makes items.summary().height
        // temporarily wrong (0px for the invalidated item).
        state.remeasure_items(19..20);

        // Paint — layout re-measures the invalidated item with its true
        // height. The re-engagement check uses these fresh measurements.
        // Since we scrolled 200px up from the 800px max, we're at
        // ~600px — NOT at the bottom, so follow_tail should NOT
        // re-engage.
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert!(
            !state.is_following_tail(),
            "follow_tail should not falsely re-engage due to an unmeasured item \
             reducing items.summary().height"
        );
    }

    #[gpux::test]
    fn test_follow_tail_reengages_after_scrollbar_disengagement(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        // 10 items × 50px = 500px total, 200px viewport, scroll_max = 300px.
        let state = ListState::new(10, crate::ListAlignment::Top, px(0.)).measure_all();

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        state.set_follow_mode(FollowMode::Tail);
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert!(state.is_following_tail());

        // Drag the scrollbar up to the middle — follow_tail should suspend.
        state.request_offset_from_scrollbar(point(px(0.), px(-150.)));
        assert!(!state.is_following_tail());

        // Drag the scrollbar back to the bottom — follow_tail should re-engage
        // on the next paint.
        state.request_offset_from_scrollbar(point(px(0.), px(-300.)));
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });
        assert!(
            state.is_following_tail(),
            "follow_tail should re-engage after scrolling back to the bottom via the scrollbar"
        );
    }

    #[gpux::test]
    fn test_follow_tail_reengages_after_scrollbar_drag_to_bottom_while_growing(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();

        let state = ListState::new(10, crate::ListAlignment::Top, px(0.)).measure_all();

        struct TestView(ListState);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(50.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let view = cx.update(|_, cx| cx.new(|_| TestView(state.clone())));

        state.set_follow_mode(FollowMode::Tail);
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });
        assert!(state.is_following_tail());

        state.scrollbar_drag_started();

        state.splice(10..10, 10);
        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.clone().into_any_element()
        });

        state.request_offset_from_scrollbar(point(px(0.), px(-300.)));
        state.scrollbar_drag_ended();

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(200.)), |_, _| {
            view.into_any_element()
        });

        assert!(
            state.is_following_tail(),
            "follow_tail should re-engage when the user drags the scrollbar to \
             the bottom of its track, even when content has grown during the drag \
             (so frozen_bottom < live_bottom)"
        );
    }
}
