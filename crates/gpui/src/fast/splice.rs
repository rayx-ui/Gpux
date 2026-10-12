//! Drawing a view again from last frame around the views nested in it that
//! have to be built again.
//!
//! Adapted from gpui-fast's `fast/splice.rs`. Notifying a view marks every
//! view around it dirty, because they have to be walked to reach it. With
//! view retention alone they are all built again. With splice, a retained
//! view that is dirty only because something nested in it is (it was not
//! notified, and nothing it read itself changed) is drawn again from last
//! frame, stretch by stretch, with the nested views that changed built again
//! in the gaps where they were drawn:
//!
//! - its layout is last frame's, and each nested view that changed is laid
//!   out again at its own nodes. If one of them is laid out at another node,
//!   the view around it is built after all, and the nested views already
//!   built are taken over by their elements there ([`take_prebuilt`]);
//! - its prepaint and paint are copied from last frame up to where each
//!   nested view began, the nested view is prepainted and painted where it
//!   was, with what it inherited there, and the copy goes on after it.
//!
//! gpux copies more than gpui-fast between gaps: its DevKit nodes, whose ids
//! are allocated before an element's children and recorded after them, and
//! the dispatch nodes they point at, are mapped across the stretches of one
//! view, since a node allocated before a gap is recorded after it. A nested
//! view inherits gpux's every stack where it was drawn: element ids, text
//! style, content masks for prepaint and paint, element offset, rem size,
//! opacity, transform, scroll and sticky contexts, hitbox behavior and
//! positioned depth.

use std::{any::TypeId, mem, ops::Range, rc::Rc};

use collections::FxHashSet;
use smallvec::SmallVec;

use crate::{
    AnyView, App, Bounds, ContentMask, ElementId, ElementPaintContext, EntityId, GlobalElementId,
    HitboxBehavior, IntoElement as _, LayoutId, Pixels, Point, ScaledPixels, StyleRefinement,
    TextStyleRefinement, TransformationMatrix, View, ViewElement, Window,
    fast::{
        dependencies::RenderDependencies,
        layout_key::LayoutKey,
        retained::{CarriedStates, PaintStatus, RetainedLayout, RetainedSubtree},
    },
    key_dispatch::{DispatchNodeId, DispatchTree},
    window::{PaintIndex, PrepaintStateIndex, ScrollContainerContext, StickyLayoutContext},
};

/// The view a [`ViewElement`] renders, kept so that it can be built again on
/// its own. Empty for a view that is not an entity or an [`AnyView`].
#[derive(Default)]
pub(crate) struct RebuildHandle(Option<AnyView>);

/// How a [`ViewElement`] keeps its view to be built again on its own: how to
/// make an [`AnyView`] of it, taken only in a frame that splices, and the
/// view once taken. Elements cost nothing more otherwise.
pub(crate) struct Rebuildable<V> {
    make: Option<fn(&V) -> AnyView>,
    pub(crate) handle: RebuildHandle,
}

impl<V> Default for Rebuildable<V> {
    fn default() -> Self {
        Self {
            make: None,
            handle: RebuildHandle::default(),
        }
    }
}

fn entity_view<T: crate::Render>(entity: &crate::Entity<T>) -> AnyView {
    AnyView::from(entity.clone())
}

fn any_view(view: &AnyView) -> AnyView {
    view.clone()
}

/// An entity's element, which can be built again on its own.
#[inline]
pub(crate) fn rebuildable_entity<T: crate::Render>(
    entity: crate::Entity<T>,
) -> ViewElement<crate::Entity<T>> {
    let mut element = ViewElement::new(entity);
    element.fast_rebuild.make = Some(entity_view::<T>);
    element
}

/// An [`AnyView`]'s element, which can be built again on its own.
#[inline]
pub(crate) fn rebuildable_any(view: AnyView) -> ViewElement<AnyView> {
    let mut element = ViewElement::new(view);
    element.fast_rebuild.make = Some(any_view);
    element
}

/// Takes what `rebuildable` needs to build its view again on its own, from
/// `view`, before it renders: in a frame that splices only.
#[inline]
pub(crate) fn capture<V>(rebuildable: &mut Rebuildable<V>, view: Option<&V>, window: &Window) {
    if let Some(make) = rebuildable.make
        && let Some(view) = view
        && window.fast_mode.splice()
        && crate::fast::retained::active(window)
    {
        rebuildable.handle = RebuildHandle(Some(make(view)));
    }
}

/// A nested view's element built on its own, kept as `any`.
fn rebuildable(any: AnyView) -> ViewElement<AnyView> {
    let mut element = ViewElement::new(any.clone());
    element.fast_rebuild.handle = RebuildHandle(Some(any));
    element
}

/// What it takes to build a view again on its own where it was drawn: the
/// view, the layout key its element was requested under, and what it
/// inherited there.
pub(crate) struct Rebuild {
    view: AnyView,
    parent_layout_key: u64,
    cached_style: Option<StyleRefinement>,
    text_style_stack: Vec<TextStyleRefinement>,
    element_offset: Point<Pixels>,
    rem_size: Pixels,
    element_opacity: f32,
    transform: TransformationMatrix,
    /// The opacity and transform prepaint saw, which a view painted under
    /// an ancestor that applies others only while painting was not painted
    /// with (see [`splice_layout`]).
    prepaint_paint_context: ElementPaintContext,
    scroll_contexts: Vec<ScrollContainerContext>,
    sticky_contexts: Vec<StickyLayoutContext>,
    hitbox_behavior: Option<HitboxBehavior>,
    pointer_semantics_suppressed: usize,
    positioned_hitbox_depth: usize,
}

