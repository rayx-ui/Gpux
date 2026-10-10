//! Views drawn again from what they drew on the last frame.
//!
//! Adapted from gpui-fast's `fast/retained.rs`. With view retention on,
//! every entity-backed view is a retained subtree. Each one drawn in a frame
//! leaves a record there: where its hitboxes, dispatch nodes, listeners and
//! primitives went (as ranges of the frame's lists), what it read while it
//! was built (`fast::dependencies`), the hovers it was painted by, and the
//! layout nodes it holds. On the next frame, a view whose record says
//! nothing it depends on changed is drawn again by copying those stretches of
//! the last frame instead of rendering, laying out, prepainting and painting
//! it.
//!
//! The records live in the frame rather than in element state, because the
//! stretches they point to belong to one frame. A view drawn again is not
//! visited, so the records of views nested in it would otherwise keep
//! pointing into the frame they were last visited in. Drawing a view again
//! copies its record and the records nested in it, shifted to where the copy
//! landed, so that any of them can be drawn again on its own later.
//!
//! The copying is gpux's own cached-view replay (`Window::reuse_prepaint`,
//! `Window::reuse_paint`), which already carries every per-frame output:
//! hitboxes re-pointed at the reused dispatch nodes, tooltips, deferred
//! draws, the dispatch subtree with its focus, element states and their
//! pending transfers, line layouts, listeners, input handlers, cursor
//! styles, tab stops, window-control hitboxes and the DevKit nodes with
//! their remapped ids, parents, focus and property setters. What records add
//! is the nesting, the layout, the dependencies and the hovers.
//!
//! Hovers follow gpux's rule (`fast::dependencies::may_replay_hovers`): a
//! view that read a hover is not drawn again while the pointer is over a
//! hitbox whose hover it read, so its hovers are never shown stale.

use std::{any::TypeId, mem, ops::Range, rc::Rc};

use collections::{FxHashMap, FxHashSet};
use refineable::Refineable as _;
use smallvec::SmallVec;

use crate::{
    AnyElement, App, Bounds, ContentMask, ElementPaintContext, EntityId, GlobalElementId, HitboxId,
    LayoutId, Pixels, ScaledPixels, Style, StyleRefinement, TextStyle, Window,
    fast::dependencies::{
        DependencyRecording, HoverArea, RecordedDependencies, RenderDependencies,
    },
    fast::splice::{Prebuilt, Rebuild, RebuildHandle, Splice, SplicedPrepaint},
    window::{PaintIndex, PrepaintStateIndex},
};

/// The retained views drawn in one frame, in the order they began
/// prepainting, which puts a view's nested views right after it.
#[derive(Default)]
pub(crate) struct RetainedSubtrees {
    pub(crate) records: Vec<RetainedSubtree>,
    by_id: FxHashMap<GlobalElementId, usize>,
    /// Ids drawn more than once in the frame: none of their views can be
    /// told apart to be drawn again.
    duplicates: FxHashSet<GlobalElementId>,
    /// The records whose prepaint is under way, innermost last.
    open: Vec<usize>,
}

pub(crate) struct RetainedSubtree {
    pub(crate) id: GlobalElementId,
    pub(crate) prepaint_range: Range<PrepaintStateIndex>,
    pub(crate) paint_range: Range<PaintIndex>,
    pub(crate) paint: PaintStatus,
    /// How many of the records following this one are nested inside it.
    pub(crate) nested: usize,
    pub(crate) context: Rc<RetainedContext>,
    /// Everything the view read, the views nested in it included.
    pub(crate) dependencies: RenderDependencies,
    /// What the view read itself, outside the views nested in it.
    pub(crate) own_dependencies: RenderDependencies,
    /// The hovers the view was drawn by, nested views included.
    pub(crate) hover_reads: Rc<[(HitboxId, bool)]>,
    /// Where those hovers can change.
    pub(crate) hover_area: HoverArea,
    /// The hitboxes inserted by what was deferred from the view, by their
    /// ranges in the frame it was built in: its own, for its hovers.
    deferred_hitboxes: SmallVec<[Range<usize>; 1]>,
    /// The layout nodes the view claimed while it was prepainted (list
    /// items), kept while it is drawn again so that building it finds them.
    pub(crate) layout_keys: Rc<[u64]>,
    /// How to lay the view out as it was without building it.
    pub(crate) layout: Option<Rc<RetainedLayout>>,
    /// How to build the view again on its own where it was drawn, for a
    /// view drawn from last frame around it. See [`crate::fast::splice`].
    pub(crate) rebuild: Option<Rc<Rebuild>>,
    /// The content mask it was painted under, for the same.
    pub(crate) paint_content_mask: Option<ContentMask<ScaledPixels>>,
}

pub(crate) enum PaintStatus {
    /// Not painted, so `paint_range` means nothing.
    Unpainted,
    /// Painted this frame into `paint_range`. When that was drawn from last
    /// frame, `source` is where it started there, for the records copied
    /// along with it to shift their own ranges by.
    Painted { source: Option<PaintIndex> },
    /// Copied along with the view at `anchor` and still holding last frame's
    /// `paint_range`, which is shifted once that one is painted.
    Pending { anchor: usize },
}

/// What a view's prepaint and paint depended on besides what it read: where
/// it was drawn and what it inherited there.
#[derive(Clone, PartialEq)]
pub(crate) struct RetainedContext {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
    /// The opacity and transform it was painted with, which cached views key
    /// on too (`Window::with_element_paint_context`).
    paint_context: ElementPaintContext,
}

impl RetainedContext {
    pub(crate) fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    pub(crate) fn content_mask(&self) -> ContentMask<Pixels> {
        self.content_mask
    }
}

/// What it takes to lay a view out as it was laid out last frame without
/// building it: a view is laid out by its content, so its layout is only
/// known from the nodes its content left.
#[derive(Clone)]
pub(crate) struct RetainedLayout {
    /// The node its content is laid out at.
    pub(crate) root: LayoutId,
    /// Every node its content claimed while its layout was requested.
    pub(crate) keys: Vec<u64>,
    /// The element states its content used while its layout was requested,
    /// kept for as long as it is not built.
    pub(crate) element_states: Vec<(GlobalElementId, TypeId)>,
    pub(crate) text_style: TextStyle,
    pub(crate) rem_size: Pixels,
}

impl RetainedLayout {
    pub(crate) fn root(&self) -> LayoutId {
        self.root
    }
}

/// A layout request being recorded as a [`RetainedLayout`].
struct RetainedLayoutRecording {
    keys: usize,
    transient: usize,
    element_states: usize,
    dependencies: DependencyRecording,
    text_style: TextStyle,
    rem_size: Pixels,
    /// How many sticky candidates and scroll contexts there were when it
    /// began.
    sticky: (usize, crate::window::StickyLayoutContextId),
}

/// A retained view being prepainted. See [`begin_retained`].
struct RetainedRecording {
    index: Option<usize>,
    dependencies: DependencyRecording,
    layout_keys: usize,
    hovers: usize,
}

/// A retained view being painted. See [`begin_retained_paint`].
struct RetainedPaintRecording {
    index: Option<usize>,
    start: PaintIndex,
    dependencies: DependencyRecording,
    hovers: usize,
}

