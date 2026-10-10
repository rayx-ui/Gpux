//! Renderers for the extra GPUI surfaces of a composed window.
//!
//! Adapted from gpui-fast's `gpui_apple/src/fast/composition.rs`, itself a
//! port of zed-industries/zed#62379. A window that composes native views
//! between GPUI's base content and its overlays draws each GPUI surface on a
//! `CAMetalLayer` of its own. Every surface's renderer shares the window
//! renderer's device, instance buffer pool and sprite atlas, so glyphs and
//! images rasterized for one surface are there for the others, and the tiles
//! a frame's scene refers to are valid whichever surface draws them. Effect
//! capabilities stay false on Metal, as for the window renderer.

use crate::metal_renderer::MetalRenderer;

impl MetalRenderer {
    /// A renderer for an overlay surface above this one: transparent
    /// (`opaque = NO`), on its own layer, sharing this renderer's device,
    /// instance buffer pool and sprite atlas.
    pub fn new_overlay(&self) -> MetalRenderer {
        let device = self.device.clone();
        let layer = metal::MetalLayer::new();
        MetalRenderer::configure_layer(&layer, &device, true);
        let mut renderer = MetalRenderer::new_internal(
            device,
            Some(layer),
            false,
            self.instance_buffer_pool.clone(),
        );
        renderer.sprite_atlas = self.sprite_atlas.clone();
        renderer
    }
}