/// How the view `handle` holds can be built again on its own from where the
/// window is, if it can: its element was requested under
/// `parent_layout_key`, and nothing it inherits is out of reach (an image
/// cache is).
pub(crate) fn rebuild_here(
    window: &Window,
    handle: &RebuildHandle,
    cached_style: Option<&StyleRefinement>,
    parent_layout_key: Option<u64>,
) -> Option<Rc<Rebuild>> {
    if !window.image_cache_stack.is_empty() {
        return None;
    }
    Some(Rc::new(Rebuild {
        view: handle.0.clone()?,
        parent_layout_key: parent_layout_key?,
        cached_style: cached_style.cloned(),
        text_style_stack: window.text_style_stack.clone(),
        element_offset: window.element_offset(),
        rem_size: window.rem_size(),
        element_opacity: window.element_opacity,
        transform: window.current_element_transform(),
        prepaint_paint_context: ElementPaintContext {
            opacity: window.element_opacity,
            transformation: window.current_element_transform(),
        },
        scroll_contexts: window.scroll_container_context_stack.stack.clone(),
        sticky_contexts: window.sticky_layout_context_stack.clone(),
        hitbox_behavior: window.hitbox_behavior_overrides.last().copied(),
        pointer_semantics_suppressed: window.pointer_semantics_suppressed,
        positioned_hitbox_depth: window.positioned_hitbox_depth,
    }))
}

/// What the window inherits at some point of the element tree, set aside
/// while a nested view is drawn where it was.
struct Inherited {
    element_id_stack: crate::window::ElementIdStack,
    text_style_stack: Vec<TextStyleRefinement>,
    content_mask_stack: Vec<ContentMask<Pixels>>,
    paint_content_mask_stack: Vec<ContentMask<ScaledPixels>>,
    element_offset_stack: Vec<Point<Pixels>>,
    rem_size_override_stack: SmallVec<[Pixels; 8]>,
    element_opacity: f32,
    element_transform_stack: Vec<TransformationMatrix>,
    scroll_contexts: Vec<ScrollContainerContext>,
    sticky_contexts: Vec<StickyLayoutContext>,
    hitbox_behavior_overrides: Vec<HitboxBehavior>,
    pointer_semantics_suppressed: usize,
    positioned_hitbox_depth: usize,
}

/// Where a nested view was drawn: the content masks it was prepainted and
/// painted under, as its record keeps them.
struct Masks(ContentMask<Pixels>, Option<ContentMask<ScaledPixels>>);

impl Masks {
    fn of(record: &RetainedSubtree) -> Self {
        Masks(record.context.content_mask(), record.paint_content_mask)
    }
}

/// Puts the window where the view `id` was drawn last frame, as `rebuild`
/// and `masks` captured it, returning what [`leave_gap`] puts back.
fn enter_gap(
    window: &mut Window,
    id: &GlobalElementId,
    rebuild: &Rebuild,
    masks: Masks,
) -> Inherited {
    let mut element_id_stack = crate::window::ElementIdStack::default();
    for id in &id[..id.len() - 1] {
        element_id_stack.push(id.clone());
    }
    let transform = if rebuild.transform == TransformationMatrix::unit() {
        Vec::new()
    } else {
        vec![rebuild.transform]
    };
    Inherited {
        element_id_stack: mem::replace(&mut window.element_id_stack, element_id_stack),
        text_style_stack: mem::replace(
            &mut window.text_style_stack,
            rebuild.text_style_stack.clone(),
        ),
        content_mask_stack: mem::replace(&mut window.content_mask_stack, vec![masks.0]),
        paint_content_mask_stack: mem::replace(
            &mut window.paint_content_mask_stack,
            masks.1.into_iter().collect(),
        ),
        element_offset_stack: mem::replace(
            &mut window.element_offset_stack,
            vec![rebuild.element_offset],
        ),
        rem_size_override_stack: mem::replace(
            &mut window.rem_size_override_stack,
            SmallVec::from_slice(&[rebuild.rem_size]),
        ),
        element_opacity: mem::replace(&mut window.element_opacity, rebuild.element_opacity),
        element_transform_stack: mem::replace(&mut window.element_transform_stack, transform),
        scroll_contexts: mem::replace(
            &mut window.scroll_container_context_stack.stack,
            rebuild.scroll_contexts.clone(),
        ),
        sticky_contexts: mem::replace(
            &mut window.sticky_layout_context_stack,
            rebuild.sticky_contexts.clone(),
        ),
        hitbox_behavior_overrides: mem::replace(
            &mut window.hitbox_behavior_overrides,
            rebuild.hitbox_behavior.into_iter().collect(),
        ),
        pointer_semantics_suppressed: mem::replace(
            &mut window.pointer_semantics_suppressed,
            rebuild.pointer_semantics_suppressed,
        ),
        positioned_hitbox_depth: mem::replace(
            &mut window.positioned_hitbox_depth,
            rebuild.positioned_hitbox_depth,
        ),
    }
}

fn leave_gap(window: &mut Window, inherited: Inherited) {
    window.element_id_stack = inherited.element_id_stack;
    window.text_style_stack = inherited.text_style_stack;
    window.content_mask_stack = inherited.content_mask_stack;
    window.paint_content_mask_stack = inherited.paint_content_mask_stack;
    window.element_offset_stack = inherited.element_offset_stack;
    window.rem_size_override_stack = inherited.rem_size_override_stack;
    window.element_opacity = inherited.element_opacity;
    window.element_transform_stack = inherited.element_transform_stack;
    window.scroll_container_context_stack.stack = inherited.scroll_contexts;
    window.sticky_layout_context_stack = inherited.sticky_contexts;
    window.hitbox_behavior_overrides = inherited.hitbox_behavior_overrides;
    window.pointer_semantics_suppressed = inherited.pointer_semantics_suppressed;
    window.positioned_hitbox_depth = inherited.positioned_hitbox_depth;
}

/// A nested view built again inside a view drawn from last frame, drawn the
/// way its element would be inside the view around it.
pub(crate) struct Gap {
    /// Its record in the last frame.
    record: usize,
    global_id: GlobalElementId,
    entity: EntityId,
    view: ViewElement<AnyView>,
    layout_id: LayoutId,
    layout_key: LayoutKey,
    /// The layout nodes it claimed.
    claimed: Vec<u64>,
    /// The element states it used and what it read while its layout was
    /// requested, which the view around it takes over if it is built after
    /// all.
    element_states: Vec<(GlobalElementId, TypeId)>,
    dependencies: RenderDependencies,
    /// Its dispatch node and DevKit node in this frame, once prepainted.
    prepainted: Option<(DispatchNodeId, usize)>,
}

