mod cosmic_text_system;
#[cfg(feature = "rayx_diagnostics")]
mod frame_telemetry;
#[cfg(target_family = "wasm")]
mod html_island_texture;
#[cfg(target_family = "wasm")]
mod html_island_upload;
mod wgpu_atlas;
mod wgpu_context;
mod wgpu_renderer;

pub use cosmic_text_system::*;
#[cfg(feature = "rayx_diagnostics")]
pub use frame_telemetry::*;
#[cfg(target_family = "wasm")]
pub use html_island_texture::*;
#[cfg(target_family = "wasm")]
pub use html_island_upload::*;
pub use wgpu;
pub use wgpu_atlas::*;
pub use wgpu_context::*;
#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
pub use wgpu_renderer::WgpuHeadlessRenderer;
pub use wgpu_renderer::{GpuContext, WgpuRenderer, WgpuSurfaceConfig};

#[cfg(all(test, target_family = "wasm"))]
mod tests {
    use super::{HtmlIslandUploadPath, WgpuRenderer};

    #[test]
    fn html_island_upload_uses_copy_element_image_to_texture_path() {
        assert_eq!(
            HtmlIslandUploadPath::CopyElementImageToTexture.as_str(),
            "copyElementImageToTexture"
        );
    }

    #[test]
    fn renderer_exposes_html_island_upload_entrypoint() {
        let _upload = WgpuRenderer::upload_html_island_element_to_texture;
    }
}
