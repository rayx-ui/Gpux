// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ActiveScreenEffect, ActiveScreenEffectSnapshot, AtlasTextureId, AtlasTile, Bounds, ContentMask,
    Corners, Edges, ElementId, PaintColor, Pixels, Point, PreparedBackground,
    PreparedElementEffect, Radians, ScaledPixels, ScreenEffectRegionId, SharedString, Size,
    bounds_tree::BoundsTree, point,
};
use std::{
    fmt::Debug,
    iter::Peekable,
    ops::{Add, Range, Sub},
    slice,
};

#[allow(non_camel_case_types, unused)]
#[expect(missing_docs)]
pub type PathVertex_ScaledPixels = PathVertex<ScaledPixels>;

#[expect(missing_docs)]
pub type DrawOrder = u32;

const POSITIONED_DRAW_ORDER_BASE: DrawOrder = 1 << 30;
const TOP_LAYER_DRAW_ORDER_BASE: DrawOrder = 1 << 31;

#[derive(Default)]
#[expect(missing_docs)]
pub struct Scene {
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    positioned_primitive_bounds: BoundsTree<ScaledPixels>,
    top_layer_primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<LayerOrder>,
    pub scale_factor: f32,
    pub active_screen_effects: Vec<ActiveScreenEffect>,
    pub active_screen_effect_snapshots: Vec<ActiveScreenEffectSnapshot>,
    pub shadows: Vec<Shadow>,
    pub visual_effects: Vec<VisualEffectPrimitive>,
    pub quads: Vec<Quad>,
    pub html_islands: Vec<HtmlIslandPrimitive>,
    pub html_elements: Vec<HtmlElementPrimitive>,
    pub screen_effect_regions: Vec<ScreenEffectRegionPrimitive>,
    pub paths: Vec<Path<ScaledPixels>>,
    pub underlines: Vec<Underline>,
    pub monochrome_sprites: Vec<MonochromeSprite>,
    pub subpixel_sprites: Vec<SubpixelSprite>,
    pub polychrome_sprites: Vec<PolychromeSprite>,
    pub surfaces: Vec<PaintSurface>,
}

#[derive(Clone, Copy)]
enum LayerOrder {
    Fixed(DrawOrder),
    Positioned,
    Top,
}

