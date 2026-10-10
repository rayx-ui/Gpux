//! What the core hands a renderer about scroll layers: the tiles it
//! composites, as polychrome sprites whose texture id lies in a range no
//! atlas allocates, and the content those tiles are rasterized from.
//!
//! Adapted from gpui-fast's `fast/layers/scene.rs`. Every gpux primitive
//! carries a transformation, so moving one into content or tile space moves
//! its transformation with it; gpux's own primitives (visual effects, HTML
//! islands and elements, screen-effect regions) and its positioned and top
//! layers never reach a layer's content (the container bypasses), and are
//! moved like the rest only so the functions here stay total.

use crate::{
    AtlasTextureId, AtlasTextureKind, Bounds, PaintColor, Point, ScaledPixels, Scene, TileId,
    point,
    scene::{PaintOperation, Primitive, TransformationMatrix},
    size,
};
use std::rc::Rc;

/// A live scroll layer, stable while it lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerKey(pub u32);

/// A tile of a layer's content space, in units of the tile size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileCoord {
    /// The tile's column; tile `x` starts at `x * tile_size` device pixels.
    pub x: i32,
    /// The tile's row; tile `y` starts at `y * tile_size` device pixels.
    pub y: i32,
}

/// The first texture index of layer tiles; atlases allocate indices from 0.
pub const LAYER_TILE_TEXTURE_BASE: u32 = 0xF000_0000;
pub(crate) const LAYER_KEY_LIMIT: u32 = 0x0100_0000;

/// What [`crate::PolychromeSprite::pad`] holds on the sprite of a layer tile:
/// the tile's texture holds premultiplied colour, as everything drawn into a
/// render target does, where an atlas texture holds straight colour. `0` on
/// every other sprite.
pub const LAYER_TILE_PREMULTIPLIED: u32 = 1;

/// The tile textures a window's layers may use, in bytes: 64 MB on desktop,
/// 32 MB in the browser and on mobile, where GPU memory is tighter and
/// shared with the page or the system. The core keeps a layer only while
/// the tiles over its viewport fit in it, and a renderer keeps no more.
pub const LAYER_TILE_BUDGET_BYTES: u64 = if cfg!(any(
    target_family = "wasm",
    target_os = "android",
    target_os = "ios"
)) {
    32 << 20
} else {
    64 << 20
};
const TILE_COORD_BIAS: i32 = 2048;

/// The texture id the tiles of `layer` are composited with.
pub fn layer_tile_texture_id(layer: LayerKey) -> AtlasTextureId {
    debug_assert!(layer.0 < LAYER_KEY_LIMIT);
    AtlasTextureId {
        index: LAYER_TILE_TEXTURE_BASE + layer.0,
        kind: AtlasTextureKind::Polychrome,
    }
}

/// The tile id a composited `tile` carries.
pub fn layer_tile_id(tile: TileCoord) -> TileId {
    debug_assert!((-TILE_COORD_BIAS..TILE_COORD_BIAS).contains(&tile.x));
    debug_assert!((-TILE_COORD_BIAS..TILE_COORD_BIAS).contains(&tile.y));
    let x = (tile.x + TILE_COORD_BIAS) as u32;
    let y = (tile.y + TILE_COORD_BIAS) as u32;
    TileId((x << 16) | y)
}

/// The layer and tile a composited sprite stands for, if it is one.
pub fn decode_layer_tile(texture: AtlasTextureId, tile: TileId) -> Option<(LayerKey, TileCoord)> {
    if texture.kind != AtlasTextureKind::Polychrome
        || texture.index < LAYER_TILE_TEXTURE_BASE
        || texture.index - LAYER_TILE_TEXTURE_BASE >= LAYER_KEY_LIMIT
    {
        return None;
    }
    let layer = LayerKey(texture.index - LAYER_TILE_TEXTURE_BASE);
    let x = (tile.0 >> 16) as i32 - TILE_COORD_BIAS;
    let y = (tile.0 & 0xFFFF) as i32 - TILE_COORD_BIAS;
    Some((layer, TileCoord { x, y }))
}

