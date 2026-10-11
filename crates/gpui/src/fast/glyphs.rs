//! Glyph painting that works out a run's rendering once, not once a glyph.
//!
//! Adapted from gpui-fast's `crates/gpui/src/fast/glyphs.rs` (`b8f0c02`):
//! [`LineGlyphPainter`] works out once a line what every glyph of it shares
//! (the paint content mask, the element transform and opacity) and once a run
//! what its glyphs share (subpixel rendering, dilation and the painted
//! colour), and [`GlyphBoundsCache`] keeps the raster bounds and, for the
//! frame, the atlas tiles of the glyphs a window painted lately, so painting a
//! glyph needn't lock and hash its way through the text system and atlas.
//! Every sprite is the one [`Window::paint_glyph`] paints.
//!
//! With scroll layers on, glyph origins are quantized so that a whole-pixel
//! shift moves a glyph by exactly that shift at negative coordinates too
//! ([`quantize_origin`]), and a line skips only the glyphs that cannot reach
//! its content mask ([`LineGlyphPainter::may_reach`]): a line painted in a
//! layer's overscan and scrolled into view then shows the glyphs the line
//! painted in place does. With the switch off both are gpux's own, unchanged.

use std::borrow::Cow;

use crate::{
    App, AtlasTile, Bounds, Color, ContentMask, DevicePixels, FontId, GlyphId, IsZero,
    MonochromeSprite, MonochromeSpriteKind, PaintColor, Pixels, Point, RenderGlyphParams,
    SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y, ScaledPixels, Size, SubpixelSprite,
    TransformationMatrix, Window, color::prepare_color_with_opacity, util::round_half_toward_zero,
};
use anyhow::Result;

/// A glyph's whole-pixel origin and subpixel variant: upstream's rounding
/// for non-negative coordinates, extended so that a whole-pixel shift moves
/// the result by exactly that shift everywhere, negative coordinates included.
///
/// Upstream rounds `origin * variants` half toward zero, then takes `trunc`
/// and `fract`, which mirror around zero; a glyph painted above or left of
/// the window (a scroll layer's overscan) would then land on a different
/// pixel or variant than the same glyph painted once scrolled into view.
pub(crate) fn quantize_origin(origin: Point<ScaledPixels>) -> (Point<ScaledPixels>, Point<u8>) {
    let (x, variant_x) = quantize_axis(origin.x.0, SUBPIXEL_VARIANTS_X);
    let (y, variant_y) = quantize_axis(origin.y.0, SUBPIXEL_VARIANTS_Y);
    (
        Point::new(ScaledPixels(x), ScaledPixels(y)),
        Point::new(variant_x, variant_y),
    )
}

/// The whole pixel at or below `value` plus the nearest of `variants` steps
/// within it, ties toward the pixel; the last step carries into the next pixel.
/// On non-negative values `floor` is `trunc` and the steps round like
/// upstream's `round_half_toward_zero(value * variants)`.
fn quantize_axis(value: f32, variants: u8) -> (f32, u8) {
    let whole = value.floor();
    let steps = round_half_toward_zero((value - whole) * variants as f32) as i32;
    let carry = steps / variants as i32;
    (whole + carry as f32, (steps % variants as i32) as u8)
}

/// An emoji's whole-pixel origin: the nearest whole pixel, ties toward the
/// pixel below, which is upstream's `round_half_toward_zero` on non-negative
/// values and shifts with whole-pixel moves on negative ones.
pub(crate) fn quantize_emoji_origin(origin: Point<ScaledPixels>) -> Point<ScaledPixels> {
    origin.map(|c| {
        let whole = c.0.floor();
        ScaledPixels(whole + round_half_toward_zero(c.0 - whole))
    })
}

