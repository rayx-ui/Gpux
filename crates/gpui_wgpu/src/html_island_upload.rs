use gpux::{DevicePixels, Size};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlIslandUploadPath {
    CopyElementImageToTexture,
}

impl HtmlIslandUploadPath {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CopyElementImageToTexture => "copyElementImageToTexture",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtmlIslandUploadResult {
    pub path: HtmlIslandUploadPath,
    pub size: Size<DevicePixels>,
    pub texture_reallocated: bool,
    pub source_sample: [u8; 4],
}

pub fn upload_html_island_element_to_texture(
    queue: &wgpu::Queue,
    element: &web_sys::HtmlElement,
    texture: &wgpu::Texture,
    size: Size<DevicePixels>,
) -> anyhow::Result<HtmlIslandUploadResult> {
    if consume_injected_upload_failure(element) {
        anyhow::bail!("Injected HTML island direct upload failure");
    }

    let width = u32::try_from(size.width.0.max(0))
        .map_err(|_| anyhow::anyhow!("HTML island width exceeds u32"))?;
    let height = u32::try_from(size.height.0.max(0))
        .map_err(|_| anyhow::anyhow!("HTML island height exceeds u32"))?;

    anyhow::ensure!(
        width > 0 && height > 0,
        "HTML island upload requires non-zero dimensions"
    );
    let parent = element
        .parent_element()
        .ok_or_else(|| anyhow::anyhow!("HTML island source element has no parent canvas"))?;
    let _canvas = parent
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| {
            anyhow::anyhow!("HTML island source element must be a direct child of the GPUI canvas")
        })?;

    queue
        .copy_element_image_to_texture(
            element.as_ref(),
            wgpu::CopyExternalImageDestInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
                color_space: wgpu::PredefinedColorSpace::Srgb,
                premultiplied_alpha: false,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "GPUQueue.copyElementImageToTexture failed: {}",
                js_error_message(&error)
            )
        })?;

    Ok(HtmlIslandUploadResult {
        path: HtmlIslandUploadPath::CopyElementImageToTexture,
        size,
        texture_reallocated: false,
        source_sample: [0, 0, 0, 0],
    })
}

fn js_error_message(value: &wasm_bindgen::JsValue) -> String {
    if let Some(message) = value.as_string() {
        return message;
    }
    if let Some(error) = value.dyn_ref::<js_sys::Error>() {
        return error.message().into();
    }
    format!("{value:?}")
}

fn consume_injected_upload_failure(element: &web_sys::HtmlElement) -> bool {
    let global = js_sys::global();
    let Ok(value) = js_sys::Reflect::get(
        &global,
        &wasm_bindgen::JsValue::from_str("__rayxHtmlIslandInjectUploadFailureId"),
    ) else {
        return false;
    };
    let Some(requested_id) = value.as_string() else {
        return false;
    };
    if requested_id.is_empty() {
        return false;
    }

    let element_matches = element
        .get_attribute("data-rayx-html-island-id")
        .is_some_and(|id| id == requested_id);
    let descendant_matches = element
        .query_selector(&format!("[data-rayx-html-island-id=\"{requested_id}\"]"))
        .ok()
        .flatten()
        .is_some();
    if !element_matches && !descendant_matches {
        return false;
    }

    js_sys::Reflect::set(
        &global,
        &wasm_bindgen::JsValue::from_str("__rayxHtmlIslandInjectUploadFailureId"),
        &wasm_bindgen::JsValue::NULL,
    )
    .ok();
    true
}
