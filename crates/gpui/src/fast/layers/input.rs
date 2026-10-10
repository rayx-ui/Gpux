//! Input under scroll layers: hitbox translation and clipping, rebuilding
//! before input, tooltips and getter translation. Adapted from gpui-fast's
//! `fast/layers/input.rs`.
//!
//! Application code sees window coordinates only (spec §7). On frames that
//! composite a layer its content is not prepainted or painted again, so the
//! hitboxes it inserted are carried moved by the scroll (see
//! [`crate::fast::layers::reuse`]), and positions its closures and element
//! states captured lag behind by as much. The content is painted again
//! before it sees input that could observe them.

use crate::{
    App, Bounds, ContentMask, GlobalElementId, Hitbox, Pixels, PlatformInput, Point, Window,
    elements::ScrollHandleState,
    fast::layers::{COMPILED, Layer, policy::Decision},
    fast::retained::view_entity,
};
use collections::FxHashMap;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

/// What a layer keeps to route input into its content.
#[derive(Default)]
pub(crate) struct LayerInput {
    /// The content's hitboxes as painted: in window space at the offset the
    /// content was painted at, their masks clipped by the content's own
    /// clips but not by the viewport.
    pub(crate) hitboxes: Vec<Hitbox>,
    /// The frame the layer's record's prepaint and paint ranges index the
    /// records of: they can be carried into the frame after it only.
    pub(crate) ranges_frame: Option<u64>,
    /// The frame that copied the layer's prepaint records along with the
    /// view holding it, if it did, for its paint records to follow them.
    /// See [`crate::fast::layers::reuse::follow_prepaint`].
    pub(crate) prepaint_followed: Option<u64>,
    /// How far the content shown last frame had scrolled since it was
    /// painted: how far behind the positions its closures and element
    /// states hold are.
    pub(crate) stale: Point<Pixels>,
    /// Whether the content is to be painted again at the offset shown the
    /// next time the container is prepainted, though nothing it is built
    /// from changed: it is about to see input (see [`before_dispatch`]).
    pub(crate) rebuild: bool,
    /// The container's clip rect in window space, last frame.
    pub(crate) viewport: Bounds<Pixels>,
    /// The retained subtree of the view holding the container, last frame.
    pub(crate) owner: Option<GlobalElementId>,
    /// How far the content shown has scrolled since it was painted, shared
    /// with the scroll handles tracking elements inside it.
    pub(crate) handle_offset: Rc<Cell<Point<Pixels>>>,
}

/// What routing input into a layer's content takes from painting it.
#[derive(Default)]
pub(crate) struct PaintingInput {
    /// The mask of each hitbox the content inserted, in order, before
    /// [`hitbox_mask`] clipped it to the viewport.
    pub(crate) hitbox_masks: Vec<ContentMask<Pixels>>,
    /// Shared with the scroll handles tracking elements inside the content.
    pub(crate) handle_offset: Rc<Cell<Point<Pixels>>>,
    /// For a virtual list's rows, the part of the content painted, which
    /// the list's own clip, its viewport, is taken out of the masks noted
    /// for: rows are prepainted inside that clip, which does not move with
    /// them. See [`unclip`].
    pub(crate) list_region: Option<Bounds<Pixels>>,
}

/// A scroll handle's state, as a handle shares it.
type HandleState = Rc<RefCell<ScrollHandleState>>;

/// A scroll handle whose tracked element was prepainted into a layer, and
/// how far the layer's scroll has moved it since.
type MovedHandle = (Weak<RefCell<ScrollHandleState>>, Rc<Cell<Point<Pixels>>>);

thread_local! {
    /// The scroll handles whose tracked element was last prepainted into a
    /// layer, by the address of their state, with how far the layer's
    /// scroll has moved the element since (spec §7, rule 7). The handle's
    /// getters take no window to find the layer in; windows are drawn on
    /// this one thread.
    static MOVED_HANDLES: RefCell<FxHashMap<usize, MovedHandle>> =
        RefCell::new(FxHashMap::default());
}

