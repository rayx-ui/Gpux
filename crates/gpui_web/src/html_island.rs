use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpux::{DevicePixels, ElementId, HtmlIslandInputMode, HtmlIslandPrimitive, Pixels, Size};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

pub(crate) struct WebHtmlIslandRegistry {
    canvas: web_sys::HtmlCanvasElement,
    roots: HashMap<String, WebHtmlIslandRoot>,
    dirty_roots: HashSet<String>,
    created_root_count: u32,
    pruned_root_count: u32,
    content_update_count: u32,
    dirty_mark_count: u32,
    upload_count: u32,
    texture_reallocation_count: u32,
    texture_count: u32,
    texture_pruned_count: u32,
    texture_draw_count: u32,
    upload_source_sample: [u8; 4],
    renderer_path: &'static str,
    renderer_last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WebHtmlIslandSyncSummary {
    pub(crate) created_roots: u32,
    pub(crate) content_updates: u32,
    pub(crate) pruned_roots: u32,
    pub(crate) dirty_marks: u32,
    pub(crate) request_paint: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct WebHtmlIslandDiagnostics {
    pub(crate) count: u32,
    pub(crate) dirty_count: u32,
    pub(crate) created_root_count: u32,
    pub(crate) pruned_root_count: u32,
    pub(crate) content_update_count: u32,
    pub(crate) dirty_mark_count: u32,
    pub(crate) upload_count: u32,
    pub(crate) texture_reallocation_count: u32,
    pub(crate) texture_count: u32,
    pub(crate) texture_pruned_count: u32,
    pub(crate) texture_draw_count: u32,
    pub(crate) upload_source_sample: [u8; 4],
    pub(crate) renderer_path: &'static str,
    pub(crate) renderer_last_error: Option<String>,
}

impl Default for WebHtmlIslandDiagnostics {
    fn default() -> Self {
        Self {
            count: 0,
            dirty_count: 0,
            created_root_count: 0,
            pruned_root_count: 0,
            content_update_count: 0,
            dirty_mark_count: 0,
            upload_count: 0,
            texture_reallocation_count: 0,
            texture_count: 0,
            texture_pruned_count: 0,
            texture_draw_count: 0,
            upload_source_sample: [0, 0, 0, 0],
            renderer_path: "unsupported",
            renderer_last_error: None,
        }
    }
}

pub(crate) struct WebHtmlIslandUploadJob {
    pub(crate) id: String,
    pub(crate) element: web_sys::HtmlElement,
    pub(crate) physical_size: Size<DevicePixels>,
}

struct WebHtmlIslandRoot {
    element: web_sys::HtmlElement,
    content_element: web_sys::HtmlElement,
    input_mode: HtmlIslandInputMode,
    physical_size: Size<DevicePixels>,
    content_revision: u64,
    upload_canvas: Option<web_sys::HtmlCanvasElement>,
    upload_element: Option<web_sys::HtmlElement>,
    upload_ready: Rc<Cell<bool>>,
    upload_paint_listener: Option<Closure<dyn FnMut(JsValue)>>,
    upload_delay_frames: u8,
}

#[cfg(test)]
struct WebHtmlIslandRootSnapshot {
    left: String,
    top: String,
    width: String,
    height: String,
    pointer_events: String,
    opacity: String,
    overflow: String,
    contain: String,
    inner_html: String,
}

impl WebHtmlIslandRegistry {
    pub(crate) fn new(canvas: web_sys::HtmlCanvasElement) -> Self {
        Self {
            canvas,
            roots: HashMap::default(),
            dirty_roots: HashSet::default(),
            created_root_count: 0,
            pruned_root_count: 0,
            content_update_count: 0,
            dirty_mark_count: 0,
            upload_count: 0,
            texture_reallocation_count: 0,
            texture_count: 0,
            texture_pruned_count: 0,
            texture_draw_count: 0,
            upload_source_sample: [0, 0, 0, 0],
            renderer_path: "unsupported",
            renderer_last_error: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn root_count(&self) -> usize {
        self.roots.len()
    }

    #[cfg(test)]
    pub(crate) fn contains_root_for_id(&self, id: &ElementId) -> bool {
        self.roots.contains_key(&registry_key(id))
    }

    #[cfg(test)]
    pub(crate) fn dirty_count(&self) -> usize {
        self.dirty_roots.len()
    }

    pub(crate) fn diagnostics(&self) -> WebHtmlIslandDiagnostics {
        WebHtmlIslandDiagnostics {
            count: self.roots.len() as u32,
            dirty_count: self.dirty_roots.len() as u32,
            created_root_count: self.created_root_count,
            pruned_root_count: self.pruned_root_count,
            content_update_count: self.content_update_count,
            dirty_mark_count: self.dirty_mark_count,
            upload_count: self.upload_count,
            texture_reallocation_count: self.texture_reallocation_count,
            texture_count: self.texture_count,
            texture_pruned_count: self.texture_pruned_count,
            texture_draw_count: self.texture_draw_count,
            upload_source_sample: self.upload_source_sample,
            renderer_path: self.renderer_path,
            renderer_last_error: self.renderer_last_error.clone(),
        }
    }

    pub(crate) fn input_mode_for_event_target(
        &self,
        target: Option<&web_sys::EventTarget>,
    ) -> Option<HtmlIslandInputMode> {
        let element = target?.dyn_ref::<web_sys::Element>()?.clone();
        let id = find_island_root_id(element)?;
        self.roots.get(&id).map(|root| root.input_mode)
    }

    pub(crate) fn dirty_upload_jobs(&mut self) -> Vec<WebHtmlIslandUploadJob> {
        let dirty_ids = self.dirty_roots.iter().cloned().collect::<Vec<_>>();
        let mut jobs = Vec::new();
        for id in dirty_ids {
            let Some(root) = self.roots.get_mut(&id) else {
                continue;
            };
            if root.upload_delay_frames > 0 {
                root.upload_delay_frames -= 1;
                if let Some(canvas) = root.upload_canvas.as_ref() {
                    request_paint(canvas);
                }
                request_paint(&self.canvas);
                continue;
            }
            if !root.upload_ready.get() {
                if let Some(canvas) = root.upload_canvas.as_ref() {
                    request_paint(canvas);
                }
                request_paint(&self.canvas);
                continue;
            }
            let Some(element) = root.upload_element.as_ref() else {
                continue;
            };
            jobs.push(WebHtmlIslandUploadJob {
                id,
                element: element.clone(),
                physical_size: root.physical_size,
            });
        }
        jobs
    }

    pub(crate) fn record_upload_success(
        &mut self,
        id: &str,
        renderer_path: &'static str,
        texture_reallocated: bool,
        source_sample: [u8; 4],
    ) {
        self.renderer_path = renderer_path;
        self.renderer_last_error = None;
        self.upload_source_sample = source_sample;
        if let Some(root) = self.roots.get_mut(id) {
            root.clear_upload_canvas();
        }
        if self.dirty_roots.remove(id) {
            self.upload_count = self.upload_count.saturating_add(1);
        }
        if texture_reallocated {
            self.texture_reallocation_count = self.texture_reallocation_count.saturating_add(1);
        }
    }

    pub(crate) fn record_upload_failure(&mut self, error: String) {
        self.renderer_last_error = Some(error);
    }

    pub(crate) fn record_texture_cache_state(
        &mut self,
        texture_count: u32,
        pruned_count: u32,
        draw_count: u32,
    ) {
        self.texture_count = texture_count;
        self.texture_pruned_count = pruned_count;
        self.texture_draw_count = draw_count;
    }

    pub(crate) fn sync_roots(
        &mut self,
        islands: &[HtmlIslandPrimitive],
    ) -> WebHtmlIslandSyncSummary {
        let mut summary = WebHtmlIslandSyncSummary::default();
        let mut live_ids = HashSet::with_capacity(islands.len());
        for island in islands {
            let key = registry_key(&island.id);
            live_ids.insert(key.clone());

            let (root, is_new_root) = if self.roots.contains_key(&key) {
                (self.roots.get_mut(&key).unwrap(), false)
            } else {
                let root = create_root(&self.canvas, &key);
                summary.created_roots = summary.created_roots.saturating_add(1);
                self.created_root_count = self.created_root_count.saturating_add(1);
                (self.roots.entry(key.clone()).or_insert(root), true)
            };

            root.sync_layout_and_mode(island);

            if is_new_root || root.content_revision != island.content_revision {
                root.content_element.set_inner_html(&island.trusted_html);
                root.content_revision = island.content_revision;
                root.prepare_upload_canvas(&self.canvas);
                summary.content_updates = summary.content_updates.saturating_add(1);
                self.content_update_count = self.content_update_count.saturating_add(1);
                if self.dirty_roots.insert(key) {
                    summary.dirty_marks = summary.dirty_marks.saturating_add(1);
                    self.dirty_mark_count = self.dirty_mark_count.saturating_add(1);
                }
                summary.request_paint = true;
            }
        }

        summary.pruned_roots = self.prune_except(&live_ids);
        self.pruned_root_count = self.pruned_root_count.saturating_add(summary.pruned_roots);
        summary
    }

    pub(crate) fn mark_dirty_from_paint_event(&mut self, event: &JsValue) -> usize {
        let changed_elements = changed_elements(event);
        if changed_elements.is_empty() {
            let old_len = self.dirty_roots.len();
            self.dirty_roots.extend(self.roots.keys().cloned());
            return self.dirty_roots.len().saturating_sub(old_len);
        }

        let old_len = self.dirty_roots.len();
        for element in changed_elements {
            if let Some(id) = find_island_root_id(element) {
                if self.roots.contains_key(&id) {
                    if self.dirty_roots.insert(id) {
                        self.dirty_mark_count = self.dirty_mark_count.saturating_add(1);
                    }
                }
            }
        }
        self.dirty_roots.len().saturating_sub(old_len)
    }

    fn prune_except(&mut self, live_ids: &HashSet<String>) -> u32 {
        let stale_ids = self
            .roots
            .keys()
            .filter(|id| !live_ids.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let pruned_count = stale_ids.len() as u32;
        for id in stale_ids {
            if let Some(mut root) = self.roots.remove(&id) {
                root.clear_upload_canvas();
                root.element.remove();
            }
            self.dirty_roots.remove(&id);
        }
        pruned_count
    }

    #[cfg(test)]
    fn sync_roots_for_testing(
        &mut self,
        islands: &[HtmlIslandPrimitive],
    ) -> WebHtmlIslandSyncSummary {
        self.sync_roots(islands)
    }

    #[cfg(test)]
    fn mark_dirty_from_paint_event_for_testing(&mut self, event: &JsValue) {
        self.mark_dirty_from_paint_event(event);
    }

    #[cfg(test)]
    fn input_mode_for_event_target_for_testing(
        &self,
        target: Option<&web_sys::EventTarget>,
    ) -> Option<HtmlIslandInputMode> {
        self.input_mode_for_event_target(target)
    }

    #[cfg(test)]
    fn dirty_upload_jobs_for_testing(&mut self) -> Vec<WebHtmlIslandUploadJob> {
        self.dirty_upload_jobs()
    }

    #[cfg(test)]
    fn root_snapshot_for_testing(&self, id: &ElementId) -> Option<WebHtmlIslandRootSnapshot> {
        let root = self.roots.get(&registry_key(id))?;
        let style = root.element.style();
        Some(WebHtmlIslandRootSnapshot {
            left: style.get_property_value("left").ok()?,
            top: style.get_property_value("top").ok()?,
            width: style.get_property_value("width").ok()?,
            height: style.get_property_value("height").ok()?,
            pointer_events: style.get_property_value("pointer-events").ok()?,
            opacity: style.get_property_value("opacity").ok()?,
            overflow: style.get_property_value("overflow").ok()?,
            contain: style.get_property_value("contain").ok()?,
            inner_html: root.content_element.inner_html(),
        })
    }
}

impl WebHtmlIslandRoot {
    fn clear_upload_canvas(&mut self) {
        if let Some(canvas) = self.upload_canvas.take() {
            canvas.remove();
        }
        self.upload_element = None;
        self.upload_paint_listener = None;
        self.upload_ready.set(false);
        self.upload_delay_frames = 0;
    }

    fn prepare_upload_canvas(&mut self, host_canvas: &web_sys::HtmlCanvasElement) {
        self.clear_upload_canvas();
        self.upload_ready.set(false);

        let Some(document) = host_canvas.owner_document() else {
            return;
        };
        let Ok(canvas_element) = document.create_element("canvas") else {
            return;
        };
        let Ok(canvas) = canvas_element.dyn_into::<web_sys::HtmlCanvasElement>() else {
            return;
        };

        let width = self.physical_size.width.0.max(0) as u32;
        let height = self.physical_size.height.0.max(0) as u32;
        if width == 0 || height == 0 {
            return;
        }

        canvas.set_width(width);
        canvas.set_height(height);
        canvas.set_attribute("layoutsubtree", "true").ok();
        js_sys::Reflect::set(
            canvas.as_ref(),
            &JsValue::from_str("layoutSubtree"),
            &JsValue::TRUE,
        )
        .ok();
        canvas
            .set_attribute(
                "style",
                &format!(
                    "position:fixed;left:0;top:0;width:{width}px;height:{height}px;pointer-events:none;"
                ),
            )
            .ok();

        let Ok(upload_node) = self.element.clone_node_with_deep(true) else {
            return;
        };
        let Ok(upload_element) = upload_node.dyn_into::<web_sys::HtmlElement>() else {
            return;
        };
        upload_element
            .set_attribute(
                "style",
                &format!(
                    "position:absolute;left:0;top:0;width:{width}px;height:{height}px;opacity:1;pointer-events:none;"
                ),
            )
            .ok();

        if canvas.append_child(&upload_element).is_err() {
            return;
        }
        let upload_ready = Rc::clone(&self.upload_ready);
        let paint_canvas = canvas.clone();
        let paint_element = upload_element.clone();
        let paint_listener = Closure::<dyn FnMut(JsValue)>::new(move |_| {
            if draw_upload_element_to_canvas(&paint_canvas, &paint_element, width, height) {
                upload_ready.set(true);
            }
        });
        canvas
            .add_event_listener_with_callback("paint", paint_listener.as_ref().unchecked_ref())
            .ok();
        let Some(body) = document.body() else {
            return;
        };
        if body
            .insert_before(&canvas, Some(host_canvas.as_ref()))
            .is_err()
        {
            return;
        }

        request_paint(&canvas);
        self.upload_canvas = Some(canvas);
        self.upload_element = Some(upload_element);
        self.upload_paint_listener = Some(paint_listener);
        self.upload_delay_frames = 2;
    }

    fn sync_layout_and_mode(&mut self, island: &HtmlIslandPrimitive) {
        self.input_mode = island.input_mode;
        self.physical_size = Size {
            width: DevicePixels(island.bounds.size.width.0.round().max(0.) as i32),
            height: DevicePixels(island.bounds.size.height.0.round().max(0.) as i32),
        };
        let style = self.element.style();
        style
            .set_property("left", &css_px(island.logical_bounds.left()))
            .ok();
        style
            .set_property("top", &css_px(island.logical_bounds.top()))
            .ok();
        style
            .set_property("width", &css_px(island.logical_bounds.size.width))
            .ok();
        style
            .set_property("height", &css_px(island.logical_bounds.size.height))
            .ok();
        style
            .set_property(
                "pointer-events",
                match island.input_mode {
                    HtmlIslandInputMode::DomOwned => "auto",
                    HtmlIslandInputMode::GpuiOwned => "none",
                },
            )
            .ok();
        self.element
            .set_attribute("data-rayx-html-island-opacity", &island.opacity.to_string())
            .ok();

        self.element
            .set_attribute(
                "data-rayx-html-island-content-revision",
                &island.content_revision.to_string(),
            )
            .ok();
        self.element
            .set_attribute(
                "data-rayx-html-island-physical-width",
                &island.bounds.size.width.0.to_string(),
            )
            .ok();
        self.element
            .set_attribute(
                "data-rayx-html-island-physical-height",
                &island.bounds.size.height.0.to_string(),
            )
            .ok();
    }
}

fn registry_key(id: &ElementId) -> String {
    id.to_string()
}

fn css_px(value: Pixels) -> String {
    let value = f32::from(value);
    if value.fract() == 0. {
        format!("{}px", value as i32)
    } else {
        format!("{}px", value)
    }
}

fn changed_elements(event: &JsValue) -> Vec<web_sys::Element> {
    js_sys::Reflect::get(event, &JsValue::from_str("changedElements"))
        .ok()
        .map(|value| js_sys::Array::from(&value))
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.dyn_into::<web_sys::Element>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn find_island_root_id(mut element: web_sys::Element) -> Option<String> {
    loop {
        if let Some(id) = element.get_attribute("data-rayx-html-island-id") {
            return Some(id);
        }

        element = element.parent_element()?;
    }
}

fn create_root(canvas: &web_sys::HtmlCanvasElement, id: &str) -> WebHtmlIslandRoot {
    let document = canvas
        .owner_document()
        .expect("HTML island canvas should belong to a document");
    let element = document
        .create_element("div")
        .expect("HTML island root element should be creatable")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HTML island root should be an HtmlElement");

    element.set_attribute("data-rayx-html-island-id", id).ok();
    element.set_attribute("aria-hidden", "true").ok();
    let style = element.style();
    style.set_property("position", "absolute").ok();
    style.set_property("display", "block").ok();
    style.set_property("box-sizing", "border-box").ok();
    style.set_property("contain", "layout style paint").ok();
    style.set_property("overflow", "hidden").ok();
    style.set_property("opacity", "0").ok();

    let content_element = document
        .create_element("div")
        .expect("HTML island content element should be creatable")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HTML island content host should be an HtmlElement");
    content_element
        .set_attribute("data-rayx-html-island-content", "true")
        .ok();
    let content_style = content_element.style();
    content_style.set_property("width", "100%").ok();
    content_style.set_property("height", "100%").ok();

    element.append_child(&content_element).ok();
    canvas.append_child(&element).ok();

    WebHtmlIslandRoot {
        element,
        content_element,
        input_mode: HtmlIslandInputMode::GpuiOwned,
        physical_size: Size::default(),
        content_revision: 0,
        upload_canvas: None,
        upload_element: None,
        upload_ready: Rc::new(Cell::new(false)),
        upload_paint_listener: None,
        upload_delay_frames: 0,
    }
}

fn request_paint(canvas: &web_sys::HtmlCanvasElement) -> bool {
    let Ok(request_paint) =
        js_sys::Reflect::get(canvas.as_ref(), &JsValue::from_str("requestPaint"))
    else {
        return false;
    };
    let Ok(request_paint) = request_paint.dyn_into::<js_sys::Function>() else {
        return false;
    };
    request_paint.call0(canvas.as_ref()).is_ok()
}

fn draw_upload_element_to_canvas(
    canvas: &web_sys::HtmlCanvasElement,
    element: &web_sys::HtmlElement,
    width: u32,
    height: u32,
) -> bool {
    let Some(context) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|context| context.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
    else {
        return false;
    };
    context.clear_rect(0., 0., width as f64, height as f64);
    let Some(draw_element_image) =
        js_sys::Reflect::get(context.as_ref(), &JsValue::from_str("drawElementImage"))
            .ok()
            .and_then(|value| value.dyn_into::<js_sys::Function>().ok())
    else {
        return false;
    };
    if draw_element_image
        .call3(
            context.as_ref(),
            element.as_ref(),
            &JsValue::from_f64(0.),
            &JsValue::from_f64(0.),
        )
        .is_err()
    {
        return false;
    }
    context.get_image_data(0., 0., 1., 1.).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpux::{ContentMask, ElementId, HtmlIslandInputMode, HtmlIslandPrimitive};
    use wasm_bindgen::JsCast;

    #[test]
    fn registry_public_surface_accepts_canvas_and_prunes_by_stable_id() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        let island = test_island("settings-form");

        registry.sync_roots_for_testing(&[island.clone()]);
        assert_eq!(registry.root_count(), 1);
        assert!(registry.contains_root_for_id(&island.id));

        registry.sync_roots_for_testing(&[]);
        assert_eq!(registry.root_count(), 0);
    }

    #[test]
    fn registry_syncs_logical_bounds_content_and_input_mode() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        let mut island = test_island("settings-form");
        island.logical_bounds = gpux::Bounds::new(
            gpux::point(gpux::px(8.), gpux::px(13.)),
            gpux::size(gpux::px(144.), gpux::px(55.)),
        );
        island.input_mode = HtmlIslandInputMode::DomOwned;
        island.trusted_html = "<input value=\"ready\">".into();

        registry.sync_roots_for_testing(&[island.clone()]);

        let snapshot = registry.root_snapshot_for_testing(&island.id).unwrap();
        assert_eq!(snapshot.left, "8px");
        assert_eq!(snapshot.top, "13px");
        assert_eq!(snapshot.width, "144px");
        assert_eq!(snapshot.height, "55px");
        assert_eq!(snapshot.pointer_events, "auto");
        assert_eq!(snapshot.inner_html, "<input value=\"ready\">");
    }

    #[test]
    fn registry_roots_are_not_visible_dom_overlays() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        let island = test_island("visual-card");

        registry.sync_roots_for_testing(&[island.clone()]);

        let snapshot = registry.root_snapshot_for_testing(&island.id).unwrap();
        assert_eq!(snapshot.opacity, "0");
        assert_eq!(snapshot.overflow, "hidden");
        assert_eq!(snapshot.contain, "layout style paint");
    }

    #[test]
    fn gpui_owned_islands_do_not_capture_browser_pointer_events() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        let mut island = test_island("gpui-owned-card");
        island.input_mode = HtmlIslandInputMode::GpuiOwned;

        registry.sync_roots_for_testing(&[island.clone()]);

        let snapshot = registry.root_snapshot_for_testing(&island.id).unwrap();
        assert_eq!(snapshot.pointer_events, "none");
    }

    #[test]
    fn registry_tracks_dirty_islands_from_paint_events() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        let island = test_island("dirty-card");
        registry.sync_roots_for_testing(&[island.clone()]);

        registry.mark_dirty_from_paint_event_for_testing(&wasm_bindgen::JsValue::NULL);

        assert_eq!(registry.dirty_count(), 1);
    }

    #[test]
    fn registry_sync_reports_when_request_paint_is_needed() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        let island = test_island("first-paint");

        let summary = registry.sync_roots_for_testing(&[island]);

        assert!(summary.request_paint);
    }

