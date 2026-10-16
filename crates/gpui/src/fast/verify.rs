//! Turning a rendered frame into text two frames can be compared by: what the
//! oracle and the fast-mode parity tests check every mechanism against.
//!
//! Adapted from gpui-fast's `fast/layout_key.rs` (`describe_rendered_frame`)
//! and `fast/layers/verify.rs` (`drawing_sequence`, `describe`). gpux
//! describes far more of the frame than gpui-fast does: every scene family
//! (its own included), the screen-effect requests, hitboxes in hit-test
//! order, cursors, tooltips, tab stops, mouse listeners, input handlers, the
//! dispatch tree and focus path, the element states the frame accessed, the
//! effect diagnostics and the accessibility tree. It compares draw orders by rank rather than by
//! gpui-fast's overlap depth, because gpux replays cached views by inserting
//! their primitives again and both windows number draw orders alike.

use crate::{
    AtlasTextureId, AtlasTile, Bounds, DevicePixels, HitboxId, PathId, Pixels, Point, Window,
    scene::{PaintOperation, Primitive},
};
use collections::{FxHashMap, FxHashSet};

/// A primitive of a scene with the key the renderer draws it in: its draw
/// order, the rank of its family among the batches of one draw order, and its
/// index in its family.
pub(crate) struct Drawn {
    pub(crate) order: u32,
    pub(crate) family: u8,
    pub(crate) index: usize,
    pub(crate) primitive: Primitive,
}

/// Every primitive `scene` holds, in the order the renderer draws them.
pub(crate) fn drawing_sequence(scene: &crate::Scene) -> Vec<Drawn> {
    let mut sequence = Vec::new();
    macro_rules! gather {
        ($field:ident, $family:expr, $variant:ident) => {
            sequence.extend(scene.$field.iter().enumerate().map(|(index, p)| Drawn {
                order: p.order,
                family: $family,
                index,
                primitive: Primitive::$variant(p.clone()),
            }))
        };
    }
    gather!(shadows, 0, Shadow);
    gather!(visual_effects, 1, VisualEffect);
    gather!(quads, 2, Quad);
    gather!(html_islands, 3, HtmlIsland);
    gather!(html_elements, 4, HtmlElement);
    gather!(screen_effect_regions, 5, ScreenEffectRegion);
    gather!(paths, 6, Path);
    gather!(underlines, 7, Underline);
    gather!(monochrome_sprites, 8, MonochromeSprite);
    gather!(subpixel_sprites, 9, SubpixelSprite);
    gather!(polychrome_sprites, 10, PolychromeSprite);
    gather!(surfaces, 11, Surface);
    sequence.sort_by_key(|drawn| (drawn.order, drawn.family, drawn.index));
    sequence
}

/// An atlas tile without where the atlas put it, which two windows painting
/// the same thing may place differently.
fn neutral(tile: AtlasTile) -> AtlasTile {
    AtlasTile {
        texture_id: AtlasTextureId {
            index: 0,
            kind: tile.texture_id.kind,
        },
        tile_id: crate::TileId(0),
        padding: tile.padding,
        bounds: Bounds::<DevicePixels>::default(),
    }
}

