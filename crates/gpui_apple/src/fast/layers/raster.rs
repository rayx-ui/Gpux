//! Rasterizing scroll layer tiles: each tile's part of its layer's content,
//! translated into the tile's space, drawn into the tile's texture cleared
//! to the layer's baked background, with the frame's pipelines and a
//! viewport the size of the tile.
//!
//! Adapted from gpui-fast's `gpui_apple/src/fast/layers/raster.rs`.
//!
//! All position-dependent shading is relative to each primitive's own bounds
//! (the gradient dither included), and the tile is a whole number of device
//! pixels from the content's origin, so a tile holds exactly the pixels the
//! content would have drawn over the same background in the window.
//!
//! The tiles' passes come first in the frame's command buffer: Metal runs a
//! command buffer's passes in order, so the frame's pass reads the tiles'
//! textures after they are written.

use std::ptr;

use block2::RcBlock;
use foreign_types::ForeignTypeRef;
use gpux::{
    DevicePixels, LayerKey, PaintColor, PrimitiveBatch, Scene, Size, TileCoord, decode_layer_tile,
    size,
};
use objc2::runtime::AnyObject;

use crate::metal_renderer::{InstanceBuffer, MetalRenderer, new_command_encoder_for_texture};

/// The pixel format of tiles: the drawable's and every pipeline's.
const TILE_FORMAT: metal::MTLPixelFormat = metal::MTLPixelFormat::BGRA8Unorm;

/// Encodes into `command_buffer`, before the pass that draws `scene`, the
/// passes that rasterize the layer tiles the scene composites and the cache
/// lacks. Returns `false` when their instances did not fit in
/// `instance_buffer`, as the renderer's batch functions do: the frame is
/// encoded again with a larger one.
pub(crate) fn rasterize_tiles(
    renderer: &mut MetalRenderer,
    scene: &Scene,
    command_buffer: &metal::CommandBufferRef,
    instance_buffer: &mut InstanceBuffer,
    instance_offset: &mut usize,
    viewport_size: Size<DevicePixels>,
) -> bool {
    let cache = &mut renderer.fast_layers;
    cache.drawn_at(viewport_size);
    if cache.take_failure() {
        cache.clear();
    }
    if scene.layers.frames.is_empty() && cache.is_idle() {
        return true;
    }
    let planned = cache.begin_frame(&scene.layers, composited_tiles(scene));
    if planned.is_empty() {
        return true;
    }
    cache.prepare(&renderer.device, TILE_FORMAT, &scene.layers, &planned);

    for &(index, tile) in &planned {
        let layer = &scene.layers.frames[index];
        let Some(texture) = renderer
            .fast_layers
            .texture(layer.key, tile)
            .map(|texture| texture.to_owned())
        else {
            debug_assert!(false, "a planned tile was not prepared");
            continue;
        };
        let tile_scene = layer.tile_scene(tile);
        let tile_size = size(
            DevicePixels(layer.tile_size as i32),
            DevicePixels(layer.tile_size as i32),
        );
        let clear = clear_color(layer.background);
        let command_encoder = new_command_encoder_for_texture(
            command_buffer,
            &texture,
            tile_size,
            |color_attachment| {
                color_attachment.set_load_action(metal::MTLLoadAction::Clear);
                color_attachment.set_clear_color(clear);
            },
        );
        let drawn = draw_tile_scene(
            renderer,
            &tile_scene,
            instance_buffer,
            instance_offset,
            tile_size,
            command_encoder,
        );
        command_encoder.end_encoding();
        if !drawn {
            frame_failed(renderer);
            return false;
        }
        #[cfg(test)]
        {
            renderer.fast_layers.rasterized += 1;
        }
    }

    // A command buffer that fails, as when the GPU is lost, leaves the tiles
    // it rasterized undefined: the next frame starts over.
    let failed = renderer.fast_layers.failure_flag();
    let block = RcBlock::new(move |command_buffer: ptr::NonNull<AnyObject>| {
        // SAFETY: Metal hands its completion handlers the command buffer that completed.
        let command_buffer =
            unsafe { metal::CommandBufferRef::from_ptr(command_buffer.as_ptr().cast()) };
        if command_buffer.status() == metal::MTLCommandBufferStatus::Error {
            failed.store(true, std::sync::atomic::Ordering::Release);
        }
    });
    // SAFETY: Both pointee types are opaque views of the same Objective-C block
    // pointer ABI, as the renderer's own completion handlers rely on too.
    unsafe {
        command_buffer.add_completed_handler(&*RcBlock::as_ptr(&block).cast());
    }
    true
}

