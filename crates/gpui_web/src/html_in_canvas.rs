use std::{cell::RefCell, rc::Rc};

#[cfg(feature = "diagnostics")]
use std::cell::Cell;

#[cfg(feature = "diagnostics")]
use crate::html_island::WebHtmlIslandDiagnostics;
use crate::html_island::WebHtmlIslandRegistry;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

#[derive(Clone, Debug, Default)]
pub struct HtmlInCanvasProbes {
    pub canvas_layout_subtree: Option<&'static str>,
    pub canvas_paint_event: Option<&'static str>,
    pub canvas_request_paint: Option<&'static str>,
    #[cfg(feature = "diagnostics")]
    pub global_capture_element_image: Option<&'static str>,
    #[cfg(feature = "diagnostics")]
    pub element_capture_element_image: Option<&'static str>,
    #[cfg(feature = "diagnostics")]
    pub canvas_draw_element_image: Option<&'static str>,
    #[cfg(feature = "diagnostics")]
    pub gpu_queue_copy_element_image_to_texture: Option<&'static str>,
    #[cfg(feature = "diagnostics")]
    pub gpu_queue_copy_external_image_to_texture: Option<&'static str>,
}

impl HtmlInCanvasProbes {
    fn support(&self) -> HtmlInCanvasSupport {
        HtmlInCanvasSupport {
            layout_subtree: self.canvas_layout_subtree.is_some(),
            paint_event: self.canvas_paint_event.is_some(),
            request_paint: self.canvas_request_paint.is_some(),
            #[cfg(feature = "diagnostics")]
            capture_element_image: self.global_capture_element_image.is_some()
                || self.element_capture_element_image.is_some(),
            #[cfg(feature = "diagnostics")]
            draw_element_image: self.canvas_draw_element_image.is_some(),
            #[cfg(feature = "diagnostics")]
            copy_element_image_to_texture: self.gpu_queue_copy_element_image_to_texture.is_some(),
            #[cfg(feature = "diagnostics")]
            copy_external_image_to_texture: self.gpu_queue_copy_external_image_to_texture.is_some(),
        }
    }

    #[cfg(feature = "diagnostics")]
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

#[derive(Clone, Copy, Debug, Default)]
pub struct HtmlInCanvasSupport {
    pub layout_subtree: bool,
    pub paint_event: bool,
    pub request_paint: bool,
    #[cfg(feature = "diagnostics")]
    pub capture_element_image: bool,
    #[cfg(feature = "diagnostics")]
    pub draw_element_image: bool,
    #[cfg(feature = "diagnostics")]
    pub copy_element_image_to_texture: bool,
    #[cfg(feature = "diagnostics")]
    pub copy_external_image_to_texture: bool,
}

impl HtmlInCanvasSupport {
    #[cfg(feature = "diagnostics")]
    fn direct_child_path_available(&self) -> bool {
        self.layout_subtree
            && self.paint_event
            && self.request_paint
            && self.copy_element_image_to_texture
    }

    #[cfg(feature = "diagnostics")]
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
            "copyExternalImageToTexture",
            JsValue::from_bool(self.copy_external_image_to_texture),
        );
        object.into()
    }
}

#[cfg(feature = "diagnostics")]
#[derive(Clone, Debug, Default)]
pub struct HtmlInCanvasDiagnostics {
    pub probes: HtmlInCanvasProbes,
    pub support: HtmlInCanvasSupport,
}