/// A primitive as text: every field but its draw order, which the caller
/// ranks, and its atlas placement.
pub(crate) fn describe_primitive(primitive: &Primitive) -> String {
    match primitive {
        Primitive::Shadow(p) => format!("{:?}", crate::Shadow { order: 0, ..*p }),
        Primitive::VisualEffect(p) => format!(
            "{:?}",
            crate::VisualEffectPrimitive {
                order: 0,
                ..p.clone()
            }
        ),
        Primitive::Quad(p) => format!(
            "{:?}",
            crate::Quad {
                order: 0,
                ..p.clone()
            }
        ),
        Primitive::HtmlIsland(p) => format!(
            "{:?}",
            crate::HtmlIslandPrimitive {
                order: 0,
                ..p.clone()
            }
        ),
        Primitive::HtmlElement(p) => format!(
            "{:?}",
            crate::HtmlElementPrimitive {
                order: 0,
                ..p.clone()
            }
        ),
        Primitive::ScreenEffectRegion(p) => format!(
            "{:?}",
            crate::ScreenEffectRegionPrimitive {
                order: 0,
                ..p.clone()
            }
        ),
        Primitive::Path(p) => {
            let mut path = p.clone();
            path.order = 0;
            path.id = PathId(0);
            format!("{path:?}")
        }
        Primitive::Underline(p) => format!("{:?}", crate::Underline { order: 0, ..*p }),
        Primitive::MonochromeSprite(p) => format!(
            "{:?}",
            crate::MonochromeSprite {
                order: 0,
                tile: neutral(p.tile),
                ..*p
            }
        ),
        Primitive::SubpixelSprite(p) => format!(
            "{:?}",
            crate::SubpixelSprite {
                order: 0,
                tile: neutral(p.tile),
                ..*p
            }
        ),
        Primitive::PolychromeSprite(p) => format!(
            "{:?}",
            crate::PolychromeSprite {
                order: 0,
                tile: neutral(p.tile),
                ..*p
            }
        ),
        Primitive::Surface(p) => format!("surface {:?} {:?}", p.bounds, p.content_mask),
    }
}

/// The scene's primitives in drawing order, each prefixed by the rank of its
/// draw order among the scene's distinct draw orders.
fn describe_scene(scene: &crate::Scene, lines: &mut Vec<String>) {
    // A scene compositing scroll layers is described as the scene painting
    // their content straight into the frame would have left.
    let expanded;
    let scene = if scene.layers.frames.is_empty() {
        scene
    } else {
        expanded = crate::fast::layers::paint::expanded_scene(scene);
        &expanded
    };
    lines.push(format!("scene scale_factor={:?}", scene.scale_factor));
    let sequence = drawing_sequence(scene);
    let mut rank = 0;
    let mut previous = None;
    for drawn in &sequence {
        if previous.is_some_and(|previous| previous != drawn.order) {
            rank += 1;
        }
        previous = Some(drawn.order);
        lines.push(format!(
            "primitive {rank} {}",
            describe_primitive(&drawn.primitive)
        ));
    }
    for operation in &scene.paint_operations {
        match operation {
            PaintOperation::Primitive(_)
            | PaintOperation::FastDropped(_)
            | PaintOperation::FastClip(_) => {}
            PaintOperation::StartLayer(bounds) => lines.push(format!("layer {bounds:?}")),
            PaintOperation::StartPositionedLayer(bounds) => {
                lines.push(format!("positioned layer {bounds:?}"))
            }
            PaintOperation::StartTopLayer(bounds) => lines.push(format!("top layer {bounds:?}")),
            PaintOperation::EndLayer => lines.push("end layer".into()),
        }
    }
    for effect in &scene.active_screen_effects {
        lines.push(format!("screen effect {effect:?}"));
    }
    for snapshot in &scene.active_screen_effect_snapshots {
        lines.push(format!("screen effect snapshot {snapshot:?}"));
    }
}

