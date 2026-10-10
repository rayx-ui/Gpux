//! Turning a rendered frame into text two frames can be compared by: what the
//! oracle and the fast-mode parity tests check every mechanism against.
//!
//! Adapted from gpui-fast's `fast/layout_key.rs` (`describe_rendered_frame`)
//! and `fast/layers/verify.rs` (`drawing_sequence`, `describe`). gpux
//! describes far more of the frame than gpui-fast does: every scene family
//! (its own included), the screen-effect requests, hitboxes in hit-test
//! order, cursors, tooltips, tab stops, mouse listeners, input handlers, the
//! dispatch tree and focus path, the element states the frame accessed and
//! the DevKit node tree. It compares draw orders by rank rather than by
//! gpui-fast's overlap depth, because gpux replays cached views by inserting
//! their primitives again and both windows number draw orders alike.

use crate::{
    AtlasTextureId, AtlasTile, Bounds, DevicePixels, HitboxId, PathId, Window,
    scene::{PaintOperation, Primitive},
};
use collections::FxHashMap;

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
            PaintOperation::Primitive(_) => {}
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

/// What the last frame `window` drew shows, where it can be hit and how it
/// dispatches input, one line per fact, in an order two windows drawing the
/// same frame share. Hitboxes are listed in hit-test order, and every
/// reference to a hitbox names its place in that order rather than its id,
/// which a replayed view keeps from the frame it was recorded in.
pub(crate) fn describe_frame(window: &Window) -> Vec<String> {
    let frame = &window.rendered_frame;
    let mut lines = Vec::new();
    describe_scene(&frame.scene, &mut lines);

    // Hitboxes in the order a point under all of them tests them: by layer,
    // topmost first, then the latest inserted first.
    let mut hitboxes: Vec<(usize, &crate::Hitbox)> = frame.hitboxes.iter().enumerate().collect();
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
    for (area, hitbox) in &frame.window_control_hitboxes {
        lines.push(format!(
            "window control {area:?} {:?} {:?} {:?}",
            hitbox.bounds, hitbox.content_mask, hitbox.behavior
        ));
    }
    for request in &frame.cursor_styles {
        lines.push(format!(
            "cursor {} {:?}",
            hitbox_name(request.hitbox_id),
            request.style
        ));
    }
    lines.push(format!(
        "tooltips {} slots {}",
        frame
            .tooltip_requests
            .iter()
            .filter(|request| request.is_some())
            .count(),
        frame.tooltip_requests.len()
    ));
    for operation in &frame.tab_stops.insertion_history {
        lines.push(match operation {
            crate::TabStopOperation::Insert(handle) => {
                format!("tab stop {:?} index={}", handle.id, handle.tab_index)
            }
            crate::TabStopOperation::Group(index) => format!("tab group {index}"),
            crate::TabStopOperation::GroupEnd => "tab group end".into(),
        });
    }
    // A listener's dispatch phase is an argument it is called with, not part of
    // what it registered for, so listeners are described by their event type.
    lines.push(format!("mouse listeners {}", frame.mouse_listeners.len()));
    let mut events: Vec<(&str, usize)> = Vec::new();
    #[cfg(any(test, feature = "test-support"))]
    for event in &frame.fast_mouse_listener_events {
        let event = event.rsplit("::").next().unwrap_or(event);
        match events.iter_mut().find(|(name, _)| *name == event) {
            Some((_, count)) => *count += 1,
            None => events.push((event, 1)),
        }
    }
    for (event, count) in events {
        lines.push(format!("mouse listeners for {event}: {count}"));
    }
    lines.push(format!(
        "input handler slots {}",
        frame.input_handlers.len()
    ));

    let dispatch_tree = &frame.dispatch_tree;
    for (index, node) in dispatch_tree.nodes.iter().enumerate() {
        lines.push(format!(
            "dispatch {index} parent={:?} context={:?} focus={:?} view={:?} keys={} actions={} modifiers={}",
            node.parent,
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
    // for nothing.
    let bookkeeping = [
        std::any::TypeId::of::<crate::view::ViewElementState>(),
        std::any::TypeId::of::<crate::fast::retained::ReleasedElementState>(),
    ];
    let mut element_states = frame
        .accessed_element_states
        .iter()
        .filter(|(_, type_id)| !bookkeeping.contains(type_id))
        .map(|(id, type_id)| format!("element state {id} {type_id:?}"))
        .collect::<Vec<_>>();
    element_states.sort();
    element_states.dedup();
    lines.extend(element_states);
    let devtools = window.devtools_frame_snapshot();
    lines.push(format!(
        "devtools viewport {:?} effects {:?}",
        devtools.viewport, devtools.effects
    ));
    for element in &devtools.elements {
        lines.push(format!("devtools {element:?}"));
    }
    lines
}

impl Window {
    /// What the last frame this window drew shows, where it can be hit and
    /// how it dispatches input, one line per fact, in an order that two
    /// windows drawing the same frame share: scene primitives by draw order
    /// with their atlas placement neutralized, then hitboxes in hit-test
    /// order, cursors, tooltips, tab stops, listeners, the dispatch tree and
    /// focus path, the element states the frame accessed, and the DevKit
    /// node tree. Fast-mode tests compare a window drawing incrementally with
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
/// has them twice. Verifying stops while AccessKit is active, which a frame
/// reports to, and on frames nothing is drawn from last frame in.
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
            || window.a11y.is_active()
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
        // Rendering a view leases it; inside a recording that is not an
        // update the retained frame would see.
        let recording = cx.begin_recording_dependencies();
        window.draw_roots(cx);
        cx.finish_recording_dependencies(recording);
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
        // What the DevKit shows of it, its text among it, without the ids a
        // frame numbers its nodes by.
        for node in &frame.devtools_nodes
            [prepaint.start.devtools_nodes_index..prepaint.end.devtools_nodes_index]
        {
            lines.push(format!(
                "devtools {:?}",
                crate::DevtoolsElementSnapshot {
                    devtools_node_id: 0,
                    parent_devtools_node_id: None,
                    dispatch_node_id: 0,
                    parent_dispatch_node_id: None,
                    ..node.snapshot()
                }
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