#[expect(missing_docs)]
impl Scene {
    pub fn clear(&mut self) {
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.positioned_primitive_bounds.clear();
        self.top_layer_primitive_bounds.clear();
        self.layer_stack.clear();
        self.active_screen_effects.clear();
        self.active_screen_effect_snapshots.clear();
        self.paths.clear();
        self.shadows.clear();
        self.visual_effects.clear();
        self.quads.clear();
        self.html_islands.clear();
        self.html_elements.clear();
        self.screen_effect_regions.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.subpixel_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub fn screen_effect_region(
        &self,
        id: impl AsRef<str>,
    ) -> Option<&ScreenEffectRegionPrimitive> {
        let id = id.as_ref();
        self.screen_effect_regions
            .iter()
            .rev()
            .find(|region| region.id.as_ref() == id)
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let order = match self.layer_stack.last().copied() {
            Some(LayerOrder::Top) => {
                TOP_LAYER_DRAW_ORDER_BASE + self.top_layer_primitive_bounds.insert(bounds)
            }
            Some(LayerOrder::Positioned) => {
                POSITIONED_DRAW_ORDER_BASE + self.positioned_primitive_bounds.insert(bounds)
            }
            Some(LayerOrder::Fixed(_)) | None => self.primitive_bounds.insert(bounds),
        };
        self.layer_stack.push(LayerOrder::Fixed(order));
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    pub fn push_positioned_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        self.layer_stack.push(LayerOrder::Positioned);
        self.paint_operations
            .push(PaintOperation::StartPositionedLayer(bounds));
    }

    pub fn push_top_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        self.layer_stack.push(LayerOrder::Top);
        self.paint_operations
            .push(PaintOperation::StartTopLayer(bounds));
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let mut primitive = primitive.into();
        let clipped_bounds = primitive
            .bounds()
            .intersect(&primitive.content_mask().bounds);

        if clipped_bounds.is_empty() {
            return;
        }

        let order = match self.layer_stack.last().copied() {
            Some(LayerOrder::Fixed(order)) => order,
            Some(LayerOrder::Positioned) => {
                POSITIONED_DRAW_ORDER_BASE + self.positioned_primitive_bounds.insert(clipped_bounds)
            }
            Some(LayerOrder::Top) => {
                TOP_LAYER_DRAW_ORDER_BASE + self.top_layer_primitive_bounds.insert(clipped_bounds)
            }
            None => self.primitive_bounds.insert(clipped_bounds),
        };
        match &mut primitive {
            Primitive::Shadow(shadow) => {
                shadow.order = order;
                self.shadows.push(*shadow);
            }
            Primitive::VisualEffect(effect) => {
                effect.order = order;
                self.visual_effects.push(effect.clone());
            }
            Primitive::Quad(quad) => {
                quad.order = order;
                self.quads.push(quad.clone());
            }
            Primitive::HtmlIsland(html_island) => {
                html_island.order = order;
                self.html_islands.push(html_island.clone());
            }
            Primitive::HtmlElement(html_element) => {
                html_element.order = order;
                self.html_elements.push(html_element.clone());
            }
            Primitive::ScreenEffectRegion(screen_effect_region) => {
                screen_effect_region.order = order;
                self.screen_effect_regions
                    .push(screen_effect_region.clone());
            }
            Primitive::Path(path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
                self.paths.push(path.clone());
            }
            Primitive::Underline(underline) => {
                underline.order = order;
                self.underlines.push(*underline);
            }
            Primitive::MonochromeSprite(sprite) => {
                sprite.order = order;
                self.monochrome_sprites.push(*sprite);
            }
            Primitive::SubpixelSprite(sprite) => {
                sprite.order = order;
                self.subpixel_sprites.push(*sprite);
            }
            Primitive::PolychromeSprite(sprite) => {
                sprite.order = order;
                self.polychrome_sprites.push(*sprite);
            }
            Primitive::Surface(surface) => {
                surface.order = order;
                self.surfaces.push(surface.clone());
            }
        }
        self.paint_operations
            .push(PaintOperation::Primitive(primitive));
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::StartPositionedLayer(bounds) => self.push_positioned_layer(*bounds),
                PaintOperation::StartTopLayer(bounds) => self.push_top_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
            }
        }
    }

    pub fn finish(&mut self) {
        self.shadows.sort_by_key(|shadow| shadow.order);
        self.visual_effects
            .sort_by_key(|visual_effect| visual_effect.order);
        self.quads.sort_by_key(|quad| quad.order);
        self.html_islands
            .sort_by_key(|html_island| html_island.order);
        self.html_elements
            .sort_by_key(|html_element| html_element.order);
        self.screen_effect_regions
            .sort_by_key(|screen_effect_region| screen_effect_region.order);
        self.paths.sort_by_key(|path| path.order);
        self.underlines.sort_by_key(|underline| underline.order);
        self.monochrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.subpixel_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.polychrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.surfaces.sort_by_key(|surface| surface.order);
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub fn batches(&self) -> impl Iterator<Item = PrimitiveBatch> + '_ {
        BatchIterator {
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            visual_effects_start: 0,
            visual_effects_iter: self.visual_effects.iter().peekable(),
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            html_islands_start: 0,
            html_islands_iter: self.html_islands.iter().peekable(),
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            subpixel_sprites_start: 0,
            subpixel_sprites_iter: self.subpixel_sprites.iter().peekable(),
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    Shadow,
    VisualEffect,
    #[default]
    Quad,
    HtmlIsland,
    Path,
    Underline,
    MonochromeSprite,
    SubpixelSprite,
    PolychromeSprite,
    Surface,
}

pub(crate) enum PaintOperation {
    Primitive(Primitive),
    StartLayer(Bounds<ScaledPixels>),
    StartPositionedLayer(Bounds<ScaledPixels>),
    StartTopLayer(Bounds<ScaledPixels>),
    EndLayer,
}

#[derive(Clone)]
#[expect(missing_docs)]
pub enum Primitive {
    Shadow(Shadow),
    VisualEffect(VisualEffectPrimitive),
    Quad(Quad),
    HtmlIsland(HtmlIslandPrimitive),
    HtmlElement(HtmlElementPrimitive),
    ScreenEffectRegion(ScreenEffectRegionPrimitive),
    Path(Path<ScaledPixels>),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    SubpixelSprite(SubpixelSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
}

#[expect(missing_docs)]
impl Primitive {
    pub fn bounds(&self) -> Bounds<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => {
                shadow.transformation.apply_to_scaled_bounds(shadow.bounds)
            }
            Primitive::VisualEffect(effect) => {
                effect.transformation.apply_to_scaled_bounds(effect.bounds)
            }
            Primitive::Quad(quad) => quad.transformation.apply_to_scaled_bounds(quad.bounds),
            Primitive::HtmlIsland(html_island) => html_island.bounds,
            Primitive::HtmlElement(html_element) => html_element.bounds,
            Primitive::ScreenEffectRegion(screen_effect_region) => screen_effect_region
                .transformation
                .apply_to_scaled_bounds(screen_effect_region.bounds),
            Primitive::Path(path) => path.bounds,
            Primitive::Underline(underline) => underline
                .transformation
                .apply_to_scaled_bounds(underline.bounds),
            Primitive::MonochromeSprite(sprite) => {
                sprite.transformation.apply_to_scaled_bounds(sprite.bounds)
            }
            Primitive::SubpixelSprite(sprite) => {
                sprite.transformation.apply_to_scaled_bounds(sprite.bounds)
            }
            Primitive::PolychromeSprite(sprite) => {
                sprite.transformation.apply_to_scaled_bounds(sprite.bounds)
            }
            Primitive::Surface(surface) => surface.bounds,
        }
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::VisualEffect(effect) => &effect.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::HtmlIsland(html_island) => &html_island.content_mask,
            Primitive::HtmlElement(html_element) => &html_element.content_mask,
            Primitive::ScreenEffectRegion(screen_effect_region) => {
                &screen_effect_region.content_mask
            }
            Primitive::Path(path) => &path.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::SubpixelSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
        }
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    visual_effects_start: usize,
    visual_effects_iter: Peekable<slice::Iter<'a, VisualEffectPrimitive>>,
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    html_islands_start: usize,
    html_islands_iter: Peekable<slice::Iter<'a, HtmlIslandPrimitive>>,
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, Path<ScaledPixels>>>,
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    subpixel_sprites_start: usize,
    subpixel_sprites_iter: Peekable<slice::Iter<'a, SubpixelSprite>>,
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (
                self.visual_effects_iter.peek().map(|e| e.order),
                PrimitiveKind::VisualEffect,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (
                self.html_islands_iter.peek().map(|q| q.order),
                PrimitiveKind::HtmlIsland,
            ),
            (self.paths_iter.peek().map(|q| q.order), PrimitiveKind::Path),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.subpixel_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::SubpixelSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        match batch_kind {
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(shadows_start..shadows_end))
            }
            PrimitiveKind::VisualEffect => {
                let effects_start = self.visual_effects_start;
                let mut effects_end = effects_start + 1;
                self.visual_effects_iter.next();
                while self
                    .visual_effects_iter
                    .next_if(|effect| (effect.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    effects_end += 1;
                }
                self.visual_effects_start = effects_end;
                Some(PrimitiveBatch::VisualEffects(effects_start..effects_end))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(quads_start..quads_end))
            }
            PrimitiveKind::HtmlIsland => {
                let html_islands_start = self.html_islands_start;
                let mut html_islands_end = html_islands_start + 1;
                self.html_islands_iter.next();
                while self
                    .html_islands_iter
                    .next_if(|html_island| (html_island.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    html_islands_end += 1;
                }
                self.html_islands_start = html_islands_end;
                Some(PrimitiveBatch::HtmlIslands(
                    html_islands_start..html_islands_end,
                ))
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(paths_start..paths_end))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(underlines_start..underlines_end))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::SubpixelSprite => {
                let texture_id = self.subpixel_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.subpixel_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.subpixel_sprites_iter.next();
                while self
                    .subpixel_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.subpixel_sprites_start = sprites_end;
                Some(PrimitiveBatch::SubpixelSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(surfaces_start..surfaces_end))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
#[allow(missing_docs)]
pub enum PrimitiveBatch {
    Shadows(Range<usize>),
    VisualEffects(Range<usize>),
    Quads(Range<usize>),
    HtmlIslands(Range<usize>),
    Paths(Range<usize>),
    Underlines(Range<usize>),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SubpixelSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    Surfaces(Range<usize>),
}

/// Controls whether browser DOM or GPUI owns interaction for an HTML island.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlIslandInputMode {
    /// Browser DOM receives pointer, focus, keyboard, composition, and
    /// accessibility behavior first.
    DomOwned,
    /// DOM is rendered but GPUI keeps owning pointer and focus behavior.
    GpuiOwned,
}

/// Scene data for an experimental HTML island.
///
/// Supported WASM renderers use this data to sync a direct canvas-child DOM
/// root and draw its uploaded texture in scene order. Native and unsupported
/// web renderers skip the batch, so callers should provide explicit fallback
/// content through the public `html_island` element API.
#[derive(Clone, Debug)]
pub struct HtmlIslandPrimitive {
    /// Draw order assigned by the scene when the primitive is inserted.
    pub order: DrawOrder,
    /// Stable application-supplied island id used for DOM registry lookup.
    pub id: ElementId,
    /// Physical scene bounds in scaled pixels.
    pub bounds: Bounds<ScaledPixels>,
    /// Logical DOM bounds in CSS pixels for browser subtree synchronization.
    pub logical_bounds: Bounds<Pixels>,
    /// Rectangular content mask inherited from the GPUI paint context.
    pub content_mask: ContentMask<ScaledPixels>,
    /// Whether the browser DOM subtree or GPUI owns input for this island.
    pub input_mode: HtmlIslandInputMode,
    /// Cumulative GPUI element opacity to apply when compositing the island.
    pub opacity: f32,
    /// Monotonic content revision used to avoid redundant DOM updates.
    pub content_revision: u64,
    /// Trusted application HTML payload for the web DOM registry.
    pub trusted_html: SharedString,
}

impl From<HtmlIslandPrimitive> for Primitive {
    fn from(html_island: HtmlIslandPrimitive) -> Self {
        Primitive::HtmlIsland(html_island)
    }
}

/// Controls whether browser DOM or GPUI owns interaction for an HTML element overlay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlElementInputMode {
    /// Browser DOM receives pointer, focus, keyboard, composition, media, iframe,
    /// and accessibility behavior first.
    DomOwned,
    /// DOM remains visible but GPUI keeps owning pointer and focus behavior.
    GpuiOwned,
}