impl LayerInput {
    /// `hitboxes`, moved by `delta` and clipped to `viewport`.
    pub(crate) fn hitboxes_at(
        hitboxes: &[Hitbox],
        delta: Point<Pixels>,
        viewport: Bounds<Pixels>,
    ) -> impl Iterator<Item = Hitbox> {
        hitboxes.iter().map(move |hitbox| Hitbox {
            id: hitbox.id,
            bounds: Bounds {
                origin: hitbox.bounds.origin + delta,
                size: hitbox.bounds.size,
            },
            content_mask: ContentMask {
                corner_radii: hitbox.content_mask.corner_radii,
                bounds: Bounds {
                    origin: hitbox.content_mask.bounds.origin + delta,
                    size: hitbox.content_mask.bounds.size,
                }
                .intersect(&viewport),
            },
            behavior: hitbox.behavior,
        })
    }
}

/// What the container `id` does with its children, given the policy's
/// `decision`: a layer is composited only if what its content added to the
/// last frame can be carried into this one, which a frame drawn without
/// the container prepainting its children in between (a reused view, a
/// refresh) prevents, and its content is not about to see input. It is
/// painted again otherwise, which the policy does not take for a change of
/// the content.
pub(crate) fn decide(
    window: &mut Window,
    id: &GlobalElementId,
    decision: Decision,
    scroll_offset: Point<Pixels>,
) -> Decision {
    let frame = window.fast_layers.frame;
    let before_input = window.fast_layers.before_input;
    let owner = crate::fast::retained::prepainting_view(window).cloned();
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return decision;
    };
    let has_record = layer.record.is_some();
    // A frame drawn for input about to be dispatched shows no content away
    // from where it was painted.
    let moved_before_input = before_input
        && layer
            .record
            .as_ref()
            .is_some_and(|record| record.scroll_offset != scroll_offset);
    let input = &mut layer.input;
    if input.owner != owner {
        input.owner = owner;
    }
    let rebuild = std::mem::take(&mut input.rebuild);
    let decision = if decision == Decision::Composite
        && (rebuild
            || moved_before_input
            || !has_record
            || input
                .ranges_frame
                .is_none_or(|painted| painted + 1 != frame))
    {
        Decision::Repaint
    } else {
        decision
    };
    if decision != Decision::Composite {
        // The content is painted at the offset shown, into the layer or
        // straight into the frame.
        input.stale = Point::default();
    }
    decision
}

