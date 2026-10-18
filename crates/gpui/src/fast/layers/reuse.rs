//! Carrying a layer's non-scene records (hitboxes, listeners, dispatch
//! nodes) through frames that composite it. Adapted from gpui-fast's
//! `fast/layers/reuse.rs`.
//!
//! gpux carries through its own [`Window::reuse_prepaint`] and
//! [`Window::reuse_paint`], so that everything they copy for a view drawn
//! from last frame (the hitboxes' dispatch nodes, element states queued to be
//! carried over, the accessibility events) is carried for a composited layer
//! too, and then moves what the scroll moved: the hitboxes and the window
//! control hitboxes.
//!
//! On a frame that composites a layer, its content is neither prepainted nor
//! painted, but what prepainting and painting it added to the last frame
//! besides the scene is still wanted: its hitboxes, for hit testing, its
//! dispatch nodes, for focus and key and action dispatch, and its mouse
//! listeners, cursor styles and tab stops. [`carry_prepaint`] and
//! [`carry_paint`] copy them from the rendered frame where the container's
//! children would add them, as [`Window::reuse_prepaint`] and
//! [`Window::reuse_paint`] do for a reused view, but without replaying the
//! scene, which the layer's tiles stand for, and with the hitboxes moved by
//! the scroll since the content was painted and clipped to the viewport.

use std::{ops::Range, rc::Rc};

use crate::{
    App, Bounds, EntityId, GlobalElementId, Hitbox, LayoutId, PaintIndex, Pixels, Point,
    PrepaintStateIndex, Window,
    fast::{
        dependencies::RenderDependencies,
        layers::input::LayerInput,
        retained::{CarriedStates, RetainedLayout},
    },
};

/// What content painted into a layer runs on every frame that composites
/// the layer instead of painting it: the content is neither prepainted nor
/// painted then, and the callback gets how far it moved, in window space,
/// since it was painted. A `ParentViewport` `virtual_list` answers its
/// metrics and calls its range listener so.
pub(crate) type CompositeCallback = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App)>;

/// Registers `callback` with the layer the content being prepainted paints
/// into, returning whether there is one that takes it: a scrolling `div`'s,
/// not a list's.
pub(crate) fn on_composite(window: &mut Window, callback: CompositeCallback) -> bool {
    match window
        .fast_layers
        .painting
        .as_mut()
        .and_then(|painting| painting.on_composite.as_mut())
    {
        Some(callbacks) => {
            callbacks.push(callback);
            true
        }
        None => false,
    }
}

/// Runs what the content of the layer of the container `id` asked to run
/// on a frame compositing it at `scroll_offset`.
pub(crate) fn run_on_composite(
    window: &mut Window,
    cx: &mut App,
    id: &GlobalElementId,
    scroll_offset: Point<Pixels>,
) {
    let Some(record) = window
        .fast_layers
        .layers
        .get(id)
        .and_then(|layer| layer.record.as_ref())
    else {
        return;
    };
    if record.on_composite.is_empty() {
        return;
    }
    let callbacks = record.on_composite.clone();
    let moved = scroll_offset - record.scroll_offset;
    for callback in callbacks.iter() {
        callback(moved, window, cx);
    }
}