/// A window's state for drawing views again, besides the records its frames
/// hold.
#[derive(Default)]
pub(crate) struct RetainedState {
    /// Whether the frame being drawn retains views; fixed when it begins.
    active: bool,
    /// Why no view is drawn from last frame in the frame being drawn, if
    /// anything rules that out for the whole frame; worked out when the
    /// first view asks, once the frame knows whether AccessKit is active.
    frame_bypass: Option<Option<Bypass>>,
    /// The retained views the deferred draw being drawn was deferred from:
    /// what it defers in turn, or copies from last frame, is theirs too.
    deferred_enclosing: EnclosingRetained,
    /// The entities notified since the last frame was drawn: a view
    /// notified is built, never drawn around its nested views.
    notified: FxHashSet<EntityId>,
    /// Nested views built for a splice that did not happen, for their
    /// elements to take over. See [`crate::fast::splice::take_prebuilt`].
    pub(crate) prebuilt: FxHashMap<GlobalElementId, crate::fast::splice::PrebuiltGap>,
}

/// Why no view is drawn from last frame: see `ViewBypassReason`, which
/// counts each where counters are compiled.
#[derive(Clone, Copy)]
enum Bypass {
    Refreshing,
    Drag,
    Inspector,
    Accessibility,
    Duplicate,
}

/// The retained views, as indices into this frame's records, that were
/// being drawn when something was deferred: what drawing it reads and the
/// hovers it is painted by are theirs too, though it is drawn after them.
#[derive(Clone, Default)]
pub(crate) struct EnclosingRetained(SmallVec<[usize; 4]>);

impl PrepaintStateIndex {
    /// This index, taken from a range that started at `from`, as it falls in
    /// a copy of that range starting at `to`.
    pub(crate) fn shifted(&self, from: &Self, to: &Self) -> Self {
        let shift = |index: usize, from: usize, to: usize| index - from + to;
        PrepaintStateIndex {
            hitboxes_index: shift(self.hitboxes_index, from.hitboxes_index, to.hitboxes_index),
            tooltips_index: shift(self.tooltips_index, from.tooltips_index, to.tooltips_index),
            deferred_draws_index: shift(
                self.deferred_draws_index,
                from.deferred_draws_index,
                to.deferred_draws_index,
            ),
            dispatch_tree_index: shift(
                self.dispatch_tree_index,
                from.dispatch_tree_index,
                to.dispatch_tree_index,
            ),
            devtools_nodes_index: shift(
                self.devtools_nodes_index,
                from.devtools_nodes_index,
                to.devtools_nodes_index,
            ),
            devtools_node_id: shift(
                self.devtools_node_id,
                from.devtools_node_id,
                to.devtools_node_id,
            ),
            accessed_element_states_index: shift(
                self.accessed_element_states_index,
                from.accessed_element_states_index,
                to.accessed_element_states_index,
            ),
            pending_element_state_transfers_index: shift(
                self.pending_element_state_transfers_index,
                from.pending_element_state_transfers_index,
                to.pending_element_state_transfers_index,
            ),
            line_layout_index: self
                .line_layout_index
                .shifted(&from.line_layout_index, &to.line_layout_index),
        }
    }
}

/// Whether two indices stand at the same place in each of a frame's lists.
impl PartialEq for PrepaintStateIndex {
    fn eq(&self, other: &Self) -> bool {
        // Destructured, so that a field added upstream cannot be missed.
        let PrepaintStateIndex {
            hitboxes_index,
            tooltips_index,
            deferred_draws_index,
            dispatch_tree_index,
            devtools_nodes_index,
            devtools_node_id,
            accessed_element_states_index,
            pending_element_state_transfers_index,
            line_layout_index,
        } = self;
        *hitboxes_index == other.hitboxes_index
            && *tooltips_index == other.tooltips_index
            && *deferred_draws_index == other.deferred_draws_index
            && *dispatch_tree_index == other.dispatch_tree_index
            && *devtools_nodes_index == other.devtools_nodes_index
            && *devtools_node_id == other.devtools_node_id
            && *accessed_element_states_index == other.accessed_element_states_index
            && *pending_element_state_transfers_index == other.pending_element_state_transfers_index
            && *line_layout_index == other.line_layout_index
    }
}

impl PaintIndex {
    /// See [`PrepaintStateIndex::shifted`].
    pub(crate) fn shifted(&self, from: &Self, to: &Self) -> Self {
        let shift = |index: usize, from: usize, to: usize| index - from + to;
        PaintIndex {
            scene_index: shift(self.scene_index, from.scene_index, to.scene_index),
            #[cfg(any(test, feature = "test-support"))]
            debug_bounds_index: shift(
                self.debug_bounds_index,
                from.debug_bounds_index,
                to.debug_bounds_index,
            ),
            mouse_listeners_index: shift(
                self.mouse_listeners_index,
                from.mouse_listeners_index,
                to.mouse_listeners_index,
            ),
            input_handlers_index: shift(
                self.input_handlers_index,
                from.input_handlers_index,
                to.input_handlers_index,
            ),
            cursor_styles_index: shift(
                self.cursor_styles_index,
                from.cursor_styles_index,
                to.cursor_styles_index,
            ),
            accessed_element_states_index: shift(
                self.accessed_element_states_index,
                from.accessed_element_states_index,
                to.accessed_element_states_index,
            ),
            tab_handle_index: shift(
                self.tab_handle_index,
                from.tab_handle_index,
                to.tab_handle_index,
            ),
            line_layout_index: self
                .line_layout_index
                .shifted(&from.line_layout_index, &to.line_layout_index),
            fast_window_control_hitboxes_index: shift(
                self.fast_window_control_hitboxes_index,
                from.fast_window_control_hitboxes_index,
                to.fast_window_control_hitboxes_index,
            ),
        }
    }
}

/// Whether two indices stand at the same place in each of a frame's lists.
impl PartialEq for PaintIndex {
    fn eq(&self, other: &Self) -> bool {
        let PaintIndex {
            scene_index,
            #[cfg(any(test, feature = "test-support"))]
            debug_bounds_index,
            mouse_listeners_index,
            input_handlers_index,
            cursor_styles_index,
            accessed_element_states_index,
            tab_handle_index,
            line_layout_index,
            fast_window_control_hitboxes_index,
        } = self;
        #[cfg(any(test, feature = "test-support"))]
        let same_debug_bounds = *debug_bounds_index == other.debug_bounds_index;
        #[cfg(not(any(test, feature = "test-support")))]
        let same_debug_bounds = true;
        *scene_index == other.scene_index
            && same_debug_bounds
            && *mouse_listeners_index == other.mouse_listeners_index
            && *input_handlers_index == other.input_handlers_index
            && *cursor_styles_index == other.cursor_styles_index
            && *accessed_element_states_index == other.accessed_element_states_index
            && *tab_handle_index == other.tab_handle_index
            && *line_layout_index == other.line_layout_index
            && *fast_window_control_hitboxes_index == other.fast_window_control_hitboxes_index
    }
}