/// Brings the content of every layer that `event` could reach up to date
/// before it is dispatched (spec §7, rule 3): a layer whose content was
/// shown scrolled since it was painted, and whose viewport holds the
/// pointer (any pointer event but a wheel's) or whose content holds the
/// focus (key events), is painted again at the current offset, in a frame
/// drawn now, so that the positions its closures and element states hold
/// are current when they see the event. Wheel events are dispatched as they
/// come: the content's wheel listeners go by its hitboxes, which are.
///
/// Called by [`Window::dispatch_event`] once it has taken in the event's
/// position.
pub(crate) fn before_dispatch(window: &mut Window, cx: &mut App, event: &PlatformInput) {
    if !COMPILED || window.fast_layers.layers.is_empty() {
        return;
    }
    // Any frame drawn until the event is dispatched is drawn for it: the one
    // drawn here, and the one the window draws itself before it dispatches
    // a key while it is dirty.
    window.fast_layers.before_input = !matches!(
        event,
        PlatformInput::ScrollWheel(_) | PlatformInput::Touch(_)
    );
    match event {
        // Dispatched as they come: the content's wheel listeners go by its
        // hitboxes, which are current.
        PlatformInput::ScrollWheel(_) => {}
        // A touch reaches the content only as the gesture recognized from
        // it, which [`before_touch_gesture`] sees.
        PlatformInput::Touch(_) => {}
        PlatformInput::KeyDown(_)
        | PlatformInput::KeyUp(_)
        | PlatformInput::ModifiersChanged(_) => {
            rebuild_reached(window, cx, &|window, _, layer| focus_inside(window, layer))
        }
        // A move reaches the content when nothing covers it under the
        // pointer: its listeners, with a hitbox of their own or not, may
        // compare the move with the bounds they hold. While a button is
        // held, the moves belong to what the press began on: a drag begun in
        // the content keeps reaching it, and one begun elsewhere — a
        // scrollbar dragged beside or over the content — does not, and must
        // not paint the content again on every frame of the drag. Nor does a
        // move while a hitbox outside the content holds the pointer captured.
        PlatformInput::MouseMove(event) => {
            let under_pointer = topmost_hitbox(window, event.position);
            let held = event.pressed_button.is_some();
            let captured = window.captured_hitbox;
            rebuild_reached(window, cx, &|window, id, layer| {
                if captured.is_some_and(|captured| !holds_hitbox(window, id, layer, captured)) {
                    return false;
                }
                if held {
                    return window.fast_layers.scrolls.pressed_in.contains(id);
                }
                under_pointer.is_some_and(|hitbox| holds_hitbox(window, id, layer, hitbox))
            })
        }
        // A press, wherever it lands: it starts what later moves continue
        // (a drag, a selection) with the bounds it sees.
        PlatformInput::MouseDown(event) => {
            rebuild_reached(window, cx, &|_, _, _| true);
            // Told from the frame the press is dispatched in, which the
            // rebuild may just have drawn.
            let under_pointer = topmost_hitbox(window, event.position);
            let pressed_in = window
                .fast_layers
                .layers
                .iter()
                .filter(|(id, layer)| {
                    under_pointer.is_some_and(|hitbox| holds_hitbox(window, id, layer, hitbox))
                })
                .map(|(id, _)| id.clone())
                .collect();
            window.fast_layers.scrolls.pressed_in = pressed_in;
        }
        PlatformInput::MouseUp(_) => {
            rebuild_reached(window, cx, &|_, _, _| true);
            window.fast_layers.scrolls.pressed_in.clear();
        }
        // Drops and whatever else carries a position.
        _ => rebuild_reached(window, cx, &|_, _, _| true),
    }
}

/// The topmost hitbox at `position`, as the last frame hit tests it.
fn topmost_hitbox(window: &Window, position: Point<Pixels>) -> Option<crate::HitboxId> {
    window
        .rendered_frame
        .hit_test(position)
        .ids
        .first()
        .copied()
}

/// Whether `hitbox` is one the content of `layer`, the layer of the scroll
/// container `id`, inserted, or the container's own, which lies under its
/// content: an event landing on it lands on the content, or on where content
/// without a hitbox of its own would see it.
fn holds_hitbox(
    window: &Window,
    id: &GlobalElementId,
    layer: &Layer,
    hitbox: crate::HitboxId,
) -> bool {
    layer.input.hitboxes.iter().any(|held| held.id == hitbox)
        || layer.rows.holds_hitbox(hitbox)
        || window.fast_layers.scrolls.container_hitbox(id) == Some(hitbox)
}

/// Ends what [`before_dispatch`] or [`before_touch_gesture`] began, once the
/// event is dispatched: frames drawn from here on are ordinary ones.
#[inline]
pub(crate) fn after_dispatch(window: &mut Window) {
    window.fast_layers.before_input = false;
}

/// [`before_dispatch`] for a gesture recognized from touches, which
/// [`Window::dispatch_event`] hands to the content after it: the scroll of a
/// pan or of its momentum is scroll input, as a wheel's is, and composites;
/// a tap, a long press and a touch drag are presses, which see content
/// painted where it shows.
pub(crate) fn before_touch_gesture(
    window: &mut Window,
    cx: &mut App,
    gesture: &crate::gestures::RecognizedTouchGesture,
) {
    if !COMPILED || window.fast_layers.layers.is_empty() {
        return;
    }
    window.fast_layers.before_input =
        !matches!(gesture, crate::gestures::RecognizedTouchGesture::Scroll(_));
    match gesture {
        crate::gestures::RecognizedTouchGesture::Scroll(_) => {}
        crate::gestures::RecognizedTouchGesture::Tap { .. }
        | crate::gestures::RecognizedTouchGesture::TouchDrag(_)
        | crate::gestures::RecognizedTouchGesture::LongPress(_) => {
            rebuild_reached(window, cx, &|_, _, _| true)
        }
    }
}

