//! Glyph painting that works out a run's rendering once, not once a glyph.
//!
//! Adapted from gpui-fast's `crates/gpui/src/fast/glyphs.rs` (`b8f0c02`):
//! [`LineGlyphPainter`] works out once a line what every glyph of it shares
//! (the paint content mask, the element transform and opacity) and once a run
//! what its glyphs share (subpixel rendering, dilation and the painted
//! colour), and [`GlyphBoundsCache`] keeps the raster bounds and, for the
//! frame, the atlas tiles of the glyphs a window painted lately, so painting a
//! glyph needn't lock and hash its way through the text system and atlas.
//! Every sprite is the one [`Window::paint_glyph`] paints. gpux keeps its own
//! glyph quantization here; the quantization that scroll-layer tiles need
//! lands with them.

use std::borrow::Cow;

use crate::{
    App, AtlasTile, Bounds, Color, ContentMask, DevicePixels, FontId, GlyphId, IsZero,
    MonochromeSprite, MonochromeSpriteKind, PaintColor, Pixels, Point, RenderGlyphParams,
    SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y, ScaledPixels, SubpixelSprite, TransformationMatrix,
    Window, color::prepare_color_with_opacity, util::round_half_toward_zero,
};
use anyhow::Result;

/// What painting the glyphs of a run needs that depends on the run, not the
/// glyph.
#[derive(Clone)]
struct RunRendering {
    font_id: FontId,
    font_size: Pixels,
    color: Color,
    subpixel_rendering: bool,
    dilation: u8,
    paint_color: PaintColor,
}

/// Paints the glyphs of a line, working out what they share once.
pub(crate) struct LineGlyphPainter {
    content_mask: ContentMask<ScaledPixels>,
    transformation: TransformationMatrix,
    element_opacity: f32,
    scale_factor: f32,
    run: Option<RunRendering>,
}

impl LineGlyphPainter {
    /// A painter for a line painted in `window`'s current paint context.
    pub(crate) fn new(window: &Window) -> Self {
        Self {
            content_mask: window.paint_content_mask(),
            transformation: window.current_element_transform(),
            element_opacity: window.element_opacity(),
            scale_factor: window.scale_factor(),
            run: None,
        }
    }

    /// The run rendering of glyphs in `font_id` at `font_size` and `color`,
    /// worked out when the run changes.
    fn rendering(
        &mut self,
        window: &Window,
        font_id: FontId,
        font_size: Pixels,
        color: &Color,
    ) -> &RunRendering {
        let current = self.run.as_ref().is_some_and(|run| {
            run.font_id == font_id && run.font_size == font_size && run.color == *color
        });
        if !current {
            self.run = Some(RunRendering {
                font_id,
                font_size,
                color: color.clone(),
                subpixel_rendering: self.transformation == TransformationMatrix::unit()
                    && window.should_use_subpixel_rendering(font_id, font_size),
                dilation: window.text_system().glyph_dilation_for_color(color.clone()),
                paint_color: prepare_color_with_opacity(color, self.element_opacity),
            });
        }
        self.run.as_ref().expect("set above")
    }

    /// [`Window::paint_glyph`], for the next glyph of the line.
    pub(crate) fn paint_glyph(
        &mut self,
        window: &mut Window,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
        color: &Color,
    ) -> Result<()> {
        window.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor;
        let glyph_origin = origin.scale(scale_factor);
        let quantized_origin = Point::new(
            round_half_toward_zero(glyph_origin.x.0 * SUBPIXEL_VARIANTS_X as f32)
                / SUBPIXEL_VARIANTS_X as f32,
            round_half_toward_zero(glyph_origin.y.0 * SUBPIXEL_VARIANTS_Y as f32)
                / SUBPIXEL_VARIANTS_Y as f32,
        );
        let subpixel_variant = Point::new(
            (quantized_origin.x.fract() * SUBPIXEL_VARIANTS_X as f32) as u8,
            (quantized_origin.y.fract() * SUBPIXEL_VARIANTS_Y as f32) as u8,
        );
        let integer_origin = quantized_origin.map(|c| ScaledPixels(c.trunc()));
        let transformation = self.transformation;
        let content_mask = self.content_mask;
        let run = self.rendering(window, font_id, font_size, color).clone();
        let params = RenderGlyphParams {
            font_id,
            glyph_id,
            font_size,
            subpixel_variant,
            scale_factor,
            is_emoji: false,
            subpixel_rendering: run.subpixel_rendering,
            dilation: run.dilation,
        };

        let (raster_bounds, kept_tile) = match window.fast_glyph_bounds.lookup(&params) {
            Some(kept) => kept,
            None => {
                let raster_bounds = window.text_system().raster_bounds(&params)?;
                window.fast_glyph_bounds.insert(&params, raster_bounds);
                (raster_bounds, None)
            }
        };
        if raster_bounds.is_zero() {
            return Ok(());
        }
        let tile = match kept_tile {
            Some(tile) => tile,
            None => {
                let tile = window
                    .sprite_atlas
                    .get_or_insert_with(params.clone().into(), &mut || {
                        let (size, bytes) = window.text_system().rasterize_glyph(&params)?;
                        Ok(Some((size, Cow::Owned(bytes))))
                    })?
                    .expect("Callback above only errors or returns Some");
                window.fast_glyph_bounds.insert_tile(&params, tile);
                tile
            }
        };
        let bounds = Bounds {
            origin: integer_origin + raster_bounds.origin.map(Into::into),
            size: tile.bounds.size.map(Into::into),
        };
        if run.subpixel_rendering {
            window.next_frame.scene.insert_primitive(SubpixelSprite {
                order: 0,
                pad: 0,
                bounds,
                content_mask,
                color: run.paint_color,
                tile,
                transformation,
            });
        } else {
            window.next_frame.scene.insert_primitive(MonochromeSprite {
                order: 0,
                kind: MonochromeSpriteKind::Glyph,
                bounds,
                content_mask,
                color: run.paint_color,
                tile,
                transformation,
            });
        }
        Ok(())
    }
}