/// The scroll layers a frame composites, for the renderer to rasterize their
/// tiles from.
#[derive(Default)]
pub struct SceneLayers {
    /// One per layer the frame composites.
    pub frames: Vec<LayerFrame>,
}

impl SceneLayers {
    /// Forgets the frame's layers, as [`Scene::clear`] forgets its primitives.
    pub fn clear(&mut self) {
        self.frames.clear();
    }
}

/// A layer the frame composites: the content its tiles are rasterized from.
#[derive(Clone)]
pub struct LayerFrame {
    /// The layer, stable while it lives.
    pub key: LayerKey,
    /// Bumps whenever `content` is replaced.
    pub generation: u64,
    /// The opaque colour every tile is cleared with before its content is
    /// drawn, premultiplied encoded sRGB as every gpux paint colour is.
    pub background: PaintColor,
    /// The side of a tile, in device pixels.
    pub tile_size: u32,
    /// The painted content in content space.
    pub content: LayerContent,
    /// The tiles whose content changed in this generation.
    pub dirty_tiles: Vec<TileCoord>,
}

impl LayerFrame {
    /// The rectangle `tile` covers in content space.
    pub fn tile_bounds(&self, tile: TileCoord) -> Bounds<ScaledPixels> {
        let side = self.tile_size as f32;
        Bounds {
            origin: point(
                ScaledPixels(tile.x as f32 * side),
                ScaledPixels(tile.y as f32 * side),
            ),
            size: size(ScaledPixels(side), ScaledPixels(side)),
        }
    }

    /// The primitives of `content` that are visible over `tile`, translated
    /// into the tile's space (its top-left corner at the origin), in drawing
    /// order, finished and ready to batch.
    ///
    /// Each is drawn at the order it has in the content as a whole
    /// ([`LayerContent::orders`]), not at one worked out among the
    /// primitives over the tile: a shadow's blur falls on neighbours its box
    /// does not overlap, over those of a lower order and under the others,
    /// and an order worked out per tile changes at the tile's edge when what
    /// lies under the shadow's box does not reach into the tile.
    pub fn tile_scene(&self, tile: TileCoord) -> Scene {
        let bounds = self.tile_bounds(tile);
        let delta = point(
            ScaledPixels(-bounds.origin.x.0),
            ScaledPixels(-bounds.origin.y.0),
        );
        let orders = self.content.orders();
        let mut scene = Scene::default();
        let mut first = 0;
        for part in self.content.parts.iter() {
            let operations = &part.scene.paint_operations;
            let start = first;
            first += operations.len();
            if part.bounds.is_some_and(|part| !part.intersects(&bounds)) {
                continue;
            }
            for (index, operation) in operations.iter().enumerate() {
                let PaintOperation::Primitive(primitive) = operation else {
                    continue;
                };
                if visible_bounds(primitive).intersects(&bounds) {
                    let order = match &orders {
                        Some(orders) => orders[start + index],
                        None => crate::fast::edge_shadows::order_of(primitive),
                    };
                    crate::fast::edge_shadows::insert_at(
                        &mut scene,
                        translate_primitive(primitive, delta),
                        order,
                    );
                }
            }
        }
        scene.finish();
        scene
    }
}

/// A layer's content, in content space: one or more parts drawn one after
/// another. A scrolling `div`'s content is one part, a finished scene; a
/// virtual list's is one part per row, so that a frame adding or changing
/// rows hands the renderer the rows it kept as they were, without copying
/// them.
#[derive(Clone, Default)]
pub struct LayerContent {
    parts: Rc<[LayerPart]>,
    /// The order of each paint operation of every part in the content as a
    /// whole, once a tile asked for them. See [`LayerContent::orders`].
    orders: Rc<std::cell::OnceCell<Vec<crate::DrawOrder>>>,
}

/// A part of a layer's content.
#[derive(Clone)]
pub(crate) struct LayerPart {
    /// Where the part's primitives can draw, if known: a tile it misses
    /// draws nothing of it.
    pub(crate) bounds: Option<Bounds<ScaledPixels>>,
    /// The part's paint operations, in drawing order. A row's scene holds
    /// nothing else.
    pub(crate) scene: Rc<Scene>,
}