/// A nested view built for a splice that did not happen, with what the view
/// around it takes over from it.
pub(crate) struct PrebuiltGap {
    view: ViewElement<AnyView>,
    layout_id: LayoutId,
    claimed: Vec<u64>,
    element_states: Vec<(GlobalElementId, TypeId)>,
    dependencies: RenderDependencies,
}

/// A nested view built for a view that could not be drawn around it after
/// all, taken over by its element when the view around it is built.
pub(crate) struct Prebuilt {
    pub(crate) view: ViewElement<AnyView>,
}

/// How a gap built again asked to be laid out, against last frame.
enum GapLayout {
    /// As it was.
    Unchanged,
    /// At the node it had, with something inside it laid out differently.
    Changed,
    /// At another node.
    NewRoot,
}

/// A view drawn from last frame around the nested views built again in it.
pub(crate) struct Splice {
    pub(crate) previous: usize,
    gaps: Vec<Gap>,
    /// This frame's layout claims, with each rebuilt gap replacing its old
    /// keys and element states. Absent if a gap introduced transient nodes.
    layout: Option<Rc<RetainedLayout>>,
    /// When a gap was laid out differently from last frame, the layouts the
    /// view's own nodes had then, which have to come out of this frame's
    /// layout the same for it to be drawn around the gap.
    held: Vec<(LayoutId, taffy::Layout)>,
    /// Where its kept element states were carried into the frame.
    pub(crate) carried: CarriedStates,
    /// The write generation its reads were found unchanged at.
    checked_at: u64,
}

/// What [`splice_prepaint`] leaves for [`splice_paint`].
pub(crate) struct SplicedPrepaint {
    previous: usize,
    /// Its record in this frame.
    index: usize,
    gaps: Vec<Gap>,
    /// The records copied along with it, in this frame, each with the
    /// stretch of last frame's range, between gaps, it was copied from.
    copied: Vec<(usize, usize)>,
}

/// The entity of the view a record's id ends in.
fn view_entity(id: &GlobalElementId) -> Option<EntityId> {
    match id.last()? {
        ElementId::View(entity) => Some(*entity),
        _ => None,
    }
}

/// Lays out the view `id`, dirty only because views nested in it are, as it
/// was laid out last frame, with those views built again at their own nodes,
/// if it can be: it was not notified, nothing it or the nested views drawn
/// around the gaps read changed, their hovers stand, and the nested views
/// built again are laid out at the nodes they had.
pub(crate) fn splice_layout(
    window: &mut Window,
    id: &GlobalElementId,
    entity: EntityId,
    cx: &mut App,
) -> Option<Splice> {
    if !window.fast_mode.splice()
        || crate::fast::retained::frame_bypassed(window, cx)
        || window.next_frame.fast_retained.contains(id)
        || crate::fast::retained::notified(window, entity)
        || crate::fast::layers::paint::inside_layer(window)
    {
        return None;
    }
    let previous = window.rendered_frame.fast_retained.find(id)?;
    let records = &window.rendered_frame.fast_retained.records;
    let record = &records[previous];
    let layout = record.layout.as_ref()?;
    if layout.rem_size != window.rem_size()
        || layout.text_style != window.text_style()
        || cx.dependencies_changed(&record.own_dependencies, true)
    {
        return None;
    }

    // The outermost dirty views nested in it are the gaps. Every view around
    // a dirty one is dirty too, so a clean one holds none; what is drawn
    // around the gaps must read and be hovered as it was.
    let mut gaps = Vec::new();
    let mut gap_reads = FxHashSet::default();
    let mut index = previous + 1;
    while index <= previous + record.nested {
        let nested = &records[index];
        let dirty =
            view_entity(&nested.id).is_some_and(|entity| window.dirty_views.contains(&entity));
        if dirty {
            let Some(nested_layout) = nested.layout.as_ref() else {
                return None;
            };
            if nested.rebuild.is_none()
                || !matches!(nested.paint, PaintStatus::Painted { .. })
                // An ancestor that applies opacity or a transform only while
                // painting is not copied around the gap, where the nested
                // view would be painted without it.
                || nested.rebuild.as_ref().is_some_and(|rebuild| {
                    rebuild.prepaint_paint_context != nested.context.paint_context
                })
                || window.rendered_frame.fast_retained.is_duplicated(&nested.id)
                // A splice skips the view's prepaint. A nested view laid
                // out apart from it (a list row) would be laid out where
                // nothing computes it.
                || !crate::fast::layout::layout_reaches(
                    window.layout_engine.as_ref().unwrap(),
                    layout.root(),
                    nested_layout.root(),
                )
            {
                return None;
            }
            gap_reads.extend(nested.hover_reads.iter().map(|(hitbox, _)| *hitbox));
            gaps.push(index);
        } else if cx.dependencies_changed(&nested.dependencies, true) {
            return None;
        }
        index += nested.nested + 1;
    }
    if gaps.is_empty() {
        return None;
    }

    // Deferred draws are prepainted after the views they are deferred from,
    // so a deferred draw outside the gaps would be copied however it reads;
    // one inside a gap is built with it.
    let mut deferred_cursor = record.prepaint_range.start.deferred_draws_index;
    for &gap in &gaps {
        let range = &records[gap].prepaint_range;
        if deferred_cursor != range.start.deferred_draws_index {
            return None;
        }
        deferred_cursor = range.end.deferred_draws_index;
    }
    if deferred_cursor != record.prepaint_range.end.deferred_draws_index {
        return None;
    }

    // The hovers of what is drawn around the gaps stand.
    let copied_reads: Vec<_> = record
        .hover_reads
        .iter()
        .filter(|(hitbox, _)| !gap_reads.contains(hitbox))
        .copied()
        .collect();
    let hitboxes = &window.rendered_frame.hitboxes
        [record.prepaint_range.start.hitboxes_index..record.prepaint_range.end.hitboxes_index];
    let area = crate::fast::dependencies::hover_area_of(hitboxes.iter(), &copied_reads);
    if !crate::fast::dependencies::may_replay_hovers(window, &copied_reads, area) {
        return None;
    }

    let mut kept = without(
        &layout.keys,
        gaps.iter()
            .flat_map(|&gap| records[gap].layout.as_ref().unwrap().keys.iter()),
    );
    let mut element_states = kept_element_states(
        &layout.element_states,
        gaps.iter()
            .flat_map(|&gap| records[gap].layout.as_ref().unwrap().element_states.iter()),
    );
    let dependencies = record.dependencies.clone();
    let previous_layout = layout.clone();
    let engine = window.layout_engine.as_mut().unwrap();
    let transient_before = engine.retention.transient_count();
    if !crate::fast::layout::keep_retained_nodes(engine, &kept, true) {
        return None;
    }

    // A gap laid out differently from last frame, at the node it had,
    // changes only what is inside it unless the view's own nodes come out of
    // layout differently too, which is checked once layout is computed. A gap
    // at another node changes the view's own tree.
    let checked_at = cx.entities.fast_access_log.write_generation();
    let mut built = Vec::with_capacity(gaps.len());
    let mut unchanged = true;
    for gap in gaps {
        let Some((gap, gap_layout)) = lay_out_gap(window, gap, cx) else {
            window
                .layout_engine
                .as_mut()
                .unwrap()
                .retention
                .release_kept(&kept);
            stash_gaps(window, built);
            return None;
        };
        built.push(gap);
        match gap_layout {
            GapLayout::Unchanged => {}
            GapLayout::Changed => unchanged = false,
            GapLayout::NewRoot => {
                // The view around them is built after all; the views built
                // so far are taken over by their elements there.
                window
                    .layout_engine
                    .as_mut()
                    .unwrap()
                    .retention
                    .release_kept(&kept);
                stash_gaps(window, built);
                return None;
            }
        }
    }

    let carried = crate::fast::retained::carry_element_states(window, &element_states);
    cx.replay_dependencies(&dependencies);
    let held = if unchanged {
        Vec::new()
    } else {
        crate::fast::layout::retained_layouts(window.layout_engine.as_ref().unwrap(), &kept)
    };
    for gap in &built {
        kept.extend_from_slice(&gap.claimed);
        element_states.extend_from_slice(&gap.element_states);
    }
    let transient_now = window
        .layout_engine
        .as_ref()
        .unwrap()
        .retention
        .transient_count();
    let layout = (transient_now == transient_before).then(|| {
        Rc::new(RetainedLayout {
            keys: kept,
            element_states,
            ..RetainedLayout::clone(&previous_layout)
        })
    });
    Some(Splice {
        previous,
        gaps: built,
        layout,
        held,
        carried,
        checked_at,
    })
}