/// A glyph's whole-pixel origin and subpixel variant in a window whose
/// scroll layers are on (`layers`, [`quantize_origin`]) or off (upstream's
/// rounding, as gpux paints without fast mode).
#[inline]
pub(crate) fn glyph_pixel_origin(
    layers: bool,
    origin: Point<ScaledPixels>,
) -> (Point<ScaledPixels>, Point<u8>) {
    if layers {
        return quantize_origin(origin);
    }
    let quantized_origin = Point::new(
        round_half_toward_zero(origin.x.0 * SUBPIXEL_VARIANTS_X as f32)
            / SUBPIXEL_VARIANTS_X as f32,
        round_half_toward_zero(origin.y.0 * SUBPIXEL_VARIANTS_Y as f32)
            / SUBPIXEL_VARIANTS_Y as f32,
    );
    let subpixel_variant = Point::new(
        (quantized_origin.x.fract() * SUBPIXEL_VARIANTS_X as f32) as u8,
        (quantized_origin.y.fract() * SUBPIXEL_VARIANTS_Y as f32) as u8,
    );
    (
        quantized_origin.map(|c| ScaledPixels(c.trunc())),
        subpixel_variant,
    )
}

/// An emoji's whole-pixel origin, with scroll layers on or off, as
/// [`glyph_pixel_origin`] is a glyph's.
#[inline]
pub(crate) fn emoji_origin(layers: bool, origin: Point<ScaledPixels>) -> Point<ScaledPixels> {
    if layers {
        quantize_emoji_origin(origin)
    } else {
        origin.map(|c| ScaledPixels(round_half_toward_zero(c.0)))
    }
}

/// [`LineGlyphPainter::may_reach`] for a glyph on its own, scroll layers on.
#[cfg(test)]
pub(crate) fn may_reach(
    line_glyph: Bounds<Pixels>,
    baseline: Pixels,
    mask: &Bounds<Pixels>,
) -> bool {
    GlyphReach::new(line_glyph.size, mask)
        .contains(line_glyph.origin.x, line_glyph.origin.y + baseline)
}

/// [`LineGlyphPainter::may_reach`] for glyphs sized alike in one mask: the
/// mask's edges pushed out by the glyphs' reach, which a line works out once
/// a run, so that each glyph only compares its origin and baseline with them.
#[derive(Clone, Copy)]
struct GlyphReach {
    size: Size<Pixels>,
    left: Pixels,
    top: Pixels,
    right: Pixels,
    bottom: Pixels,
}

impl GlyphReach {
    /// A reach [`GlyphReach::is_for`] no size, to be worked out on first use.
    const UNSET: Self = Self {
        size: Size {
            width: Pixels(f32::NAN),
            height: Pixels(f32::NAN),
        },
        left: Pixels(0.),
        top: Pixels(0.),
        right: Pixels(0.),
        bottom: Pixels(0.),
    };

    /// Whether this is the reach of glyphs of `size`, comparing bits, as a
    /// size's only use is to work out the same reach again.
    #[inline]
    fn is_for(&self, size: Size<Pixels>) -> bool {
        self.size.width.0.to_bits() == size.width.0.to_bits()
            && self.size.height.0.to_bits() == size.height.0.to_bits()
    }

    fn new(size: Size<Pixels>, mask: &Bounds<Pixels>) -> Self {
        const MARGIN: Pixels = Pixels(2.);
        // An empty mask's edges would let in the glyphs straddling it.
        if mask.is_empty() {
            return Self {
                size,
                left: Pixels(f32::INFINITY),
                top: Pixels(f32::INFINITY),
                right: Pixels(f32::NEG_INFINITY),
                bottom: Pixels(f32::NEG_INFINITY),
            };
        }
        let height = size.height + MARGIN;
        let width = size.width + MARGIN;
        let end = mask.bottom_right();
        Self {
            size,
            left: mask.origin.x - width,
            top: mask.origin.y - height,
            right: end.x + height,
            bottom: end.y + height,
        }
    }

