mod cosmic_text_system;
#[cfg(target_family = "wasm")]
mod html_island_texture;
#[cfg(target_family = "wasm")]
mod html_island_upload;
mod wgpu_atlas;
mod wgpu_context;
mod wgpu_renderer;

pub use cosmic_text_system::*;
#[cfg(target_family = "wasm")]
pub use html_island_texture::*;
#[cfg(target_family = "wasm")]
pub use html_island_upload::*;
pub use wgpu;
pub use wgpu_atlas::*;
pub use wgpu_context::*;
pub use wgpu_renderer::{GpuContext, WgpuRenderer, WgpuSurfaceConfig};

#[cfg(all(test, target_family = "wasm"))]
mod tests {
    use super::{HtmlIslandUploadPath, WgpuRenderer};

    #[test]
    fn html_island_upload_uses_intermediate_canvas_fallback_path() {
        assert_eq!(
            HtmlIslandUploadPath::IntermediateCanvas.as_str(),
            "intermediate-canvas"
        );
    }

    #[test]
    fn renderer_exposes_html_island_upload_entrypoint() {
        let _upload = WgpuRenderer::upload_html_island_element_to_texture;
    }
}