impl From<Scene> for LayerContent {
    fn from(scene: Scene) -> Self {
        Self::from(Rc::new(scene))
    }
}

impl From<Rc<Scene>> for LayerContent {
    fn from(scene: Rc<Scene>) -> Self {
        LayerContent {
            parts: Rc::new([LayerPart {
                bounds: None,
                scene,
            }]),
            orders: Rc::default(),
        }
    }
}

impl LayerContent {
    /// Content made of `parts`, drawn in order.
    pub(crate) fn from_parts(parts: impl IntoIterator<Item = LayerPart>) -> Self {
        LayerContent {
            parts: parts.into_iter().collect(),
            orders: Rc::default(),
        }
    }

    /// The draw order of each paint operation of every part, part after
    /// part, in the content as a whole; `None` for content of one part,
    /// whose scene holds them.
    ///
    /// A part is a scene of its own, a list's row, ordered when it was
    /// painted among whatever was painted with it. What the rows are to one
    /// another is worked out here, once for the content: the order each
    /// operation gets when all of them are painted one after another.
    fn orders(&self) -> Option<&[crate::DrawOrder]> {
        if self.parts.len() <= 1 {
            return None;
        }
        Some(self.orders.get_or_init(|| {
            let mut whole = Scene::default();
            for part in self.parts.iter() {
                let before = whole.paint_operations.len();
                whole.replay(0..part.scene.paint_operations.len(), &part.scene);
                // An operation replayed is an operation again.
                debug_assert_eq!(
                    whole.paint_operations.len() - before,
                    part.scene.paint_operations.len()
                );
            }
            whole
                .paint_operations
                .iter()
                .map(|operation| match operation {
                    PaintOperation::Primitive(primitive) => {
                        crate::fast::edge_shadows::order_of(primitive)
                    }
                    _ => 0,
                })
                .collect()
        }))
    }

    /// The content's scene, when it is one part, as a `div`'s is.
    pub fn scene(&self) -> Option<&Scene> {
        match &*self.parts {
            [part] => Some(&part.scene),
            _ => None,
        }
    }

    /// The scenes of the content's parts, in order.
    #[cfg(test)]
    pub(crate) fn part_scenes(&self) -> impl Iterator<Item = &Rc<Scene>> {
        self.parts.iter().map(|part| &part.scene)
    }

    /// Whether `self` and `other` are the same content, shared.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.parts, &other.parts)
    }

    /// Every paint operation of the content, in drawing order.
    pub(crate) fn operations(&self) -> impl Iterator<Item = &PaintOperation> {
        self.parts
            .iter()
            .flat_map(|part| part.scene.paint_operations.iter())
    }
}

fn translate_bounds(
    bounds: Bounds<ScaledPixels>,
    delta: Point<ScaledPixels>,
) -> Bounds<ScaledPixels> {
    Bounds {
        origin: bounds.origin + delta,
        size: bounds.size,
    }
}

/// The part of window (or content) space `primitive` can draw into: what
/// the renderer rasterizes for it, clipped to its content mask. A drop
/// shadow reaches three blur radii past its bounds and an inset one fills
/// its element's bounds (`vs_shadow`); every primitive's bounds are
/// transformed (`to_device_position_transformed`).
pub(crate) fn visible_bounds(primitive: &Primitive) -> Bounds<ScaledPixels> {
    drawn_bounds(primitive).intersect(&primitive.content_mask().bounds)
}

/// The part of window (or content) space `primitive` can draw into, its
/// content mask aside. See [`visible_bounds`].
pub(crate) fn drawn_bounds(primitive: &Primitive) -> Bounds<ScaledPixels> {
    match primitive {
        Primitive::Shadow(shadow) if shadow.inset != 0 => shadow
            .transformation
            .apply_to_scaled_bounds(shadow.element_bounds),
        Primitive::Shadow(shadow) => {
            let margin = ScaledPixels(3. * shadow.blur_radius.0.max(0.));
            shadow.transformation.apply_to_scaled_bounds(Bounds {
                origin: point(
                    shadow.bounds.origin.x - margin,
                    shadow.bounds.origin.y - margin,
                ),
                size: size(
                    shadow.bounds.size.width + margin * 2.,
                    shadow.bounds.size.height + margin * 2.,
                ),
            })
        }
        // `Primitive::bounds` applies the primitive's transformation.
        primitive => primitive.bounds(),
    }
}