/// The indices `0..len` of the records of one kind a frame holds, as a frame
/// drawn without list layers would hold them: in the frame's order, but with
/// the records of each list's rows (`lists`, each row's range of the kind
/// given by `range_of`) gathered where the first of them lies, row after
/// row, the row a `list` draws off screen last, and those of the rows the
/// list does not show left out. See [`crate::fast::layers::lists::ToldRow`].
///
/// A record of a row the list does not show is left out only while it
/// answers nothing, which `answers` tells of the record at an index: one
/// that can still be hit, walked to or called is told, after the rows shown,
/// and the frame differs from the one drawn without the layer.
///
/// With no list shown from a layer this is `0..len`.
fn told_order(
    len: usize,
    lists: &[&[ToldRow]],
    range_of: impl Fn(&ToldRow) -> Option<std::ops::Range<usize>>,
    answers: impl Fn(&ToldRow, usize) -> bool,
) -> Vec<usize> {
    if lists.is_empty() {
        return (0..len).collect();
    }
    // Which list's rows each index belongs to, if any.
    let mut owner: Vec<Option<usize>> = vec![None; len];
    for (list, rows) in lists.iter().enumerate() {
        for row in rows.iter() {
            for index in range_of(row).into_iter().flatten() {
                if let Some(owner) = owner.get_mut(index) {
                    *owner = Some(list);
                }
            }
        }
    }
    let mut order = Vec::with_capacity(len);
    let mut gathered = vec![false; lists.len()];
    for (index, owner) in owner.iter().enumerate() {
        let Some(list) = *owner else {
            order.push(index);
            continue;
        };
        if std::mem::replace(&mut gathered[list], true) {
            continue;
        }
        let mut rows: Vec<&ToldRow> = lists[list].iter().collect();
        rows.sort_by_key(|row| (!row.shown, row.loose, row.row));
        for row in rows {
            order.extend(
                range_of(row)
                    .into_iter()
                    .flatten()
                    .filter(|at| *at < len)
                    .filter(|at| row.shown || answers(row, *at)),
            );
        }
    }
    order
}

use crate::fast::layers::lists::ToldRow;

/// What the last frame `window` drew shows, where it can be hit and how it
/// dispatches input, one line per fact, in an order two windows drawing the
/// same frame share. Hitboxes are listed in hit-test order, and every
/// reference to a hitbox names its place in that order rather than its id,
/// which a replayed view keeps from the frame it was recorded in.
///
/// A frame shown from a list's layer is described as the frame drawing the
/// rows its list shows, row after row, would have left it (see
/// [`told_order`]), as a scene compositing layers is described by their
/// content.
pub(crate) fn describe_frame(window: &Window) -> Vec<String> {
    describe_frame_hit_at(window, &[])
}