#[cfg(feature = "diagnostics")]
#[derive(Clone, Debug, Default)]
pub struct HtmlInCanvasRuntimeDiagnostics {
    pub request_paint_count: u32,
    pub paint_event_count: u32,
    pub changed_element_count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct HtmlInCanvasCapabilities {
    probes: HtmlInCanvasProbes,
    support: HtmlInCanvasSupport,
}

#[cfg(feature = "diagnostics")]
impl HtmlInCanvasCapabilities {
    pub(crate) fn diagnostics(&self) -> HtmlInCanvasDiagnostics {
        HtmlInCanvasDiagnostics {
            probes: self.probes.clone(),
            support: self.support,
        }
    }
}

#[derive(Debug, Default)]
pub struct HtmlInCanvasPaintObserver {
    #[cfg(feature = "diagnostics")]
    counters: Rc<HtmlInCanvasRuntimeCounters>,
    _paint_listener: Option<Closure<dyn FnMut(JsValue)>>,
}

#[cfg(feature = "diagnostics")]
#[derive(Debug, Default)]
struct HtmlInCanvasRuntimeCounters {
    request_paint_count: Cell<u32>,
    paint_event_count: Cell<u32>,
    changed_element_count: Cell<u32>,
}

impl HtmlInCanvasPaintObserver {
    pub fn start(
        capabilities: &HtmlInCanvasCapabilities,
        canvas: &web_sys::HtmlCanvasElement,
        layout_subtree_enabled: bool,
        html_island_registry: Option<Rc<RefCell<WebHtmlIslandRegistry>>>,
    ) -> Self {
        #[cfg(feature = "diagnostics")]
        let counters = Rc::new(HtmlInCanvasRuntimeCounters::default());
        let mut observer = Self {
            #[cfg(feature = "diagnostics")]
            counters: Rc::clone(&counters),
            _paint_listener: None,
        };

        if capabilities.support.paint_event {
            let listener_canvas = canvas.clone();
            #[cfg(feature = "diagnostics")]
            let diagnostic_capabilities = capabilities.clone();
            #[cfg(feature = "diagnostics")]
            let diagnostics_canvas = canvas.clone();
            #[cfg(feature = "diagnostics")]
            let listener_counters = Rc::clone(&counters);
            let closure = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
                #[cfg(feature = "diagnostics")]
                {
                    listener_counters
                        .paint_event_count
                        .set(listener_counters.paint_event_count.get().saturating_add(1));
                }

                let mut registry = html_island_registry
                    .as_ref()
                    .and_then(|registry| registry.try_borrow_mut().ok());
                if let Some(registry) = registry.as_mut() {
                    registry.mark_dirty_from_paint_event(&event);
                }

                #[cfg(feature = "diagnostics")]
                {
                    listener_counters.changed_element_count.set(
                        listener_counters
                            .changed_element_count
                            .get()
                            .saturating_add(changed_elements_count(&event)),
                    );
                    let island_diagnostics =
                        registry.as_deref().map(|registry| registry.diagnostics());
                    write_html_in_canvas_diagnostics_to_global(
                        &diagnostic_capabilities.diagnostics(),
                        &diagnostics_canvas,
                        layout_subtree_is_enabled(&diagnostics_canvas, &diagnostic_capabilities),
                        &listener_counters.snapshot(),
                        island_diagnostics.as_ref(),
                    );
                }
            });
            listener_canvas
                .add_event_listener_with_callback("paint", closure.as_ref().unchecked_ref())
                .ok();
            observer._paint_listener = Some(closure);
        }

        if layout_subtree_enabled && capabilities.support.request_paint {
            let requested = request_paint(canvas);
            #[cfg(feature = "diagnostics")]
            if requested {
                counters
                    .request_paint_count
                    .set(counters.request_paint_count.get().saturating_add(1));
            }
            #[cfg(not(feature = "diagnostics"))]
            let _ = requested;
        }

        observer
    }

    #[cfg(feature = "diagnostics")]
    pub fn snapshot(&self) -> HtmlInCanvasRuntimeDiagnostics {
        self.counters.snapshot()
    }

    pub fn request_paint(&self, canvas: &web_sys::HtmlCanvasElement) -> bool {
        let requested = request_paint(canvas);
        #[cfg(feature = "diagnostics")]
        if requested {
            self.counters
                .request_paint_count
                .set(self.counters.request_paint_count.get().saturating_add(1));
        }
        requested
    }
}

#[cfg(feature = "diagnostics")]
impl HtmlInCanvasRuntimeCounters {
    fn snapshot(&self) -> HtmlInCanvasRuntimeDiagnostics {
        HtmlInCanvasRuntimeDiagnostics {
            request_paint_count: self.request_paint_count.get(),
            paint_event_count: self.paint_event_count.get(),
            changed_element_count: self.changed_element_count.get(),
        }
    }
}

pub fn detect_html_in_canvas() -> HtmlInCanvasCapabilities {
    let probes = detect_probes();
    let support = probes.support();
    HtmlInCanvasCapabilities { probes, support }
}