/// Carries the prepaint records of the content of the layer of the
/// container `id`, composited at `scroll_offset` in `viewport`, into the
/// frame being drawn, where prepainting the content would add them.
pub(crate) fn carry_prepaint(
    window: &mut Window,
    id: &GlobalElementId,
    viewport: Bounds<Pixels>,
    scroll_offset: Point<Pixels>,
) {
    let frame = window.fast_layers.frame;
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    // The rows of lists in the content, carried each shown or hidden as its
    // list shows it at this offset (see `lists::show_content_rows`).
    let told: Vec<(Range<PrepaintStateIndex>, bool)> = layer
        .rows
        .told_before(frame)
        .map(|rows| {
            rows.iter()
                .map(|row| (row.prepaint.clone(), row.shown))
                .collect()
        })
        .unwrap_or_default();
    // A `div`'s records begin with its own scroll target; a list's are its rows'.
    let container = !layer.rows.list;
    let Some(record) = layer.record.as_mut() else {
        return;
    };
    let delta = scroll_offset - record.scroll_offset;
    let range = record.prepaint_range.clone();
    let input = &mut layer.input;
    let moved = delta != input.stale;
    // The records were carried into the last frame too, moved as far as the
    // content had scrolled by then.
    let step = delta - input.stale;
    input.step = step;
    input.stale = delta;
    input.viewport = viewport;
    input.handle_offset.set(delta);
    // The hitboxes are the layer's, lent to the carry while it runs.
    let hitboxes = std::mem::take(&mut input.hitboxes);
    if container {
        window.a11y.begin_container_replay();
    }
    let carried = carry_prepaint_records(
        window, &range, &hitboxes, delta, step, viewport, !moved, true, &told,
    );
    window.a11y.end_container_replay();
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    layer.input.hitboxes = hitboxes;
    if let Some(record) = layer.record.as_mut() {
        record.prepaint_range = carried.clone();
    }
    if told.is_empty() {
        return;
    }
    // The told rows lie where the carry put the records, and those hidden
    // take nothing of the focus the copy gave back.
    let shift = |prepaint: &Range<PrepaintStateIndex>| {
        prepaint.start.shifted(&range.start, &carried.start)
            ..prepaint.end.shifted(&range.start, &carried.start)
    };
    let mut hidden = Vec::new();
    if let Some(rows) = layer.rows.told_before(frame) {
        for row in rows.iter_mut() {
            row.prepaint = shift(&row.prepaint);
            if !row.shown {
                hidden.push(row.prepaint.clone());
            }
        }
    }
    layer.rows.told_again(frame);
    for prepaint in hidden {
        crate::fast::layers::lists::hide_focus(&mut window.next_frame, &prepaint);
    }
}

/// Carries the prepaint records the rendered frame holds over `range` into
/// the frame being drawn, where prepainting what added them would add them
/// again, returning where they lie in it: `hitboxes`, which stand for those
/// of the range, moved by `delta` and clipped to `viewport`; its tooltips if
/// `tooltips`, a tooltip showing where its element was when it was
/// requested; its element states if `element_states`, which are dropped
/// otherwise; its line layouts and dispatch nodes; and what it tells of its
/// elements, moved by `step`.
///
/// `delta` is how far the content has moved since it was painted, which the
/// hitboxes kept from then are moved by; `step` how far since the last
/// frame, whose records, copied here, lie where that frame showed them.
#[inline]
#[allow(clippy::too_many_arguments)]
pub(crate) fn carry_prepaint_records(
    window: &mut Window,
    range: &Range<PrepaintStateIndex>,
    hitboxes: &[Hitbox],
    delta: Point<Pixels>,
    step: Point<Pixels>,
    viewport: Bounds<Pixels>,
    tooltips: bool,
    element_states: bool,
    rows: &[(Range<PrepaintStateIndex>, bool)],
) -> Range<PrepaintStateIndex> {
    // Content that deferred draws is never composited.
    debug_assert_eq!(
        range.start.deferred_draws_index,
        range.end.deferred_draws_index
    );
    let start = window.prepaint_index();
    // The accessibility nodes the records carry lie where the last frame showed them.
    window.a11y.replay_offset = step;
    // Each of `rows`, in order within `range`, replayed shown or hidden.
    window.reuse_prepaint_with_rows(range.clone(), rows);
    window.a11y.replay_offset = Point::default();
    let next = &mut window.next_frame;
    // The hitboxes copied are where the content was painted; the layer's
    // stand for them where it is composited.
    debug_assert_eq!(
        next.hitboxes.len() - start.hitboxes_index,
        hitboxes.len(),
        "a layer's hitboxes are those of its prepaint range"
    );
    next.hitboxes.truncate(start.hitboxes_index);
    next.hitboxes
        .extend(LayerInput::hitboxes_at(hitboxes, delta, viewport));
    // A scroll hides tooltips.
    if !tooltips {
        next.tooltip_requests.truncate(start.tooltips_index);
    }
    if !element_states {
        next.accessed_element_states
            .truncate(start.accessed_element_states_index);
        next.pending_element_state_transfers
            .truncate(start.pending_element_state_transfers_index);
    }
    start..window.prepaint_index()
}