/// Controls what happens to browser DOM roots when an HTML element is absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlElementLifetime {
    /// Remove the DOM root when it is absent from a rendered frame.
    PruneWhenAbsent,
    /// Park the DOM root hidden/offscreen when absent so iframe browsing context can survive.
    KeepAliveHidden,
}

/// Controls how the browser DOM subtree is fit inside the GPUI layout box.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlElementContentFit {
    /// Do not add fitting styles to authored trusted HTML.
    Natural,
    /// Make top-level trusted HTML children fill the GPUI layout box.
    Fill,
}

/// Controls how wheel events over the browser DOM overlay interact with GPUI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlElementWheelBehavior {
    /// Browser DOM owns wheel behavior. Cross-origin iframes may consume wheel
    /// without scrolling GPUI parents.
    Browser,
    /// Forward wheel events that are observable in the parent document to GPUI.
    ///
    /// This works for same-document DOM content, but browser security prevents
    /// parent documents from observing wheel events that occur inside
    /// cross-origin iframe browsing contexts.
    ForwardIfObservable,
    /// Add a transparent capture layer above the DOM subtree and forward wheel
    /// events to GPUI.
    ///
    /// This works over iframes, including cross-origin iframes, but the capture
    /// layer also owns pointer hit testing while active.
    CaptureForward,
}