    #[test]
    fn registry_diagnostics_report_lifecycle_counters() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        registry.sync_roots_for_testing(&[test_island("diagnostic-card")]);
        registry.sync_roots_for_testing(&[]);

        let diagnostics = registry.diagnostics();

        assert_eq!(diagnostics.count, 0);
        assert_eq!(diagnostics.created_root_count, 1);
        assert_eq!(diagnostics.pruned_root_count, 1);
        assert_eq!(diagnostics.content_update_count, 1);
        assert_eq!(diagnostics.dirty_mark_count, 1);
    }

    #[test]
    fn registry_exposes_input_mode_for_event_target_lookup() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let registry = WebHtmlIslandRegistry::new(canvas);
        let target: web_sys::EventTarget = wasm_bindgen::JsValue::NULL.unchecked_into();

        let _input_mode = registry.input_mode_for_event_target_for_testing(Some(&target));
    }

    #[test]
    fn registry_exposes_dirty_upload_jobs_for_renderer() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        registry.sync_roots_for_testing(&[test_island("upload-card")]);

        let _jobs = registry.dirty_upload_jobs_for_testing();
    }

    #[test]
    fn registry_records_renderer_upload_diagnostics() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        registry.sync_roots_for_testing(&[test_island("upload-card")]);

        registry.record_upload_success("upload-card", "intermediate-canvas", true, [1, 2, 3, 4]);
        let diagnostics = registry.diagnostics();
        assert_eq!(diagnostics.upload_count, 1);
        assert_eq!(diagnostics.texture_reallocation_count, 1);
        assert_eq!(diagnostics.upload_source_sample, [1, 2, 3, 4]);
        assert_eq!(diagnostics.renderer_path, "intermediate-canvas");
        assert_eq!(diagnostics.renderer_last_error, None);

        registry.record_texture_cache_state(1, 0, 1);
        let diagnostics = registry.diagnostics();
        assert_eq!(diagnostics.texture_count, 1);
        assert_eq!(diagnostics.texture_pruned_count, 0);
        assert_eq!(diagnostics.texture_draw_count, 1);

        registry.record_upload_failure("drawElementImage failed".to_string());
        let diagnostics = registry.diagnostics();
        assert_eq!(
            diagnostics.renderer_last_error.as_deref(),
            Some("drawElementImage failed")
        );
    }

    fn test_island(id: &'static str) -> HtmlIslandPrimitive {
        HtmlIslandPrimitive {
            order: 0,
            id: ElementId::from(id),
            bounds: Default::default(),
            logical_bounds: Default::default(),
            content_mask: ContentMask {
                bounds: Default::default(),
            },
            input_mode: HtmlIslandInputMode::GpuiOwned,
            opacity: 1.,
            content_revision: 1,
            trusted_html: "".into(),
        }
    }
}
