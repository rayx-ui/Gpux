//! A scrollable list of elements with uniform height, optimized for large lists.
//! Rather than use the full taffy layout system, uniform_list simply measures
//! the first element and then lays out all remaining elements in a line based on that
//! measurement. This is much faster than the full layout system, but only works for
//! elements with uniform height.

use crate::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement, Interactivity,
    IntoElement, IsZero, LayoutId, ListSizingBehavior, Overflow, Pixels, Point, ScrollBehavior,
    ScrollContainerContext, ScrollHandle, ScrollMutationSource, Size, StickyItem, StyleRefinement,
    Styled, Window, point, px, size, sticky_push_offset,
};
use smallvec::SmallVec;
use std::{cell::RefCell, cmp, ops::Range, rc::Rc, usize};

use super::{ListHorizontalSizingBehavior, StickyItems};

/// uniform_list provides lazy rendering for a set of items that are of uniform height.
/// When rendered into a container with overflow-y: hidden and a fixed (or max) height,
/// uniform_list will only render the visible subset of items.
#[track_caller]
pub fn uniform_list<R>(
    id: impl Into<ElementId>,
    item_count: usize,
    f: impl 'static + Fn(Range<usize>, &mut Window, &mut App) -> Vec<R>,
) -> UniformList
where
    R: IntoElement,
{
    let id = id.into();
    let mut base_style = StyleRefinement::default();
    base_style.overflow.y = Some(Overflow::Scroll);

    let render_range = move |range: Range<usize>, window: &mut Window, cx: &mut App| {
        f(range, window, cx)
            .into_iter()
            .map(|component| component.into_any_element())
            .collect()
    };

    UniformList {
        item_count,
        item_to_measure_index: 0,
        render_items: Box::new(render_range),
        decorations: Vec::new(),
        interactivity: Interactivity {
            element_id: Some(id),
            base_style: Box::new(base_style),
            ..Interactivity::new()
        },
        scroll_handle: None,
        sizing_behavior: ListSizingBehavior::default(),
        horizontal_sizing_behavior: ListHorizontalSizingBehavior::default(),
        sticky_items: StickyItems::default(),
    }
}

/// A list element for efficiently laying out and displaying a list of uniform-height elements.
pub struct UniformList {
    item_count: usize,
    item_to_measure_index: usize,
    render_items: Box<
        dyn for<'a> Fn(Range<usize>, &'a mut Window, &'a mut App) -> SmallVec<[AnyElement; 64]>,
    >,
    decorations: Vec<Box<dyn UniformListDecoration>>,
    interactivity: Interactivity,
    scroll_handle: Option<UniformListScrollHandle>,
    sizing_behavior: ListSizingBehavior,
    horizontal_sizing_behavior: ListHorizontalSizingBehavior,
    sticky_items: StickyItems,
}

/// Frame state used by the [UniformList].
pub struct UniformListFrameState {
    items: SmallVec<[AnyElement; 32]>,
    decorations: SmallVec<[AnyElement; 2]>,
    scroll_context: Option<ScrollContainerContext>,
}

/// A handle for controlling the scroll position of a uniform list.
/// This should be stored in your view and passed to the uniform_list on each frame.
#[derive(Clone, Debug, Default)]
pub struct UniformListScrollHandle(pub Rc<RefCell<UniformListScrollState>>);

/// Where to place the element scrolled to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollStrategy {
    /// Place the element at the top of the list's viewport.
    Top,
    /// Attempt to place the element in the middle of the list's viewport.
    /// May not be possible if there's not enough list items above the item scrolled to:
    /// in this case, the element will be placed at the closest possible position.
    Center,
    /// Attempt to place the element at the bottom of the list's viewport.
    /// May not be possible if there's not enough list items above the item scrolled to:
    /// in this case, the element will be placed at the closest possible position.
    Bottom,
    /// If the element is not visible attempt to place it at:
    /// - The top of the list's viewport if the target element is above currently visible elements.
    /// - The bottom of the list's viewport if the target element is above currently visible elements.
    Nearest,
}

#[derive(Clone, Copy, Debug)]
#[allow(missing_docs)]
pub struct DeferredScrollToItem {
    /// The item index to scroll to
    pub item_index: usize,
    /// The scroll strategy to use
    pub strategy: ScrollStrategy,
    /// The offset in number of items
    pub offset: usize,
    pub scroll_strict: bool,
}

#[derive(Clone, Debug, Default)]
#[allow(missing_docs)]
pub struct UniformListScrollState {
    pub base_handle: ScrollHandle,
    pub deferred_scroll_to_item: Option<DeferredScrollToItem>,
    deferred_scroll_behavior: Option<ScrollBehavior>,
    /// Size of the item, captured during last layout.
    pub last_item_size: Option<ItemSize>,
    /// Whether the list was vertically flipped during last layout.
    pub y_flipped: bool,
}

#[derive(Copy, Clone, Debug, Default)]
/// The size of the item and its contents.
pub struct ItemSize {
    /// The size of the item.
    pub item: Size<Pixels>,
    /// The size of the item's contents, which may be larger than the item itself,
    /// if the item was bounded by a parent element.
    pub contents: Size<Pixels>,
}