/// The node the view of `splice` is laid out at.
pub(crate) fn splice_root(window: &Window, splice: &Splice) -> LayoutId {
    window.rendered_frame.fast_retained.records[splice.previous]
        .layout
        .as_ref()
        .expect("a spliced view has a retained layout")
        .root()
}

/// Builds the view last frame's record `record` stands for again, where it
/// was, and lays it out, returning it and how its layout compares with the
/// one it had.
fn lay_out_gap(window: &mut Window, record: usize, cx: &mut App) -> Option<(Gap, GapLayout)> {
    let nested = &window.rendered_frame.fast_retained.records[record];
    let rebuild = nested.rebuild.clone()?;
    let root = nested.layout.as_ref()?.root();
    let global_id = nested.id.clone();
    let element_id = global_id.last().cloned()?;
    let entity = view_entity(&global_id)?;

    let engine = window.layout_engine.as_mut().unwrap();
    let (changes, remeasures, transient) = (
        engine.retention.layout_changes,
        engine.retention.remeasures,
        engine.retention.transient_count(),
    );
    let keys = engine.retention.record_claimed_keys();
    let element_states_start = window.next_frame.accessed_element_states.len();
    let recording = cx.begin_recording_dependencies();
    let masks = Masks::of(&window.rendered_frame.fast_retained.records[record]);
    let inherited = enter_gap(window, &global_id, &rebuild, masks);
    let mut view = rebuildable(rebuild.view.clone());
    view.cached_style = rebuild.cached_style.clone();
    // What its element's request for layout does, inside the view around it.
    let (layout_id, layout_key) = crate::fast::layout_key::with_parent_layout_key(
        window,
        rebuild.parent_layout_key,
        |window| {
            let layout_key = crate::fast::layout_key::push_layout_key(window, Some(&element_id));
            window.element_id_stack.push(element_id.clone());
            let layout_id = request_layout(&mut view, entity, &global_id, window, cx);
            window.element_id_stack.pop();
            crate::fast::layout_key::pop_layout_key(window);
            (layout_id, layout_key)
        },
    );
    leave_gap(window, inherited);
    let dependencies = cx.finish_recording_dependencies(recording).all;
    let element_states = window.next_frame.accessed_element_states[element_states_start..].to_vec();
    let engine = window.layout_engine.as_mut().unwrap();
    let claimed = engine.retention.finish_recording_claimed_keys(keys);

    let gap_layout = if layout_id != root {
        GapLayout::NewRoot
    } else if engine.retention.layout_changes == changes
        && engine.retention.remeasures == remeasures
        && engine.retention.transient_count() == transient
    {
        GapLayout::Unchanged
    } else {
        GapLayout::Changed
    };
    Some((
        Gap {
            record,
            global_id,
            entity,
            view,
            layout_id,
            layout_key,
            claimed,
            element_states,
            dependencies,
            prepainted: None,
        },
        gap_layout,
    ))
}

/// Requests the layout of a nested view built on its own, as its element's
/// `request_layout` does.
fn request_layout(
    view: &mut ViewElement<AnyView>,
    entity: EntityId,
    global_id: &GlobalElementId,
    window: &mut Window,
    cx: &mut App,
) -> LayoutId {
    let ViewElement {
        view: any,
        cached_style,
        fast_retention,
        fast_rebuild,
        ..
    } = view;
    let fast_rebuild = &fast_rebuild.handle;
    crate::fast::retained::request_view_layout(
        fast_retention,
        entity,
        cached_style.as_ref(),
        Some(global_id),
        fast_rebuild,
        window,
        cx,
        &mut |window, cx| any.take().unwrap().render(window, cx).into_any_element(),
    )
}