/// Carries the paint records of the content of the layer of the container
/// `id` into the frame being drawn, where painting the content would add
/// them: what [`carry_prepaint`] carried the prepaint records of.
pub(crate) fn carry_paint(window: &mut Window, id: &GlobalElementId) {
    let frame = window.fast_layers.frame;
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    let Some(record) = layer.record.as_ref() else {
        return;
    };
    let range = record.paint_range.clone();
    let step = layer.input.step;
    let viewport = layer.input.viewport;
    let mut carried = carry_paint_records(window, &range, step, viewport, true);
    // The told rows' paint records lie where the carry put them, and those
    // hidden answer nothing: the copy gave their tab stops back.
    let from = PaintIndex {
        scene_index: 0,
        ..range.start.clone()
    };
    let to = PaintIndex {
        scene_index: 0,
        ..carried.start.clone()
    };
    let mut hidden = Vec::new();
    if let Some(rows) = window
        .fast_layers
        .layers
        .get_mut(id)
        .and_then(|layer| layer.rows.told_in(frame))
    {
        for row in rows.iter_mut() {
            if let Some(paint) = row.paint.as_mut() {
                *paint = paint.start.shifted(&from, &to)..paint.end.shifted(&from, &to);
                if !row.shown {
                    hidden.push(paint.clone());
                }
            }
        }
    }
    for paint in hidden {
        crate::fast::layers::lists::hide_records(&mut window.next_frame, &paint);
    }
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    if let Some(record) = layer.record.as_mut() {
        // The content's scene is the layer's, which is not carried.
        carried.start.scene_index = record.paint_range.start.scene_index;
        carried.end.scene_index = record.paint_range.end.scene_index;
        record.paint_range = carried;
    }
    layer.input.ranges_frame = Some(frame);
}

/// Carries the paint records the rendered frame holds over `range` into
/// the frame being drawn, where painting what added them would add them
/// again, returning where they lie in it: its window control hitboxes,
/// moved by `step` and clipped to `viewport`, cursor styles, input
/// handlers, mouse listeners, element states if `element_states`, tab stops,
/// line layouts and, for tests, debug bounds, moved by `step`. The scene is
/// not carried.
///
/// `step` is how far the content has moved since the last frame, whose
/// records, copied here, lie where that frame showed them.
#[inline]
pub(crate) fn carry_paint_records(
    window: &mut Window,
    range: &Range<PaintIndex>,
    step: Point<Pixels>,
    viewport: Bounds<Pixels>,
    element_states: bool,
) -> Range<PaintIndex> {
    let start = window.paint_index();
    // The content's scene is the layer's own, which its tiles stand for:
    // nothing of the frame's scene is replayed.
    let mut records = range.clone();
    records.start.scene_index = 0;
    records.end.scene_index = 0;
    let pending = window.next_frame.pending_element_state_transfers.len();
    window.reuse_paint(records);
    let next = &mut window.next_frame;
    let controls = start.fast_window_control_hitboxes_index;
    for (_, hitbox) in &mut next.window_control_hitboxes[controls..] {
        let moved = LayerInput::hitboxes_at(std::slice::from_ref(hitbox), step, viewport)
            .next()
            .expect("one hitbox");
        *hitbox = moved;
    }
    if !element_states {
        next.accessed_element_states
            .truncate(start.accessed_element_states_index);
        next.pending_element_state_transfers.truncate(pending);
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        let moved: Vec<_> = next
            .debug_bounds_records
            .drain(start.debug_bounds_index..)
            .map(|(selector, bounds)| (selector, Bounds::new(bounds.origin + step, bounds.size)))
            .collect();
        for (selector, bounds) in moved {
            next.record_debug_bounds(selector, bounds);
        }
    }
    start..window.paint_index()
}