impl crate::text_system::LineLayoutIndex {
    /// See [`PrepaintStateIndex::shifted`].
    pub(crate) fn shifted(&self, from: &Self, to: &Self) -> Self {
        let shift = |index: usize, from: usize, to: usize| index - from + to;
        Self {
            font_generation: to.font_generation,
            lines_index: shift(self.lines_index, from.lines_index, to.lines_index),
            wrapped_lines_index: shift(
                self.wrapped_lines_index,
                from.wrapped_lines_index,
                to.wrapped_lines_index,
            ),
            lines_by_hash_index: shift(
                self.lines_by_hash_index,
                from.lines_by_hash_index,
                to.lines_by_hash_index,
            ),
            wrapped_lines_by_hash_index: shift(
                self.wrapped_lines_by_hash_index,
                from.wrapped_lines_by_hash_index,
                to.wrapped_lines_by_hash_index,
            ),
        }
    }
}

impl RetainedSubtrees {
    pub(crate) fn clear(&mut self) {
        self.records.clear();
        self.by_id.clear();
        self.duplicates.clear();
        self.open.clear();
    }

    /// The painted record `id` left, if any.
    pub(crate) fn find(&self, id: &GlobalElementId) -> Option<usize> {
        if self.duplicates.contains(id) {
            return None;
        }
        let index = *self.by_id.get(id)?;
        matches!(self.records[index].paint, PaintStatus::Painted { .. }).then_some(index)
    }

    /// Whether the frame holds a record for `id` already.
    pub(crate) fn contains(&self, id: &GlobalElementId) -> bool {
        self.by_id.contains_key(id)
    }

    /// Whether `id` was drawn more than once in the frame.
    pub(crate) fn is_duplicated(&self, id: &GlobalElementId) -> bool {
        self.duplicates.contains(id)
    }

    /// The record `id` left in the frame being drawn, painted or not.
    pub(crate) fn find_open_or_painted(&self, id: &GlobalElementId) -> Option<usize> {
        if self.duplicates.contains(id) {
            return None;
        }
        self.by_id.get(id).copied()
    }

    fn push(&mut self, record: RetainedSubtree) -> usize {
        let index = self.records.len();
        // Two views with one id can both be drawn, but neither can be found
        // to be drawn again.
        if self.by_id.contains_key(&record.id) {
            self.duplicates.insert(record.id.clone());
        } else {
            self.by_id.insert(record.id.clone(), index);
        }
        self.records.push(record);
        index
    }

    /// Shifts the paint ranges of records copied along with a view drawn
    /// from last frame, now that it has been painted, and forgets those that
    /// were not.
    fn finish_frame(&mut self) {
        debug_assert!(self.open.is_empty());
        for index in 0..self.records.len() {
            let PaintStatus::Pending { anchor } = self.records[index].paint else {
                continue;
            };
            let shift = match &self.records[anchor].paint {
                PaintStatus::Painted {
                    source: Some(source),
                } if anchor != index => Some((
                    source.clone(),
                    self.records[anchor].paint_range.start.clone(),
                )),
                _ => None,
            };
            let record = &mut self.records[index];
            match shift {
                Some((from, to)) => {
                    record.paint_range = record.paint_range.start.shifted(&from, &to)
                        ..record.paint_range.end.shifted(&from, &to);
                    record.paint = PaintStatus::Painted { source: None };
                }
                None => record.paint = PaintStatus::Unpainted,
            }
        }
    }

    /// How many records the frame holds, for a prepaint discarded by
    /// [`Window::transact`] to drop those it made.
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    /// Drops the records made since there were `len`.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.records.truncate(len);
        self.by_id.retain(|_, index| *index < len);
        self.open.retain(|index| *index < len);
    }
}

/// Whether the frame being drawn retains views.
#[inline(always)]
pub(crate) fn active(window: &Window) -> bool {
    window.fast_retained_state.active
}

/// Begins a frame's view retention: on while view retention is, and the
/// views that last frame read something themselves that changed since are
/// marked dirty, as a notification marks a view and the views around it.
pub(crate) fn begin_frame(window: &mut Window, cx: &App) {
    let active = window.fast_mode.view_retention();
    let state = &mut window.fast_retained_state;
    state.active = active;
    state.frame_bypass = None;
    if !active || window.refreshing {
        return;
    }
    let mut changed = SmallVec::<[EntityId; 8]>::new();
    for record in &window.rendered_frame.fast_retained.records {
        let Some(entity) = view_entity(&record.id) else {
            continue;
        };
        if cx.dependencies_changed(&record.own_dependencies, UPDATES_CHANGE) {
            changed.push(entity);
        }
    }
    for entity in changed {
        for view in window
            .rendered_frame
            .dispatch_tree
            .view_path_reversed(entity)
        {
            if !window.dirty_views.insert(view) {
                break;
            }
        }
    }
}

/// The entity of the view a record's id ends in.
fn view_entity(id: &GlobalElementId) -> Option<EntityId> {
    match id.last()? {
        crate::ElementId::View(entity) => Some(*entity),
        _ => None,
    }
}

/// Ends the retained bookkeeping of the frame being drawn, before it becomes
/// the rendered frame.
pub(crate) fn finish_frame(window: &mut Window) {
    window.fast_retained_state.prebuilt.clear();
    window.next_frame.fast_retained.finish_frame();
}

/// Whether an entity updated without being notified counts as changed for
/// the views that read it: always, in gpux.
///
/// gpui-fast counts such an update only for a view drawn inside a view
/// notified since, as a subscriber updated for an event it does not render
/// is. gpux holds every frame equal to the frame drawn from scratch, which
/// renders every view and so shows the update; and without fast mode a view
/// that is not cached is rendered on every frame, and shows it too.
const UPDATES_CHANGE: bool = true;

/// The record `id` left last frame, if nothing about this frame rules out
/// drawing it again: nothing it read changed, its hovers stand, and no
/// interaction marked it. Where it is drawn is checked by
/// [`context_matches`].
/// Whether something rules out drawing any view from last frame this frame.
pub(crate) fn frame_bypassed(window: &mut Window, cx: &App) -> bool {
    frame_bypass(window, cx).is_some()
}

/// Whether `entity` was notified since the last frame was drawn.
pub(crate) fn notified(window: &Window, entity: EntityId) -> bool {
    window.fast_retained_state.notified.contains(&entity)
}

/// Notes the entities notified since the last frame was drawn.
pub(crate) fn note_notified(window: &mut Window, entities: &FxHashSet<EntityId>) {
    let notified = &mut window.fast_retained_state.notified;
    notified.clear();
    notified.extend(entities.iter().copied());
}

fn frame_bypass(window: &mut Window, cx: &App) -> Option<Bypass> {
    // A drag or the inspector refreshes the window as it starts, and is the
    // reason the frame is drawn so.
    match window.fast_retained_state.frame_bypass {
        Some(bypass) => bypass,
        None => {
            let bypass = if cx.has_active_drag() {
                Some(Bypass::Drag)
            } else if window.is_inspector_picking(cx) {
                Some(Bypass::Inspector)
            } else if window.a11y.is_active() {
                Some(Bypass::Accessibility)
            } else if window.refreshing {
                Some(Bypass::Refreshing)
            } else {
                None
            };
            window.fast_retained_state.frame_bypass = Some(bypass);
            bypass
        }
    }
}

