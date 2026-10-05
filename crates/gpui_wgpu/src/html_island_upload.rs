use gpux::{DevicePixels, Size};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};

#[wasm_bindgen]
extern "C" {
    /// A `GPUQueue` viewed through Chrome's experimental HTML-in-canvas API, which wgpu does not
    /// bind.
    type ElementCopyQueue;

    #[wasm_bindgen(catch, method, structural, js_name = copyElementImageToTexture)]
    fn copy_element_image_to_texture(
        this: &ElementCopyQueue,
        source: &JsValue,
        destination: &JsValue,
        copy_size: &JsValue,
    ) -> Result<(), JsValue>;
}

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
    #[cfg(feature = "rayx_diagnostics")]
    pub path: HtmlIslandUploadPath,
    #[cfg(feature = "rayx_diagnostics")]
    pub texture_reallocated: bool,
}

impl Default for HtmlIslandUploadResult {
    fn default() -> Self {
        Self {
            #[cfg(feature = "rayx_diagnostics")]
            path: HtmlIslandUploadPath::CopyElementImageToTexture,
            #[cfg(feature = "rayx_diagnostics")]
            texture_reallocated: false,
        }
    }
}

pub fn upload_html_island_element_to_texture(
    queue: &wgpu::Queue,
    element: &web_sys::HtmlElement,
    texture: &wgpu::Texture,
    size: Size<DevicePixels>,
) -> anyhow::Result<HtmlIslandUploadResult> {
    #[cfg(feature = "rayx_diagnostics")]
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

    copy_element_image(queue, element, texture, width, height).map_err(|error| {
        anyhow::anyhow!(
            "GPUQueue.copyElementImageToTexture failed: {}",
            js_error_message(&error)
        )
    })?;

    Ok(HtmlIslandUploadResult {
        #[cfg(feature = "rayx_diagnostics")]
        path: HtmlIslandUploadPath::CopyElementImageToTexture,
        #[cfg(feature = "rayx_diagnostics")]
        texture_reallocated: false,
    })
}

/// Copies `element`, a live child of the GPUI canvas, into the top-left `width` x `height` texels
/// of `texture`'s first mip level as unpremultiplied sRGB.
fn copy_element_image(
    queue: &wgpu::Queue,
    element: &web_sys::HtmlElement,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<(), JsValue> {
    let (Some(queue), Some(texture)) = (queue.as_webgpu(), texture.as_webgpu()) else {
        return Err(JsValue::from_str(
            "HTML island uploads require the WebGPU backend",
        ));
    };
    let zero = JsValue::from(0u32);
    let source = js_object(&[("source", element.as_ref())])?;
    let origin = js_object(&[("x", &zero), ("y", &zero), ("z", &zero)])?;
    let target = js_object(&[
        ("texture", texture.as_ref()),
        ("mipLevel", &zero),
        ("origin", &origin),
        ("aspect", &JsValue::from_str("all")),
    ])?;
    let destination = js_object(&[
        ("destination", &target),
        ("colorSpace", &JsValue::from_str("srgb")),
        ("premultipliedAlpha", &JsValue::FALSE),
    ])?;
    let size = js_object(&[
        ("width", &JsValue::from(width)),
        ("height", &JsValue::from(height)),
        ("depthOrArrayLayers", &JsValue::from(1u32)),
    ])?;
    queue
        .unchecked_ref::<ElementCopyQueue>()
        .copy_element_image_to_texture(&source, &destination, &size)
}

fn js_object(fields: &[(&str, &JsValue)]) -> Result<JsValue, JsValue> {
    let object = js_sys::Object::new();
    for (key, value) in fields {
        js_sys::Reflect::set(&object, &JsValue::from_str(key), value)?;
    }
    Ok(object.into())
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

#[cfg(feature = "rayx_diagnostics")]
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