/// Follows, in the records of the virtual lists' layers whose rows lie
/// inside a retained subtree the frame being drawn copies from the last
/// one, the copy of its prepaint records: those the rendered frame held over
/// `from` land from `to` on. A frame drawing the view holding a list from
/// the last one neither prepaints nor paints the list, but carries its rows'
/// records along, so that the next frame can carry them in turn.
///
/// A `div`'s layer whose content read hovers is painted again after such a
/// frame instead: whether those hovers changed is told from the last frame's
/// hit test, which a scroll since leaves behind, while a list keeps a row
/// only where no hitbox can change the hovers it read (see
/// [`crate::fast::layers::lists`]). Content that read no hover follows the
/// copy as a list's rows do, so that a view holding a scrolling `div`, drawn
/// from the last frame while the view around it renders again, leaves the
/// `div`'s layer compositing.
pub(crate) fn follow_prepaint(
    window: &mut Window,
    from: &Range<PrepaintStateIndex>,
    to: &PrepaintStateIndex,
) {
    if window.fast_layers.layers.is_empty() {
        return;
    }
    let frame = window.fast_layers.frame;
    let inside = |range: &Range<PrepaintStateIndex>| {
        let (a, b) = (&range.start, &range.end);
        let (c, d) = (&from.start, &from.end);
        a.hitboxes_index >= c.hitboxes_index
            && b.hitboxes_index <= d.hitboxes_index
            && a.tooltips_index >= c.tooltips_index
            && b.tooltips_index <= d.tooltips_index
            && a.deferred_draws_index >= c.deferred_draws_index
            && b.deferred_draws_index <= d.deferred_draws_index
            && a.dispatch_tree_index >= c.dispatch_tree_index
            && b.dispatch_tree_index <= d.dispatch_tree_index
            && a.accessed_element_states_index >= c.accessed_element_states_index
            && b.accessed_element_states_index <= d.accessed_element_states_index
            && a.line_layout_index.lines_index >= c.line_layout_index.lines_index
            && b.line_layout_index.lines_index <= d.line_layout_index.lines_index
    };
    let shift = |range: &Range<PrepaintStateIndex>| {
        range.start.shifted(&from.start, to)..range.end.shifted(&from.start, to)
    };
    for layer in window.fast_layers.layers.values_mut() {
        layer
            .rows
            .follow_told_prepaint(frame, &mut window.next_frame, &inside, &shift);
        let current = layer
            .input
            .ranges_frame
            .is_some_and(|painted| painted + 1 == frame);
        let Some(record) = layer.record.as_mut() else {
            continue;
        };
        let follows = layer.rows.list || record.hovers.is_empty();
        if !current || !follows || !inside(&record.prepaint_range) {
            continue;
        }
        record.prepaint_range = shift(&record.prepaint_range);
        layer.rows.follow_prepaint(&shift);
        layer.input.prepaint_followed = Some(frame);
    }
}