/// Scene data for a browser DOM overlay element.
///
/// Supported WASM renderers use this side metadata to sync real DOM overlay
/// roots. Native and non-web renderers do not draw this primitive.
#[derive(Clone, Debug)]
pub struct HtmlElementPrimitive {
    /// Draw order assigned by the scene when the primitive is inserted.
    pub order: DrawOrder,
    /// Stable application-supplied element id used for DOM registry lookup.
    pub id: ElementId,
    /// Physical scene bounds in scaled pixels.
    pub bounds: Bounds<ScaledPixels>,
    /// Logical DOM bounds in CSS pixels for browser overlay synchronization.
    pub logical_bounds: Bounds<Pixels>,
    /// Logical rectangular content mask in CSS pixels.
    pub logical_content_mask: ContentMask<Pixels>,
    /// Scaled rectangular content mask inherited from the GPUI paint context.
    pub content_mask: ContentMask<ScaledPixels>,
    /// Whether the browser DOM subtree or GPUI owns input for this overlay.
    pub input_mode: HtmlElementInputMode,
    /// Whether absent/offscreen DOM roots should be pruned or parked.
    pub lifetime: HtmlElementLifetime,
    /// DOM content fitting policy.
    pub content_fit: HtmlElementContentFit,
    /// Wheel forwarding policy.
    pub wheel_behavior: HtmlElementWheelBehavior,
    /// Cumulative GPUI element opacity to apply to the overlay root.
    pub opacity: f32,
    /// Monotonic content revision used to avoid redundant DOM updates.
    pub content_revision: u64,
    /// Trusted application HTML payload for the web DOM registry.
    pub trusted_html: SharedString,
    /// Additional trusted CSS declarations applied to the root clipping element.
    pub extra_styles: SharedString,
}

impl From<HtmlElementPrimitive> for Primitive {
    fn from(html_element: HtmlElementPrimitive) -> Self {
        Primitive::HtmlElement(html_element)
    }
}

#[derive(Default, Debug, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: PreparedBackground,
    pub border_color: PreparedBackground,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
    pub transformation: TransformationMatrix,
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: PaintColor,
    pub thickness: ScaledPixels,
    pub wavy: u32,
    pub transformation: TransformationMatrix,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: PaintColor,
    pub transformation: TransformationMatrix,
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

/// Scene data for shader-backed element effects.
///
/// WGPU renderers consume this batch after shadows and before quads so effects
/// can sample or decorate already-painted content without replacing the
/// element's normal background, children, or border primitives.
#[derive(Clone, Debug)]
pub struct VisualEffectPrimitive {
    /// Draw order assigned by the scene when the primitive is inserted.
    pub order: DrawOrder,
    /// Physical scene bounds in scaled pixels.
    pub bounds: Bounds<ScaledPixels>,
    /// Rectangular content mask inherited from the GPUI paint context.
    pub content_mask: ContentMask<ScaledPixels>,
    /// Rounded clipping shape for effect sampling and decoration.
    pub corner_radii: Corners<ScaledPixels>,
    /// Cumulative GPUI element opacity to apply when compositing effects.
    pub opacity: f32,
    /// Ordered element effects requested by the styled element.
    pub effects: Vec<PreparedElementEffect>,
    /// Element transform active when the primitive was emitted.
    pub transformation: TransformationMatrix,
}

impl From<VisualEffectPrimitive> for Primitive {
    fn from(effect: VisualEffectPrimitive) -> Self {
        Primitive::VisualEffect(effect)
    }
}

/// Scene metadata for a named region that can receive screen effects.
///
/// This primitive is replayable scene metadata and intentionally does not
/// produce a renderer batch.
#[derive(Clone, Debug)]
pub struct ScreenEffectRegionPrimitive {
    /// Draw order assigned by the scene when the primitive is inserted.
    pub order: DrawOrder,
    /// Stable application-supplied region id.
    pub id: ScreenEffectRegionId,
    /// Physical scene bounds in scaled pixels.
    pub bounds: Bounds<ScaledPixels>,
    /// Logical bounds in CSS pixels for browser diagnostics and target mapping.
    pub logical_bounds: Bounds<Pixels>,
    /// Rectangular content mask inherited from the GPUI paint context.
    pub content_mask: ContentMask<ScaledPixels>,
    /// Rounded clipping shape for region-limited screen effects.
    pub corner_radii: Corners<ScaledPixels>,
    /// Element transform active when the region was emitted.
    pub transformation: TransformationMatrix,
}