fn reusable(window: &mut Window, id: &GlobalElementId, cx: &App) -> Option<usize> {
    let frame_bypass = frame_bypass(window, cx);
    let bypass = if frame_bypass.is_some() {
        frame_bypass
    } else if window.next_frame.fast_retained.by_id.contains_key(id)
        || window.rendered_frame.fast_retained.duplicates.contains(id)
    {
        Some(Bypass::Duplicate)
    } else {
        None
    };
    if let Some(_reason) = bypass {
        #[cfg(any(
            test,
            feature = "test-support",
            feature = "bench-support",
            feature = "diagnostics"
        ))]
        window.fast_stats.frame.bypass_view(match _reason {
            Bypass::Refreshing => crate::fast::stats::ViewBypassReason::Refreshing,
            Bypass::Drag => crate::fast::stats::ViewBypassReason::Drag,
            Bypass::Inspector => crate::fast::stats::ViewBypassReason::Inspector,
            Bypass::Accessibility => crate::fast::stats::ViewBypassReason::Accessibility,
            Bypass::Duplicate => crate::fast::stats::ViewBypassReason::Duplicate,
        });
        return None;
    }
    let index = window.rendered_frame.fast_retained.find(id)?;
    let record = &window.rendered_frame.fast_retained.records[index];
    if cx.dependencies_changed(&record.dependencies, UPDATES_CHANGE)
        || !crate::fast::dependencies::may_replay_hovers(
            window,
            &record.hover_reads,
            record.hover_area,
        )
    {
        return None;
    }
    Some(index)
}

fn context_here(window: &Window, bounds: Bounds<Pixels>) -> RetainedContext {
    RetainedContext {
        bounds,
        content_mask: window.content_mask(),
        text_style: window.text_style(),
        paint_context: window.element_paint_context(),
    }
}

/// Whether the view last frame's record `previous` stands for would be
/// drawn at `bounds` just as it was.
fn context_matches(window: &Window, previous: usize, bounds: Bounds<Pixels>) -> bool {
    *window.rendered_frame.fast_retained.records[previous].context == context_here(window, bounds)
}

/// Lays out the view of last frame's record `previous` as it was, without
/// building it, if it can be: its content left every node it was laid out
/// with, and it inherits what it did.
fn reuse_layout(
    window: &mut Window,
    previous: usize,
    cx: &mut App,
) -> Option<(LayoutId, CarriedStates)> {
    let record = &window.rendered_frame.fast_retained.records[previous];
    let layout = record.layout.clone()?;
    if layout.rem_size != window.rem_size() || layout.text_style != window.text_style() {
        return None;
    }
    if !crate::fast::layout::keep_retained_nodes(
        window.layout_engine.as_mut().unwrap(),
        &layout.keys,
        true,
    ) {
        return None;
    }
    let carried = carry_element_states(window, &layout.element_states);
    let dependencies = window.rendered_frame.fast_retained.records[previous]
        .dependencies
        .clone();
    cx.replay_dependencies(&dependencies);
    Some((layout.root, carried))
}

/// Carries `element_states` into this frame, as building the view that used
/// them would, for a view laid out without being built.
pub(crate) fn carry_element_states(
    window: &mut Window,
    element_states: &[(GlobalElementId, TypeId)],
) -> CarriedStates {
    let frame = &mut window.next_frame;
    let carried = CarriedStates {
        accessed: frame.accessed_element_states.len(),
        pending: frame.pending_element_state_transfers.len(),
        count: element_states.len(),
    };
    frame
        .accessed_element_states
        .extend(element_states.iter().cloned());
    frame
        .pending_element_state_transfers
        .extend(element_states.iter().cloned());
    carried
}

/// Where [`reuse_layout`] carried a view's element states into this frame.
#[derive(Clone, Copy)]
pub(crate) struct CarriedStates {
    accessed: usize,
    pending: usize,
    count: usize,
}

/// Stands in for an element state a view laid out without being built
/// carried into the frame, once the view is built after all: building it
/// carries the states it still uses, and those it no longer does go, as in
/// a frame drawn from scratch. The entries are overwritten rather than
/// removed, so that the ranges recorded around them stay put.
pub(crate) struct ReleasedElementState;

/// Gives back the layout nodes and element states [`reuse_layout`] kept, for
/// a view that has to be built after all.
pub(crate) fn release_layout(window: &mut Window, previous: usize, carried: CarriedStates) {
    if let Some(layout) = window.rendered_frame.fast_retained.records[previous]
        .layout
        .clone()
    {
        window
            .layout_engine
            .as_mut()
            .unwrap()
            .retention
            .release_kept(&layout.keys);
    }
    let released = TypeId::of::<ReleasedElementState>();
    let frame = &mut window.next_frame;
    for (_, type_id) in
        &mut frame.accessed_element_states[carried.accessed..carried.accessed + carried.count]
    {
        *type_id = released;
    }
    for (_, type_id) in
        &mut frame.pending_element_state_transfers[carried.pending..carried.pending + carried.count]
    {
        *type_id = released;
    }
}

fn begin_layout(window: &mut Window, cx: &mut App) -> RetainedLayoutRecording {
    let engine = window.layout_engine.as_mut().unwrap();
    RetainedLayoutRecording {
        keys: engine.retention.record_claimed_keys(),
        transient: engine.retention.transient_count(),
        element_states: window.next_frame.accessed_element_states.len(),
        dependencies: cx.begin_recording_dependencies(),
        text_style: window.text_style(),
        rem_size: window.rem_size(),
        sticky: (
            window.sticky_candidates.len(),
            window.next_sticky_layout_context_id,
        ),
    }
}

/// Ends `recording` for a layout request that produced `root`, returning
/// what was read during it and, if its layout can be reused, how.
fn finish_layout(
    window: &mut Window,
    recording: RetainedLayoutRecording,
    root: LayoutId,
    cx: &mut App,
) -> (Option<Rc<RetainedLayout>>, RecordedDependencies) {
    let engine = window.layout_engine.as_mut().unwrap();
    let keys = engine
        .retention
        .finish_recording_claimed_keys(recording.keys);
    let transient = engine.retention.transient_count();
    let dependencies = cx.finish_recording_dependencies(recording.dependencies);
    // A node nothing keeps is gone when the frame ends. A sticky element
    // pinned by a scroll container outside the view is placed by that
    // container's prepaint, from the candidates registered while layouts are
    // requested, which a layout reused without being requested leaves out.
    let (candidates, contexts) = recording.sticky;
    let outer = |context: Option<crate::window::StickyLayoutContextId>| {
        context.is_some_and(|context| context < contexts)
    };
    if transient != recording.transient
        || window.sticky_candidates[candidates..]
            .iter()
            .any(|candidate| {
                outer(candidate.horizontal_context) || outer(candidate.vertical_context)
            })
    {
        return (None, dependencies);
    }
    let layout = RetainedLayout {
        root,
        keys,
        element_states: window.next_frame.accessed_element_states[recording.element_states..]
            .to_vec(),
        text_style: recording.text_style,
        rem_size: recording.rem_size,
    };
    (Some(Rc::new(layout)), dependencies)
}

