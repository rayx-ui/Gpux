use wasm_bindgen::{JsCast, JsValue};

#[derive(Clone, Debug, Default)]
pub struct HtmlInCanvasProbes {
    pub canvas_layout_subtree: Option<&'static str>,
    pub canvas_paint_event: Option<&'static str>,
    pub canvas_request_paint: Option<&'static str>,
    pub global_capture_element_image: Option<&'static str>,
    pub element_capture_element_image: Option<&'static str>,
    pub canvas_draw_element_image: Option<&'static str>,
    pub gpu_queue_copy_element_image_to_texture: Option<&'static str>,
    pub gpu_queue_copy_external_image_to_texture: Option<&'static str>,
}

impl HtmlInCanvasProbes {
    pub fn support(&self) -> HtmlInCanvasSupport {
        HtmlInCanvasSupport {
            layout_subtree: self.canvas_layout_subtree.is_some(),
            paint_event: self.canvas_paint_event.is_some(),
            request_paint: self.canvas_request_paint.is_some(),
            capture_element_image: self.global_capture_element_image.is_some()
                || self.element_capture_element_image.is_some(),
            draw_element_image: self.canvas_draw_element_image.is_some(),
            copy_element_image_to_texture: self.gpu_queue_copy_element_image_to_texture.is_some(),
            fallback_external_image_copy: self.gpu_queue_copy_external_image_to_texture.is_some(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HtmlInCanvasSupport {
    pub layout_subtree: bool,
    pub paint_event: bool,
    pub request_paint: bool,
    pub capture_element_image: bool,
    pub draw_element_image: bool,
    pub copy_element_image_to_texture: bool,
    pub fallback_external_image_copy: bool,
}

impl HtmlInCanvasSupport {
    pub fn preferred_direct_path_available(&self) -> bool {
        self.layout_subtree
            && self.paint_event
            && self.request_paint
            && self.capture_element_image
            && self.copy_element_image_to_texture
    }

    pub fn measured_fallback_path_available(&self) -> bool {
        self.layout_subtree
            && self.paint_event
            && self.request_paint
            && self.draw_element_image
            && self.fallback_external_image_copy
    }
}

#[derive(Clone, Debug, Default)]
pub struct HtmlInCanvasDiagnostics {
    pub probes: HtmlInCanvasProbes,
    pub support: HtmlInCanvasSupport,
}

pub fn detect_html_in_canvas() -> HtmlInCanvasDiagnostics {
    let probes = detect_probes();
    let support = probes.support();
    HtmlInCanvasDiagnostics { probes, support }
}

fn detect_probes() -> HtmlInCanvasProbes {
    let Some(window) = web_sys::window() else {
        return HtmlInCanvasProbes::default();
    };
    let Some(document) = window.document() else {
        return HtmlInCanvasProbes::default();
    };
    let Ok(canvas_element) = document.create_element("canvas") else {
        return HtmlInCanvasProbes::default();
    };
    let Ok(canvas) = canvas_element.dyn_into::<web_sys::HtmlCanvasElement>() else {
        return HtmlInCanvasProbes::default();
    };

    let canvas_value = canvas.as_ref();
    let canvas_context = canvas.get_context("2d").ok().flatten().map(JsValue::from);
    let global = js_sys::global();
    let element_prototype = get_property(&global, "Element")
        .and_then(|constructor| get_property(&constructor, "prototype"));
    let gpu_queue_prototype = get_property(&global, "GPUQueue")
        .and_then(|constructor| get_property(&constructor, "prototype"));

    let canvas_layout_subtree = if has_property(canvas_value, "layoutSubtree") {
        Some("layoutSubtree")
    } else if has_property(canvas_value, "layoutsubtree") {
        Some("layoutsubtree")
    } else {
        None
    };

    HtmlInCanvasProbes {
        canvas_layout_subtree,
        canvas_paint_event: has_property(canvas_value, "onpaint").then_some("paint"),
        canvas_request_paint: has_method(canvas_value, "requestPaint").then_some("requestPaint"),
        global_capture_element_image: has_method(&global, "captureElementImage")
            .then_some("captureElementImage"),
        element_capture_element_image: element_prototype
            .as_ref()
            .is_some_and(|prototype| has_method(prototype, "captureElementImage"))
            .then_some("Element.prototype.captureElementImage"),
        canvas_draw_element_image: canvas_context
            .as_ref()
            .is_some_and(|context| has_method(context, "drawElementImage"))
            .then_some("CanvasRenderingContext2D.drawElementImage"),
        gpu_queue_copy_element_image_to_texture: gpu_queue_prototype
            .as_ref()
            .is_some_and(|prototype| has_method(prototype, "copyElementImageToTexture"))
            .then_some("GPUQueue.copyElementImageToTexture"),
        gpu_queue_copy_external_image_to_texture: gpu_queue_prototype
            .as_ref()
            .is_some_and(|prototype| has_method(prototype, "copyExternalImageToTexture"))
            .then_some("GPUQueue.copyExternalImageToTexture"),
    }
}

fn get_property(owner: &JsValue, name: &str) -> Option<JsValue> {
    js_sys::Reflect::get(owner, &JsValue::from_str(name))
        .ok()
        .filter(|value| !value.is_null() && !value.is_undefined())
}

fn has_property(owner: &JsValue, name: &str) -> bool {
    js_sys::Reflect::has(owner, &JsValue::from_str(name)).unwrap_or(false)
}

fn has_method(owner: &JsValue, name: &str) -> bool {
    get_property(owner, name).is_some_and(|value| value.is_function())
}