/// Keeps views built for a splice that did not happen, for their elements to
/// take over. See [`take_prebuilt`].
fn stash_gaps(window: &mut Window, gaps: Vec<Gap>) {
    for gap in gaps {
        window.fast_retained_state.prebuilt.insert(
            gap.global_id,
            PrebuiltGap {
                view: gap.view,
                layout_id: gap.layout_id,
                claimed: gap.claimed,
                element_states: gap.element_states,
                dependencies: gap.dependencies,
            },
        );
    }
}

/// Hands the views [`splice_layout`] built over to their elements, for a
/// view that has to be built after all.
pub(crate) fn abandon_splice(window: &mut Window, splice: Splice) {
    stash_gaps(window, splice.gaps);
}

/// The view built for the element `id` by a splice that did not happen, if
/// there is one, with the layout it requested. The view being built around
/// it takes over its nodes, the element states it used and what it read, as
/// though it had built it itself.
pub(crate) fn take_prebuilt(
    window: &mut Window,
    id: &GlobalElementId,
    cx: &mut App,
) -> Option<(Prebuilt, LayoutId)> {
    let gap = window.fast_retained_state.prebuilt.remove(id)?;
    window
        .layout_engine
        .as_mut()
        .unwrap()
        .retention
        .adopt_claimed(&gap.claimed);
    window
        .next_frame
        .accessed_element_states
        .extend(gap.element_states);
    cx.replay_dependencies(&gap.dependencies);
    crate::fast::fast_stat!(window.fast_stats.frame, views_prebuilt += 1);
    Some((Prebuilt { view: gap.view }, gap.layout_id))
}

/// Prepaints a nested view [`take_prebuilt`] handed over.
pub(crate) fn prepaint_prebuilt(
    prebuilt: &mut Prebuilt,
    global_id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let entity = prebuilt.view.view.as_ref().map(|view| view.entity_id());
    let entity = entity
        .or_else(|| global_id.and_then(view_entity))
        .expect("a prebuilt view has an entity");
    prepaint_nested(&mut prebuilt.view, entity, global_id, bounds, window, cx);
}

/// Paints a nested view [`take_prebuilt`] handed over.
pub(crate) fn paint_prebuilt(
    prebuilt: &mut Prebuilt,
    entity: EntityId,
    window: &mut Window,
    cx: &mut App,
) {
    crate::fast::retained::paint_view(&mut prebuilt.view.fast_retention, entity, window, cx);
}

/// Prepaints a nested view built on its own, as its element's `prepaint`
/// does.
fn prepaint_nested(
    view: &mut ViewElement<AnyView>,
    entity: EntityId,
    global_id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let ViewElement {
        view: any,
        cached_style,
        fast_retention,
        fast_rebuild,
        ..
    } = view;
    let fast_rebuild = &fast_rebuild.handle;
    let prepainted = crate::fast::retained::prepaint_view(
        fast_retention,
        entity,
        global_id,
        bounds,
        cached_style.as_ref(),
        fast_rebuild,
        window,
        cx,
        &mut |window, cx| any.take().unwrap().render(window, cx).into_any_element(),
    );
    debug_assert!(prepainted, "a nested view is laid out with view retention");
}

/// Whether the view `splice` draws around its gaps came out of this frame's
/// layout as it did last frame, and its gaps where they were, so that what
/// it drew around them, and the DevKit nodes of their elements, still stand.
pub(crate) fn splice_holds(window: &mut Window, splice: &Splice) -> bool {
    if !crate::fast::layout::layouts_unchanged(window.layout_engine.as_ref().unwrap(), &splice.held)
    {
        return false;
    }
    splice.gaps.iter().all(|gap| {
        let bounds = window.layout_bounds(gap.layout_id);
        window.rendered_frame.fast_retained.records[gap.record]
            .context
            .bounds()
            == bounds
    })
}