/// Draws the view of last frame's record `previous` again, as far as its
/// prepaint goes, returning its record in this frame for [`reuse_paint`].
/// What it read is read again, unless reusing its layout already did: then
/// `layout_reused_at` is the write generation it was found unchanged at.
fn reuse_prepaint(
    window: &mut Window,
    previous: usize,
    layout_reused_at: Option<u64>,
    cx: &mut App,
) -> usize {
    let (prepaint_range, layout_keys, hover_reads) = {
        let record = &window.rendered_frame.fast_retained.records[previous];
        (
            record.prepaint_range.clone(),
            record.layout_keys.clone(),
            record.hover_reads.clone(),
        )
    };
    crate::fast::layout::keep_retained_nodes(
        window.layout_engine.as_mut().unwrap(),
        &layout_keys,
        false,
    );
    crate::fast::fast_stat!(window.fast_stats.frame, views_reused += 1);
    let start = window.prepaint_index();
    window.reuse_prepaint(prepaint_range.clone());
    let end = window.prepaint_index();
    if layout_reused_at.is_none() {
        let dependencies = window.rendered_frame.fast_retained.records[previous]
            .dependencies
            .clone();
        cx.replay_dependencies(&dependencies);
    }
    crate::fast::dependencies::note_replayed_hovers(window, &hover_reads);

    // The nested records can be shifted into this frame only if the copy is
    // what it was copied from, entry for entry.
    // Element states copied are also queued to be carried over, which a
    // frame building them does not, so that list grows by its own count.
    let mut expected = prepaint_range.end.shifted(&prepaint_range.start, &start);
    expected.pending_element_state_transfers_index = end.pending_element_state_transfers_index;
    let copied_whole = end == expected;
    debug_assert!(
        copied_whole,
        "a reused prepaint range changed length: {prepaint_range:?} copied to {start:?}..{end:?}"
    );
    // What it read was found unchanged up to this write; one after, by a view
    // drawn between, is a change for the next frame.
    let checked_at =
        layout_reused_at.unwrap_or_else(|| cx.entities.fast_access_log.write_generation());
    let source = &window.rendered_frame.fast_retained;
    let target = &mut window.next_frame.fast_retained;
    let anchor = target.records.len();
    let nested = if copied_whole {
        source.records[previous].nested
    } else {
        0
    };
    for index in previous..=previous + nested {
        let record = &source.records[index];
        let prepaint_range = if index == previous {
            start.clone()..end.clone()
        } else {
            record
                .prepaint_range
                .start
                .shifted(&prepaint_range.start, &start)
                ..record
                    .prepaint_range
                    .end
                    .shifted(&prepaint_range.start, &start)
        };
        let mut copy = copy_record(record, prepaint_range, anchor);
        if index == previous {
            copy.nested = nested;
        }
        // Nothing written since the records were built changed what they
        // read, or they would not be reused: up to date as of the decision.
        copy.dependencies = copy.dependencies.written_up_to(checked_at);
        copy.own_dependencies = copy.own_dependencies.written_up_to(checked_at);
        target.push(copy);
    }
    anchor
}

/// A copy of last frame's `record`, prepainted at `prepaint_range` in this
/// frame, with its paint to be placed once `anchor`, the view it was copied
/// along with, is painted: unpainted if it was not painted.
pub(crate) fn copy_record(
    record: &RetainedSubtree,
    prepaint_range: Range<PrepaintStateIndex>,
    anchor: usize,
) -> RetainedSubtree {
    RetainedSubtree {
        id: record.id.clone(),
        prepaint_range,
        paint_range: record.paint_range.clone(),
        paint: match record.paint {
            PaintStatus::Painted { .. } => PaintStatus::Pending { anchor },
            _ => PaintStatus::Unpainted,
        },
        nested: record.nested,
        context: record.context.clone(),
        dependencies: record.dependencies.clone(),
        own_dependencies: record.own_dependencies.clone(),
        hover_reads: record.hover_reads.clone(),
        hover_area: record.hover_area,
        deferred_hitboxes: SmallVec::new(),
        layout_keys: record.layout_keys.clone(),
        layout: record.layout.clone(),
        rebuild: record.rebuild.clone(),
        paint_content_mask: record.paint_content_mask,
    }
}

/// Adds `record` to this frame's records.
pub(crate) fn push_record(window: &mut Window, record: RetainedSubtree) -> usize {
    window.next_frame.fast_retained.push(record)
}

/// Adds `record` to this frame's records as a view whose prepaint is under
/// way: views nested in it are recorded inside it, and what is deferred
/// from it is its own.
pub(crate) fn push_open(window: &mut Window, record: RetainedSubtree) -> usize {
    let retained = &mut window.next_frame.fast_retained;
    let index = retained.push(record);
    retained.open.push(index);
    index
}

/// Ends the prepaint of the record [`push_open`] opened at `index`, which
/// covered `range` and holds `layout_keys` and `dependencies`.
pub(crate) fn close_open(
    window: &mut Window,
    index: usize,
    range: Range<PrepaintStateIndex>,
    layout_keys: Vec<u64>,
    dependencies: RenderDependencies,
) {
    let retained = &mut window.next_frame.fast_retained;
    debug_assert_eq!(retained.open.last(), Some(&index));
    retained.open.pop();
    let nested = retained.records.len() - index - 1;
    let record = &mut retained.records[index];
    record.prepaint_range = range;
    record.nested = nested;
    record.layout_keys = layout_keys.into();
    record.dependencies = dependencies;
}

/// Places the paint of the record `copy`, copied with a view drawn around
/// its nested views, where the stretch it was copied from, `from` last
/// frame, landed: `to`.
pub(crate) fn place_copied_paint(
    window: &mut Window,
    copy: usize,
    from: &PaintIndex,
    to: &PaintIndex,
) {
    let record = &mut window.next_frame.fast_retained.records[copy];
    if let PaintStatus::Pending { .. } = record.paint {
        record.paint_range =
            record.paint_range.start.shifted(from, to)..record.paint_range.end.shifted(from, to);
        record.paint = PaintStatus::Painted { source: None };
    }
}

/// Ends the paint of a view drawn around its nested views, which painted
/// `range` and was hovered by `reads`.
pub(crate) fn finish_spliced_paint(
    window: &mut Window,
    index: usize,
    range: Range<PaintIndex>,
    reads: Vec<(HitboxId, bool)>,
) {
    let paint_content_mask = window.paint_content_mask_stack.last().copied();
    let record = &mut window.next_frame.fast_retained.records[index];
    record.paint_range = range;
    record.paint = PaintStatus::Painted { source: None };
    record.paint_content_mask = paint_content_mask;
    record.hover_reads = reads.into();
    refresh_hover_area(window, index);
}

/// Draws the view whose prepaint [`reuse_prepaint`] drew again as far as its
/// paint goes.
fn reuse_paint(window: &mut Window, index: usize) {
    let record = &window.next_frame.fast_retained.records[index];
    let PaintStatus::Pending { .. } = record.paint else {
        return;
    };
    let source = record.paint_range.clone();
    let hover_reads = record.hover_reads.clone();
    let painted_with = record.context.paint_context;
    let entity = view_entity(&record.id);
    let start = window.paint_index();
    window.reuse_paint(source.clone());
    let end = window.paint_index();
    let copied_whole = end == source.end.shifted(&source.start, &start);
    debug_assert!(copied_whole, "a reused paint range changed length");
    crate::fast::dependencies::note_replayed_hovers(window, &hover_reads);
    // Prepaint drew it again only with the pointer away from the hitboxes
    // whose hovers it read, where nothing this frame adds can hover them.
    debug_assert!(
        crate::fast::dependencies::hovers_unchanged(window, &hover_reads),
        "a reused view's hovers changed after prepaint"
    );
    // An ancestor that applies opacity or a transform only while painting
    // can change after prepaint drew this view again, so this frame shows
    // what it painted before: it is built on the next, as a cached view is.
    if painted_with != window.element_paint_context()
        && let Some(entity) = entity
    {
        window.on_next_frame(move |_, cx| cx.notify(entity));
    }
    let record = &mut window.next_frame.fast_retained.records[index];
    record.paint_range = start..end;
    record.paint = PaintStatus::Painted {
        source: copied_whole.then_some(source.start),
    };
}

