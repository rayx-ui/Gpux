//! Checking scroll layers against drawing from scratch: a scene's composited
//! tiles expanded into the content they were rasterized from, and what the
//! scene draws as text two scenes drawing the same pixels share. Used by the
//! layers oracle (`fast/tests/oracle_layers.rs`) through
//! [`Window::painted_primitives`].
//!
//! Adapted from gpui-fast's `fast/layers/verify.rs`. A primitive's text and
//! the drawing sequence are gpux's own (`fast::verify`), which describe every
//! gpux scene family; the canonical overlap-depth order is gpui-fast's, since
//! a composited layer and the content painted straight into the frame number
//! their draw orders differently.

use std::collections::HashMap;

use crate::{
    Bounds, LayerFrame, LayerKey, Point, ScaledPixels, Scene, TileCoord, Window, decode_layer_tile,
    point,
    scene::{PaintOperation, Primitive},
};

/// Every primitive `scene` draws, in the order the renderer draws them.
fn drawing_sequence(scene: &Scene) -> Vec<Primitive> {
    crate::fast::verify::drawing_sequence(scene)
        .into_iter()
        .map(|drawn| drawn.primitive)
        .collect()
}

fn clipped(primitive: &Primitive) -> Bounds<ScaledPixels> {
    primitive
        .bounds()
        .intersect(&primitive.content_mask().bounds)
}

/// `primitive` clipped to `mask` as well as to its own mask.
fn clip_to(primitive: &mut Primitive, mask: &Bounds<ScaledPixels>) {
    let content_mask = match primitive {
        Primitive::Shadow(p) => &mut p.content_mask,
        Primitive::VisualEffect(p) => &mut p.content_mask,
        Primitive::HtmlIsland(p) => &mut p.content_mask,
        Primitive::HtmlElement(p) => &mut p.content_mask,
        Primitive::ScreenEffectRegion(p) => &mut p.content_mask,
        Primitive::Quad(p) => &mut p.content_mask,
        Primitive::Path(p) => {
            for vertex in &mut p.vertices {
                vertex.content_mask.bounds = vertex.content_mask.bounds.intersect(mask);
            }
            &mut p.content_mask
        }
        Primitive::Underline(p) => &mut p.content_mask,
        Primitive::MonochromeSprite(p) => &mut p.content_mask,
        Primitive::SubpixelSprite(p) => &mut p.content_mask,
        Primitive::PolychromeSprite(p) => &mut p.content_mask,
        Primitive::Surface(p) => &mut p.content_mask,
    };
    content_mask.bounds = content_mask.bounds.intersect(mask);
}

/// The drawing sequence of `layer`'s content moved by `translation` and
/// clipped to `viewport`, as the renderer draws it when it composites the
/// layer's tiles. Checks that every piece of it lies under one of the tiles
/// in `composited`.
fn expand_layer(
    layer: &LayerFrame,
    translation: Point<ScaledPixels>,
    viewport: &Bounds<ScaledPixels>,
    composited: &[TileCoord],
) -> Vec<Primitive> {
    let mut scratch = Scene::default();
    for operation in layer.content.operations() {
        match operation {
            PaintOperation::Primitive(primitive) => {
                let mut moved =
                    crate::fast::layers::scene::translate_primitive(primitive, translation);
                clip_to(&mut moved, viewport);
                scratch.insert_primitive(moved);
            }
            PaintOperation::StartLayer(bounds) => scratch.push_layer(Bounds {
                origin: bounds.origin + translation,
                size: bounds.size,
            }),
            PaintOperation::StartPositionedLayer(bounds) => scratch.push_positioned_layer(Bounds {
                origin: bounds.origin + translation,
                size: bounds.size,
            }),
            PaintOperation::StartTopLayer(bounds) => scratch.push_top_layer(Bounds {
                origin: bounds.origin + translation,
                size: bounds.size,
            }),
            PaintOperation::EndLayer => scratch.pop_layer(),
        }
    }
    scratch.finish();
    let sequence = drawing_sequence(&scratch);
    let side = layer.tile_size as f32;
    let first = |at: ScaledPixels| (at.0 / side).floor() as i32;
    let last = |start: ScaledPixels, end: ScaledPixels| {
        ((end.0 / side).ceil() as i32 - 1).max(first(start))
    };
    for primitive in &sequence {
        let window = clipped(primitive);
        let start = point(
            window.origin.x - translation.x,
            window.origin.y - translation.y,
        );
        let end = point(start.x + window.size.width, start.y + window.size.height);
        for y in first(start.y)..=last(start.y, end.y) {
            for x in first(start.x)..=last(start.x, end.x) {
                assert!(
                    composited.contains(&TileCoord { x, y }),
                    "no tile composited at ({x}, {y}) of layer {:?}, under {}",
                    layer.key,
                    crate::fast::verify::describe_primitive(primitive)
                );
            }
        }
    }
    sequence
}

