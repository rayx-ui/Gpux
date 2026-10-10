//! Scroll layer tiles on wgpu: the textures a layer's content is rasterized
//! into, and the passes that draw them where the scene composites them.
//! The core side is `gpux::fast::layers`.
//!
//! Adapted from gpui-fast's `gpui_wgpu/src/fast/layers/`. gpui-fast records a
//! frame through a `FrameTarget` that its tiles are drawn through too; gpux
//! keeps the renderer it imports from Zed, so a tile is drawn by the
//! renderer's own batch functions, into the tile's texture and with the
//! tile's globals ([`globals_bind_group`]), in the frame's encoder before
//! its main pass.

use std::cell::Cell;

use crate::WgpuRenderer;

pub(crate) mod composite;
pub(crate) mod raster;
pub(crate) mod tile_cache;

pub(crate) use tile_cache::TileCache;

#[cfg(test)]
mod tests;

/// What a renderer keeps for scroll layers.
#[derive(Default)]
pub(crate) struct RendererLayers {
    pub(crate) cache: TileCache,
    /// The size of the tile being rasterized, while one is.
    rasterizing: Cell<Option<u32>>,
    /// How many tiles were rasterized, for tests.
    #[cfg(test)]
    pub(crate) rasterized: Cell<usize>,
}

/// The globals the batch being drawn is drawn with: those of the tile being
/// rasterized, whose viewport is the tile, and the frame's otherwise.
pub(crate) fn globals_bind_group(renderer: &WgpuRenderer) -> &wgpu::BindGroup {
    let layers = &renderer.fast_layers;
    layers
        .rasterizing
        .get()
        .and_then(|tile_size| layers.cache.globals(tile_size))
        .unwrap_or(&renderer.resources().globals_bind_group)
}

/// Releases every tile: what the renderer draws them with or into changed
/// (its size, its alpha mode, its sprite atlas), or the frame that was to
/// rasterize the planned ones was not drawn.
pub(crate) fn release(renderer: &mut WgpuRenderer) {
    renderer.fast_layers.cache.clear();
}