/// Starts recording the prepaint of the retained view `id`, which is being
/// built.
fn begin_retained(
    window: &mut Window,
    id: &GlobalElementId,
    bounds: Bounds<Pixels>,
    cx: &mut App,
) -> RetainedRecording {
    let start = window.prepaint_index();
    let context = Rc::new(context_here(window, bounds));
    let retained = &mut window.next_frame.fast_retained;
    if retained.by_id.contains_key(id) {
        retained.duplicates.insert(id.clone());
    }
    let index = (!retained.by_id.contains_key(id)).then(|| {
        let index = retained.push(RetainedSubtree {
            id: id.clone(),
            prepaint_range: start.clone()..start,
            paint_range: PaintIndex::default()..PaintIndex::default(),
            paint: PaintStatus::Unpainted,
            nested: 0,
            context,
            dependencies: RenderDependencies::default(),
            own_dependencies: RenderDependencies::default(),
            hover_reads: Rc::new([]),
            hover_area: HoverArea::Own(None),
            deferred_hitboxes: SmallVec::new(),
            layout_keys: Rc::new([]),
            layout: None,
            rebuild: None,
            paint_content_mask: None,
        });
        retained.open.push(index);
        index
    });
    RetainedRecording {
        index,
        dependencies: cx.begin_recording_dependencies(),
        layout_keys: window
            .layout_engine
            .as_mut()
            .unwrap()
            .retention
            .record_claimed_keys(),
        hovers: crate::fast::dependencies::begin_recording(window),
    }
}

/// Ends `recording` for a view prepainted at `bounds`, returning its record,
/// if it has one, for [`begin_retained_paint`].
fn finish_retained_prepaint(
    window: &mut Window,
    recording: RetainedRecording,
    layout: Option<Rc<RetainedLayout>>,
    layout_dependencies: Option<RecordedDependencies>,
    rebuild: Option<Rc<Rebuild>>,
    cx: &mut App,
) -> Option<usize> {
    let layout_keys = window
        .layout_engine
        .as_mut()
        .unwrap()
        .retention
        .finish_recording_claimed_keys(recording.layout_keys);
    let mut dependencies = cx.finish_recording_dependencies(recording.dependencies);
    let hover_reads = crate::fast::dependencies::finish_recording(window, recording.hovers);
    let index = recording.index?;
    if let Some(layout_dependencies) = layout_dependencies {
        dependencies = RecordedDependencies {
            all: layout_dependencies.all.union(&dependencies.all),
            own: layout_dependencies.own.union(&dependencies.own),
        };
    }
    let end = window.prepaint_index();
    let retained = &mut window.next_frame.fast_retained;
    debug_assert_eq!(retained.open.last(), Some(&index));
    retained.open.pop();
    let nested = retained.records.len() - index - 1;
    let record = &mut retained.records[index];
    record.prepaint_range.end = end;
    record.nested = nested;
    record.dependencies = dependencies.all;
    record.own_dependencies = dependencies.own;
    record.hover_reads = hover_reads.into();
    record.layout_keys = layout_keys.into();
    record.layout = layout;
    record.rebuild = rebuild;
    refresh_hover_area(window, index);
    Some(index)
}

/// Starts recording the paint of the retained view whose prepaint left the
/// record `index`.
fn begin_retained_paint(
    window: &mut Window,
    index: Option<usize>,
    cx: &mut App,
) -> RetainedPaintRecording {
    if let Some(index) = index {
        let paint_content_mask = window.paint_content_mask_stack.last().copied();
        window.next_frame.fast_retained.records[index].paint_content_mask = paint_content_mask;
    }
    RetainedPaintRecording {
        index,
        start: window.paint_index(),
        dependencies: cx.begin_recording_dependencies(),
        hovers: crate::fast::dependencies::begin_recording(window),
    }
}

/// Ends `recording`, keeping what the view painted, the hovers it was
/// painted by and what it read.
fn finish_retained_paint(window: &mut Window, recording: RetainedPaintRecording, cx: &mut App) {
    let dependencies = cx.finish_recording_dependencies(recording.dependencies);
    let painted_hovers = crate::fast::dependencies::finish_recording(window, recording.hovers);
    let Some(index) = recording.index else {
        return;
    };
    let end = window.paint_index();
    let paint_context = window.element_paint_context();
    let record = &mut window.next_frame.fast_retained.records[index];
    record.paint_range = recording.start..end;
    record.paint = PaintStatus::Painted { source: None };
    // An ancestor that applies opacity or a transform only while painting
    // hides it from prepaint: keyed on the painted context, the view is
    // built again under such an ancestor, as a cached view is.
    if record.context.paint_context != paint_context {
        let mut context = RetainedContext::clone(&record.context);
        context.paint_context = paint_context;
        record.context = Rc::new(context);
    }
    record.dependencies = record.dependencies.union(&dependencies.all);
    record.own_dependencies = record.own_dependencies.union(&dependencies.own);
    if !painted_hovers.is_empty() {
        let mut reads = record.hover_reads.to_vec();
        reads.extend(painted_hovers);
        record.hover_reads = reads.into();
        refresh_hover_area(window, index);
    }
}

/// Works out again where the hovers of this frame's record `index` can
/// change, from the hitboxes it and what was deferred from it inserted.
fn refresh_hover_area(window: &mut Window, index: usize) {
    let record = &window.next_frame.fast_retained.records[index];
    let mut hitboxes = SmallVec::<[Range<usize>; 2]>::new();
    hitboxes
        .push(record.prepaint_range.start.hitboxes_index..record.prepaint_range.end.hitboxes_index);
    hitboxes.extend(record.deferred_hitboxes.iter().cloned());
    let reads = record.hover_reads.clone();
    window.next_frame.fast_retained.records[index].hover_area =
        crate::fast::dependencies::hover_area(window, &reads, &hitboxes);
}

/// The retained views being prepainted right now, for something deferred
/// from them to be counted as theirs.
pub(crate) fn enclosing_retained(window: &Window) -> EnclosingRetained {
    if !active(window) {
        return EnclosingRetained::default();
    }
    let mut enclosing = window.fast_retained_state.deferred_enclosing.clone();
    enclosing
        .0
        .extend(window.next_frame.fast_retained.open.iter().copied());
    enclosing
}

/// Something deferred from retained views, being prepainted or painted.
pub(crate) struct DeferredRecording {
    enclosing: EnclosingRetained,
    dependencies: DependencyRecording,
    hovers: usize,
    /// Where the hitboxes it inserted start, while it is prepainted, or the
    /// range they took, while it is painted.
    hitboxes: Result<Range<usize>, usize>,
}