    /// Whether a glyph whose origin is at `x` and whose baseline is at `y`
    /// may draw inside the mask.
    #[inline]
    fn contains(&self, x: Pixels, y: Pixels) -> bool {
        y > self.top && y < self.bottom && x > self.left && x < self.right
    }
}

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
    /// Whether the window's scroll layers are on: glyphs are then quantized
    /// and culled the way a layer's tiles need.
    layers: bool,
    /// The line's content mask, for [`Self::may_reach`].
    mask: Bounds<Pixels>,
    reach: GlyphReach,
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
            layers: window.fast_mode.snapping(),
            // A transformed line's glyphs are culled against the mask the window without a
            // layer has, the viewport's, not the region a layer's content is painted into.
            mask: crate::fast::layers::paint::transformed_clip(window, window.content_mask())
                .bounds,
            reach: GlyphReach::UNSET,
        }
    }

    /// Whether the line's next glyph may draw inside `mask`, the line's
    /// content mask, for the line to skip the glyphs it need not paint.
    /// `line_glyph` is the line's glyph box as upstream builds it: the
    /// glyph's origin on the top of its line, sized by the font's bounding
    /// box. The glyph itself is drawn `baseline` further down, on the line's
    /// baseline, where a tall line puts it well below the top; upstream's
    /// `line_glyph.intersects(mask)` then skips glyphs that reach into the
    /// mask.
    ///
    /// With scroll layers on this test is only conservative, leaving the
    /// exact test to the scene, which drops a sprite outside its content
    /// mask: vertically it takes the bounding box's height above and below
    /// the baseline; horizontally, as upstream does, the bounding box's width
    /// from the glyph's origin on, and its height before it, for glyphs
    /// reaching left of their origin; each plus a margin for glyph dilation
    /// and subpixel positioning. Nothing reaches into an empty mask. Where a
    /// glyph is drawn therefore depends on its sprite alone, so a line
    /// painted in a scroll layer's overscan and scrolled into view shows the
    /// same glyphs as the line painted in place. With scroll layers off it is
    /// upstream's test.
    #[inline]
    pub(crate) fn may_reach(
        &mut self,
        line_glyph: Bounds<Pixels>,
        baseline: Pixels,
        mask: &Bounds<Pixels>,
    ) -> bool {
        if !self.layers {
            return line_glyph.intersects(mask);
        }
        debug_assert!(
            *mask == self.mask || self.transformation != TransformationMatrix::unit(),
            "the line's mask is the window's, but for a transformed line a layer paints"
        );
        if !self.reach.is_for(line_glyph.size) {
            self.reach = GlyphReach::new(line_glyph.size, &self.mask);
        }
        self.reach
            .contains(line_glyph.origin.x, line_glyph.origin.y + baseline)
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
        let (integer_origin, subpixel_variant) = glyph_pixel_origin(self.layers, glyph_origin);
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

        let (raster_bounds, kept_tile, sprite_size) = match window.fast_glyph_bounds.lookup(&params)
        {
            Some(kept) => kept,
            None => {
                let raster_bounds = window.text_system().raster_bounds(&params)?;
                window.fast_glyph_bounds.insert(&params, raster_bounds);
                (raster_bounds, None, None)
            }
        };
        if raster_bounds.is_zero() {
            return Ok(());
        }
        let sprite_origin = integer_origin + raster_bounds.origin.map(Into::into);
        let tile = match kept_tile {
            Some(tile) => tile,
            // The scene drops a sprite that misses its content mask; where
            // the sprite's size is known and the reach test let it through,
            // drop it before looking up its tile.
            None if self.layers
                && transformation == TransformationMatrix::unit()
                && sprite_size.is_some_and(|size| {
                    Bounds::new(sprite_origin, size.map(Into::into))
                        .intersect(&content_mask.bounds)
                        .is_empty()
                }) =>
            {
                return Ok(());
            }
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
            origin: sprite_origin,
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

    /// The glyph's raster bounds, if kept, its tile, if it was looked up this
    /// frame, and the size of its sprite, if its tile was ever looked up: a
    /// glyph's rasterization, and so its tile's size, only depends on its
    /// description, whether or not the atlas still holds it.
    fn lookup(
        &self,
        params: &RenderGlyphParams,
    ) -> Option<(
        Bounds<DevicePixels>,
        Option<AtlasTile>,
        Option<Size<DevicePixels>>,
    )> {
        match &self.slots[Self::slot(params)] {
            Some(slot) if slot.params == *params => Some((
                slot.raster_bounds,
                slot.tile
                    .and_then(|(frame, tile)| (frame == self.frame).then_some(tile)),
                slot.tile.map(|(_, tile)| tile.bounds.size),
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