/// [`describe_frame`], and which hitboxes a pointer at each of `points`
/// hits, topmost first, and how many of them take the hover.
pub(crate) fn describe_frame_hit_at(window: &Window, points: &[Point<Pixels>]) -> Vec<String> {
    let frame = &window.rendered_frame;
    let mut lines = Vec::new();
    describe_scene(&frame.scene, &mut lines);

    let lists = crate::fast::layers::lists::told_rows(window);
    let prepaint = |index: fn(&crate::PrepaintStateIndex) -> usize| {
        move |row: &ToldRow| Some(index(&row.prepaint.start)..index(&row.prepaint.end))
    };
    let paint = |index: fn(&crate::PaintIndex) -> usize| {
        move |row: &ToldRow| {
            row.paint
                .as_ref()
                .map(|paint| index(&paint.start)..index(&paint.end))
        }
    };

    // Hitboxes in the order a point under all of them tests them: by layer,
    // topmost first, then the latest inserted first.
    // A hitbox clipped to nothing is hit nowhere.
    let hit = |hitbox: &crate::Hitbox| {
        !hitbox
            .bounds
            .intersect(&hitbox.content_mask.bounds)
            .is_empty()
    };
    let live: FxHashSet<HitboxId> = frame
        .hitboxes
        .iter()
        .filter(|hitbox| hit(hitbox))
        .map(|hitbox| hitbox.id)
        .collect();
    let mut hitboxes: Vec<(usize, &crate::Hitbox)> = told_order(
        frame.hitboxes.len(),
        &lists,
        prepaint(|index| index.hitboxes_index),
        |_, index| hit(&frame.hitboxes[index]),
    )
    .into_iter()
    .map(|index| &frame.hitboxes[index])
    .enumerate()
    .collect();
    hitboxes.sort_by_key(|(index, hitbox)| {
        (
            std::cmp::Reverse(hitbox.id.layer()),
            std::cmp::Reverse(*index),
        )
    });
    let hit_order: FxHashMap<HitboxId, usize> = hitboxes
        .iter()
        .enumerate()
        .map(|(position, (_, hitbox))| (hitbox.id, position))
        .collect();
    let hitbox_name = |id: Option<HitboxId>| match id {
        Some(id) => match hit_order.get(&id) {
            Some(position) => format!("hitbox#{position}"),
            None => "hitbox#absent".to_string(),
        },
        None => "window".to_string(),
    };
    for (position, (_, hitbox)) in hitboxes.iter().enumerate() {
        lines.push(format!(
            "hitbox#{position} layer={} {:?} {:?} {:?}",
            hitbox.id.layer(),
            hitbox.bounds,
            hitbox.content_mask,
            hitbox.behavior
        ));
    }
    for point in points {
        let hit_test = frame.hit_test(*point);
        let hit: Vec<String> = hit_test
            .ids
            .iter()
            .map(|id| hitbox_name(Some(*id)))
            .collect();
        lines.push(format!(
            "hit at {point:?}: {} hovering {}",
            hit.join(" "),
            hit_test.hover_hitbox_count
        ));
    }
    for index in told_order(
        frame.window_control_hitboxes.len(),
        &lists,
        paint(|index| index.fast_window_control_hitboxes_index),
        |_, index| hit(&frame.window_control_hitboxes[index].1),
    ) {
        let (area, hitbox) = &frame.window_control_hitboxes[index];
        lines.push(format!(
            "window control {area:?} {:?} {:?} {:?}",
            hitbox.bounds, hitbox.content_mask, hitbox.behavior
        ));
    }
    for index in told_order(
        frame.cursor_styles.len(),
        &lists,
        paint(|index| index.cursor_styles_index),
        // A cursor asked for over a hitbox shows where the hitbox is hit.
        |_, index| {
            let hitbox = frame.cursor_styles[index].hitbox_id;
            hitbox.is_none_or(|id| live.contains(&id))
        },
    ) {
        let request = &frame.cursor_styles[index];
        lines.push(format!(
            "cursor {} {:?}",
            hitbox_name(request.hitbox_id),
            request.style
        ));
    }
    let tooltips = told_order(
        frame.tooltip_requests.len(),
        &lists,
        prepaint(|index| index.tooltips_index),
        // A tooltip shows over its element's hitbox, which is told.
        |_, _| false,
    );
    lines.push(format!(
        "tooltips {} slots {}",
        tooltips
            .iter()
            .filter(|index| frame.tooltip_requests[**index].is_some())
            .count(),
        tooltips.len()
    ));
    for index in told_order(
        frame.tab_stops.insertion_history.len(),
        &lists,
        paint(|index| index.tab_handle_index),
        // A stop the keyboard walks to is in the order at its own place.
        |_, index| match &frame.tab_stops.insertion_history[index] {
            crate::TabStopOperation::Insert(handle) => frame
                .tab_stops
                .by_id
                .get(&handle.id)
                .is_some_and(|node| node.node_insertion_index == index),
            _ => false,
        },
    ) {
        lines.push(match &frame.tab_stops.insertion_history[index] {
            crate::TabStopOperation::Insert(handle) => {
                format!("tab stop {:?} index={}", handle.id, handle.tab_index)
            }
            crate::TabStopOperation::Group(index) => format!("tab group {index}"),
            crate::TabStopOperation::GroupEnd => "tab group end".into(),
        });
    }
    // A listener's dispatch phase is an argument it is called with, not part of
    // what it registered for, so listeners are described by their event type.
    let listeners = told_order(
        frame.mouse_listeners.len(),
        &lists,
        paint(|index| index.mouse_listeners_index),
        // A listener is called while its row's are listening.
        |row, _| row.listens.as_ref().is_none_or(|listens| listens.get()),
    );
    lines.push(format!("mouse listeners {}", listeners.len()));
    let mut events: Vec<(&str, usize)> = Vec::new();
    #[cfg(any(test, feature = "test-support"))]
    for index in &listeners {
        let Some(event) = frame.fast_mouse_listener_events.get(*index) else {
            continue;
        };
        let event = event.rsplit("::").next().unwrap_or(event);
        match events.iter_mut().find(|(name, _)| *name == event) {
            Some((_, count)) => *count += 1,
            None => events.push((event, 1)),
        }
    }
    #[cfg(not(any(test, feature = "test-support")))]
    let _ = &listeners;
    for (event, count) in events {
        lines.push(format!("mouse listeners for {event}: {count}"));
    }
    lines.push(format!(
        "input handler slots {}",
        told_order(
            frame.input_handlers.len(),
            &lists,
            paint(|index| index.input_handlers_index),
            // Text input is handled wherever it is asked for.
            |_, _| true,
        )
        .len()
    ));

    // The dispatch nodes, numbered as they are told.
    let dispatch_tree = &frame.dispatch_tree;
    let nodes = told_order(
        dispatch_tree.nodes.len(),
        &lists,
        prepaint(|index| index.dispatch_tree_index),
        // Keys reach a node along the path from the focus, which a node
        // the tree does not find by its focus is not on.
        |_, index| {
            let focus = dispatch_tree.nodes[index].focus_id;
            focus.is_some_and(|focus| {
                frame.focus == Some(focus)
                    || dispatch_tree
                        .focusable_node_ids
                        .get(&focus)
                        .is_some_and(|node| node.as_usize() == index)
            })
        },
    );
    let mut told_node: FxHashMap<usize, usize> = FxHashMap::default();
    for (told, index) in nodes.iter().enumerate() {
        told_node.insert(*index, told);
    }
    let node_id = |id: crate::key_dispatch::DispatchNodeId| {
        told_node
            .get(&id.as_usize())
            .map(|told| crate::key_dispatch::DispatchNodeId::from_usize(*told))
    };
    for (told, index) in nodes.iter().enumerate() {
        let node = &dispatch_tree.nodes[*index];
        lines.push(format!(
            "dispatch {told} parent={:?} context={:?} focus={:?} view={:?} keys={} actions={} modifiers={}",
            node.parent.and_then(node_id),
            node.context,
            node.focus_id,
            node.view_id,
            node.key_listeners.len(),
            node.action_listeners.len(),
            node.modifiers_changed_listeners.len(),
        ));
    }
    lines.push(format!("focus {:?}", frame.focus));
    if let Some(focus) = frame.focus {
        lines.push(format!("focus path {:?}", dispatch_tree.focus_path(focus)));
    }
    // The states the frame keeps, as a set: the order elements reach them in
    // is not something the frame shows, and a view drawn from last frame
    // carries the states its layout used before those its prepaint did. A
    // cached view's own replay bookkeeping is the ordinary path's, which
    // view retention replaces, and view retention's released entries stand
    // for nothing. A row a list's layer holds off screen keeps its states
    // while a list without a layer has yet to draw it.
    let bookkeeping = [
        std::any::TypeId::of::<crate::view::ViewElementState>(),
        std::any::TypeId::of::<crate::fast::retained::ReleasedElementState>(),
    ];
    let states = |row: &ToldRow| {
        let prepaint = |range: &std::ops::Range<crate::PrepaintStateIndex>| {
            range.start.accessed_element_states_index..range.end.accessed_element_states_index
        };
        let paint = row.paint.as_ref().map_or(0..0, |paint| {
            paint.start.accessed_element_states_index..paint.end.accessed_element_states_index
        });
        [
            prepaint(&row.prepaint),
            row.layout.as_ref().map_or(0..0, prepaint),
            paint,
        ]
    };
    let mut hidden_states = vec![false; frame.accessed_element_states.len()];
    for row in lists
        .iter()
        .flat_map(|rows| rows.iter())
        .filter(|row| !row.shown)
    {
        for index in states(row).into_iter().flatten() {
            if let Some(hidden) = hidden_states.get_mut(index) {
                *hidden = true;
            }
        }
    }
    let mut element_states = frame
        .accessed_element_states
        .iter()
        .zip(&hidden_states)
        .filter(|((_, type_id), hidden)| !**hidden && !bookkeeping.contains(type_id))
        .map(|((id, type_id), _)| format!("element state {id} {type_id:?}"))
        .collect::<Vec<_>>();
    element_states.sort();
    element_states.dedup();
    lines.extend(element_states);
    // The shader-effect counts.
    lines.push(format!("effects {:?}", window.effect_diagnostics()));
    // The accessibility tree, as a frame drawn from scratch builds it.
    window.a11y.describe(&mut lines);
    lines
}