/// Prepaints the view `id` that [`splice_layout`] laid out: last frame's
/// prepaint, copied around the gaps, and the gaps prepainted where they
/// were.
pub(crate) fn splice_prepaint(
    window: &mut Window,
    splice: Splice,
    cx: &mut App,
) -> SplicedPrepaint {
    let Splice {
        previous,
        mut gaps,
        layout,
        checked_at,
        ..
    } = splice;
    let source = &window.rendered_frame.fast_retained;
    let record = &source.records[previous];
    let prepaint_range = record.prepaint_range.clone();
    let last = previous + record.nested;
    let kept_layout_keys = without(
        &record.layout_keys,
        gaps.iter()
            .flat_map(|gap| source.records[gap.record].layout_keys.iter()),
    );
    let mut own = crate::fast::retained::copy_record(record, prepaint_range.clone(), 0);
    own.layout = layout;
    own.paint = PaintStatus::Unpainted;
    own.dependencies = own.dependencies.written_up_to(checked_at);
    own.own_dependencies = own.own_dependencies.written_up_to(checked_at);
    crate::fast::layout::keep_retained_nodes(
        window.layout_engine.as_mut().unwrap(),
        &kept_layout_keys,
        false,
    );
    crate::fast::fast_stat!(window.fast_stats.frame, views_spliced += 1);

    let index = crate::fast::retained::push_open(window, own);

    let start = window.prepaint_index();
    let mut copy = StretchCopy::default();
    for gap in &gaps {
        if let Some(layout) = window.rendered_frame.fast_retained.records[gap.record]
            .layout
            .as_ref()
        {
            copy.gap_states
                .extend(layout.element_states.iter().cloned());
        }
    }
    let mut copied = Vec::new();
    let mut cursor = prepaint_range.start.clone();
    let mut next_record = previous + 1;
    for (segment, gap) in gaps.iter_mut().enumerate() {
        let gap_record = &window.rendered_frame.fast_retained.records[gap.record];
        let gap_range = gap_record.prepaint_range.clone();
        let gap_nested = gap_record.nested;

        let segment_start = window.prepaint_index();
        copy.prepaint(window, cursor.clone()..gap_range.start.clone());
        copy_records(
            window,
            next_record..gap.record,
            &cursor,
            &segment_start,
            (segment, index),
            &mut copied,
        );

        // The gap hangs off its element's dispatch node, which that element
        // pushed before the view began and which was copied with the
        // stretch before it, as its DevKit node's id was.
        let parent = window.rendered_frame.dispatch_tree.nodes[gap_range.start.dispatch_tree_index]
            .parent
            .expect("a nested view hangs off its element's node");
        copy.unwind_to(Some(parent), &mut window.next_frame.dispatch_tree);
        let node = copy.copied(parent).expect("its element's node was copied");
        let devtools_node = copy.devtools_id(gap_range.start.devtools_node_id - 1);
        prepaint_gap(window, gap, node, devtools_node, cx);

        cursor = gap_range.end.clone();
        next_record = gap.record + gap_nested + 1;
    }
    let segment = gaps.len();
    let segment_start = window.prepaint_index();
    copy.prepaint(window, cursor.clone()..prepaint_range.end);
    copy_records(
        window,
        next_record..last + 1,
        &cursor,
        &segment_start,
        (segment, index),
        &mut copied,
    );
    copy.close(&mut window.next_frame.dispatch_tree);
    let end = window.prepaint_index();

    // A DevKit node counts the hitboxes of its element's whole subtree: the
    // gap's element, and every copied node around it up to the view's root,
    // count the gap's change in hitboxes.
    for gap in &gaps {
        let Some((_, devtools_node)) = gap.prepainted else {
            continue;
        };
        let previous = &window.rendered_frame.fast_retained.records[gap.record].prepaint_range;
        let previous = previous.end.hitboxes_index - previous.start.hitboxes_index;
        let change = gap_hitbox_count(window, gap) as isize - previous as isize;
        if change == 0 {
            continue;
        }
        let frame = &mut window.next_frame;
        let mut node = Some(devtools_node);
        while let Some(id) = node.filter(|id| copy.copied_devtools_id(*id)) {
            let Some(entry) = frame
                .devtools_node_indices
                .get(id)
                .and_then(|&index| frame.devtools_nodes.get_mut(index))
            else {
                break;
            };
            entry.hitbox_count = (entry.hitbox_count as isize + change).max(0) as usize;
            node = entry.parent_devtools_node_id;
        }
    }

    // The gaps' layout keys and dependencies are this frame's.
    let mut layout_keys = kept_layout_keys;
    let target = &window.next_frame.fast_retained;
    let mut dependencies = target.records[index].dependencies.clone();
    for gap in &gaps {
        if let Some(gap_index) = target.find_open_or_painted(&gap.global_id) {
            let gap_record = &target.records[gap_index];
            layout_keys.extend(gap_record.layout_keys.iter().copied());
            dependencies = dependencies.union(&gap_record.dependencies);
        }
    }
    crate::fast::retained::close_open(window, index, start..end, layout_keys, dependencies);

    SplicedPrepaint {
        previous,
        index,
        gaps,
        copied,
    }
}

/// How many hitboxes the gap's subtree inserted this frame.
fn gap_hitbox_count(window: &Window, gap: &Gap) -> usize {
    window
        .next_frame
        .fast_retained
        .find_open_or_painted(&gap.global_id)
        .map(|index| {
            let range = &window.next_frame.fast_retained.records[index].prepaint_range;
            range.end.hitboxes_index - range.start.hitboxes_index
        })
        .unwrap_or(0)
}

/// Prepaints a gap where its element prepainted it last frame: under the
/// element's copied dispatch node `node` and DevKit node `devtools_node`,
/// with what it inherited there.
fn prepaint_gap(
    window: &mut Window,
    gap: &mut Gap,
    node: DispatchNodeId,
    devtools_node: usize,
    cx: &mut App,
) {
    debug_assert_eq!(window.next_frame.dispatch_tree.active_node_id(), Some(node));
    let gap_record = &window.rendered_frame.fast_retained.records[gap.record];
    let rebuild = gap_record
        .rebuild
        .clone()
        .expect("a gap can be built on its own");
    let masks = Masks::of(gap_record);
    let inherited = enter_gap(window, &gap.global_id.clone(), &rebuild, masks);
    let bounds = window.layout_bounds(gap.layout_id);
    let element_id = gap.global_id.last().cloned().expect("a view has an id");
    window.element_id_stack.push(element_id);
    window.push_existing_devtools_node(devtools_node);
    let scope = crate::fast::layout_key::enter_prepaint_scope(window, gap.layout_key);
    let global_id = gap.global_id.clone();
    prepaint_nested(
        &mut gap.view,
        gap.entity,
        Some(&global_id),
        bounds,
        window,
        cx,
    );
    crate::fast::layout_key::exit_prepaint_scope(window, scope);
    window.pop_devtools_node(devtools_node);
    window.element_id_stack.pop();
    leave_gap(window, inherited);
    gap.prepainted = Some((node, devtools_node));
}

/// Copies last frame's records `records`, whose prepaint was copied from
/// `from` on to `to`, into this frame inside the spliced view `anchor`,
/// adding to `copied` where they went and the stretch they came from.
fn copy_records(
    window: &mut Window,
    records: Range<usize>,
    from: &PrepaintStateIndex,
    to: &PrepaintStateIndex,
    (segment, anchor): (usize, usize),
    copied: &mut Vec<(usize, usize)>,
) {
    for index in records {
        let record = &window.rendered_frame.fast_retained.records[index];
        let prepaint_range = record.prepaint_range.start.shifted(from, to)
            ..record.prepaint_range.end.shifted(from, to);
        let painted = matches!(record.paint, PaintStatus::Painted { .. });
        let mut copy = crate::fast::retained::copy_record(record, prepaint_range, anchor);
        if !painted {
            copy.paint = PaintStatus::Unpainted;
        }
        copied.push((crate::fast::retained::push_record(window, copy), segment));
    }
}

