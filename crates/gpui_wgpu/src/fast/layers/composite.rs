//! Drawing scroll layer tiles where the scene composites them: polychrome
//! sprites whose texture id lies in the range no atlas allocates stand for
//! tiles, and are drawn with the polychrome sprite pipeline from the tiles'
//! own textures instead of an atlas texture.
//!
//! Adapted from gpui-fast's `gpui_wgpu/src/fast/layers/composite.rs`.
//!
//! A tile sprite covers the tile's whole texture at whole device pixels, so
//! every fragment samples a texel center and copies the tile's pixel: the
//! sprite says its texture is premultiplied
//! ([`gpux::LAYER_TILE_PREMULTIPLIED`]), where the shader multiplies an
//! atlas texture's colour by its alpha.

use std::ops::Range;

use gpux::{
    AtlasTextureId, LayerKey, PolychromeSprite, Scene, TileCoord, TileId, decode_layer_tile,
};

use crate::{WgpuRenderer, fast::layers::TileCache};

/// The layer tiles `scene` composites, in the order it lists them.
pub(crate) fn composited_tiles(scene: &Scene) -> impl Iterator<Item = (LayerKey, TileCoord)> + '_ {
    let has_layers = !scene.layers.frames.is_empty();
    scene
        .polychrome_sprites
        .iter()
        .take_while(move |_| has_layers)
        .filter_map(|sprite| decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id))
}

/// Whether sprites of `texture` are layer tiles rather than atlas sprites.
pub(crate) fn is_layer_texture(texture: AtlasTextureId) -> bool {
    decode_layer_tile(texture, TileId(0)).is_some()
}

/// Splits a batch of layer tile sprites into runs of the same tile: the
/// scene batches sprites by texture id, which is one per layer, while each
/// tile has a texture of its own.
pub(crate) fn tile_runs(sprites: &[PolychromeSprite]) -> impl Iterator<Item = Range<usize>> + '_ {
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

/// The texture a run of sprites of one layer tile is drawn from, or `None`
/// when the cache does not hold the tile, which rasterizing every composited
/// tile it lacks before the frame is drawn prevents.
pub(crate) fn texture_for_batch<'a>(
    cache: &'a TileCache,
    texture: AtlasTextureId,
    sprites: &[PolychromeSprite],
) -> Option<&'a wgpu::TextureView> {
    let first = sprites.first()?;
    debug_assert!(
        sprites
            .iter()
            .all(|sprite| sprite.tile.tile_id == first.tile.tile_id),
        "a run of layer tile sprites spans tiles"
    );
    let (layer, tile) = decode_layer_tile(texture, first.tile.tile_id)?;
    let view = cache.texture(layer, tile);
    debug_assert!(view.is_some(), "a composited layer tile was not rasterized");
    view
}

/// Draws the batch `sprites` of tiles of the layer `texture` stands for,
/// each run of one tile from that tile's texture. Returns `false` when the
/// instance data did not fit, as the renderer's batch functions do.
pub(crate) fn draw(
    renderer: &WgpuRenderer,
    sprites: &[PolychromeSprite],
    texture: AtlasTextureId,
    instance_offset: &mut u64,
    pass: &mut wgpu::RenderPass<'_>,
) -> bool {
    for run in tile_runs(sprites) {
        let run = &sprites[run];
        let Some(view) = texture_for_batch(&renderer.fast_layers.cache, texture, run) else {
            continue;
        };
        let data = unsafe { WgpuRenderer::instance_bytes(run) };
        if !renderer.draw_instances_with_texture(
            data,
            run.len() as u32,
            view,
            &renderer.resources().pipelines.poly_sprites,
            instance_offset,
            pass,
        ) {
            return false;
        }
    }
    true
}