/// `primitive` moved by `delta`: every position it carries, its content mask
/// included, so it draws the same pixels `delta` away.
pub(crate) fn translate_primitive(primitive: &Primitive, delta: Point<ScaledPixels>) -> Primitive {
    let mut primitive = primitive.clone();
    move_primitive(&mut primitive, delta);
    primitive
}

/// Moves `primitive` by `delta`, as [`translate_primitive`] does, in place.
pub(crate) fn move_primitive(primitive: &mut Primitive, delta: Point<ScaledPixels>) {
    let mv = |bounds: &mut Bounds<ScaledPixels>| *bounds = translate_bounds(*bounds, delta);
    match primitive {
        Primitive::Shadow(shadow) => {
            mv(&mut shadow.bounds);
            mv(&mut shadow.element_bounds);
            mv(&mut shadow.content_mask.bounds);
            translate_transformation(&mut shadow.transformation, delta);
        }
        Primitive::Quad(quad) => {
            mv(&mut quad.bounds);
            mv(&mut quad.content_mask.bounds);
            translate_transformation(&mut quad.transformation, delta);
        }
        Primitive::Path(path) => {
            mv(&mut path.bounds);
            mv(&mut path.content_mask.bounds);
            // Where the path was begun and last drawn to: compared with the
            // rest of it.
            path.start = path.start + delta;
            path.current = path.current + delta;
            for vertex in &mut path.vertices {
                vertex.xy_position = vertex.xy_position + delta;
                mv(&mut vertex.content_mask.bounds);
            }
        }
        Primitive::Underline(underline) => {
            mv(&mut underline.bounds);
            mv(&mut underline.content_mask.bounds);
            translate_transformation(&mut underline.transformation, delta);
        }
        Primitive::MonochromeSprite(sprite) => {
            mv(&mut sprite.bounds);
            mv(&mut sprite.content_mask.bounds);
            translate_transformation(&mut sprite.transformation, delta);
        }
        Primitive::SubpixelSprite(sprite) => {
            mv(&mut sprite.bounds);
            mv(&mut sprite.content_mask.bounds);
            translate_transformation(&mut sprite.transformation, delta);
        }
        Primitive::PolychromeSprite(sprite) => {
            mv(&mut sprite.bounds);
            mv(&mut sprite.content_mask.bounds);
            translate_transformation(&mut sprite.transformation, delta);
        }
        Primitive::Surface(surface) => {
            mv(&mut surface.bounds);
            mv(&mut surface.content_mask.bounds);
        }
        // What follows keeps a container on today's path and never reaches
        // a layer's content; it is moved in device pixels only, its logical
        // bounds left as they are.
        Primitive::VisualEffect(effect) => {
            mv(&mut effect.bounds);
            mv(&mut effect.content_mask.bounds);
            translate_transformation(&mut effect.transformation, delta);
        }
        Primitive::ScreenEffectRegion(region) => {
            mv(&mut region.bounds);
            mv(&mut region.content_mask.bounds);
            translate_transformation(&mut region.transformation, delta);
        }
        Primitive::HtmlIsland(island) => {
            mv(&mut island.bounds);
            mv(&mut island.content_mask.bounds);
        }
        Primitive::HtmlElement(element) => {
            mv(&mut element.bounds);
            mv(&mut element.content_mask.bounds);
        }
    }
}

/// A primitive's transformation applies to window positions (`R·p + t`, see
/// `to_device_position_transformed` in the shaders), so for the primitive to
/// move by `delta` with it, `t` becomes `t + (I − R)·delta`. The unit
/// transformation stays the unit.
pub(crate) fn translate_transformation(
    matrix: &mut TransformationMatrix,
    delta: Point<ScaledPixels>,
) {
    let r = matrix.rotation_scale;
    let (dx, dy) = (delta.x.0, delta.y.0);
    matrix.translation[0] += dx - (r[0][0] * dx + r[0][1] * dy);
    matrix.translation[1] += dy - (r[1][0] * dx + r[1][1] * dy);
}