impl UniformListScrollHandle {
    /// Create a new scroll handle to bind to a uniform list.
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(UniformListScrollState {
            base_handle: ScrollHandle::new(),
            deferred_scroll_to_item: None,
            deferred_scroll_behavior: None,
            last_item_size: None,
            y_flipped: false,
        })))
    }

    /// Scroll the list so that the given item index is visible.
    ///
    /// This uses non-strict scrolling: if the item is already fully visible, no scrolling occurs.
    /// If the item is out of view, it scrolls the minimum amount to bring it into view according
    /// to the strategy.
    pub fn scroll_to_item(&self, ix: usize, strategy: ScrollStrategy) {
        self.queue_scroll_to_item(
            DeferredScrollToItem {
                item_index: ix,
                strategy,
                offset: 0,
                scroll_strict: false,
            },
            None,
        );
    }

    /// Scroll the list so that the given item index is at scroll strategy position.
    ///
    /// This uses strict scrolling: the item will always be scrolled to match the strategy position,
    /// even if it's already visible. Use this when you need precise positioning.
    pub fn scroll_to_item_strict(&self, ix: usize, strategy: ScrollStrategy) {
        self.queue_scroll_to_item(
            DeferredScrollToItem {
                item_index: ix,
                strategy,
                offset: 0,
                scroll_strict: true,
            },
            None,
        );
    }

    /// Scroll the list to the given item index with an offset in number of items.
    ///
    /// This uses non-strict scrolling: if the item is already visible within the offset region,
    /// no scrolling occurs.
    ///
    /// The offset parameter shrinks the effective viewport by the specified number of items
    /// from the corresponding edge, then applies the scroll strategy within that reduced viewport:
    /// - `ScrollStrategy::Top`: Shrinks from top, positions item at the new top
    /// - `ScrollStrategy::Center`: Shrinks from top, centers item in the reduced viewport
    /// - `ScrollStrategy::Bottom`: Shrinks from bottom, positions item at the new bottom
    pub fn scroll_to_item_with_offset(&self, ix: usize, strategy: ScrollStrategy, offset: usize) {
        self.queue_scroll_to_item(
            DeferredScrollToItem {
                item_index: ix,
                strategy,
                offset,
                scroll_strict: false,
            },
            None,
        );
    }

    /// Scroll the list so that the given item index is at the exact scroll strategy position with an offset.
    ///
    /// This uses strict scrolling: the item will always be scrolled to match the strategy position,
    /// even if it's already visible.
    ///
    /// The offset parameter shrinks the effective viewport by the specified number of items
    /// from the corresponding edge, then applies the scroll strategy within that reduced viewport:
    /// - `ScrollStrategy::Top`: Shrinks from top, positions item at the new top
    /// - `ScrollStrategy::Center`: Shrinks from top, centers item in the reduced viewport
    /// - `ScrollStrategy::Bottom`: Shrinks from bottom, positions item at the new bottom
    pub fn scroll_to_item_strict_with_offset(
        &self,
        ix: usize,
        strategy: ScrollStrategy,
        offset: usize,
    ) {
        self.queue_scroll_to_item(
            DeferredScrollToItem {
                item_index: ix,
                strategy,
                offset,
                scroll_strict: true,
            },
            None,
        );
    }

    /// Queue one explicit programmatic item request with its final behavior.
    ///
    /// The logical request is retained until uniform item geometry is available, then resolved
    /// through the base [`ScrollHandle`] and shared programmatic driver.
    pub fn scroll_to_item_with_behavior(
        &self,
        request: DeferredScrollToItem,
        behavior: ScrollBehavior,
    ) {
        self.queue_scroll_to_item(request, Some(behavior));
    }

    fn queue_scroll_to_item(
        &self,
        request: DeferredScrollToItem,
        behavior: Option<ScrollBehavior>,
    ) {
        let mut state = self.0.borrow_mut();
        state.base_handle.cancel_programmatic_scroll();
        state.deferred_scroll_to_item = Some(request);
        state.deferred_scroll_behavior = behavior;
    }

    fn try_take_deferred_scroll_to_item(
        &self,
        item_size: ItemSize,
    ) -> Option<Option<(DeferredScrollToItem, Option<ScrollBehavior>)>> {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return None;
        };
        state.last_item_size = Some(item_size);
        let request = state
            .deferred_scroll_to_item
            .take()
            .map(|request| (request, state.deferred_scroll_behavior));
        state.deferred_scroll_behavior = None;
        Some(request)
    }

    fn try_base_handle(&self) -> Option<ScrollHandle> {
        self.0
            .try_borrow()
            .ok()
            .map(|state| state.base_handle.clone())
    }

    /// Try to check whether the list is flipped vertically without panicking if the handle is already borrowed.
    pub fn try_y_flipped(&self) -> Option<bool> {
        self.0.try_borrow().ok().map(|state| state.y_flipped)
    }

    /// Check if the list is flipped vertically.
    pub fn y_flipped(&self) -> bool {
        self.0.borrow().y_flipped
    }

    /// Get the index of the topmost visible child.
    #[cfg(any(test, feature = "test-support"))]
    pub fn logical_scroll_top_index(&self) -> usize {
        let this = self.0.borrow();
        this.deferred_scroll_to_item
            .as_ref()
            .map(|deferred| deferred.item_index)
            .unwrap_or_else(|| this.base_handle.logical_scroll_top().0)
    }

    /// Checks if the list can be scrolled vertically.
    pub fn is_scrollable(&self) -> bool {
        if let Some(size) = self.0.borrow().last_item_size {
            size.contents.height > size.item.height
        } else {
            false
        }
    }

    /// Whether the list is scrolled to the end, or `None` if the list is
    /// not scrollable.
    pub fn is_scrolled_to_end(&self) -> Option<bool> {
        let state = self.0.borrow();
        let max_offset = state.base_handle.max_offset();
        if max_offset.y <= px(0.) {
            return None;
        }
        let offset = state.base_handle.offset();
        Some(-offset.y >= max_offset.y)
    }

    /// Scroll to the bottom of the list.
    pub fn scroll_to_bottom(&self) {
        self.scroll_to_item(usize::MAX, ScrollStrategy::Bottom);
    }
}