impl From<ScreenEffectRegionPrimitive> for Primitive {
    fn from(region: ScreenEffectRegionPrimitive) -> Self {
        Primitive::ScreenEffectRegion(region)
    }
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(mut self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Skew around the x axis by the given angle in radians.
    pub fn skew_x(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, angle.0.tan()], [0.0, 1.0]],
            translation: [0.0, 0.0],
        })
    }

    /// Skew around the y axis by the given angle in radians.
    pub fn skew_y(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [angle.0.tan(), 1.0]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }

    /// Apply transformation to a point in scaled pixels.
    pub fn apply_to_scaled_point(&self, point: Point<ScaledPixels>) -> Point<ScaledPixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(ScaledPixels(output[0]), ScaledPixels(output[1]))
    }

    /// Apply transformation to the bounds and return the enclosing axis-aligned bounds.
    pub fn apply_to_scaled_bounds(&self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        let top_left = self.apply_to_scaled_point(bounds.origin);
        let top_right = self.apply_to_scaled_point(bounds.top_right());
        let bottom_left = self.apply_to_scaled_point(bounds.bottom_left());
        let bottom_right = self.apply_to_scaled_point(bounds.bottom_right());

        let min_x = top_left
            .x
            .0
            .min(top_right.x.0)
            .min(bottom_left.x.0)
            .min(bottom_right.x.0);
        let max_x = top_left
            .x
            .0
            .max(top_right.x.0)
            .max(bottom_left.x.0)
            .max(bottom_right.x.0);
        let min_y = top_left
            .y
            .0
            .min(top_right.y.0)
            .min(bottom_left.y.0)
            .min(bottom_right.y.0);
        let max_y = top_left
            .y
            .0
            .max(top_right.y.0)
            .max(bottom_left.y.0)
            .max(bottom_right.y.0);

        Bounds::from_corners(
            point(ScaledPixels(min_x), ScaledPixels(min_y)),
            point(ScaledPixels(max_x), ScaledPixels(max_y)),
        )
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct MonochromeSprite {
    pub order: DrawOrder,
    pub kind: MonochromeSpriteKind,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: PaintColor,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
#[expect(missing_docs)]
pub enum MonochromeSpriteKind {
    Glyph = 0,
    ImageMask = 1,
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct SubpixelSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: PaintColor,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

impl From<SubpixelSprite> for Primitive {
    fn from(sprite: SubpixelSprite) -> Self {
        Primitive::SubpixelSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PolychromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub grayscale: bool,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(target_os = "macos")]
    pub image_buffer: core_video::pixel_buffer::CVPixelBuffer,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs)]
pub struct PathId(pub usize);

/// A line made up of a series of vertices and control points.
#[derive(Clone, Debug)]
#[expect(missing_docs)]
pub struct Path<P: Clone + Debug + Default + PartialEq> {
    pub id: PathId,
    pub order: DrawOrder,
    pub bounds: Bounds<P>,
    pub content_mask: ContentMask<P>,
    pub vertices: Vec<PathVertex<P>>,
    pub color: PreparedBackground,
    start: Point<P>,
    current: Point<P>,
    contour_count: usize,
}

impl Path<Pixels> {
    /// Create a new path with the given starting point.
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            id: PathId(0),
            order: DrawOrder::default(),
            vertices: Vec::new(),
            start,
            current: start,
            bounds: Bounds {
                origin: start,
                size: Default::default(),
            },
            content_mask: Default::default(),
            color: Default::default(),
            contour_count: 0,
        }
    }

    /// Scale this path by the given factor.
    pub fn scale(&self, factor: f32) -> Path<ScaledPixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: self.bounds.scale(factor),
            content_mask: self.content_mask.scale(factor),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.scale(factor))
                .collect(),
            start: self.start.map(|start| start.scale(factor)),
            current: self.current.scale(factor),
            contour_count: self.contour_count,
            color: self.color.clone(),
        }
    }

    /// Move the start, current point to the given point.
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        self.start = to;
        self.current = to;
    }

    /// Draw a straight line from the current point to the given point.
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }
        self.current = to;
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        self.push_triangle(
            (self.current, ctrl, to),
            (point(0., 0.), point(0.5, 0.), point(1., 1.)),
        );
        self.current = to;
    }

    /// Push a triangle to the Path.
    pub fn push_triangle(
        &mut self,
        xy: (Point<Pixels>, Point<Pixels>, Point<Pixels>),
        st: (Point<f32>, Point<f32>, Point<f32>),
    ) {
        self.bounds = self
            .bounds
            .union(&Bounds {
                origin: xy.0,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.1,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.2,
                size: Default::default(),
            });

        self.vertices.push(PathVertex {
            xy_position: xy.0,
            st_position: st.0,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.1,
            st_position: st.1,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.2,
            st_position: st.2,
            content_mask: Default::default(),
        });
    }
}