impl Window {
    /// What the last frame this window drew shows, where it can be hit and
    /// how it dispatches input, one line per fact, in an order that two
    /// windows drawing the same frame share: scene primitives by draw order
    /// with their atlas placement neutralized, then hitboxes in hit-test
    /// order, cursors, tooltips, tab stops, listeners, the dispatch tree and
    /// focus path, the element states the frame accessed, the effect
    /// diagnostics and the accessibility tree. Fast-mode tests compare a window drawing incrementally with
    /// one drawing from scratch by it.
    pub fn describe_rendered_frame(&self) -> Vec<String> {
        describe_frame(self)
    }
}

/// The first line at which `actual` and `expected` differ, with a few lines
/// of each around it, for a failure message; `None` when they are equal.
pub fn first_frame_difference(actual: &[String], expected: &[String]) -> Option<String> {
    if actual == expected {
        return None;
    }
    let first = actual
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected)
        .unwrap_or(actual.len().min(expected.len()));
    let excerpt = |lines: &[String]| {
        lines
            .iter()
            .enumerate()
            .skip(first.saturating_sub(2))
            .take(5)
            .map(|(index, line)| format!("  {index}: {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some(format!(
        "frames differ at line {first} ({} lines against {})\nactual:\n{}\nexpected:\n{}",
        actual.len(),
        expected.len(),
        excerpt(actual),
        excerpt(expected)
    ))
}

