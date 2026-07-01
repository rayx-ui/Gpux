use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

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

#[derive(Clone, Debug, Default)]
pub struct HtmlInCanvasRuntimeDiagnostics {
    pub request_paint_count: u32,
    pub paint_event_count: u32,
    pub changed_element_count: u32,
}

#[derive(Debug, Default)]
pub struct HtmlInCanvasPaintObserver {
    counters: Rc<HtmlInCanvasRuntimeCounters>,
    _paint_listener: Option<Closure<dyn FnMut(JsValue)>>,
}

#[derive(Debug, Default)]
struct HtmlInCanvasRuntimeCounters {
    request_paint_count: Cell<u32>,
    paint_event_count: Cell<u32>,
    changed_element_count: Cell<u32>,
}

impl HtmlInCanvasPaintObserver {
    pub fn start(
        diagnostics: &HtmlInCanvasDiagnostics,
        canvas: &web_sys::HtmlCanvasElement,
        layout_subtree_enabled: bool,
    ) -> Self {
        let counters = Rc::new(HtmlInCanvasRuntimeCounters::default());
        let mut observer = Self {
            counters: Rc::clone(&counters),
            _paint_listener: None,
        };

        if layout_subtree_enabled && diagnostics.support.paint_event {
            let diagnostics = diagnostics.clone();
            let listener_canvas = canvas.clone();
            let diagnostics_canvas = canvas.clone();
            let listener_counters = Rc::clone(&counters);
            let closure = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
                listener_counters
                    .paint_event_count
                    .set(listener_counters.paint_event_count.get().saturating_add(1));
                listener_counters.changed_element_count.set(
                    listener_counters
                        .changed_element_count
                        .get()
                        .saturating_add(changed_elements_count(&event)),
                );
                write_html_in_canvas_diagnostics_to_global(
                    &diagnostics,
                    &diagnostics_canvas,
                    layout_subtree_enabled,
                    &listener_counters.snapshot(),
                );
            });
            listener_canvas
                .add_event_listener_with_callback("paint", closure.as_ref().unchecked_ref())
                .ok();
            observer._paint_listener = Some(closure);
        }

        if layout_subtree_enabled && diagnostics.support.request_paint && request_paint(canvas) {
            counters
                .request_paint_count
                .set(counters.request_paint_count.get().saturating_add(1));
        }

        observer
    }

    pub fn snapshot(&self) -> HtmlInCanvasRuntimeDiagnostics {
        self.counters.snapshot()
    }
}

impl HtmlInCanvasRuntimeCounters {
    fn snapshot(&self) -> HtmlInCanvasRuntimeDiagnostics {
        HtmlInCanvasRuntimeDiagnostics {
            request_paint_count: self.request_paint_count.get(),
            paint_event_count: self.paint_event_count.get(),
            changed_element_count: self.changed_element_count.get(),
        }
    }
}

pub fn detect_html_in_canvas() -> HtmlInCanvasDiagnostics {
    let probes = detect_probes();
    let support = probes.support();
    HtmlInCanvasDiagnostics { probes, support }
}

pub fn write_html_in_canvas_diagnostics_to_global(
    diagnostics: &HtmlInCanvasDiagnostics,
    canvas: &web_sys::HtmlCanvasElement,
    layout_subtree_enabled: bool,
    runtime: &HtmlInCanvasRuntimeDiagnostics,
) {
    let object = js_sys::Object::new();
    set_property(&object, "version", JsValue::from_f64(1.0));
    set_property(&object, "source", JsValue::from_str("gpui_web"));
    set_property(
        &object,
        "available",
        JsValue::from_bool(
            diagnostics.support.preferred_direct_path_available()
                || diagnostics.support.measured_fallback_path_available(),
        ),
    );
    set_property(&object, "probes", diagnostics.probes.to_js_object());
    set_property(&object, "support", diagnostics.support.to_js_object());

    let canvas_diagnostics = js_sys::Object::new();
    set_property(
        &canvas_diagnostics,
        "layoutSubtreeEnabled",
        JsValue::from_bool(layout_subtree_enabled),
    );
    set_property(
        &canvas_diagnostics,
        "childCount",
        JsValue::from_f64(child_element_count(canvas) as f64),
    );
    set_property(
        &canvas_diagnostics,
        "requestPaintCount",
        JsValue::from_f64(runtime.request_paint_count as f64),
    );
    set_property(
        &canvas_diagnostics,
        "paintEventCount",
        JsValue::from_f64(runtime.paint_event_count as f64),
    );
    set_property(
        &canvas_diagnostics,
        "changedElementCount",
        JsValue::from_f64(runtime.changed_element_count as f64),
    );
    set_property(&object, "canvas", canvas_diagnostics.into());

    let islands = js_sys::Object::new();
    set_property(&islands, "count", JsValue::from_f64(0.0));
    set_property(&islands, "dirtyCount", JsValue::from_f64(0.0));
    set_property(&islands, "uploadCount", JsValue::from_f64(0.0));
    set_property(&islands, "textureReallocationCount", JsValue::from_f64(0.0));
    set_property(&object, "islands", islands.into());

    let renderer = js_sys::Object::new();
    set_property(&renderer, "path", JsValue::from_str("unsupported"));
    set_property(&renderer, "lastError", JsValue::NULL);
    set_property(&object, "renderer", renderer.into());

    set_property(
        &js_sys::global().unchecked_into::<js_sys::Object>(),
        "__rayxHtmlIslandDiagnostics",
        object.into(),
    );
}