#[cfg(feature = "diagnostics")]
pub fn write_html_in_canvas_diagnostics_to_global(
    diagnostics: &HtmlInCanvasDiagnostics,
    canvas: &web_sys::HtmlCanvasElement,
    layout_subtree_enabled: bool,
    runtime: &HtmlInCanvasRuntimeDiagnostics,
    island_diagnostics: Option<&WebHtmlIslandDiagnostics>,
) {
    let object = js_sys::Object::new();
    set_property(&object, "version", JsValue::from_f64(1.0));
    set_property(&object, "source", JsValue::from_str("gpui_web"));
    set_property(
        &object,
        "available",
        JsValue::from_bool(diagnostics.support.direct_child_path_available()),
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

    let island_diagnostics = island_diagnostics.cloned().unwrap_or_default();
    let islands = js_sys::Object::new();
    set_property(
        &islands,
        "count",
        JsValue::from_f64(island_diagnostics.count as f64),
    );
    set_property(
        &islands,
        "dirtyCount",
        JsValue::from_f64(island_diagnostics.dirty_count as f64),
    );
    set_property(
        &islands,
        "createdRootCount",
        JsValue::from_f64(island_diagnostics.created_root_count as f64),
    );
    set_property(
        &islands,
        "prunedRootCount",
        JsValue::from_f64(island_diagnostics.pruned_root_count as f64),
    );
    set_property(
        &islands,
        "contentUpdateCount",
        JsValue::from_f64(island_diagnostics.content_update_count as f64),
    );
    set_property(
        &islands,
        "dirtyMarkCount",
        JsValue::from_f64(island_diagnostics.dirty_mark_count as f64),
    );
    set_property(
        &islands,
        "uploadCount",
        JsValue::from_f64(island_diagnostics.upload_count as f64),
    );
    set_property(
        &islands,
        "uploadFailureCount",
        JsValue::from_f64(island_diagnostics.upload_failure_count as f64),
    );
    set_property(
        &islands,
        "textureReallocationCount",
        JsValue::from_f64(island_diagnostics.texture_reallocation_count as f64),
    );
    set_property(
        &islands,
        "textureCount",
        JsValue::from_f64(island_diagnostics.texture_count as f64),
    );
    set_property(
        &islands,
        "texturePrunedCount",
        JsValue::from_f64(island_diagnostics.texture_pruned_count as f64),
    );
    set_property(
        &islands,
        "textureDrawCount",
        JsValue::from_f64(island_diagnostics.texture_draw_count as f64),
    );
    set_property(&object, "islands", islands.into());

    let renderer = js_sys::Object::new();
    set_property(
        &renderer,
        "path",
        JsValue::from_str(island_diagnostics.renderer_path),
    );
    set_property(
        &renderer,
        "lastError",
        island_diagnostics
            .renderer_last_error
            .as_deref()
            .map(JsValue::from_str)
            .unwrap_or(JsValue::NULL),
    );
    set_property(&object, "renderer", renderer.into());
    gpui_wgpu::web_globals::publish("HtmlIslandDiagnostics", &object.into());
}

pub fn enable_layout_subtree_if_requested(
    browser_window: &web_sys::Window,
    canvas: &web_sys::HtmlCanvasElement,
    capabilities: &HtmlInCanvasCapabilities,
) -> bool {
    if !experiment_enabled(browser_window) {
        return false;
    }

    enable_layout_subtree(canvas, capabilities)
}

pub fn enable_layout_subtree(
    canvas: &web_sys::HtmlCanvasElement,
    capabilities: &HtmlInCanvasCapabilities,
) -> bool {
    if !capabilities.support.layout_subtree
        || !capabilities.support.paint_event
        || !capabilities.support.request_paint
    {
        return false;
    }

    let Some(property_name) = capabilities.probes.canvas_layout_subtree else {
        return false;
    };
    if canvas.set_attribute("layoutsubtree", "true").is_err() {
        return false;
    }

    js_sys::Reflect::set(
        canvas.as_ref(),
        &JsValue::from_str(property_name),
        &JsValue::TRUE,
    )
    .ok();
    layout_subtree_is_enabled(canvas, capabilities)
}

fn layout_subtree_is_enabled(
    canvas: &web_sys::HtmlCanvasElement,
    capabilities: &HtmlInCanvasCapabilities,
) -> bool {
    canvas.has_attribute("layoutsubtree")
        || capabilities
            .probes
            .canvas_layout_subtree
            .is_some_and(|property_name| {
                js_sys::Reflect::get(canvas.as_ref(), &JsValue::from_str(property_name))
                    .ok()
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
            })
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
    #[cfg(feature = "diagnostics")]
    let canvas_context = canvas.get_context("2d").ok().flatten().map(JsValue::from);
    #[cfg(feature = "diagnostics")]
    let global = js_sys::global();
    #[cfg(feature = "diagnostics")]
    let element_prototype = get_property(&global, "Element")
        .and_then(|constructor| get_property(&constructor, "prototype"));
    #[cfg(feature = "diagnostics")]
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
        #[cfg(feature = "diagnostics")]
        global_capture_element_image: has_method(&global, "captureElementImage")
            .then_some("captureElementImage"),
        #[cfg(feature = "diagnostics")]
        element_capture_element_image: element_prototype
            .as_ref()
            .is_some_and(|prototype| has_method(prototype, "captureElementImage"))
            .then_some("Element.prototype.captureElementImage"),
        #[cfg(feature = "diagnostics")]
        canvas_draw_element_image: canvas_context
            .as_ref()
            .is_some_and(|context| has_method(context, "drawElementImage"))
            .then_some("CanvasRenderingContext2D.drawElementImage"),
        #[cfg(feature = "diagnostics")]
        gpu_queue_copy_element_image_to_texture: gpu_queue_prototype
            .as_ref()
            .is_some_and(|prototype| has_method(prototype, "copyElementImageToTexture"))
            .then_some("GPUQueue.copyElementImageToTexture"),
        #[cfg(feature = "diagnostics")]
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

#[cfg(feature = "diagnostics")]
fn changed_elements_count(event: &JsValue) -> u32 {
    get_property(event, "changedElements")
        .map(|value| js_sys::Array::from(&value))
        .map(|array| array.length())
        .unwrap_or(0)
}

#[cfg(feature = "diagnostics")]
fn set_optional_string(owner: &js_sys::Object, name: &str, value: Option<&str>) {
    set_property(
        owner,
        name,
        value.map(JsValue::from_str).unwrap_or(JsValue::NULL),
    );
}

#[cfg(feature = "diagnostics")]
fn set_property(owner: &js_sys::Object, name: &str, value: JsValue) {
    js_sys::Reflect::set(owner, &JsValue::from_str(name), &value).ok();
}

#[cfg(feature = "diagnostics")]
fn child_element_count(canvas: &web_sys::HtmlCanvasElement) -> u32 {
    js_sys::Reflect::get(canvas.as_ref(), &JsValue::from_str("childElementCount"))
        .ok()
        .and_then(|value| value.as_f64())
        .map(|value| value as u32)
        .unwrap_or(0)
}

fn experiment_enabled(browser_window: &web_sys::Window) -> bool {
    // The page sets `__gpuxHtmlIslandEnabled` (or the legacy `__rayxHtmlIslandEnabled`) from
    // the URL before WASM starts; the URL itself is the fallback.
    if gpui_wgpu::web_globals::flag("HtmlIslandEnabled") {
        return true;
    }

    browser_window
        .location()
        .search()
        .ok()
        .is_some_and(|search| {
            query_flag_enabled(&search, "gpux-html-island")
                || query_flag_enabled(&search, "rayx-html-island")
        })
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

#[cfg(all(test, feature = "diagnostics"))]
mod tests {
    use super::*;
    use crate::html_island::WebHtmlIslandDiagnostics;
    use wasm_bindgen::JsCast;

    #[test]
    fn diagnostics_writer_accepts_island_lifecycle_counters() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let runtime = HtmlInCanvasRuntimeDiagnostics::default();
        write_html_in_canvas_diagnostics_to_global(
            &HtmlInCanvasDiagnostics::default(),
            &canvas,
            false,
            &runtime,
            Some(&WebHtmlIslandDiagnostics::default()),
        );
    }
}