/// Verify mode: after a frame that drew views from last frame, the frame
/// drawn from scratch they stand for is drawn too, without being presented,
/// and the first view whose part of the two differs is reported, through
/// the log and the window's mismatch callback.
///
/// The shadow frame is drawn before the retained one, from the same state,
/// into the frame that is about to be drawn, with view retention recording
/// every view and reusing none; what it leaves (element states taken, a
/// fresh layout) is put back before the retained frame draws. A view is
/// rendered twice a frame while verifying, so a render with side effects
/// has them twice. While accessibility is active the shadow frame builds its
/// own tree, which is compared with the retained frame's and never sent to
/// the platform. Verifying stops on frames nothing is drawn from last frame in.
#[cfg(feature = "diagnostics")]
pub(crate) mod retention {
    use std::{mem, rc::Rc};

    use collections::FxHashMap;

    use crate::{App, EntityId, GlobalElementId, Window, fast::retained::RetainedSubtree};

    /// A window's verify mode.
    pub(crate) struct RetentionVerify {
        enabled: bool,
        on_mismatch: Option<Rc<dyn Fn(&RetentionMismatch)>>,
        /// The type of each view rendered while verifying, to name a view
        /// that differs by.
        view_types: FxHashMap<EntityId, &'static str>,
    }

    impl Default for RetentionVerify {
        fn default() -> Self {
            Self {
                enabled: std::env::var("GPUX_RETENTION_VERIFY").is_ok_and(|value| value == "1"),
                on_mismatch: None,
                view_types: FxHashMap::default(),
            }
        }
    }

