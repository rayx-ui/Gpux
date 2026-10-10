//! Renderers for the extra surfaces of a window that uses window composition
//! (zed-industries/zed#62379).
//!
//! Adapted from gpui-fast's `gpui_wgpu/src/fast/composition.rs`. A platform
//! that stacks GPUI overlays as surfaces of their own (Wayland subsurfaces,
//! X11 child windows, DirectComposition visuals) draws each with a renderer
//! of its own, created here from the window's renderer: the two share the GPU
//! context and the sprite atlas, so atlas tiles rasterized for the window are
//! valid on every surface, and the compiled pipelines when the surfaces take
//! the same format. Overlays are transparent, premultiplied and cleared
//! transparent; text painted onto them is grayscale (see `gpux::fast`).

#![cfg(not(target_family = "wasm"))]

use std::rc::Rc;

use raw_window_handle::HasWindowHandle;

use crate::{WgpuRenderer, WgpuSurfaceConfig};

impl WgpuRenderer {
    /// Creates a renderer for another surface of the same window, sharing
    /// this renderer's GPU context, sprite atlas and, for the same format,
    /// its pipelines. A `transparent` surface is configured premultiplied.
    pub fn new_sharing<W>(
        &self,
        window: &W,
        mut config: WgpuSurfaceConfig,
        transparent: bool,
    ) -> anyhow::Result<Self>
    where
        W: HasWindowHandle,
    {
        let gpu_context = self
            .context
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("renderer has no shared GPU context"))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;
        let context_ref = gpu_context.borrow();
        let context = context_ref
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("GPU context is not initialized"))?;
        let surface =
            crate::wgpu_renderer::create_surface(&context.instance, window_handle.as_raw())?;
        context.check_compatible_with_surface(&surface)?;
        config.transparent = transparent;
        let mut renderer = Self::new_internal(
            Some(Rc::clone(gpu_context)),
            context,
            surface,
            config,
            self.compositor_gpu,
            self.atlas.clone(),
            self.backend_policy,
            Some(self),
        )?;
        renderer.set_subpixel_layout(self.is_bgr);
        Ok(renderer)
    }

    /// Moves this renderer onto another surface of the same GPU context,
    /// keeping its pipelines and sprite atlas, which are much slower to create
    /// than a surface.
    pub fn replace_surface_sharing_context<W>(
        &mut self,
        window: &W,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<()>
    where
        W: HasWindowHandle,
    {
        let gpu_context = Rc::clone(
            self.context
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("renderer has no shared GPU context"))?,
        );
        let context_ref = gpu_context.borrow();
        let context = context_ref
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("GPU context is not initialized"))?;
        self.replace_surface(window, config, &context.instance)
    }
}
