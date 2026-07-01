use gpux::{DevicePixels, Size};
use wasm_bindgen::{JsCast, JsValue};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlIslandUploadPath {
    CopyElementImageToTexture,
    IntermediateCanvas,
}

impl HtmlIslandUploadPath {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CopyElementImageToTexture => "copyElementImageToTexture",
            Self::IntermediateCanvas => "intermediate-canvas",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtmlIslandUploadResult {
    pub path: HtmlIslandUploadPath,
    pub size: Size<DevicePixels>,
    pub texture_reallocated: bool,
}

pub fn upload_html_island_element_to_texture(
    queue: &wgpu::Queue,
    element: &web_sys::HtmlElement,
    texture: &wgpu::Texture,
    size: Size<DevicePixels>,
) -> anyhow::Result<HtmlIslandUploadResult> {
    let width = u32::try_from(size.width.0.max(0))
        .map_err(|_| anyhow::anyhow!("HTML island width exceeds u32"))?;
    let height = u32::try_from(size.height.0.max(0))
        .map_err(|_| anyhow::anyhow!("HTML island height exceeds u32"))?;

    anyhow::ensure!(
        width > 0 && height > 0,
        "HTML island upload requires non-zero dimensions"
    );

    let canvas = draw_element_to_intermediate_canvas(element, width, height)?;
    queue.copy_external_image_to_texture(
        &wgpu::CopyExternalImageSourceInfo {
            source: wgpu::ExternalImageSource::HTMLCanvasElement(canvas.clone()),
            origin: wgpu::Origin2d::ZERO,
            flip_y: false,
        },
        wgpu::CopyExternalImageDestInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
            color_space: wgpu::PredefinedColorSpace::Srgb,
            premultiplied_alpha: true,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(HtmlIslandUploadResult {
        path: HtmlIslandUploadPath::IntermediateCanvas,
        size,
        texture_reallocated: false,
    })
}

fn draw_element_to_intermediate_canvas(
    element: &web_sys::HtmlElement,
    width: u32,
    height: u32,
) -> anyhow::Result<web_sys::HtmlCanvasElement> {
    let canvas = element
        .parent_element()
        .ok_or_else(|| anyhow::anyhow!("HTML island upload element has no parent canvas"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|error| anyhow::anyhow!("HTML island upload parent is not a canvas: {error:?}"))?;
    canvas.set_width(width);
    canvas.set_height(height);

    let context = canvas
        .get_context("2d")
        .map_err(|error| anyhow::anyhow!("Failed to query 2D context: {error:?}"))?
        .ok_or_else(|| anyhow::anyhow!("2D context unavailable for HTML island upload"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .map_err(|error| anyhow::anyhow!("Context is not CanvasRenderingContext2D: {error:?}"))?;

    context.clear_rect(0., 0., width as f64, height as f64);
    let draw_element_image =
        js_sys::Reflect::get(context.as_ref(), &JsValue::from_str("drawElementImage"))
            .map_err(|error| anyhow::anyhow!("Failed to query drawElementImage: {error:?}"))?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| anyhow::anyhow!("CanvasRenderingContext2D.drawElementImage missing"))?;
    draw_element_image
        .call3(
            context.as_ref(),
            element.as_ref(),
            &JsValue::from_f64(0.),
            &JsValue::from_f64(0.),
        )
        .map_err(|error| anyhow::anyhow!("drawElementImage failed: {error:?}"))?;

    Ok(canvas)
}