    /// A view whose part of a frame differs from the frame drawn from
    /// scratch, as verify mode reports it.
    #[derive(Clone, Debug)]
    pub struct RetentionMismatch {
        /// The view's element id path, as `GlobalElementId` displays it, or
        /// `window` when the difference lies outside every view.
        pub global_id: String,
        /// The view's type.
        pub view_type: &'static str,
        /// The first difference, as `first_frame_difference` words it.
        pub difference: String,
    }

    /// What the shadow frame drawn from scratch showed.
    pub(crate) struct ShadowFrame {
        frame: Vec<String>,
        views: FxHashMap<GlobalElementId, Vec<String>>,
    }

    /// Notes the type of the view `entity` as it renders, for a report to
    /// name it by.
    #[inline]
    pub(crate) fn note_view_type(window: &mut Window, entity: EntityId, view_type: &'static str) {
        if window.fast_verify.enabled {
            window.fast_verify.view_types.insert(entity, view_type);
        }
    }

    /// Draws the frame `window` is about to draw from scratch, when verify
    /// mode is on and the frame may draw views from last frame, and returns
    /// what it showed. See the module documentation.
    pub(crate) fn draw_shadow(window: &mut Window, cx: &mut App) -> Option<ShadowFrame> {
        if !window.fast_verify.enabled
            || !window.fast_mode.view_retention()
            || window.refreshing
            || cx.has_active_drag()
            || cx.mode.skip_drawing()
        {
            return None;
        }
        let engine = window
            .layout_engine
            .replace(crate::TaffyLayoutEngine::new());
        let refreshing = mem::replace(&mut window.refreshing, true);
        crate::fast::layout::begin_frame(window);
        crate::fast::retained::begin_frame(window, cx);
        // The shadow builds its own accessibility tree, beside the real frame's.
        let shadow_a11y = window.a11y.shadow();
        let a11y = mem::replace(&mut window.a11y, shadow_a11y);
        // Rendering a view leases it; inside a recording that is not an
        // update the retained frame would see.
        let recording = cx.begin_recording_dependencies();
        window.draw_roots(cx);
        cx.finish_recording_dependencies(recording);
        window.a11y.end_shadow();
        cx.entities.clear_accessed();
        // As a frame drawn is before it is presented.
        window.next_frame.scene.finish();

        mem::swap(&mut window.rendered_frame, &mut window.next_frame);
        let shadow = ShadowFrame {
            frame: super::describe_frame(window),
            views: window
                .rendered_frame
                .fast_retained
                .records
                .iter()
                .map(|record| (record.id.clone(), describe_view(window, record)))
                .collect(),
        };
        mem::swap(&mut window.rendered_frame, &mut window.next_frame);
        window.a11y = a11y;

        // The element states the shadow took go back where the retained
        // frame finds them.
        let states = mem::take(&mut window.next_frame.element_states);
        window.rendered_frame.element_states.extend(states);
        window.next_frame.clear();
        window.layout_engine = engine;
        window.refreshing = refreshing;
        Some(shadow)
    }