/// How many glyphs' raster bounds a window keeps at hand, as a power of two.
const GLYPH_BOUNDS_SLOT_BITS: u32 = 12;

/// The raster bounds of the glyphs a window painted lately, and for the frame
/// they were looked up in, their atlas tiles. A glyph's raster bounds depend
/// on its description alone, so they are the text system's own answer; a
/// tile is kept for its frame only, since the atlas may be cleared when the
/// frame is presented. Each glyph has one slot; a glyph needing a slot
/// another holds takes it over.
pub(crate) struct GlyphBoundsCache {
    slots: Box<[Option<GlyphSlot>]>,
    /// Counts the frames drawn, telling a tile of this frame from older ones.
    frame: u64,
    bounding_boxes: Vec<(FontId, Pixels, Bounds<Pixels>)>,
}

#[derive(Clone)]
struct GlyphSlot {
    params: RenderGlyphParams,
    raster_bounds: Bounds<DevicePixels>,
    tile: Option<(u64, AtlasTile)>,
}

impl Default for GlyphBoundsCache {
    fn default() -> Self {
        Self {
            slots: vec![None; 1 << GLYPH_BOUNDS_SLOT_BITS].into_boxed_slice(),
            frame: 0,
            bounding_boxes: Vec::new(),
        }
    }
}

impl GlyphBoundsCache {
    fn slot(params: &RenderGlyphParams) -> usize {
        const K: u64 = 0x9e37_79b9_7f4a_7c15;
        let words = [
            params.glyph_id.0 as u64 | (params.font_id.0 as u64) << 32,
            params.font_size.0.to_bits() as u64
                | (params.subpixel_variant.x as u64) << 32
                | (params.subpixel_variant.y as u64) << 40
                | (params.dilation as u64) << 48
                | (params.subpixel_rendering as u64) << 56
                | (params.is_emoji as u64) << 57,
            params.scale_factor.to_bits() as u64,
        ];
        let hash = words.iter().fold(0u64, |hash, word| {
            (hash.rotate_left(26) ^ word).wrapping_mul(K)
        });
        (hash >> (64 - GLYPH_BOUNDS_SLOT_BITS)) as usize
    }

    /// The glyph's raster bounds, if kept, and its tile, if it was looked up
    /// this frame.
    fn lookup(
        &self,
        params: &RenderGlyphParams,
    ) -> Option<(Bounds<DevicePixels>, Option<AtlasTile>)> {
        match &self.slots[Self::slot(params)] {
            Some(slot) if slot.params == *params => Some((
                slot.raster_bounds,
                slot.tile
                    .and_then(|(frame, tile)| (frame == self.frame).then_some(tile)),
            )),
            _ => None,
        }
    }

    fn insert(&mut self, params: &RenderGlyphParams, bounds: Bounds<DevicePixels>) {
        self.slots[Self::slot(params)] = Some(GlyphSlot {
            params: params.clone(),
            raster_bounds: bounds,
            tile: None,
        });
    }

    fn insert_tile(&mut self, params: &RenderGlyphParams, tile: AtlasTile) {
        if let Some(slot) = &mut self.slots[Self::slot(params)]
            && slot.params == *params
        {
            slot.tile = Some((self.frame, tile));
        }
    }

    /// Ends the frame whose tiles were kept: the atlas may be cleared before
    /// the next.
    pub(crate) fn finish_frame(&mut self) {
        self.frame += 1;
    }

    /// Forgets every glyph, as after the atlas or the fonts changed.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn forget(&mut self) {
        *self = Self::default();
    }
}

/// How many fonts' bounding boxes [`bounding_box`] keeps, most recent last.
const BOUNDING_BOXES: usize = 16;

/// [`crate::TextSystem::bounding_box`], which painting a line asks for once a
/// run: it takes a lock and hashes the font, where a window paints its text
/// in a handful of fonts and sizes, whose bounding boxes it keeps.
pub(crate) fn bounding_box(
    window: &mut Window,
    cx: &App,
    font_id: FontId,
    font_size: Pixels,
) -> Bounds<Pixels> {
    let boxes = &mut window.fast_glyph_bounds.bounding_boxes;
    if let Some((_, _, bounds)) = boxes
        .iter()
        .rev()
        .find(|(id, size, _)| *id == font_id && *size == font_size)
    {
        return *bounds;
    }
    let bounds = cx.text_system().bounding_box(font_id, font_size);
    if boxes.len() == BOUNDING_BOXES {
        boxes.remove(0);
    }
    boxes.push((font_id, font_size, bounds));
    bounds
}
