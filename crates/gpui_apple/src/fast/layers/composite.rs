//! Drawing scroll layer tiles where the scene composites them: polychrome
//! sprites whose texture id lies in the range no atlas allocates stand for
//! tiles, and are drawn with the polychrome sprite pipeline from the tiles'
//! own textures instead of an atlas texture.
//!
//! Adapted from gpui-fast's `gpui_apple/src/fast/layers/composite.rs`.
//!
//! A tile sprite covers the tile's whole texture at whole device pixels, so
//! every fragment samples a texel center and copies the tile's pixel: the
//! sprite says its texture is premultiplied
//! ([`gpux::LAYER_TILE_PREMULTIPLIED`]), where the shader multiplies an
//! atlas texture's colour by its alpha.

use std::{mem, ops::Range, ptr};

use gpux::{AtlasTextureId, DevicePixels, PolychromeSprite, Size, TileId, decode_layer_tile, size};

use crate::metal_renderer::{InstanceBuffer, MetalRenderer, SpriteInputIndex, align_offset};

/// Whether sprites of `texture` are layer tiles rather than atlas sprites.
pub(crate) fn is_layer_texture(texture: AtlasTextureId) -> bool {
    decode_layer_tile(texture, TileId(0)).is_some()
}

/// Splits a batch of layer tile sprites into runs of the same tile: the
/// scene batches sprites by texture id, which is one per layer, while each
/// tile has a texture of its own.
fn tile_runs(sprites: &[PolychromeSprite]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = 0;
    std::iter::from_fn(move || {
        if start >= sprites.len() {
            return None;
        }
        let tile = sprites[start].tile.tile_id;
        let end = (start..sprites.len())
            .find(|&index| sprites[index].tile.tile_id != tile)
            .unwrap_or(sprites.len());
        let run = start..end;
        start = end;
        Some(run)
    })
}

/// Draws the batch `sprites` of tiles of the layer `texture_id` stands for,
/// each run of one tile from that tile's texture, as
/// `MetalRenderer::draw_polychrome_sprites` draws a batch from an atlas
/// texture. Returns `false` when the sprites did not fit in
/// `instance_buffer`. A tile the cache does not hold draws nothing;
/// rasterizing every composited tile it lacks before the frame is drawn
/// prevents that.
pub(crate) fn draw_tiles(
    renderer: &MetalRenderer,
    texture_id: AtlasTextureId,
    sprites: &[PolychromeSprite],
    instance_buffer: &mut InstanceBuffer,
    instance_offset: &mut usize,
    viewport_size: Size<DevicePixels>,
    command_encoder: &metal::RenderCommandEncoderRef,
) -> bool {
    if sprites.is_empty() {
        return true;
    }
    align_offset(instance_offset);
    let sprite_bytes_len = mem::size_of_val(sprites);
    let next_offset = *instance_offset + sprite_bytes_len;
    if next_offset > instance_buffer.size {
        return false;
    }
    // SAFETY: the sprites fit in the buffer from the offset, checked above.
    unsafe {
        ptr::copy_nonoverlapping(
            sprites.as_ptr() as *const u8,
            (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset),
            sprite_bytes_len,
        );
    }

    command_encoder.set_render_pipeline_state(&renderer.polychrome_sprites_pipeline_state);
    command_encoder.set_vertex_buffer(
        SpriteInputIndex::Vertices as u64,
        Some(&renderer.unit_vertices),
        0,
    );
    command_encoder.set_vertex_buffer(
        SpriteInputIndex::Sprites as u64,
        Some(&instance_buffer.metal_buffer),
        *instance_offset as u64,
    );
    command_encoder.set_vertex_bytes(
        SpriteInputIndex::ViewportSize as u64,
        mem::size_of_val(&viewport_size) as u64,
        &viewport_size as *const Size<DevicePixels> as *const _,
    );
    command_encoder.set_fragment_buffer(
        SpriteInputIndex::Sprites as u64,
        Some(&instance_buffer.metal_buffer),
        *instance_offset as u64,
    );

    let cache = &renderer.fast_layers;
    for run in tile_runs(sprites) {
        let first = &sprites[run.start];
        let Some((layer, tile)) = decode_layer_tile(texture_id, first.tile.tile_id) else {
            continue;
        };
        let Some(texture) = cache.texture(layer, tile) else {
            debug_assert!(false, "a composited layer tile was not rasterized");
            continue;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(texture));
        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            run.len() as u64,
            run.start as u64,
        );
    }
    *instance_offset = next_offset;
    true
}