/// Starts drawing something deferred from retained views, as a part of
/// them, though it is drawn after them: what it reads, and the hovers it is
/// drawn by, are theirs. Its paint passes the range of hitboxes its prepaint
/// inserted, `prepainted`.
pub(crate) fn begin_deferred(
    window: &mut Window,
    enclosing: &EnclosingRetained,
    prepainted: Option<Range<PrepaintStateIndex>>,
    cx: &mut App,
) -> Option<DeferredRecording> {
    if enclosing.0.is_empty() || !active(window) {
        return None;
    }
    window.fast_retained_state.deferred_enclosing = enclosing.clone();
    Some(DeferredRecording {
        enclosing: enclosing.clone(),
        dependencies: cx.begin_recording_dependencies(),
        hovers: crate::fast::dependencies::begin_recording(window),
        hitboxes: match prepainted {
            Some(range) => Ok(range.start.hitboxes_index..range.end.hitboxes_index),
            None => Err(window.next_frame.hitboxes.len()),
        },
    })
}

/// Ends `recording`, adding what the deferred draw read, and the hovers it
/// was drawn by, to the retained views it was deferred from.
pub(crate) fn finish_deferred(
    window: &mut Window,
    recording: Option<DeferredRecording>,
    cx: &mut App,
) {
    let Some(recording) = recording else {
        return;
    };
    window.fast_retained_state.deferred_enclosing = EnclosingRetained::default();
    let hovers = crate::fast::dependencies::finish_recording(window, recording.hovers);
    let dependencies = cx.finish_recording_dependencies(recording.dependencies);
    let hitboxes = recording
        .hitboxes
        .unwrap_or_else(|start| start..window.next_frame.hitboxes.len());
    let enclosing = &recording.enclosing.0;
    for &index in enclosing {
        let Some(record) = window.next_frame.fast_retained.records.get_mut(index) else {
            continue;
        };
        record.dependencies = record.dependencies.union(&dependencies.all);
        // Copied along with the views it was deferred from, its hitboxes
        // keep their ids and bounds: their hovers are the views' own.
        if !hitboxes.is_empty() && !record.deferred_hitboxes.contains(&hitboxes) {
            record.deferred_hitboxes.push(hitboxes.clone());
        }
        if !hovers.is_empty() {
            let mut reads = record.hover_reads.to_vec();
            reads.extend_from_slice(&hovers);
            record.hover_reads = reads.into();
            refresh_hover_area(window, index);
        }
    }
    if let Some(&innermost) = enclosing.last()
        && let Some(record) = window.next_frame.fast_retained.records.get_mut(innermost)
    {
        record.own_dependencies = record.own_dependencies.union(&dependencies.own);
    }
}

/// How a retained view was laid out and prepainted, kept on its element from
/// one phase to the next.
#[derive(Default)]
pub(crate) enum ViewRetention {
    #[default]
    None,
    /// Laid out, with the layout key of the element it hangs off.
    Layout(ViewLayout, Option<u64>),
    Prepaint(ViewPrepaint),
}

pub(crate) enum ViewLayout {
    /// Laid out by the style it is cached with; built at prepaint if at all.
    Cached {
        layout: Option<Rc<RetainedLayout>>,
        dependencies: RecordedDependencies,
    },
    /// Built, and laid out by its content.
    Built {
        element: AnyElement,
        layout: Option<Rc<RetainedLayout>>,
        dependencies: RecordedDependencies,
    },
    /// Laid out as it was last frame without being built, from the record it
    /// left then, which it is drawn again from if nothing moved it.
    Retained {
        previous: usize,
        carried: CarriedStates,
        /// The write generation it was found unchanged at.
        checked_at: u64,
    },
    /// Laid out as it was last frame around the nested views that changed,
    /// which were built again. See [`crate::fast::splice`].
    Spliced(Box<Splice>),
    /// Built and laid out already, by a splice of the view around it that
    /// did not happen.
    Prebuilt(Box<Prebuilt>),
}

pub(crate) enum ViewPrepaint {
    /// Built this frame, into its record in this frame.
    Built {
        element: AnyElement,
        record: Option<usize>,
    },
    /// Drawn from last frame, as the record at this index in this frame.
    Reused(usize),
    /// Drawn from last frame around the nested views built again.
    Spliced(Box<SplicedPrepaint>),
    /// Built for the view around it, which could not be drawn around it.
    Prebuilt(Box<Prebuilt>),
}

/// Requests the layout of the view `entity_id` as `ViewElement` does with
/// view retention on, drawing it again from last frame when nothing it
/// depends on changed, or around the nested views that did.
#[allow(clippy::too_many_arguments)]
pub(crate) fn request_view_layout(
    state: &mut ViewRetention,
    entity_id: EntityId,
    cached_style: Option<&StyleRefinement>,
    global_id: Option<&GlobalElementId>,
    _rebuild: &RebuildHandle,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> LayoutId {
    let parent_key = crate::fast::layout_key::parent_layout_key(window);
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        match cached_style {
            Some(style) if !caching_disabled => {
                let mut root_style = Style::default();
                root_style.refine(style);
                let recording = begin_layout(window, cx);
                let layout_id = window.request_layout(root_style, None, cx);
                let (layout, dependencies) = finish_layout(window, recording, layout_id, cx);
                *state = ViewRetention::Layout(
                    ViewLayout::Cached {
                        layout,
                        dependencies,
                    },
                    parent_key,
                );
                layout_id
            }
            _ => {
                let global_id = global_id.expect("a view always has an id");
                if let Some((prebuilt, layout_id)) =
                    crate::fast::splice::take_prebuilt(window, global_id, cx)
                {
                    *state =
                        ViewRetention::Layout(ViewLayout::Prebuilt(Box::new(prebuilt)), parent_key);
                    return layout_id;
                }
                let dirty = window.dirty_views.contains(&entity_id);
                if !dirty
                    && let Some(previous) = reusable(window, global_id, cx)
                    && let Some((layout_id, carried)) = reuse_layout(window, previous, cx)
                {
                    let checked_at = cx.entities.fast_access_log.write_generation();
                    *state = ViewRetention::Layout(
                        ViewLayout::Retained {
                            previous,
                            carried,
                            checked_at,
                        },
                        parent_key,
                    );
                    return layout_id;
                }
                if dirty
                    && let Some(splice) =
                        crate::fast::splice::splice_layout(window, global_id, entity_id, cx)
                {
                    let root = crate::fast::splice::splice_root(window, &splice);
                    *state =
                        ViewRetention::Layout(ViewLayout::Spliced(Box::new(splice)), parent_key);
                    return root;
                }
                crate::fast::fast_stat!(window.fast_stats.frame, views_built += 1);
                let recording = begin_layout(window, cx);
                let mut element = render(window, cx);
                let layout_id = element.request_layout(window, cx);
                let (layout, dependencies) = finish_layout(window, recording, layout_id, cx);
                *state = ViewRetention::Layout(
                    ViewLayout::Built {
                        element,
                        layout,
                        dependencies,
                    },
                    parent_key,
                );
                layout_id
            }
        }
    })
}