impl<T> Path<T>
where
    T: Clone + Debug + Default + PartialEq + PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[allow(unused)]
    #[expect(missing_docs)]
    pub fn clipped_bounds(&self) -> Bounds<T> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl Path<ScaledPixels> {
    /// Replace the path content mask on the path and its vertices.
    pub fn with_content_mask(&self, content_mask: ContentMask<ScaledPixels>) -> Self {
        Self {
            id: self.id,
            order: self.order,
            bounds: self.bounds,
            content_mask,
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.with_content_mask(content_mask))
                .collect(),
            color: self.color.clone(),
            start: self.start,
            current: self.current,
            contour_count: self.contour_count,
        }
    }

    /// Apply a transformation to the path and return the enclosing transformed path.
    pub fn transformed(
        &self,
        transformation: TransformationMatrix,
        content_mask: ContentMask<ScaledPixels>,
    ) -> Self {
        Self {
            id: self.id,
            order: self.order,
            bounds: transformation.apply_to_scaled_bounds(self.bounds),
            content_mask,
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.transformed(transformation, content_mask))
                .collect(),
            color: self.color.clone(),
            start: transformation.apply_to_scaled_point(self.start),
            current: transformation.apply_to_scaled_point(self.current),
            contour_count: self.contour_count,
        }
    }
}

impl From<Path<ScaledPixels>> for Primitive {
    fn from(path: Path<ScaledPixels>) -> Self {
        Primitive::Path(path)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PathVertex<P: Clone + Debug + Default + PartialEq> {
    pub xy_position: Point<P>,
    pub st_position: Point<f32>,
    pub content_mask: ContentMask<P>,
}

#[expect(missing_docs)]
impl PathVertex<Pixels> {
    pub fn scale(&self, factor: f32) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: self.xy_position.scale(factor),
            st_position: self.st_position,
            content_mask: self.content_mask.scale(factor),
        }
    }
}

impl PathVertex<ScaledPixels> {
    /// Replace the vertex content mask.
    pub fn with_content_mask(&self, content_mask: ContentMask<ScaledPixels>) -> Self {
        Self {
            xy_position: self.xy_position,
            st_position: self.st_position,
            content_mask,
        }
    }