pub fn enable_layout_subtree_if_requested(
    browser_window: &web_sys::Window,
    canvas: &web_sys::HtmlCanvasElement,
    diagnostics: &HtmlInCanvasDiagnostics,
) -> bool {
    if !experiment_enabled(browser_window)
        || !diagnostics.support.layout_subtree
        || !diagnostics.support.paint_event
        || !diagnostics.support.request_paint
    {
        return false;
    }

    let Some(property_name) = diagnostics.probes.canvas_layout_subtree else {
        return false;
    };

    if canvas.set_attribute("layoutsubtree", "true").is_err() {
        return false;
    }

    let property = JsValue::from_str(property_name);
    js_sys::Reflect::set(canvas.as_ref(), &property, &JsValue::TRUE).ok();

    canvas.has_attribute("layoutsubtree")
        || js_sys::Reflect::get(canvas.as_ref(), &property)
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
}

impl HtmlInCanvasProbes {
    fn to_js_object(&self) -> JsValue {
        let object = js_sys::Object::new();
        set_optional_string(&object, "canvasLayoutSubtree", self.canvas_layout_subtree);
        set_optional_string(&object, "canvasPaintEvent", self.canvas_paint_event);
        set_optional_string(&object, "canvasRequestPaint", self.canvas_request_paint);
        set_optional_string(
            &object,
            "globalCaptureElementImage",
            self.global_capture_element_image,
        );
        set_optional_string(
            &object,
            "elementCaptureElementImage",
            self.element_capture_element_image,
        );
        set_optional_string(
            &object,
            "canvasDrawElementImage",
            self.canvas_draw_element_image,
        );
        set_optional_string(
            &object,
            "gpuQueueCopyElementImageToTexture",
            self.gpu_queue_copy_element_image_to_texture,
        );
        set_optional_string(
            &object,
            "gpuQueueCopyExternalImageToTexture",
            self.gpu_queue_copy_external_image_to_texture,
        );
        object.into()
    }
}

impl HtmlInCanvasSupport {
    fn to_js_object(self) -> JsValue {
        let object = js_sys::Object::new();
        set_property(
            &object,
            "layoutSubtree",
            JsValue::from_bool(self.layout_subtree),
        );
        set_property(&object, "paintEvent", JsValue::from_bool(self.paint_event));
        set_property(
            &object,
            "requestPaint",
            JsValue::from_bool(self.request_paint),
        );
        set_property(
            &object,
            "captureElementImage",
            JsValue::from_bool(self.capture_element_image),
        );
        set_property(
            &object,
            "drawElementImage",
            JsValue::from_bool(self.draw_element_image),
        );
        set_property(
            &object,
            "copyElementImageToTexture",
            JsValue::from_bool(self.copy_element_image_to_texture),
        );
        set_property(
            &object,
            "fallbackExternalImageCopy",
            JsValue::from_bool(self.fallback_external_image_copy),
        );
        object.into()
    }
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

fn request_paint(canvas: &web_sys::HtmlCanvasElement) -> bool {
    get_property(canvas.as_ref(), "requestPaint")
        .and_then(|value| value.dyn_into::<js_sys::Function>().ok())
        .and_then(|request_paint| request_paint.call0(canvas.as_ref()).ok())
        .is_some()
}

fn changed_elements_count(event: &JsValue) -> u32 {
    get_property(event, "changedElements")
        .map(|value| js_sys::Array::from(&value))
        .map(|array| array.length())
        .unwrap_or(0)
}

fn set_optional_string(owner: &js_sys::Object, name: &str, value: Option<&str>) {
    let value = value.map(JsValue::from_str).unwrap_or(JsValue::NULL);
    set_property(owner, name, value);
}

fn set_property(owner: &js_sys::Object, name: &str, value: JsValue) {
    js_sys::Reflect::set(owner, &JsValue::from_str(name), &value).ok();
}

fn child_element_count(canvas: &web_sys::HtmlCanvasElement) -> u32 {
    js_sys::Reflect::get(canvas.as_ref(), &JsValue::from_str("childElementCount"))
        .ok()
        .and_then(|value| value.as_f64())
        .map(|value| value as u32)
        .unwrap_or(0)
}

fn experiment_enabled(browser_window: &web_sys::Window) -> bool {
    let global_enabled = js_sys::Reflect::get(
        &js_sys::global(),
        &JsValue::from_str("__rayxHtmlIslandEnabled"),
    )
    .ok()
    .and_then(|value| value.as_bool())
    .unwrap_or(false);
    if global_enabled {
        return true;
    }

    browser_window
        .location()
        .search()
        .ok()
        .is_some_and(|search| query_flag_enabled(&search, "rayx-html-island"))
}

fn query_flag_enabled(search: &str, name: &str) -> bool {
    search.trim_start_matches('?').split('&').any(|pair| {
        let mut parts = pair.splitn(2, '=');
        parts.next() == Some(name)
            && parts
                .next()
                .is_some_and(|value| value == "1" || value == "true")
    })
}