/// Paints again, in a frame drawn now, every layer whose content shows
/// scrolled since it was painted and which `reaches` picks.
fn rebuild_reached(
    window: &mut Window,
    cx: &mut App,
    reaches: &dyn Fn(&Window, &GlobalElementId, &Layer) -> bool,
) {
    let mut owners = Vec::new();
    let mut unknown_owner = false;
    let mut rebuilt = 0;
    for (id, layer) in &window.fast_layers.layers {
        if layer.input.stale != Point::default() && reaches(window, id, layer) {
            rebuilt += 1;
            match layer.input.owner.as_ref() {
                Some(owner) if view_entity(owner).is_some() => {
                    if !owners.contains(owner) {
                        owners.push(owner.clone());
                    }
                }
                _ => unknown_owner = true,
            }
        }
    }
    if rebuilt == 0 {
        return;
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        window.fast_layers.input_rebuilds += rebuilt as u64;
    }
    for layer in window.fast_layers.layers.values_mut() {
        let input = &mut layer.input;
        if input.stale != Point::default()
            && (unknown_owner
                || input
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owners.contains(owner)))
        {
            input.rebuild = true;
        }
    }
    // The view holding the container is built again, as when an
    // interaction inside it changes it, and the container, told to by
    // `rebuild`, paints its layer. The view is not notified: nothing it is
    // built from changed, and its observers are not to hear otherwise. A
    // dirty view is never drawn from last frame, and the views around it are
    // at most drawn around it.
    for owner in &owners {
        if let Some(view) = view_entity(owner) {
            window.dirty_views.insert(view);
            for view in window.rendered_frame.dispatch_tree.view_path_reversed(view) {
                window.dirty_views.insert(view);
            }
        }
    }
    if unknown_owner {
        window.refresh();
    }
    crate::fast::dependencies::AmbientInput::stamp_all(cx);
    window.draw(cx).clear(cx);
}

/// Whether the focused element is inside the content of `layer`, as the
/// rendered frame holds it. When the rendered frame's records of the
/// content are not the layer's to tell, any focus is taken to be inside.
fn focus_inside(window: &Window, layer: &Layer) -> bool {
    let Some(focus) = window.focus else {
        return false;
    };
    let ranges_current = layer
        .input
        .ranges_frame
        .is_some_and(|frame| frame + 1 == window.fast_layers.frame);
    let Some(record) = layer.record.as_ref().filter(|_| ranges_current) else {
        return true;
    };
    let range = &record.prepaint_range;
    let nodes = &window.rendered_frame.dispatch_tree.nodes;
    let start = range.start.dispatch_tree_index.min(nodes.len());
    let end = range.end.dispatch_tree_index.clamp(start, nodes.len());
    nodes[start..end]
        .iter()
        .any(|node| node.focus_id == Some(focus))
}

/// Notes that the content of the layer of the container `id` was just
/// painted into the frame being drawn, over its record's prepaint range,
/// with what `painting` took from painting it.
pub(crate) fn painted(window: &mut Window, id: &GlobalElementId, painting: PaintingInput) {
    let frame = window.fast_layers.frame;
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    let Some(record) = layer.record.as_ref() else {
        return;
    };
    let range = &record.prepaint_range;
    let inserted =
        &window.next_frame.hitboxes[range.start.hitboxes_index..range.end.hitboxes_index];
    let input = &mut layer.input;
    input.hitboxes.clear();
    input.hitboxes.extend_from_slice(inserted);
    if painting.hitbox_masks.len() == inserted.len() {
        for (hitbox, mask) in input.hitboxes.iter_mut().zip(painting.hitbox_masks) {
            hitbox.content_mask = mask;
        }
    } else {
        debug_assert!(false, "a hitbox inserted inside a layer went unnoted");
    }
    input.ranges_frame = Some(frame);
    input.stale = Point::default();
    input.viewport = record.viewport;
    input.handle_offset = painting.handle_offset;
    MOVED_HANDLES.with_borrow_mut(|moved| moved.retain(|_, (state, _)| state.strong_count() > 0));
}

