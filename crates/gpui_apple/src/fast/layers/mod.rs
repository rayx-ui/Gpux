//! Scroll layer tiles on Metal: the textures a layer's content is rasterized
//! into, and the passes that draw them where the scene composites them.
//! The core side is `gpux::fast::layers`; the wgpu renderer's counterpart is
//! `gpui_wgpu`'s `fast::layers`.
//!
//! Adapted from gpui-fast's `gpui_apple/src/fast/layers/`. gpui-fast writes a
//! frame's instances before it encodes and gives the tiles a command buffer
//! of their own; gpux's Metal renderer is the one it imports from Zed, which
//! writes each batch's instances as it encodes it, so the tiles are encoded
//! by the renderer's own batch functions into the frame's command buffer,
//! before its first pass, sharing its instance buffer.

pub(crate) mod composite;
pub(crate) mod raster;
pub(crate) mod tile_cache;

pub(crate) use tile_cache::TileCache;

#[cfg(test)]
mod tests;