/// Prepaints the view as `ViewElement` does with view retention on,
/// following up on how [`request_view_layout`] laid it out, if it did.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepaint_view(
    state: &mut ViewRetention,
    entity_id: EntityId,
    global_id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    cached_style: Option<&StyleRefinement>,
    rebuild: &RebuildHandle,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> bool {
    if !matches!(state, ViewRetention::Layout(..)) {
        return false;
    }
    let ViewRetention::Layout(layout, parent_key) = mem::take(state) else {
        unreachable!()
    };
    if let ViewLayout::Prebuilt(mut prebuilt) = layout {
        crate::fast::splice::prepaint_prebuilt(&mut prebuilt, global_id, bounds, window, cx);
        *state = ViewRetention::Prepaint(ViewPrepaint::Prebuilt(prebuilt));
        return true;
    }
    window.set_view_id(entity_id);
    let prepaint = window.with_rendered_view(entity_id, |window| {
        let global_id = global_id.expect("a view always has an id");
        // How to build it again on its own here, kept when it is built.
        let rebuild_here = |window: &Window| {
            crate::fast::splice::rebuild_here(window, rebuild, cached_style, parent_key)
        };
        match layout {
            ViewLayout::Built {
                mut element,
                layout,
                dependencies,
            } => {
                let rebuild = rebuild_here(window);
                let recording = begin_retained(window, global_id, bounds, cx);
                element.prepaint(window, cx);
                let record = finish_retained_prepaint(
                    window,
                    recording,
                    layout,
                    Some(dependencies),
                    rebuild,
                    cx,
                );
                ViewPrepaint::Built { element, record }
            }
            ViewLayout::Retained {
                previous,
                carried,
                checked_at,
            } => {
                if context_matches(window, previous, bounds) {
                    return ViewPrepaint::Reused(reuse_prepaint(
                        window,
                        previous,
                        Some(checked_at),
                        cx,
                    ));
                }
                let rebuild = rebuild_here(window);
                build_at_retained_layout(
                    previous, carried, rebuild, global_id, bounds, window, cx, render,
                )
            }
            ViewLayout::Spliced(splice) => {
                let previous = splice.previous;
                if context_matches(window, previous, bounds)
                    && crate::fast::splice::splice_holds(window, &splice)
                {
                    return ViewPrepaint::Spliced(Box::new(crate::fast::splice::splice_prepaint(
                        window, *splice, cx,
                    )));
                }
                let carried = splice.carried;
                crate::fast::splice::abandon_splice(window, *splice);
                let rebuild = rebuild_here(window);
                build_at_retained_layout(
                    previous, carried, rebuild, global_id, bounds, window, cx, render,
                )
            }
            ViewLayout::Cached {
                layout,
                dependencies,
            } => {
                if !window.dirty_views.contains(&entity_id)
                    && let Some(previous) = reusable(window, global_id, cx)
                    && context_matches(window, previous, bounds)
                {
                    let index = reuse_prepaint(window, previous, None, cx);
                    // The cached node was requested again this frame, at a
                    // path that may have changed: keep that and where it
                    // can be built from, not the copied ones.
                    let rebuild = rebuild_here(window);
                    let record = &mut window.next_frame.fast_retained.records[index];
                    record.layout = layout;
                    record.rebuild = rebuild;
                    return ViewPrepaint::Reused(index);
                }
                crate::fast::fast_stat!(window.fast_stats.frame, views_built += 1);
                let rebuild = rebuild_here(window);
                let recording = begin_retained(window, global_id, bounds, cx);
                let mut element = render(window, cx);
                element.layout_as_root(bounds.size.into(), window, cx);
                element.prepaint_at(bounds.origin, window, cx);
                let record = finish_retained_prepaint(
                    window,
                    recording,
                    layout,
                    Some(dependencies),
                    rebuild,
                    cx,
                );
                ViewPrepaint::Built { element, record }
            }
            ViewLayout::Prebuilt(_) => unreachable!("handled above"),
        }
    });
    *state = ViewRetention::Prepaint(prepaint);
    true
}

/// Builds a view whose layout was reused, but which cannot be drawn again
/// from last frame because it is drawn somewhere else: it moved, or what it
/// inherits changed. It is laid out at the nodes it kept, which it finds
/// again as it requests them. If it asks for another layout after all, it is
/// laid out within the bounds it was given, and on the next frame from
/// scratch.
#[allow(clippy::too_many_arguments)]
fn build_at_retained_layout(
    previous: usize,
    carried: CarriedStates,
    rebuild: Option<Rc<Rebuild>>,
    global_id: &GlobalElementId,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> ViewPrepaint {
    let root = window.rendered_frame.fast_retained.records[previous]
        .layout
        .as_ref()
        .map(|layout| layout.root);
    release_layout(window, previous, carried);
    crate::fast::fast_stat!(window.fast_stats.frame, views_built += 1);
    crate::fast::fast_stat!(window.fast_stats.frame, views_built_at_retained_layout += 1);
    let recording = begin_layout(window, cx);
    let (changes_before, remeasures_before) = {
        let retention = &window.layout_engine.as_ref().unwrap().retention;
        (retention.layout_changes, retention.remeasures)
    };
    let (mut element, layout_id) =
        crate::fast::layout_key::with_layout_key_of_prepainting_element(window, |window| {
            let mut element = render(window, cx);
            let layout_id = element.request_layout(window, cx);
            (element, layout_id)
        });
    let (unchanged, remeasured) = {
        let retention = &window.layout_engine.as_ref().unwrap().retention;
        (
            retention.layout_changes == changes_before,
            retention.remeasures != remeasures_before,
        )
    };
    let (layout, dependencies) = finish_layout(window, recording, layout_id, cx);
    let recording = begin_retained(window, global_id, bounds, cx);
    if Some(layout_id) == root {
        // Measurements taken again are laid out again within the view, held
        // at its size; only a layout that changed can change that size, which
        // the next frame lays out from scratch.
        if !unchanged || remeasured {
            let mut engine = window.layout_engine.take().unwrap();
            crate::fast::layout::relayout_in_place(
                &mut engine,
                layout_id,
                bounds.size.into(),
                window,
                cx,
            );
            window.layout_engine = Some(engine);
        }
        if !unchanged {
            window.request_animation_frame();
        }
        element.prepaint(window, cx);
    } else {
        element.layout_as_root(bounds.size.into(), window, cx);
        element.prepaint_at(bounds.origin, window, cx);
        window.request_animation_frame();
    }
    let record =
        finish_retained_prepaint(window, recording, layout, Some(dependencies), rebuild, cx);
    ViewPrepaint::Built { element, record }
}

/// Paints the view as `ViewElement` does with view retention on, if it was
/// prepainted so.
pub(crate) fn paint_view(
    state: &mut ViewRetention,
    entity_id: EntityId,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let ViewRetention::Prepaint(prepaint) = state else {
        return false;
    };
    if let ViewPrepaint::Prebuilt(prebuilt) = prepaint {
        crate::fast::splice::paint_prebuilt(prebuilt, entity_id, window, cx);
        return true;
    }
    window.with_rendered_view(entity_id, |window| match prepaint {
        ViewPrepaint::Reused(index) => reuse_paint(window, *index),
        ViewPrepaint::Spliced(spliced) => crate::fast::splice::splice_paint(window, spliced, cx),
        ViewPrepaint::Built {
            element,
            record: None,
        } => element.paint(window, cx),
        ViewPrepaint::Built {
            element,
            record: Some(record),
        } => {
            let recording = begin_retained_paint(window, Some(*record), cx);
            element.paint(window, cx);
            finish_retained_paint(window, recording, cx);
        }
        ViewPrepaint::Prebuilt(_) => unreachable!("handled above"),
    });
    true
}
