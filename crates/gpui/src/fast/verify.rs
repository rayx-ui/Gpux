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

/// `sequence` with every composited scroll-layer tile replaced by the content
/// it was rasterized from. Scroll layers fill this in when they land; until
/// then a scene holds no tiles.
fn expand_layers(_scene: &crate::Scene, sequence: Vec<Drawn>) -> Vec<Drawn> {
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
    lines.push(format!("scene scale_factor={:?}", scene.scale_factor));
    let sequence = expand_layers(scene, drawing_sequence(scene));
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
    for (id, type_id) in &frame.accessed_element_states {
        lines.push(format!("element state {id} {type_id:?}"));
    }
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