/// Follows, as [`follow_prepaint`] does, the copy of the paint records the
/// rendered frame held over `from` to `to` on. The scene index of a layer's
/// paint range is its own scene's, which is not copied.
pub(crate) fn follow_paint(window: &mut Window, from: &Range<PaintIndex>, to: &PaintIndex) {
    if window.fast_layers.layers.is_empty() {
        return;
    }
    let frame = window.fast_layers.frame;
    let inside = |range: &Range<PaintIndex>| {
        let (a, b) = (&range.start, &range.end);
        let (c, d) = (&from.start, &from.end);
        a.fast_window_control_hitboxes_index >= c.fast_window_control_hitboxes_index
            && b.fast_window_control_hitboxes_index <= d.fast_window_control_hitboxes_index
            && a.mouse_listeners_index >= c.mouse_listeners_index
            && b.mouse_listeners_index <= d.mouse_listeners_index
            && a.input_handlers_index >= c.input_handlers_index
            && b.input_handlers_index <= d.input_handlers_index
            && a.cursor_styles_index >= c.cursor_styles_index
            && b.cursor_styles_index <= d.cursor_styles_index
            && a.accessed_element_states_index >= c.accessed_element_states_index
            && b.accessed_element_states_index <= d.accessed_element_states_index
            && a.tab_handle_index >= c.tab_handle_index
            && b.tab_handle_index <= d.tab_handle_index
            && a.line_layout_index.lines_index >= c.line_layout_index.lines_index
            && b.line_layout_index.lines_index <= d.line_layout_index.lines_index
    };
    // A layer's scene index counts from its own scene, so it can lie before
    // the window's, where shifting it would underflow: it is shifted by none.
    let from_start = PaintIndex {
        scene_index: 0,
        ..from.start.clone()
    };
    let to = PaintIndex {
        scene_index: 0,
        ..to.clone()
    };
    let shift = |range: &Range<PaintIndex>| {
        range.start.shifted(&from_start, &to)..range.end.shifted(&from_start, &to)
    };
    for layer in window.fast_layers.layers.values_mut() {
        // A row's scene index is its layer's own, as the record's is.
        let copied = |paint: &Range<PaintIndex>| {
            let mut paint = paint.clone();
            paint.start.scene_index = from.start.scene_index;
            paint.end.scene_index = from.start.scene_index;
            inside(&paint)
        };
        layer
            .rows
            .follow_told_paint(frame, &mut window.next_frame, &copied, &shift);
        if layer.input.prepaint_followed != Some(frame) {
            continue;
        }
        let Some(record) = layer.record.as_mut() else {
            continue;
        };
        let mut paint_range = record.paint_range.clone();
        // Only its scene index can lie outside what was copied.
        paint_range.start.scene_index = from.start.scene_index;
        paint_range.end.scene_index = from.start.scene_index;
        if !inside(&paint_range) {
            continue;
        }
        record.paint_range = shift(&record.paint_range);
        layer.rows.follow_paint(&shift);
        layer.input.ranges_frame = Some(frame);
    }
}

/// How a view drawn inside a layer's content was laid out when the content
/// was painted, for frames that composite the layer to lay it out again
/// without rendering it.
///
/// Views inside a layer's content keep no retained record of their own (the
/// layer is their retention, see [`crate::fast::layers::paint::inside_layer`]),
/// and a frame that composites the layer neither prepaints nor paints them;
/// but the view holding the container renders it again, and lays out its
/// children, a child view among them (pattern A). Without a record to reuse,
/// the child view would render every frame; its layer keeps its layout
/// instead.
pub(crate) struct KeptLayout {
    pub(crate) layout: Rc<RetainedLayout>,
    /// Everything laying the view out and prepainting it read.
    pub(crate) dependencies: RenderDependencies,
}

/// Keeps how the view `id`, prepainted inside the content of the layer being
/// painted, was laid out, and what laying it out and prepainting it read.
pub(crate) fn keep_view_layout(
    window: &mut Window,
    id: GlobalElementId,
    layout: Rc<RetainedLayout>,
    dependencies: RenderDependencies,
) {
    if let Some(painting) = window.fast_layers.painting.as_mut() {
        painting.view_layouts.insert(
            id,
            KeptLayout {
                layout,
                dependencies,
            },
        );
    }
}