impl Styled for UniformList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl Element for UniformList {
    type RequestLayoutState = UniformListFrameState;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let max_items = self.item_count;
        let item_size = self.measure_item(None, window, cx);
        let layout_id = self.interactivity.request_layout_with_style_handoff(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| match self.sizing_behavior {
                ListSizingBehavior::Infer => {
                    window.with_text_style(style.text_style().cloned(), |window| {
                        window.request_measured_layout_ref(
                            style,
                            move |known_dimensions, available_space, _window, _cx| {
                                let desired_height = item_size.height * max_items;
                                let width = known_dimensions.width.unwrap_or(match available_space
                                    .width
                                {
                                    AvailableSpace::Definite(x) => x,
                                    AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                                        item_size.width
                                    }
                                });
                                let height = match available_space.height {
                                    AvailableSpace::Definite(height) => desired_height.min(height),
                                    AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                                        desired_height
                                    }
                                };
                                size(width, height)
                            },
                        )
                    })
                }
                ListSizingBehavior::Auto => window
                    .with_text_style(style.text_style().cloned(), |window| {
                        window.request_layout_ref(style, None, cx)
                    }),
            },
        );

        (
            layout_id,
            UniformListFrameState {
                items: SmallVec::new(),
                decorations: SmallVec::new(),
                scroll_context: None,
            },
        )
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        frame_state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Hitbox> {
        let style = self
            .interactivity
            .compute_style(global_id, None, window, cx);
        let border = style.border_widths.to_pixels(window.rem_size());
        let padding = style
            .padding
            .to_pixels(bounds.size.into(), window.rem_size());

        let padded_bounds = Bounds::from_corners(
            bounds.origin + point(border.left + padding.left, border.top + padding.top),
            bounds.bottom_right()
                - point(border.right + padding.right, border.bottom + padding.bottom),
        );

        let can_scroll_horizontally = matches!(
            self.horizontal_sizing_behavior,
            ListHorizontalSizingBehavior::Unconstrained
        );

        let longest_item_size = self.measure_item(None, window, cx);
        let content_width = if can_scroll_horizontally {
            padded_bounds.size.width.max(longest_item_size.width)
        } else {
            padded_bounds.size.width
        };
        let content_size = Size {
            width: content_width,
            height: longest_item_size.height * self.item_count,
        };

        let shared_scroll_offset = self.interactivity.scroll_offset.clone().unwrap();
        let item_height = longest_item_size.height;
        let shared_scroll_to_item = self.scroll_handle.as_ref().and_then(|handle| {
            handle
                .try_take_deferred_scroll_to_item(ItemSize {
                    item: padded_bounds.size,
                    contents: content_size,
                })
                .unwrap_or(None)
        });
        let sticky_items = self.sticky_items.clone();

        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            content_size,
            window,
            cx,
            |_style, mut scroll_offset, hitbox, _, _, window, cx| {
                let y_flipped = if let Some(scroll_handle) = &self.scroll_handle {
                    scroll_handle.try_y_flipped().unwrap_or(false)
                } else {
                    false
                };

                if self.item_count > 0 {
                    let content_height = item_height * self.item_count;

                    let is_scrolled_vertically = !scroll_offset.y.is_zero();
                    let max_scroll_offset = padded_bounds.size.height - content_height;

                    if is_scrolled_vertically && scroll_offset.y < max_scroll_offset {
                        shared_scroll_offset.borrow_mut().y = max_scroll_offset;
                        scroll_offset.y = max_scroll_offset;
                    }

                    let content_width = content_size.width + padding.left + padding.right;
                    let is_scrolled_horizontally =
                        can_scroll_horizontally && !scroll_offset.x.is_zero();
                    if is_scrolled_horizontally && content_width <= padded_bounds.size.width {
                        shared_scroll_offset.borrow_mut().x = Pixels::ZERO;
                        scroll_offset.x = Pixels::ZERO;
                    }

                    if let Some((
                        DeferredScrollToItem {
                            mut item_index,
                            mut strategy,
                            offset,
                            scroll_strict,
                        },
                        behavior,
                    )) = shared_scroll_to_item
                    {
                        if y_flipped {
                            item_index = self.item_count.saturating_sub(item_index + 1);
                        }
                        let list_height = padded_bounds.size.height;
                        let mut updated_scroll_offset = *shared_scroll_offset.borrow();
                        let item_top = item_height * item_index;
                        let item_bottom = item_top + item_height;
                        let scroll_top = -updated_scroll_offset.y;
                        let offset_pixels = item_height * offset;

                        // is the selected item above/below currently visible items
                        let is_above = item_top < scroll_top + offset_pixels;
                        let is_below = item_bottom > scroll_top + list_height;

                        if scroll_strict || is_above || is_below {
                            if strategy == ScrollStrategy::Nearest {
                                if is_above {
                                    strategy = ScrollStrategy::Top;
                                } else if is_below {
                                    strategy = ScrollStrategy::Bottom;
                                }
                            }

                            let max_scroll_offset =
                                (content_height - list_height).max(Pixels::ZERO);
                            match strategy {
                                ScrollStrategy::Top => {
                                    updated_scroll_offset.y = -(item_top - offset_pixels)
                                        .clamp(Pixels::ZERO, max_scroll_offset);
                                }
                                ScrollStrategy::Center => {
                                    let item_center = item_top + item_height / 2.0;

                                    let viewport_height = list_height - offset_pixels;
                                    let viewport_center = offset_pixels + viewport_height / 2.0;
                                    let target_scroll_top = item_center - viewport_center;
                                    updated_scroll_offset.y =
                                        -target_scroll_top.clamp(Pixels::ZERO, max_scroll_offset);
                                }
                                ScrollStrategy::Bottom => {
                                    updated_scroll_offset.y = -(item_bottom - list_height)
                                        .clamp(Pixels::ZERO, max_scroll_offset);
                                }
                                ScrollStrategy::Nearest => {
                                    // Nearest, but the item is visible -> no scroll is required
                                }
                            }
                        }
                        if behavior.is_none() {
                            *shared_scroll_offset.borrow_mut() = updated_scroll_offset;
                            scroll_offset = updated_scroll_offset;
                        } else if let (Some(behavior), Some(handle)) = (
                            behavior,
                            self.scroll_handle
                                .as_ref()
                                .and_then(UniformListScrollHandle::try_base_handle),
                        ) {
                            handle.scroll_to(updated_scroll_offset, behavior);
                            window.refresh();
                        }
                    }

                    let first_visible_element_ix =
                        (-(scroll_offset.y + padding.top) / item_height).floor() as usize;
                    let last_visible_element_ix = ((-scroll_offset.y + padded_bounds.size.height)
                        / item_height)
                        .ceil() as usize;

                    let visible_range = first_visible_element_ix
                        ..cmp::min(last_visible_element_ix, self.item_count);

                    let candidate_indices =
                        Self::active_sticky_item_indices(&sticky_items, &visible_range);
                    let mut candidate_items =
                        SmallVec::<[(AnyElement, usize, Option<Pixels>, Pixels); 4]>::new();
                    if !y_flipped {
                        for (depth, ix) in candidate_indices.iter().copied().enumerate() {
                            let mut items = (self.render_items)(ix..ix + 1, window, cx);
                            if let Some(item) = items.pop() {
                                candidate_items.push((item, ix, Some(item_height * depth), px(0.)));
                            }
                        }
                        if let Some((next_candidate, retained_depth)) = candidate_indices
                            .last()
                            .and_then(|ix| sticky_items.next_replacement_after(*ix))
                        {
                            let active_stack_end = item_height * candidate_indices.len();
                            let incoming_start = scroll_offset.y + item_height * next_candidate;
                            let push_offset = sticky_push_offset(active_stack_end, incoming_start);
                            for (_, _, offset, sticky_push_offset) in
                                candidate_items.iter_mut().skip(retained_depth)
                            {
                                *offset = offset.map(|offset| offset + push_offset);
                                *sticky_push_offset = push_offset;
                            }
                        }
                    }

                    let rendered_visible_items = if y_flipped {
                        let flipped_range = self.item_count.saturating_sub(visible_range.end)
                            ..self.item_count.saturating_sub(visible_range.start);
                        let mut items = (self.render_items)(flipped_range, window, cx);
                        items.reverse();
                        items
                            .into_iter()
                            .zip(visible_range.clone())
                            .map(|(item, ix)| (item, ix, None, px(0.)))
                            .collect::<SmallVec<[(AnyElement, usize, Option<Pixels>, Pixels); 64]>>(
                            )
                    } else {
                        (self.render_items)(visible_range.clone(), window, cx)
                            .into_iter()
                            .zip(visible_range.clone())
                            .map(|(item, ix)| (item, ix, None, px(0.)))
                            .collect::<SmallVec<[(AnyElement, usize, Option<Pixels>, Pixels); 64]>>(
                            )
                    };
                    let mut rendered_items =
                        SmallVec::<[(AnyElement, usize, Option<Pixels>, Pixels); 64]>::new();
                    let mut incoming_sticky_items =
                        SmallVec::<[(AnyElement, usize, Option<Pixels>, Pixels); 8]>::new();
                    for (item, ix, _, _) in rendered_visible_items {
                        if Self::is_sticky_item_index(&sticky_items, ix) {
                            let target_offset =
                                item_height * sticky_items.predecessor_item_indices(ix).len();
                            let normal_offset = scroll_offset.y + item_height * ix;
                            incoming_sticky_items.push((
                                item,
                                ix,
                                (normal_offset < target_offset).then_some(target_offset),
                                px(0.),
                            ));
                        } else {
                            rendered_items.push((item, ix, None, px(0.)));
                        }
                    }
                    candidate_items.reverse();
                    rendered_items.extend(candidate_items);
                    rendered_items.extend(incoming_sticky_items);

                    let scroll_context = ScrollContainerContext {
                        scrollport_bounds: bounds,
                        content_bounds: Bounds::new(bounds.origin + scroll_offset, content_size),
                        scroll_offset,
                        overflow: _style.overflow,
                    };
                    frame_state.scroll_context = Some(scroll_context);

                    let content_mask = ContentMask::rectangular(bounds);
                    window.with_scroll_container_context(scroll_context, |window| {
                        window.with_content_mask(Some(content_mask), |window| {
                            for (mut item, ix, sticky_stack_offset, sticky_push_offset) in
                                rendered_items
                            {
                                let item_origin = padded_bounds.origin
                                    + point(
                                        scroll_offset.x,
                                        sticky_stack_offset
                                            .unwrap_or(scroll_offset.y + item_height * ix),
                                    );

                                let available_width = if can_scroll_horizontally {
                                    padded_bounds.size.width + scroll_offset.x.abs()
                                } else {
                                    padded_bounds.size.width
                                };
                                let available_space = size(
                                    AvailableSpace::Definite(available_width),
                                    AvailableSpace::Definite(item_height),
                                );
                                let item_size = item.layout_as_root(available_space, window, cx);
                                if sticky_stack_offset.is_some() {
                                    window.insert_hitbox(
                                        Bounds::new(item_origin, item_size),
                                        HitboxBehavior::BlockMouseExceptScroll,
                                    );
                                }
                                if sticky_push_offset < px(0.) {
                                    let mut item_scroll_context = scroll_context;
                                    item_scroll_context.scrollport_bounds.origin.y +=
                                        sticky_push_offset;
                                    window.with_scroll_container_context(
                                        item_scroll_context,
                                        |window| item.prepaint_at(item_origin, window, cx),
                                    );
                                } else {
                                    item.prepaint_at(item_origin, window, cx);
                                }
                                frame_state.items.push(item);
                            }

                            let bounds = Bounds::new(
                                padded_bounds.origin + scroll_offset,
                                padded_bounds.size,
                            );
                            for decoration in &self.decorations {
                                let mut decoration = decoration.as_ref().compute(
                                    visible_range.clone(),
                                    bounds,
                                    scroll_offset,
                                    item_height,
                                    self.item_count,
                                    window,
                                    cx,
                                );
                                let available_space = size(
                                    AvailableSpace::Definite(bounds.size.width),
                                    AvailableSpace::Definite(bounds.size.height),
                                );
                                decoration.layout_as_root(available_space, window, cx);
                                decoration.prepaint_at(bounds.origin, window, cx);
                                frame_state.decorations.push(decoration);
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
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<crate::Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Option<Hitbox>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            |_, window, cx| {
                let mut paint_items = |window: &mut Window, cx: &mut App| {
                    for item in &mut request_layout.items {
                        item.paint(window, cx);
                    }
                    for decoration in &mut request_layout.decorations {
                        decoration.paint(window, cx);
                    }
                };

                if let Some(scroll_context) = request_layout.scroll_context {
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

impl IntoElement for UniformList {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// A decoration for a [`UniformList`]. This can be used for various things,
/// such as rendering indent guides, or other visual effects.
pub trait UniformListDecoration {
    /// Compute the decoration element, given the visible range of list items,
    /// the bounds of the list, and the height of each item.
    fn compute(
        &self,
        visible_range: Range<usize>,
        bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        item_height: Pixels,
        item_count: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
}

impl<T: UniformListDecoration + 'static> UniformListDecoration for Entity<T> {
    fn compute(
        &self,
        visible_range: Range<usize>,
        bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        item_height: Pixels,
        item_count: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.update(cx, |inner, cx| {
            inner.compute(
                visible_range,
                bounds,
                scroll_offset,
                item_height,
                item_count,
                window,
                cx,
            )
        })
    }
}

impl UniformList {
    /// Selects a specific list item for measurement.
    pub fn with_width_from_item(mut self, item_index: Option<usize>) -> Self {
        self.item_to_measure_index = item_index.unwrap_or(0);
        self
    }

    /// Sets the sizing behavior, similar to the `List` element.
    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing_behavior = behavior;
        self
    }

    /// Sets the horizontal sizing behavior, controlling the way list items laid out horizontally.
    /// With [`ListHorizontalSizingBehavior::Unconstrained`] behavior, every item and the list itself will
    /// have the size of the widest item and lay out pushing the `end_slot` to the right end.
    pub fn with_horizontal_sizing_behavior(
        mut self,
        behavior: ListHorizontalSizingBehavior,
    ) -> Self {
        self.horizontal_sizing_behavior = behavior;
        match behavior {
            ListHorizontalSizingBehavior::FitList => {
                self.interactivity.base_style.overflow.x = None;
            }
            ListHorizontalSizingBehavior::Unconstrained => {
                self.interactivity.base_style.overflow.x = Some(Overflow::Scroll);
            }
        }
        self
    }

    /// Adds a decoration element to the list.
    pub fn with_decoration(mut self, decoration: impl UniformListDecoration + 'static) -> Self {
        self.decorations.push(Box::new(decoration));
        self
    }

    /// Adds items that may contain sticky positioned descendants.
    ///
    /// Plain indices map to [`crate::StickyStacking::Level(0)`].
    pub fn sticky_items<I>(mut self, items: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<StickyItem>,
    {
        self.sticky_items = StickyItems::new(items, self.item_count);
        self
    }

    fn active_sticky_item_indices(
        sticky_items: &StickyItems,
        visible_range: &Range<usize>,
    ) -> SmallVec<[usize; 4]> {
        sticky_items
            .active_item_indices_before(visible_range.start)
            .into_iter()
            .collect()
    }

    fn is_sticky_item_index(sticky_items: &StickyItems, index: usize) -> bool {
        sticky_items.contains(index)
    }

    fn measure_item(
        &self,
        list_width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Size<Pixels> {
        if self.item_count == 0 {
            return Size::default();
        }

        let item_ix = cmp::min(self.item_to_measure_index, self.item_count - 1);
        let mut items = (self.render_items)(item_ix..item_ix + 1, window, cx);
        let Some(mut item_to_measure) = items.pop() else {
            return Size::default();
        };
        let available_space = size(
            list_width.map_or(AvailableSpace::MaxContent, |width| {
                AvailableSpace::Definite(width)
            }),
            AvailableSpace::MinContent,
        );
        item_to_measure.layout_as_root(available_space, window, cx)
    }

    /// Track and render scroll state of this list with reference to the given scroll handle.
    pub fn track_scroll(mut self, handle: &UniformListScrollHandle) -> Self {
        if let Some(base_handle) = handle.try_base_handle() {
            self.interactivity.tracked_scroll_handle = Some(base_handle);
        }
        self.scroll_handle = Some(handle.clone());
        self
    }

    /// Sets whether the list is flipped vertically, such that item 0 appears at the bottom.
    pub fn y_flipped(mut self, y_flipped: bool) -> Self {
        if let Some(ref scroll_handle) = self.scroll_handle {
            if let Ok(mut scroll_state) = scroll_handle.0.try_borrow_mut() {
                let base_handle = scroll_state.base_handle.clone();
                if let Some(offset) = base_handle.try_offset() {
                    match scroll_state.last_item_size {
                        Some(last_size) if scroll_state.y_flipped != y_flipped => {
                            let new_y_offset =
                                -(offset.y + last_size.contents.height - last_size.item.height);
                            if base_handle.request_offset(
                                point(offset.x, new_y_offset),
                                ScrollMutationSource::Deferred,
                            ) {
                                scroll_state.y_flipped = y_flipped;
                            }
                        }
                        // Handle case where list is initially flipped.
                        None if y_flipped => {
                            if base_handle.request_offset(
                                point(offset.x, Pixels::MIN),
                                ScrollMutationSource::Deferred,
                            ) {
                                scroll_state.y_flipped = y_flipped;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        self
    }
}

impl InteractiveElement for UniformList {
    fn interactivity(&mut self) -> &mut crate::Interactivity {
        &mut self.interactivity
    }
}

#[cfg(test)]
mod test {
    use crate::{ScrollMutationSource, StickyItem, StickyStacking, TestApp, TestAppContext};

    use super::{StickyItems, UniformList};

    #[test]
    fn keyed_sticky_item_uses_contiguous_stack_runs() {
        let sticky_items = StickyItems::new(
            [
                StickyItem::new(0, StickyStacking::StackId("a".into())),
                StickyItem::new(8, StickyStacking::StackId("a".into())),
                StickyItem::new(16, StickyStacking::StackId("b".into())),
                StickyItem::new(24, StickyStacking::StackId("a".into())),
            ],
            40,
        );

        let stack = UniformList::active_sticky_item_indices(&sticky_items, &(10..18));
        assert_eq!(stack.as_slice(), &[0, 8]);

        let stack = UniformList::active_sticky_item_indices(&sticky_items, &(20..28));
        assert_eq!(stack.as_slice(), &[16]);

        let stack = UniformList::active_sticky_item_indices(&sticky_items, &(30..34));
        assert_eq!(stack.as_slice(), &[24]);
    }

    #[test]
    fn keyed_sticky_item_blocks_clicks_to_overlapped_uniform_list_rows() {
        use crate::{
            Context, IntoElement, MouseButton, Render, UniformListScrollHandle, Window, div, point,
            prelude::*, px, uniform_list,
        };
        use std::{cell::RefCell, ops::Range, rc::Rc};

        struct StickyUniformListView {
            scroll_handle: UniformListScrollHandle,
            clicks: Rc<RefCell<Vec<usize>>>,
        }

        impl Render for StickyUniformListView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let clicks = self.clicks.clone();
                uniform_list("entries", 20, move |range: Range<usize>, _, _| {
                    range
                        .map(|ix| {
                            let clicks = clicks.clone();
                            let row = div()
                                .id(("row", ix))
                                .w(px(100.))
                                .h(px(20.))
                                .on_click(move |_, _, _| clicks.borrow_mut().push(ix));
                            if ix < 2 {
                                row.sticky().top(px(0.)).into_any_element()
                            } else {
                                row.into_any_element()
                            }
                        })
                        .collect()
                })
                .w(px(100.))
                .h(px(100.))
                .sticky_items([
                    StickyItem::new(0, StickyStacking::StackId("a".into())),
                    StickyItem::new(1, StickyStacking::StackId("a".into())),
                ])
                .track_scroll(&self.scroll_handle)
            }
        }

        let scroll_handle = UniformListScrollHandle::new();
        scroll_handle
            .0
            .borrow()
            .base_handle
            .request_offset(point(px(0.), px(-80.)), ScrollMutationSource::Programmatic);
        let clicks = Rc::new(RefCell::new(Vec::new()));
        let view_clicks = clicks.clone();
        let mut app = TestApp::new();
        let mut window = app.open_window(move |_, _| StickyUniformListView {
            scroll_handle,
            clicks: view_clicks,
        });

        window.draw();
        window.simulate_click(point(px(50.), px(30.)), MouseButton::Left);

        assert_eq!(&*clicks.borrow(), &[1]);
    }

    #[test]
    fn pushed_sticky_uniform_list_child_hit_tests_below_its_retained_parent() {
        use crate::{
            Context, IntoElement, Render, UniformListScrollHandle, Window, div, point, prelude::*,
            px, uniform_list,
        };
        use std::{cell::RefCell, ops::Range, rc::Rc};

        struct StickyUniformListView {
            scroll_handle: UniformListScrollHandle,
            clicks: Rc<RefCell<Vec<usize>>>,
        }

        impl Render for StickyUniformListView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let clicks = self.clicks.clone();
                uniform_list("hierarchy-entries", 40, move |range: Range<usize>, _, _| {
                    range
                        .map(|ix| {
                            let clicks = clicks.clone();
                            let row = div()
                                .id(("hierarchy-row", ix))
                                .w(px(100.))
                                .h(px(20.))
                                .on_click(move |_, _, _| clicks.borrow_mut().push(ix));
                            match ix {
                                0 | 24 => row.sticky().top(px(0.)).into_any_element(),
                                8 | 16 => row.sticky().top(px(20.)).into_any_element(),
                                _ => row.into_any_element(),
                            }
                        })
                        .collect()
                })
                .w(px(100.))
                .h(px(100.))
                .sticky_items([
                    StickyItem::new(0, StickyStacking::Level(0)),
                    StickyItem::new(8, StickyStacking::Level(1)),
                    StickyItem::new(16, StickyStacking::Level(1)),
                    StickyItem::new(24, StickyStacking::Level(0)),
                ])
                .track_scroll(&self.scroll_handle)
            }
        }

        let scroll_handle = UniformListScrollHandle::new();
        scroll_handle
            .0
            .borrow()
            .base_handle
            .request_offset(point(px(0.), px(-310.)), ScrollMutationSource::Programmatic);
        let clicks = Rc::new(RefCell::new(Vec::new()));
        let mut app = TestApp::new();
        let mut window = app.open_window(move |_, _| StickyUniformListView {
            scroll_handle,
            clicks,
        });

        window.draw();
        window.update(|_, window, _| {
            let parent_bounds =
                crate::Bounds::new(point(px(0.), px(0.)), crate::size(px(100.), px(20.)));
            let top_hitbox_id = window
                .rendered_frame
                .hit_test(point(px(50.), px(5.)))
                .ids
                .first()
                .copied()
                .expect("overlapping uniform-list stickies should insert hitboxes");
            let top_hitbox = window
                .rendered_frame
                .hitboxes
                .iter()
                .find(|hitbox| hitbox.id == top_hitbox_id)
                .expect("top uniform-list hitbox should be retained");
            assert_eq!(parent_bounds, top_hitbox.bounds);
        });
    }

    #[test]
    fn active_sticky_item_indices_stack_nested_levels() {
        let sticky_items = StickyItems::new(
            [
                StickyItem::new(0, StickyStacking::Level(0)),
                StickyItem::new(8, StickyStacking::Level(1)),
                StickyItem::new(16, StickyStacking::Level(1)),
                StickyItem::new(24, StickyStacking::Level(0)),
            ],
            40,
        );

        let stack = UniformList::active_sticky_item_indices(&sticky_items, &(10..18));
        assert_eq!(stack.as_slice(), &[0, 8]);

        let stack = UniformList::active_sticky_item_indices(&sticky_items, &(20..28));
        assert_eq!(stack.as_slice(), &[0, 16]);
    }

    #[test]
    fn active_sticky_item_indices_replaces_parent_and_drops_old_descendants() {
        let sticky_items = StickyItems::new(
            [
                StickyItem::new(0, StickyStacking::Level(0)),
                StickyItem::new(8, StickyStacking::Level(1)),
                StickyItem::new(24, StickyStacking::Level(0)),
            ],
            40,
        );

        let stack = UniformList::active_sticky_item_indices(&sticky_items, &(30..34));
        assert_eq!(stack.as_slice(), &[24]);
    }

    #[test]
    fn sticky_replacement_retains_only_unchanged_uniform_list_ancestors() {
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
    fn rendered_sticky_row_uses_uniform_list_scroll_context() {
        use crate::{
            AppContext as _, Bounds, Context, IntoElement, Point, Render, UniformListScrollHandle,
            Window, div, point, prelude::*, px, size, uniform_list,
        };
        use std::ops::Range;

        struct StickyUniformListView {
            scroll_handle: UniformListScrollHandle,
        }

        impl Render for StickyUniformListView {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                uniform_list(
                    "entries",
                    20,
                    |range: Range<usize>, _window: &mut Window, _cx| {
                        range
                            .map(|ix| {
                                let row = div().w(px(100.)).h(px(20.));
                                if ix == 3 {
                                    row.sticky()
                                        .top(px(0.))
                                        .debug_selector(|| "sticky-row".into())
                                        .into_any_element()
                                } else {
                                    row.into_any_element()
                                }
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .w(px(100.))
                .h(px(100.))
                .track_scroll(&self.scroll_handle)
            }
        }

        let mut test_app = TestAppContext::single();
        let scroll_handle = UniformListScrollHandle::new();
        scroll_handle
            .0
            .borrow()
            .base_handle
            .request_offset(point(px(0.), px(-70.)), ScrollMutationSource::Programmatic);
        let window = test_app.add_window(move |_, _| StickyUniformListView { scroll_handle });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear();
            })
            .unwrap();

        let sticky_bounds = test_app
            .update_window(any_window, |_, window, _| {
                window
                    .rendered_frame
                    .debug_bounds
                    .get("sticky-row")
                    .copied()
            })
            .unwrap();

        assert_eq!(
            Some(Bounds::new(Point::default(), size(px(100.), px(20.)))),
            sticky_bounds
        );
    }

    #[test]
    fn sticky_candidate_row_remains_rendered_without_changing_visible_range() {
        use crate::{
            AppContext as _, Bounds, Context, IntoElement, Point, Render, UniformListScrollHandle,
            Window, div, point, prelude::*, px, size, uniform_list,
        };
        use std::{cell::RefCell, ops::Range, rc::Rc};

        struct StickyCandidateUniformListView {
            scroll_handle: UniformListScrollHandle,
            visible_range: Rc<RefCell<Range<usize>>>,
        }

        impl Render for StickyCandidateUniformListView {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                let visible_range = self.visible_range.clone();
                uniform_list(
                    "entries",
                    20,
                    move |range: Range<usize>, _window: &mut Window, _cx| {
                        *visible_range.borrow_mut() = range.clone();
                        range
                            .map(|ix| {
                                let row = div().w(px(100.)).h(px(20.));
                                if ix == 0 {
                                    row.sticky()
                                        .top(px(0.))
                                        .debug_selector(|| "sticky-candidate".into())
                                        .into_any_element()
                                } else {
                                    row.into_any_element()
                                }
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .w(px(100.))
                .h(px(100.))
                .sticky_items([0])
                .track_scroll(&self.scroll_handle)
            }
        }

        let mut test_app = TestAppContext::single();
        let scroll_handle = UniformListScrollHandle::new();
        scroll_handle
            .0
            .borrow()
            .base_handle
            .request_offset(point(px(0.), px(-70.)), ScrollMutationSource::Programmatic);
        let visible_range = Rc::new(RefCell::new(0..0));
        let window = test_app.add_window({
            let visible_range = visible_range.clone();
            move |_, _| StickyCandidateUniformListView {
                scroll_handle,
                visible_range,
            }
        });
        let any_window = window.into();

        test_app
            .update_window(any_window, |_, window, cx| {
                window.draw(cx).clear();
            })
            .unwrap();

        let sticky_bounds = test_app
            .update_window(any_window, |_, window, _| {
                window
                    .rendered_frame
                    .debug_bounds
                    .get("sticky-candidate")
                    .copied()
            })
            .unwrap();

        assert_eq!(
            Some(Bounds::new(Point::default(), size(px(100.), px(20.)))),
            sticky_bounds
        );
        assert_eq!(3..9, *visible_range.borrow());
    }

    #[gpux::test]
    fn test_scroll_strategy_nearest(cx: &mut TestAppContext) {
        use crate::{
            Context, FocusHandle, ScrollStrategy, UniformListScrollHandle, Window, div, prelude::*,
            px, uniform_list,
        };
        use std::ops::Range;

        actions!(example, [SelectNext, SelectPrev]);

        struct TestView {
            index: usize,
            length: usize,
            scroll_handle: UniformListScrollHandle,
            focus_handle: FocusHandle,
            visible_range: Range<usize>,
        }

        impl TestView {
            pub fn select_next(
                &mut self,
                _: &SelectNext,
                window: &mut Window,
                _: &mut Context<Self>,
            ) {
                if self.index + 1 == self.length {
                    self.index = 0
                } else {
                    self.index += 1;
                }
                self.scroll_handle
                    .scroll_to_item(self.index, ScrollStrategy::Nearest);
                window.refresh();
            }

            pub fn select_previous(
                &mut self,
                _: &SelectPrev,
                window: &mut Window,
                _: &mut Context<Self>,
            ) {
                if self.index == 0 {
                    self.index = self.length - 1
                } else {
                    self.index -= 1;
                }
                self.scroll_handle
                    .scroll_to_item(self.index, ScrollStrategy::Nearest);
                window.refresh();
            }
        }

        impl Render for TestView {
            fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .id("list-example")
                    .track_focus(&self.focus_handle)
                    .on_action(cx.listener(Self::select_next))
                    .on_action(cx.listener(Self::select_previous))
                    .size_full()
                    .child(
                        uniform_list(
                            "entries",
                            self.length,
                            cx.processor(|this, range: Range<usize>, _window, _cx| {
                                this.visible_range = range.clone();
                                range
                                    .map(|ix| div().id(ix).h(px(20.0)).child(format!("Item {ix}")))
                                    .collect()
                            }),
                        )
                        .track_scroll(&self.scroll_handle)
                        .h(px(200.0)),
                    )
            }
        }

        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle, cx);
            TestView {
                scroll_handle: UniformListScrollHandle::new(),
                index: 0,
                focus_handle,
                length: 47,
                visible_range: 0..0,
            }
        });

        // 10 out of 47 items are visible

        // First 9 times selecting next item does not scroll
        for ix in 1..10 {
            cx.dispatch_action(SelectNext);
            view.read_with(cx, |view, _| {
                assert_eq!(view.index, ix);
                assert_eq!(view.visible_range, 0..10);
            })
        }

        // Now each time the list scrolls down by 1
        for ix in 10..47 {
            cx.dispatch_action(SelectNext);
            view.read_with(cx, |view, _| {
                assert_eq!(view.index, ix);
                assert_eq!(view.visible_range, ix - 9..ix + 1);
            })
        }

        // After the last item we move back to the start
        cx.dispatch_action(SelectNext);
        view.read_with(cx, |view, _| {
            assert_eq!(view.index, 0);
            assert_eq!(view.visible_range, 0..10);
        });

        // Return to the last element
        cx.dispatch_action(SelectPrev);
        view.read_with(cx, |view, _| {
            assert_eq!(view.index, 46);
            assert_eq!(view.visible_range, 37..47);
        });

        // First 9 times selecting previous does not scroll
        for ix in (37..46).rev() {
            cx.dispatch_action(SelectPrev);
            view.read_with(cx, |view, _| {
                assert_eq!(view.index, ix);
                assert_eq!(view.visible_range, 37..47);
            })
        }

        // Now each time the list scrolls up by 1
        for ix in (0..37).rev() {
            cx.dispatch_action(SelectPrev);
            view.read_with(cx, |view, _| {
                assert_eq!(view.index, ix);
                assert_eq!(view.visible_range, ix..ix + 10);
            })
        }
    }
}