/// The content mask of a hitbox being inserted: the current one, clipped to
/// the viewport of the layer being painted, if any. A layer culls its
/// content against the viewport and its overscan, but only what shows in
/// the viewport may be hit, as without a layer. The mask before clipping is
/// kept, for the hitbox to be clipped anew once the content has scrolled.
pub(crate) fn hitbox_mask(window: &mut Window) -> ContentMask<Pixels> {
    let mask = window.content_mask();
    let Some(painting) = window.fast_layers.painting.as_mut() else {
        return mask;
    };
    let noted = match painting.input.list_region {
        Some(region) => unclip(mask, painting.viewport, region),
        None => mask,
    };
    painting.input.hitbox_masks.push(noted);
    ContentMask {
        bounds: mask.bounds.intersect(&painting.viewport),
        corner_radii: mask.corner_radii,
    }
}

/// `mask`, a mask inside `viewport`, with each edge it shares with
/// `viewport` moved out to `region`'s: the clips of what lies inside a
/// virtual list's rows, which move with the rows, without the list's own,
/// which does not.
fn unclip(
    mask: ContentMask<Pixels>,
    viewport: Bounds<Pixels>,
    region: Bounds<Pixels>,
) -> ContentMask<Pixels> {
    let mask = mask.bounds;
    let pick = |edge: Pixels, viewport: Pixels, region: Pixels| {
        if edge == viewport { region } else { edge }
    };
    let top_left = crate::point(
        pick(
            mask.origin.x,
            viewport.origin.x,
            region.origin.x.min(viewport.origin.x),
        ),
        pick(
            mask.origin.y,
            viewport.origin.y,
            region.origin.y.min(viewport.origin.y),
        ),
    );
    let bottom_right = crate::point(
        pick(
            mask.right(),
            viewport.right(),
            region.right().max(viewport.right()),
        ),
        pick(
            mask.bottom(),
            viewport.bottom(),
            region.bottom().max(viewport.bottom()),
        ),
    );
    ContentMask::rectangular(Bounds::from_corners(top_left, bottom_right))
}

/// Notes, as the element the scroll handle whose state is `handle` tracks
/// is prepainted, whether it is prepainted into a layer, whose scroll then
/// moves it.
pub(crate) fn track_handle(window: &Window, handle: &HandleState) {
    if !window.fast_mode.scroll_layers() {
        return;
    }
    let key = Rc::as_ptr(handle) as usize;
    match window.fast_layers.painting.as_ref() {
        Some(painting) => MOVED_HANDLES.with_borrow_mut(|moved| {
            let offset = painting.input.handle_offset.clone();
            moved.insert(key, (Rc::downgrade(handle), offset));
        }),
        None => MOVED_HANDLES.with_borrow_mut(|moved| {
            if !moved.is_empty() {
                moved.remove(&key);
            }
        }),
    }
}

/// `bounds`, of a child of the element the scroll handle whose state is
/// `handle` tracks as it was prepainted, moved to where the child shows
/// now: by the scroll of the layer it was prepainted into since.
pub(crate) fn moved(
    handle: &HandleState,
    bounds: Option<Bounds<Pixels>>,
) -> Option<Bounds<Pixels>> {
    let bounds = bounds?;
    if !crate::fast::layers::on_thread() {
        return Some(bounds);
    }
    let key = Rc::as_ptr(handle) as usize;
    let delta = MOVED_HANDLES.with_borrow(|moved| {
        moved
            .get(&key)
            .filter(|(state, _)| std::ptr::eq(state.as_ptr(), Rc::as_ptr(handle)))
            .map_or(Point::default(), |(_, offset)| offset.get())
    });
    Some(Bounds {
        origin: bounds.origin + delta,
        size: bounds.size,
    })
}