/// Paints the view [`splice_prepaint`] prepainted: last frame's paint,
/// copied around the gaps, and the gaps painted where they were.
pub(crate) fn splice_paint(window: &mut Window, spliced: &mut SplicedPrepaint, cx: &mut App) {
    let SplicedPrepaint {
        previous,
        index,
        gaps,
        copied,
    } = spliced;
    let record = &window.rendered_frame.fast_retained.records[*previous];
    let paint_range = record.paint_range.clone();
    let gap_reads: FxHashSet<_> = gaps
        .iter()
        .flat_map(|gap| {
            window.rendered_frame.fast_retained.records[gap.record]
                .hover_reads
                .iter()
                .map(|(hitbox, _)| *hitbox)
        })
        .collect();
    let copied_reads: Vec<_> = record
        .hover_reads
        .iter()
        .filter(|(hitbox, _)| !gap_reads.contains(hitbox))
        .copied()
        .collect();

    let start = window.paint_index();
    let mut segments: Vec<(PaintIndex, PaintIndex)> = Vec::with_capacity(gaps.len() + 1);
    let mut cursor = paint_range.start.clone();
    for gap in gaps.iter_mut() {
        let gap_range = window.rendered_frame.fast_retained.records[gap.record]
            .paint_range
            .clone();
        segments.push((cursor.clone(), window.paint_index()));
        window.reuse_paint(cursor.clone()..gap_range.start.clone());
        paint_gap(window, gap, cx);
        cursor = gap_range.end.clone();
    }
    segments.push((cursor.clone(), window.paint_index()));
    window.reuse_paint(cursor..paint_range.end);
    let end = window.paint_index();
    crate::fast::dependencies::note_replayed_hovers(window, &copied_reads);

    // The records copied along with it land where their stretch did.
    for &(copy, segment) in copied.iter() {
        let (from, to) = &segments[segment];
        crate::fast::retained::place_copied_paint(window, copy, from, to);
    }

    // Its hovers: those of what was copied, and the gaps' new ones.
    let mut reads = copied_reads;
    for gap in gaps.iter() {
        if let Some(gap_index) = window
            .next_frame
            .fast_retained
            .find_open_or_painted(&gap.global_id)
        {
            reads
                .extend_from_slice(&window.next_frame.fast_retained.records[gap_index].hover_reads);
        }
    }
    crate::fast::retained::finish_spliced_paint(window, *index, start..end, reads);
}

/// Paints a gap where its element painted it last frame.
fn paint_gap(window: &mut Window, gap: &mut Gap, cx: &mut App) {
    let Some((node, devtools_node)) = gap.prepainted else {
        return;
    };
    let gap_record = &window.rendered_frame.fast_retained.records[gap.record];
    let rebuild = gap_record
        .rebuild
        .clone()
        .expect("a gap can be built on its own");
    let masks = Masks::of(gap_record);
    let inherited = enter_gap(window, &gap.global_id.clone(), &rebuild, masks);
    let element_id = gap.global_id.last().cloned().expect("a view has an id");
    window.element_id_stack.push(element_id);
    window.next_frame.dispatch_tree.set_active_node(node);
    window.push_existing_devtools_node(devtools_node);
    crate::fast::retained::paint_view(&mut gap.view.fast_retention, gap.entity, window, cx);
    window.update_devtools_element_after_paint(devtools_node, node);
    window.pop_devtools_node(devtools_node);
    window.element_id_stack.pop();
    leave_gap(window, inherited);
}

/// Last frame's prepaint copied stretch by stretch, with the dispatch nodes
/// the copy is inside of left open between stretches, and the DevKit ids and
/// dispatch nodes of every stretch mapped, for a node recorded in one
/// stretch to find what it points at in another.
#[derive(Default)]
struct StretchCopy {
    /// Last frame's dispatch nodes the copy is inside of, innermost last.
    open: Vec<DispatchNodeId>,
    /// Each stretch of dispatch nodes copied, and where it landed.
    nodes: Vec<(Range<usize>, usize)>,
    /// Each stretch of DevKit ids copied, and where it landed.
    devtools_ids: Vec<(Range<usize>, usize)>,
    /// The element states the gaps used last frame. The stretch before a
    /// gap holds the accesses its view's layout made for everything nested
    /// in it, the gaps included: a gap built again uses what it needs
    /// anew, and a state it no longer uses (its element is gone) must not
    /// be carried into the frame by the copy.
    gap_states: FxHashSet<(GlobalElementId, TypeId)>,
}