    /// Compares the frame `window` just drew with the `shadow` drawn from
    /// scratch before it, and reports the first view that differs.
    pub(crate) fn check(window: &mut Window, shadow: Option<ShadowFrame>) {
        let Some(shadow) = shadow else {
            return;
        };
        let frame = super::describe_frame(window);
        let Some(difference) = super::first_frame_difference(&frame, &shadow.frame) else {
            return;
        };
        let records = &window.rendered_frame.fast_retained.records;
        let differs = |index: usize| {
            let record = &records[index];
            shadow
                .views
                .get(&record.id)
                .is_some_and(|expected| *expected != describe_view(window, record))
        };
        // The innermost view that differs: within the first that does, the
        // last nested one that does too.
        let mut found = (0..records.len()).find(|index| differs(*index));
        while let Some(index) = found {
            let nested = (index + 1..=index + records[index].nested)
                .rev()
                .find(|nested| differs(*nested));
            match nested {
                Some(nested) => found = Some(nested),
                None => break,
            }
        }
        let mismatch = match found {
            Some(index) => {
                let record = &records[index];
                let view_type = match record.id.last() {
                    Some(crate::ElementId::View(entity)) => window
                        .fast_verify
                        .view_types
                        .get(entity)
                        .copied()
                        .unwrap_or("an unnamed view"),
                    _ => "an unnamed view",
                };
                let expected = &shadow.views[&record.id];
                RetentionMismatch {
                    global_id: record.id.to_string(),
                    view_type,
                    difference: super::first_frame_difference(
                        &describe_view(window, record),
                        expected,
                    )
                    .unwrap_or(difference),
                }
            }
            None => RetentionMismatch {
                global_id: "window".into(),
                view_type: "",
                difference,
            },
        };
        log::error!(
            "view retention verify: {} {} differs from the frame drawn from scratch: {}",
            mismatch.view_type,
            mismatch.global_id,
            mismatch.difference
        );
        if let Some(on_mismatch) = window.fast_verify.on_mismatch.clone() {
            on_mismatch(&mismatch);
        }
    }

    /// The part of the rendered frame `record` stands for: what it painted
    /// and the hitboxes it inserted.
    fn describe_view(window: &Window, record: &RetainedSubtree) -> Vec<String> {
        let frame = &window.rendered_frame;
        let mut lines = Vec::new();
        let paint = &record.paint_range;
        if paint.start.scene_index <= paint.end.scene_index
            && paint.end.scene_index <= frame.scene.paint_operations.len()
        {
            for operation in
                &frame.scene.paint_operations[paint.start.scene_index..paint.end.scene_index]
            {
                lines.push(match operation {
                    crate::scene::PaintOperation::Primitive(primitive) => {
                        super::describe_primitive(primitive)
                    }
                    crate::scene::PaintOperation::FastDropped(dropped) => {
                        format!("dropped {dropped:?}")
                    }
                    crate::scene::PaintOperation::FastClip(clip) => format!("{clip:?}"),
                    crate::scene::PaintOperation::StartLayer(bounds) => format!("layer {bounds:?}"),
                    crate::scene::PaintOperation::StartPositionedLayer(bounds) => {
                        format!("positioned layer {bounds:?}")
                    }
                    crate::scene::PaintOperation::StartTopLayer(bounds) => {
                        format!("top layer {bounds:?}")
                    }
                    crate::scene::PaintOperation::EndLayer => "end layer".into(),
                });
            }
        }
        let prepaint = &record.prepaint_range;
        for hitbox in &frame.hitboxes[prepaint.start.hitboxes_index..prepaint.end.hitboxes_index] {
            lines.push(format!(
                "hitbox {:?} {:?} {:?}",
                hitbox.bounds, hitbox.content_mask, hitbox.behavior
            ));
        }
        lines
    }

    impl Window {
        /// Turns verify mode on or off: while it is on, a frame that draws
        /// views from last frame is checked against the frame drawn from
        /// scratch, and the first view that differs is reported through the
        /// log and [`Window::on_retention_mismatch`]. Presented frames are
        /// unaffected. `GPUX_RETENTION_VERIFY=1` turns it on for every
        /// window.
        pub fn set_retention_verify(&mut self, enabled: bool) {
            self.fast_verify.enabled = enabled;
            if !enabled {
                self.fast_verify.view_types.clear();
            }
        }

        /// Whether verify mode is on. See [`Window::set_retention_verify`].
        pub fn retention_verify(&self) -> bool {
            self.fast_verify.enabled
        }

        /// Calls `callback` with each view verify mode finds differing from
        /// the frame drawn from scratch.
        pub fn on_retention_mismatch(&mut self, callback: impl Fn(&RetentionMismatch) + 'static) {
            self.fast_verify.on_mismatch = Some(Rc::new(callback));
        }
    }
}