/// Lays out the view `id`, of entity `entity`, which is not dirty, as the
/// layer whose content it is drawn in kept it, if one kept it and nothing it
/// read changed since, returning its node and the layout: the frame is then
/// expected to composite the layer, and if it paints the content afresh
/// after all, the view is rendered at that layout where it is prepainted.
pub(crate) fn reuse_kept_layout(
    window: &mut Window,
    id: &GlobalElementId,
    entity: EntityId,
    cx: &mut App,
) -> Option<(LayoutId, Rc<RetainedLayout>, CarriedStates)> {
    if window.fast_layers.layers.is_empty()
        || crate::fast::layers::paint::inside_layer(window)
        || !crate::fast::layers::active(window, cx)
        || crate::fast::retained::notified(window, entity)
    {
        return None;
    }
    let kept = window.fast_layers.layers.values().find_map(|layer| {
        let record = layer.record.as_ref()?;
        record.view_layouts.get(id)
    })?;
    let layout = kept.layout.clone();
    if layout.rem_size != window.rem_size()
        || layout.text_style != window.text_style()
        // gpux counts an entity updated without being notified as changed
        // for every reader.
        || cx.dependencies_changed(&kept.dependencies, true)
        || crate::fast::layers::invalidate::offset_read_changed(window, &kept.dependencies)
    {
        return None;
    }
    let dependencies = kept.dependencies.clone();
    if !crate::fast::layout::keep_retained_nodes(
        window.layout_engine.as_mut().unwrap(),
        &layout.keys,
        true,
    ) {
        return None;
    }
    let carried = crate::fast::retained::carry_element_states(window, &layout.element_states);
    cx.replay_dependencies(&dependencies);
    Some((layout.root, layout, carried))
}

/// How a scroll container's content was laid out when it was painted into
/// its layer, for frames that composite the layer to lay it out again
/// without requesting the layout of its children: the nodes they claimed,
/// the element states they used, the ids of the children's nodes, and what
/// laying them out read.
pub(crate) struct KeptContent {
    pub(crate) layout: Rc<RetainedLayout>,
    pub(crate) child_layout_ids: Vec<LayoutId>,
    pub(crate) dependencies: RenderDependencies,
}

/// Lays out the content of the scroll container `id`, which has `children`
/// children, as its layer kept it, if the
/// frame is expected to composite the layer and nothing the content read
/// changed: the children's layout is then not requested. Returns what the
/// container needs to request its own layout, and to request the children's
/// afterwards if its prepaint paints the content after all.
pub(crate) fn reuse_kept_content(
    window: &mut Window,
    cx: &mut App,
    id: &GlobalElementId,
    children: usize,
) -> Option<(Rc<KeptContent>, CarriedStates)> {
    if window.fast_layers.layers.is_empty()
        || !crate::fast::layout_key::keyed(window)
        || !crate::fast::layers::policy::expects_composite(window, cx, id)
    {
        return None;
    }
    let kept = window
        .fast_layers
        .layers
        .get(id)?
        .record
        .as_ref()?
        .content_layout
        .clone()?;
    if kept.child_layout_ids.len() != children
        || kept.layout.rem_size != window.rem_size()
        || kept.layout.text_style != window.text_style()
        || cx.dependencies_changed(&kept.dependencies, true)
        || crate::fast::layers::invalidate::offset_read_changed(window, &kept.dependencies)
    {
        return None;
    }
    if !crate::fast::layout::keep_retained_nodes(
        window.layout_engine.as_mut().unwrap(),
        &kept.layout.keys,
        true,
    ) {
        return None;
    }
    let carried = crate::fast::retained::carry_element_states(window, &kept.layout.element_states);
    cx.replay_dependencies(&kept.dependencies);
    // Kept for the layer, if its container paints it afresh after all: nothing about how the
    // content was laid out has changed.
    window
        .fast_layers
        .pending_content
        .insert(id.clone(), kept.clone());
    Some((kept, carried))
}

/// Gives back what [`reuse_kept_content`] kept, for the children to request
/// their layout themselves after all.
pub(crate) fn release_kept_content(
    window: &mut Window,
    kept: &KeptContent,
    carried: CarriedStates,
) {
    crate::fast::retained::release_carried_states(window, &kept.layout, carried);
    window
        .layout_engine
        .as_mut()
        .unwrap()
        .retention
        .release_kept(&kept.layout.keys);
}