/// What `scene` draws, with every composited scroll layer's tiles replaced
/// by the content they were rasterized from, in drawing order.
fn expanded_sequence(scene: &Scene) -> Vec<Primitive> {
    let mut composited: HashMap<
        LayerKey,
        (Point<ScaledPixels>, Bounds<ScaledPixels>, Vec<TileCoord>),
    > = HashMap::new();
    let mut first_tiles: Vec<(usize, LayerKey)> = Vec::new();
    let mut sequence = Vec::new();
    for primitive in drawing_sequence(scene) {
        if let Primitive::PolychromeSprite(sprite) = &primitive
            && let Some((key, tile)) =
                decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id)
        {
            let layer = layer_frame(scene, key);
            let origin = layer.tile_bounds(tile).origin;
            let translation = point(
                sprite.bounds.origin.x - origin.x,
                sprite.bounds.origin.y - origin.y,
            );
            let viewport = sprite.content_mask.bounds;
            let entry = composited.entry(key).or_insert_with(|| {
                first_tiles.push((sequence.len(), key));
                (translation, viewport, Vec::new())
            });
            assert_eq!(
                (entry.0, entry.1),
                (translation, viewport),
                "tiles of layer {key:?} composited at different offsets or clips"
            );
            entry.2.push(tile);
            continue;
        }
        sequence.push(primitive);
    }
    for (at, key) in first_tiles.into_iter().rev() {
        let (translation, viewport, tiles) = &composited[&key];
        let content = expand_layer(layer_frame(scene, key), *translation, viewport, tiles);
        sequence.splice(at..at, content);
    }
    sequence
}

fn layer_frame(scene: &Scene, key: LayerKey) -> &LayerFrame {
    scene
        .layers
        .frames
        .iter()
        .find(|layer| layer.key == key)
        .unwrap_or_else(|| panic!("a tile of layer {key:?}, which the scene has no frame of"))
}

/// What `scene` draws, as text that two scenes drawing the same pixels share.
///
/// The drawing sequence, layers expanded, is put in a canonical order: each
/// primitive's depth is one more than the deepest primitive drawn before it
/// that it overlaps, and primitives are sorted by depth, then by text. Two
/// sequences that draw every overlapping pair in the same order get the same
/// depths, however they number their draw orders and whatever order they
/// draw primitives that do not overlap in.
pub(crate) fn drawn(scene: &Scene) -> Vec<String> {
    let sequence = expanded_sequence(scene);
    let clips: Vec<Bounds<ScaledPixels>> = sequence.iter().map(clipped).collect();
    let mut depths: Vec<u32> = Vec::with_capacity(sequence.len());
    for (ix, clip) in clips.iter().enumerate() {
        let mut depth = 0;
        for (earlier, earlier_clip) in clips[..ix].iter().enumerate() {
            if depths[earlier] + 1 > depth && clip.intersects(earlier_clip) {
                depth = depths[earlier] + 1;
            }
        }
        depths.push(depth);
    }
    let mut lines: Vec<(u32, String)> = depths
        .into_iter()
        .zip(sequence.iter().map(crate::fast::verify::describe_primitive))
        .collect();
    lines.sort();
    lines
        .into_iter()
        .map(|(depth, line)| format!("{depth} {line}"))
        .collect()
}

impl Window {
    /// What the last frame drew, one line per primitive, each composited
    /// scroll layer's tiles replaced by the content they were rasterized
    /// from, in a canonical drawing order that does not depend on draw order
    /// numbers or atlas placement. Two windows drawing the same pixels
    /// return the same lines, with or without scroll layers.
    ///
    /// Panics if the scene composites a layer's content without the tile it
    /// lies in.
    pub fn painted_primitives(&self) -> Vec<String> {
        drawn(&self.rendered_frame.scene)
    }
}