    /// Apply a transformation to the vertex while overriding its content mask.
    pub fn transformed(
        &self,
        transformation: TransformationMatrix,
        content_mask: ContentMask<ScaledPixels>,
    ) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: transformation.apply_to_scaled_point(self.xy_position),
            st_position: self.st_position,
            content_mask,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, ElementEffect, backdrop_blur, glow, hsla, point, px, size};

    fn prepared_effect(effect: impl Into<ElementEffect>) -> PreparedElementEffect {
        effect.into().prepare()
    }

    #[test]
    fn html_island_primitive_carries_dom_sync_metadata() {
        let primitive = HtmlIslandPrimitive {
            order: 0,
            id: ElementId::from("settings-form"),
            bounds: Bounds::new(
                point(ScaledPixels(10.), ScaledPixels(20.)),
                size(ScaledPixels(300.), ScaledPixels(120.)),
            ),
            logical_bounds: Bounds::new(point(px(5.), px(10.)), size(px(150.), px(60.))),
            content_mask: ContentMask::rectangular(Bounds::new(
                point(ScaledPixels(0.), ScaledPixels(0.)),
                size(ScaledPixels(400.), ScaledPixels(300.)),
            )),
            input_mode: HtmlIslandInputMode::DomOwned,
            opacity: 0.75,
            content_revision: 42,
            trusted_html: SharedString::from("<button>Save</button>"),
        };

        assert_eq!(primitive.id, ElementId::from("settings-form"));
        assert_eq!(primitive.input_mode, HtmlIslandInputMode::DomOwned);
        assert_eq!(primitive.opacity, 0.75);
        assert_eq!(primitive.content_revision, 42);
        assert_eq!(primitive.trusted_html.as_ref(), "<button>Save</button>");
        assert_eq!(primitive.bounds.size.width, ScaledPixels(300.));
        assert_eq!(primitive.logical_bounds.size.width, px(150.));
    }

    #[test]
    fn html_island_batches_preserve_scene_order_between_quads() {
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(100.), ScaledPixels(100.)),
        );
        let mut scene = Scene::default();

        scene.insert_primitive(test_quad(bounds));
        scene.insert_primitive(test_html_island(bounds, "settings-form"));
        scene.insert_primitive(test_quad(bounds));
        scene.finish();

        assert_eq!(scene.quads.len(), 2);
        assert_eq!(scene.html_islands.len(), 1);

        let batches = scene.batches().collect::<Vec<_>>();
        assert!(matches!(batches[0], PrimitiveBatch::Quads(ref range) if range == &(0..1)));
        assert!(matches!(batches[1], PrimitiveBatch::HtmlIslands(ref range) if range == &(0..1)));
        assert!(matches!(batches[2], PrimitiveBatch::Quads(ref range) if range == &(1..2)));
        assert_eq!(batches.len(), 3);
    }

    #[test]
    fn html_element_primitive_carries_dom_overlay_metadata() {
        let primitive = HtmlElementPrimitive {
            order: 0,
            id: ElementId::from("provider-frame"),
            bounds: Bounds::new(
                point(ScaledPixels(10.), ScaledPixels(20.)),
                size(ScaledPixels(300.), ScaledPixels(120.)),
            ),
            logical_bounds: Bounds::new(point(px(5.), px(10.)), size(px(150.), px(60.))),
            logical_content_mask: ContentMask::rectangular(Bounds::new(
                point(px(7.), px(12.)),
                size(px(80.), px(40.)),
            )),
            content_mask: ContentMask::rectangular(Bounds::new(
                point(ScaledPixels(14.), ScaledPixels(24.)),
                size(ScaledPixels(160.), ScaledPixels(80.)),
            )),
            input_mode: HtmlElementInputMode::DomOwned,
            lifetime: HtmlElementLifetime::KeepAliveHidden,
            content_fit: HtmlElementContentFit::Fill,
            wheel_behavior: HtmlElementWheelBehavior::CaptureForward,
            opacity: 0.75,
            content_revision: 42,
            trusted_html: SharedString::from("<iframe title=\"Provider\"></iframe>"),
            extra_styles: SharedString::from("border: 1px solid #fff; border-radius: 8px;"),
        };

        assert_eq!(primitive.id, ElementId::from("provider-frame"));
        assert_eq!(primitive.input_mode, HtmlElementInputMode::DomOwned);
        assert_eq!(primitive.lifetime, HtmlElementLifetime::KeepAliveHidden);
        assert_eq!(primitive.content_fit, HtmlElementContentFit::Fill);
        assert_eq!(
            primitive.wheel_behavior,
            HtmlElementWheelBehavior::CaptureForward
        );
        assert_eq!(primitive.opacity, 0.75);
        assert_eq!(primitive.content_revision, 42);
        assert_eq!(
            primitive.trusted_html.as_ref(),
            "<iframe title=\"Provider\"></iframe>"
        );
        assert_eq!(primitive.bounds.size.width, ScaledPixels(300.));
        assert_eq!(primitive.logical_bounds.size.width, px(150.));
        assert_eq!(primitive.logical_content_mask.bounds.size.width, px(80.));
        assert_eq!(
            primitive.extra_styles.as_ref(),
            "border: 1px solid #fff; border-radius: 8px;"
        );
    }

    #[test]
    fn html_element_is_scene_metadata_without_renderer_batch() {
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(100.), ScaledPixels(100.)),
        );
        let mut scene = Scene::default();

        scene.insert_primitive(test_quad(bounds));
        scene.insert_primitive(test_html_element(bounds, "provider-frame"));
        scene.insert_primitive(test_quad(bounds));
        scene.finish();

        assert_eq!(scene.quads.len(), 2);
        assert_eq!(scene.html_elements.len(), 1);
        assert_ne!(scene.html_elements[0].order, 0);

        let batches = scene.batches().collect::<Vec<_>>();
        assert!(matches!(batches[0], PrimitiveBatch::Quads(ref range) if range == &(0..2)));
        assert_eq!(batches.len(), 1);
    }

    #[test]
    fn top_layer_orders_above_positioned_primitives() {
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(100.), ScaledPixels(100.)),
        );
        let mut scene = Scene::default();

        scene.push_positioned_layer(bounds);
        scene.insert_primitive(test_quad(bounds));
        scene.pop_layer();
        scene.push_top_layer(bounds);
        scene.insert_primitive(test_quad(bounds));
        scene.pop_layer();
        scene.finish();

        assert_eq!(scene.quads.len(), 2);
        assert!(scene.quads[1].order > scene.quads[0].order);
    }

    #[test]
    fn clear_removes_html_element_metadata() {
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(100.), ScaledPixels(100.)),
        );
        let mut scene = Scene::default();

        scene.insert_primitive(test_html_element(bounds, "provider-frame"));
        assert_eq!(scene.html_elements.len(), 1);

        scene.clear();

        assert!(scene.html_elements.is_empty());
    }

    #[test]
    fn visual_effect_batches_preserve_scene_order_between_shadows_quads_and_paths() {
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(100.), ScaledPixels(100.)),
        );
        let mut scene = Scene::default();

        scene.insert_primitive(test_shadow(bounds));
        scene.insert_primitive(test_visual_effect(bounds));
        scene.insert_primitive(test_quad(bounds));
        scene.insert_primitive(test_path(bounds));
        scene.finish();

        assert_eq!(scene.shadows.len(), 1);
        assert_eq!(scene.visual_effects.len(), 1);
        assert_eq!(scene.quads.len(), 1);
        assert_eq!(scene.paths.len(), 1);

        let batches = scene.batches().collect::<Vec<_>>();
        assert!(matches!(batches[0], PrimitiveBatch::Shadows(ref range) if range == &(0..1)));
        assert!(matches!(batches[1], PrimitiveBatch::VisualEffects(ref range) if range == &(0..1)));
        assert!(matches!(batches[2], PrimitiveBatch::Quads(ref range) if range == &(0..1)));
        assert!(matches!(batches[3], PrimitiveBatch::Paths(ref range) if range == &(0..1)));
        assert_eq!(batches.len(), 4);
    }

    #[test]
    fn visual_effects_and_screen_regions_replay_with_scene_metadata() {
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(100.), ScaledPixels(100.)),
        );
        let mut scene = Scene::default();

        scene.insert_primitive(test_visual_effect(bounds));
        scene.insert_primitive(test_screen_effect_region(bounds, "card-region"));
        scene.finish();

        let mut replayed = Scene::default();
        replayed.replay(0..scene.len(), &scene);
        replayed.finish();

        assert_eq!(replayed.visual_effects.len(), 1);
        assert_eq!(replayed.screen_effect_regions.len(), 1);
        assert_eq!(replayed.screen_effect_regions[0].id.as_ref(), "card-region");

        let batches = replayed.batches().collect::<Vec<_>>();
        assert!(
            matches!(batches.as_slice(), [PrimitiveBatch::VisualEffects(range)] if range == &(0..1))
        );
    }

    fn test_quad(bounds: Bounds<ScaledPixels>) -> Quad {
        Quad {
            bounds,
            content_mask: ContentMask::rectangular(bounds),
            ..Default::default()
        }
    }

    fn test_shadow(bounds: Bounds<ScaledPixels>) -> Shadow {
        Shadow {
            order: 0,
            blur_radius: ScaledPixels(8.),
            bounds,
            corner_radii: Corners::all(ScaledPixels(8.)),
            content_mask: ContentMask::rectangular(bounds),
            color: Color::from(hsla(0., 0., 0., 0.25)).paint_color(),
            transformation: TransformationMatrix::unit(),
        }
    }

    fn test_visual_effect(bounds: Bounds<ScaledPixels>) -> VisualEffectPrimitive {
        VisualEffectPrimitive {
            order: 0,
            bounds,
            content_mask: ContentMask::rectangular(bounds),
            corner_radii: Corners::all(ScaledPixels(12.)),
            opacity: 1.,
            effects: vec![
                prepared_effect(backdrop_blur(px(16.))),
                prepared_effect(glow(hsla(0.55, 0.8, 0.62, 0.5))),
            ],
            transformation: TransformationMatrix::unit(),
        }
    }

    fn test_screen_effect_region(
        bounds: Bounds<ScaledPixels>,
        id: &'static str,
    ) -> ScreenEffectRegionPrimitive {
        ScreenEffectRegionPrimitive {
            order: 0,
            id: id.into(),
            bounds,
            logical_bounds: Bounds::new(point(px(0.), px(0.)), size(px(50.), px(50.))),
            content_mask: ContentMask::rectangular(bounds),
            corner_radii: Corners::all(ScaledPixels(12.)),
            transformation: TransformationMatrix::unit(),
        }
    }

    fn test_path(bounds: Bounds<ScaledPixels>) -> Path<ScaledPixels> {
        let content_mask = ContentMask::rectangular(bounds);

        Path {
            id: PathId(0),
            order: 0,
            bounds,
            content_mask,
            vertices: vec![
                PathVertex {
                    xy_position: bounds.origin,
                    st_position: point(0., 0.),
                    content_mask,
                },
                PathVertex {
                    xy_position: bounds.top_right(),
                    st_position: point(1., 0.),
                    content_mask,
                },
                PathVertex {
                    xy_position: bounds.bottom_right(),
                    st_position: point(1., 1.),
                    content_mask,
                },
            ],
            color: crate::solid_background(hsla(0.1, 0.8, 0.5, 1.)).prepare(),
            start: bounds.origin,
            current: bounds.bottom_right(),
            contour_count: 1,
        }
    }

    fn test_html_island(bounds: Bounds<ScaledPixels>, id: &'static str) -> HtmlIslandPrimitive {
        HtmlIslandPrimitive {
            order: 0,
            id: ElementId::from(id),
            bounds,
            logical_bounds: Bounds::new(point(px(0.), px(0.)), size(px(50.), px(50.))),
            content_mask: ContentMask::rectangular(bounds),
            input_mode: HtmlIslandInputMode::GpuiOwned,
            opacity: 1.,
            content_revision: 1,
            trusted_html: SharedString::from("<div>Island</div>"),
        }
    }

    fn test_html_element(bounds: Bounds<ScaledPixels>, id: &'static str) -> HtmlElementPrimitive {
        HtmlElementPrimitive {
            order: 0,
            id: ElementId::from(id),
            bounds,
            logical_bounds: Bounds::new(point(px(0.), px(0.)), size(px(50.), px(50.))),
            logical_content_mask: ContentMask::rectangular(Bounds::new(
                point(px(0.), px(0.)),
                size(px(50.), px(50.)),
            )),
            content_mask: ContentMask::rectangular(bounds),
            input_mode: HtmlElementInputMode::GpuiOwned,
            lifetime: HtmlElementLifetime::PruneWhenAbsent,
            content_fit: HtmlElementContentFit::Natural,
            wheel_behavior: HtmlElementWheelBehavior::Browser,
            opacity: 1.,
            content_revision: 1,
            trusted_html: SharedString::from("<iframe title=\"Provider\"></iframe>"),
            extra_styles: SharedString::default(),
        }
    }
}