impl StretchCopy {
    /// Copies last frame's prepaint of `range` as [`Window::reuse_prepaint`]
    /// does, leaving the dispatch nodes it enters open.
    fn prepaint(&mut self, window: &mut Window, range: Range<PrepaintStateIndex>) {
        debug_assert_eq!(
            range.start.deferred_draws_index, range.end.deferred_draws_index,
            "a splice copies no deferred draw"
        );
        crate::fast::composition::reuse_placements(window, &range);
        let rendered = &mut window.rendered_frame;
        let next = &mut window.next_frame;
        next.hitboxes.extend(
            rendered.hitboxes[range.start.hitboxes_index..range.end.hitboxes_index]
                .iter()
                .cloned(),
        );
        next.tooltip_requests.extend(
            rendered.tooltip_requests[range.start.tooltips_index..range.end.tooltips_index]
                .iter_mut()
                .map(|request| request.take()),
        );
        let states = &rendered.accessed_element_states
            [range.start.accessed_element_states_index..range.end.accessed_element_states_index];
        // The entries stay where they are, for the ranges recorded around
        // them to stand: the states of the gaps become stand-ins that
        // transfer nothing.
        let released = TypeId::of::<crate::fast::retained::ReleasedElementState>();
        let states: Vec<_> = states
            .iter()
            .map(|state| {
                if self.gap_states.contains(state) {
                    (state.0.clone(), released)
                } else {
                    state.clone()
                }
            })
            .collect();
        next.accessed_element_states.extend(states.iter().cloned());
        next.pending_element_state_transfers.extend(states);
        window
            .text_system()
            .reuse_layouts(range.start.line_layout_index..range.end.line_layout_index);

        let contains_focus = self.copy_nodes(
            range.start.dispatch_tree_index..range.end.dispatch_tree_index,
            &mut window.rendered_frame.dispatch_tree,
            &mut window.next_frame.dispatch_tree,
            window.focus,
        );
        if contains_focus {
            window.next_frame.focus = window.focus;
        }
        let rendered = &window.rendered_frame;
        let next = &mut window.next_frame;
        for hitbox in &rendered.hitboxes[range.start.hitboxes_index..range.end.hitboxes_index] {
            if let Some(node) = rendered.hitbox_dispatch_nodes.get(&hitbox.id)
                && let Some(node) = self.copied(*node)
            {
                next.hitbox_dispatch_nodes.insert(hitbox.id, node);
            }
        }

        // DevKit ids are allocated before an element's children, and its
        // node is recorded after them: a node recorded here may have been
        // allocated in an earlier stretch.
        let old_ids = range.start.devtools_node_id..range.end.devtools_node_id;
        let new_start = next.next_devtools_node_id;
        next.next_devtools_node_id += old_ids.len();
        next.devtools_node_indices
            .resize(next.next_devtools_node_id, usize::MAX);
        self.devtools_ids.push((old_ids, new_start));
        let enclosing = next.devtools_node_stack.last().copied();
        for source in &rendered.devtools_nodes
            [range.start.devtools_nodes_index..range.end.devtools_nodes_index]
        {
            let devtools_node_id = self.devtools_id(source.devtools_node_id);
            let dispatch_node_id = self
                .copied(DispatchNodeId::from_usize(source.dispatch_node_id))
                .expect("a copied DevKit node's dispatch node was copied");
            let dispatch_node = next.dispatch_tree.node(dispatch_node_id);
            next.devtools_node_indices[devtools_node_id] = next.devtools_nodes.len();
            next.devtools_nodes.push(crate::window::DevtoolsFrameNode {
                devtools_node_id,
                parent_devtools_node_id: source
                    .parent_devtools_node_id
                    .and_then(|parent| self.try_devtools_id(parent))
                    .or(enclosing),
                dispatch_node_id: dispatch_node_id.as_usize(),
                parent_dispatch_node_id: next
                    .dispatch_tree
                    .parent_node_id(dispatch_node_id)
                    .map(DispatchNodeId::as_usize),
                type_name: source.type_name,
                global_id: source.global_id.clone(),
                source: source.source,
                bounds: source.bounds,
                semantics: source.semantics.clone(),
                hitbox_count: source.hitbox_count,
                key_listener_count: dispatch_node.key_listeners.len(),
                action_listener_count: dispatch_node.action_listeners.len(),
                focused: dispatch_node.focus_id.is_some() && dispatch_node.focus_id == window.focus,
                fast_row: source.fast_row,
            });
            if let Some(setters) = rendered
                .devtools_property_setters
                .get(&source.dispatch_node_id)
            {
                next.devtools_property_setters
                    .insert(dispatch_node_id.as_usize(), setters.clone());
            }
        }
    }

    /// Copies the dispatch nodes `range` of `source` into `target`, each
    /// under its copied parent, leaving open the nodes the stretch leaves
    /// open; returns whether one of them holds `focus`.
    fn copy_nodes(
        &mut self,
        range: Range<usize>,
        source: &mut DispatchTree,
        target: &mut DispatchTree,
        focus: Option<crate::FocusId>,
    ) -> bool {
        self.nodes.push((range.clone(), target.len()));
        let mut contains_focus = false;
        for index in range {
            let parent = source.nodes[index].parent;
            while let Some(&open) = self.open.last() {
                if Some(open) == parent {
                    break;
                }
                self.open.pop();
                target.pop_node();
            }
            let node = &mut source.nodes[index];
            if node.focus_id.is_some() && node.focus_id == focus {
                contains_focus = true;
            }
            target.move_node(node);
            self.open.push(DispatchNodeId::from_usize(index));
        }
        contains_focus
    }

    /// Closes open nodes until the innermost is `parent`, or none is.
    fn unwind_to(&mut self, parent: Option<DispatchNodeId>, target: &mut DispatchTree) {
        while let Some(&open) = self.open.last() {
            if Some(open) == parent {
                break;
            }
            self.open.pop();
            target.pop_node();
        }
    }

    fn close(&mut self, target: &mut DispatchTree) {
        self.unwind_to(None, target);
    }

    /// Where last frame's dispatch node `node` is now, if it was copied.
    fn copied(&self, node: DispatchNodeId) -> Option<DispatchNodeId> {
        let node = node.as_usize();
        let (range, start) = self.nodes.iter().find(|(range, _)| range.contains(&node))?;
        Some(DispatchNodeId::from_usize(node - range.start + start))
    }

    /// What last frame's DevKit id `id` is now, if it was copied.
    fn try_devtools_id(&self, id: usize) -> Option<usize> {
        let (range, start) = self
            .devtools_ids
            .iter()
            .find(|(range, _)| range.contains(&id))?;
        Some(id - range.start + start)
    }

    /// Whether `id`, a DevKit id of this frame, was given to a node copied
    /// in a stretch.
    fn copied_devtools_id(&self, id: usize) -> bool {
        self.devtools_ids
            .iter()
            .any(|(range, start)| (*start..start + range.len()).contains(&id))
    }

    fn devtools_id(&self, id: usize) -> usize {
        self.try_devtools_id(id)
            .expect("a copied DevKit node's id was allocated in a copied stretch")
    }
}

/// `keys` without `removed`, in their order.
fn without<'a>(keys: &[u64], removed: impl Iterator<Item = &'a u64>) -> Vec<u64> {
    let removed: FxHashSet<u64> = removed.copied().collect();
    if removed.is_empty() {
        return keys.to_vec();
    }
    keys.iter()
        .copied()
        .filter(|key| !removed.contains(key))
        .collect()
}

/// The element states a view drawn around its gaps keeps from last frame:
/// those it used itself. A nested view built again in its gap uses its own
/// anew; one it no longer uses goes at the end of the frame, as upstream
/// drops the state of every element a frame does not draw.
fn kept_element_states<'a>(
    element_states: &[(GlobalElementId, TypeId)],
    gaps: impl Iterator<Item = &'a (GlobalElementId, TypeId)>,
) -> Vec<(GlobalElementId, TypeId)> {
    let in_gaps: FxHashSet<&(GlobalElementId, TypeId)> = gaps.collect();
    element_states
        .iter()
        .filter(|state| !in_gaps.contains(state))
        .cloned()
        .collect()
}