/// The frame that was to rasterize the planned tiles is not drawn as it was
/// encoded: the cache counts them as held, so it starts over.
pub(crate) fn frame_failed(renderer: &mut MetalRenderer) {
    renderer.fast_layers.clear();
}

/// The layer tiles `scene` composites, in the order it lists them.
fn composited_tiles(scene: &Scene) -> impl Iterator<Item = (LayerKey, TileCoord)> + '_ {
    let has_layers = !scene.layers.frames.is_empty();
    scene
        .polychrome_sprites
        .iter()
        .take_while(move |_| has_layers)
        .filter_map(|sprite| decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id))
}

/// The clear colour a tile over `background` starts from: the bytes an
/// opaque quad of that colour writes to the frame's non-sRGB texture.
pub(crate) fn clear_color(background: PaintColor) -> metal::MTLClearColor {
    metal::MTLClearColor::new(
        background.r as f64,
        background.g as f64,
        background.b as f64,
        background.a as f64,
    )
}

/// Draws a tile's `scene` with the renderer's pipelines. A layer's content
/// holds what a tile can: no path (the core draws a layer's paths into the
/// frame), no effect, island or surface (their container is never a layer),
/// no other layer's tile (a scroll container inside a layer is painted into
/// it). Metal draws no subpixel sprites.
fn draw_tile_scene(
    renderer: &MetalRenderer,
    scene: &Scene,
    instance_buffer: &mut InstanceBuffer,
    instance_offset: &mut usize,
    viewport_size: Size<DevicePixels>,
    command_encoder: &metal::RenderCommandEncoderRef,
) -> bool {
    for batch in scene.batches() {
        let ok = match batch {
            PrimitiveBatch::Shadows(range) => renderer.draw_shadows(
                &scene.shadows[range],
                instance_buffer,
                instance_offset,
                viewport_size,
                command_encoder,
            ),
            PrimitiveBatch::Quads(range) => renderer.draw_quads(
                &scene.quads[range],
                instance_buffer,
                instance_offset,
                viewport_size,
                command_encoder,
            ),
            PrimitiveBatch::Underlines(range) => renderer.draw_underlines(
                &scene.underlines[range],
                instance_buffer,
                instance_offset,
                viewport_size,
                command_encoder,
            ),
            PrimitiveBatch::MonochromeSprites { texture_id, range } => renderer
                .draw_monochrome_sprites(
                    texture_id,
                    &scene.monochrome_sprites[range],
                    instance_buffer,
                    instance_offset,
                    viewport_size,
                    command_encoder,
                ),
            PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                if super::composite::is_layer_texture(texture_id) {
                    debug_assert!(false, "a layer tile inside a layer tile");
                    true
                } else {
                    renderer.draw_polychrome_sprites(
                        texture_id,
                        &scene.polychrome_sprites[range],
                        instance_buffer,
                        instance_offset,
                        viewport_size,
                        command_encoder,
                    )
                }
            }
            PrimitiveBatch::Paths(_)
            | PrimitiveBatch::SubpixelSprites { .. }
            | PrimitiveBatch::VisualEffects(_)
            | PrimitiveBatch::HtmlIslands(_)
            | PrimitiveBatch::Surfaces(_) => {
                debug_assert!(false, "a layer tile holds what no tile can");
                true
            }
        };
        if !ok {
            return false;
        }
    }
    true
}
