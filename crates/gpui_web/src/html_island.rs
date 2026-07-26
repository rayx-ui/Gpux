use std::collections::{HashMap, HashSet};

use gpux::{DevicePixels, ElementId, HtmlIslandInputMode, HtmlIslandPrimitive, Pixels, Size};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

pub(crate) struct WebHtmlIslandRegistry {
    canvas: web_sys::HtmlCanvasElement,
    roots: HashMap<String, WebHtmlIslandRoot>,
    dirty_roots: HashSet<String>,
    #[cfg(feature = "rayx_diagnostics")]
    created_root_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    pruned_root_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    content_update_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    dirty_mark_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    upload_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    upload_failure_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    texture_reallocation_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    texture_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    texture_pruned_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    texture_draw_count: u32,
    #[cfg(feature = "rayx_diagnostics")]
    renderer_path: &'static str,
    #[cfg(feature = "rayx_diagnostics")]
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

#[cfg(feature = "rayx_diagnostics")]
#[derive(Clone, Debug)]
pub(crate) struct WebHtmlIslandDiagnostics {
    pub(crate) count: u32,
    pub(crate) dirty_count: u32,
    pub(crate) created_root_count: u32,
    pub(crate) pruned_root_count: u32,
    pub(crate) content_update_count: u32,
    pub(crate) dirty_mark_count: u32,
    pub(crate) upload_count: u32,
    pub(crate) upload_failure_count: u32,
    pub(crate) texture_reallocation_count: u32,
    pub(crate) texture_count: u32,
    pub(crate) texture_pruned_count: u32,
    pub(crate) texture_draw_count: u32,
    pub(crate) renderer_path: &'static str,
    pub(crate) renderer_last_error: Option<String>,
}

#[cfg(feature = "rayx_diagnostics")]
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
            upload_failure_count: 0,
            texture_reallocation_count: 0,
            texture_count: 0,
            texture_pruned_count: 0,
            texture_draw_count: 0,
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
    paint_ready: bool,
    _dom_dirty_listeners: Vec<Closure<dyn FnMut(web_sys::Event)>>,
    next_focused_direct_upload_at_ms: f64,
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
    transform: String,
    transform_origin: String,
    inner_html: String,
}

impl WebHtmlIslandRegistry {
    pub(crate) fn new(canvas: web_sys::HtmlCanvasElement) -> Self {
        Self {
            canvas,
            roots: HashMap::default(),
            dirty_roots: HashSet::default(),
            #[cfg(feature = "rayx_diagnostics")]
            created_root_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            pruned_root_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            content_update_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            dirty_mark_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            upload_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            upload_failure_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            texture_reallocation_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            texture_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            texture_pruned_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            texture_draw_count: 0,
            #[cfg(feature = "rayx_diagnostics")]
            renderer_path: "unsupported",
            #[cfg(feature = "rayx_diagnostics")]
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

    #[cfg(feature = "rayx_diagnostics")]
    pub(crate) fn diagnostics(&self) -> WebHtmlIslandDiagnostics {
        WebHtmlIslandDiagnostics {
            count: self.roots.len() as u32,
            dirty_count: self.dirty_roots.len() as u32,
            created_root_count: self.created_root_count,
            pruned_root_count: self.pruned_root_count,
            content_update_count: self.content_update_count,
            dirty_mark_count: self.dirty_mark_count,
            upload_count: self.upload_count,
            upload_failure_count: self.upload_failure_count,
            texture_reallocation_count: self.texture_reallocation_count,
            texture_count: self.texture_count,
            texture_pruned_count: self.texture_pruned_count,
            texture_draw_count: self.texture_draw_count,
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
        self.mark_dirty_from_dom_state();

        let dirty_ids = self.dirty_roots.iter().cloned().collect::<Vec<_>>();
        let mut jobs = Vec::new();
        let mut skipped_pending_paint = false;
        for id in dirty_ids {
            let Some(root) = self.roots.get_mut(&id) else {
                continue;
            };
            if !root.paint_ready {
                skipped_pending_paint = true;
                continue;
            }
            jobs.push(WebHtmlIslandUploadJob {
                id,
                element: root.element.clone(),
                physical_size: root.physical_size,
            });
        }
        if skipped_pending_paint {
            request_paint(&self.canvas);
        }
        jobs
    }

    pub(crate) fn has_pending_upload_work(&self) -> bool {
        !self.dirty_roots.is_empty()
    }

    pub(crate) fn record_upload_success(&mut self, id: &str) -> bool {
        self.dirty_roots.remove(id)
    }

    #[cfg(feature = "rayx_diagnostics")]
    pub(crate) fn record_upload_diagnostics_success(
        &mut self,
        uploaded_dirty_root: bool,
        renderer_path: &'static str,
        texture_reallocated: bool,
    ) {
        self.renderer_path = renderer_path;
        if self.upload_failure_count == 0 {
            self.renderer_last_error = None;
        }
        if uploaded_dirty_root {
            self.upload_count = self.upload_count.saturating_add(1);
        }
        if texture_reallocated {
            self.texture_reallocation_count = self.texture_reallocation_count.saturating_add(1);
        }
    }

    pub(crate) fn record_upload_failure(&mut self, id: &str) {
        self.dirty_roots.remove(id);
    }

    #[cfg(feature = "rayx_diagnostics")]
    pub(crate) fn record_upload_diagnostics_failure(&mut self, error: String) {
        self.upload_failure_count = self.upload_failure_count.saturating_add(1);
        self.renderer_path = "copyElementImageToTexture";
        self.renderer_last_error = Some(error);
    }

    pub(crate) fn defer_upload_until_next_paint(&mut self, id: &str) {
        let Some(root) = self.roots.get_mut(id) else {
            return;
        };
        root.paint_ready = false;
        if self.dirty_roots.insert(id.to_string()) {
            #[cfg(feature = "rayx_diagnostics")]
            {
                self.dirty_mark_count = self.dirty_mark_count.saturating_add(1);
            }
        }
        request_paint(&self.canvas);
    }

    #[cfg(feature = "rayx_diagnostics")]
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
                #[cfg(feature = "rayx_diagnostics")]
                {
                    self.created_root_count = self.created_root_count.saturating_add(1);
                }
                (self.roots.entry(key.clone()).or_insert(root), true)
            };

            root.sync_layout_and_mode(island);

            if is_new_root || root.content_revision != island.content_revision {
                root.content_element.set_inner_html(&island.trusted_html);
                root.content_revision = island.content_revision;
                root.paint_ready = false;
                summary.content_updates = summary.content_updates.saturating_add(1);
                #[cfg(feature = "rayx_diagnostics")]
                {
                    self.content_update_count = self.content_update_count.saturating_add(1);
                }
                if self.dirty_roots.insert(key) {
                    summary.dirty_marks = summary.dirty_marks.saturating_add(1);
                    #[cfg(feature = "rayx_diagnostics")]
                    {
                        self.dirty_mark_count = self.dirty_mark_count.saturating_add(1);
                    }
                }
                summary.request_paint = true;
            }
        }

        summary.pruned_roots = self.prune_except(&live_ids);
        #[cfg(feature = "rayx_diagnostics")]
        {
            self.pruned_root_count = self.pruned_root_count.saturating_add(summary.pruned_roots);
        }
        summary
    }

    pub(crate) fn mark_dirty_from_paint_event(&mut self, event: &JsValue) -> usize {
        let changed_elements = changed_elements(event);
        if changed_elements.is_empty() {
            let old_len = self.dirty_roots.len();
            for root in self.roots.values_mut() {
                root.paint_ready = true;
            }
            self.dirty_roots.extend(self.roots.keys().cloned());
            return self.dirty_roots.len().saturating_sub(old_len);
        }

        let old_len = self.dirty_roots.len();
        for element in changed_elements {
            if let Some(id) = find_island_root_id(element) {
                if let Some(root) = self.roots.get_mut(&id) {
                    root.paint_ready = true;
                    if self.dirty_roots.insert(id) {
                        #[cfg(feature = "rayx_diagnostics")]
                        {
                            self.dirty_mark_count = self.dirty_mark_count.saturating_add(1);
                        }
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
            if let Some(root) = self.roots.remove(&id) {
                root.element.remove();
            }
            self.dirty_roots.remove(&id);
        }
        pruned_count
    }

    pub(crate) fn mark_dirty_from_dom_state(&mut self) -> bool {
        let active_root_id = self
            .canvas
            .owner_document()
            .and_then(|document| document.active_element())
            .and_then(find_island_root_id);
        let now_ms = js_sys::Date::now();
        let mut marked_dirty = false;
        let mut request_paint_needed = false;

        for (id, root) in self.roots.iter_mut() {
            let dom_dirty = root.take_dom_dirty();
            let focused = root.input_mode == HtmlIslandInputMode::DomOwned
                && active_root_id.as_deref() == Some(id.as_str());
            let focused_upload_due = root.focused_upload_due(focused, now_ms);
            if dom_dirty || focused_upload_due {
                root.paint_ready = false;
                request_paint_needed = true;
                if self.dirty_roots.insert(id.clone()) {
                    #[cfg(feature = "rayx_diagnostics")]
                    {
                        self.dirty_mark_count = self.dirty_mark_count.saturating_add(1);
                    }
                }
                marked_dirty = true;
            }
        }
        if request_paint_needed {
            request_paint(&self.canvas);
        }
        marked_dirty
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
            transform: style.get_property_value("transform").ok()?,
            transform_origin: style.get_property_value("transform-origin").ok()?,
            inner_html: root.content_element.inner_html(),
        })
    }
}

impl WebHtmlIslandRoot {
    fn take_dom_dirty(&self) -> bool {
        if !self
            .element
            .has_attribute("data-rayx-html-island-dom-dirty")
        {
            return false;
        }

        self.element
            .remove_attribute("data-rayx-html-island-dom-dirty")
            .ok();
        true
    }

    fn focused_upload_due(&mut self, focused: bool, now_ms: f64) -> bool {
        if !focused {
            self.next_focused_direct_upload_at_ms = 0.;
            return false;
        }

        if self.next_focused_direct_upload_at_ms == 0.
            || now_ms >= self.next_focused_direct_upload_at_ms
        {
            self.next_focused_direct_upload_at_ms = now_ms + 250.;
            true
        } else {
            false
        }
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
                "transform",
                &format!(
                    "translate({}, {})",
                    css_px(island.logical_bounds.left()),
                    css_px(island.logical_bounds.top())
                ),
            )
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
    style.set_property("opacity", "1").ok();
    style.set_property("transform-origin", "0 0").ok();

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
    let dom_dirty_listeners = attach_dom_dirty_listeners(&element, canvas);
    canvas.append_child(&element).ok();

    WebHtmlIslandRoot {
        element,
        content_element,
        input_mode: HtmlIslandInputMode::GpuiOwned,
        physical_size: Size::default(),
        content_revision: 0,
        paint_ready: false,
        _dom_dirty_listeners: dom_dirty_listeners,
        next_focused_direct_upload_at_ms: 0.,
    }
}

fn attach_dom_dirty_listeners(
    element: &web_sys::HtmlElement,
    canvas: &web_sys::HtmlCanvasElement,
) -> Vec<Closure<dyn FnMut(web_sys::Event)>> {
    [
        "beforeinput",
        "input",
        "change",
        "compositionupdate",
        "compositionend",
        "keydown",
        "keyup",
        "pointerup",
        "focusin",
        "focusout",
        "select",
    ]
    .into_iter()
    .map(|event_name| {
        let dirty_element = element.clone();
        let paint_canvas = canvas.clone();
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            mark_dom_dirty(&dirty_element, &paint_canvas);
        });
        element
            .add_event_listener_with_callback(event_name, listener.as_ref().unchecked_ref())
            .ok();
        listener
    })
    .collect()
}

fn mark_dom_dirty(element: &web_sys::HtmlElement, canvas: &web_sys::HtmlCanvasElement) {
    element
        .set_attribute("data-rayx-html-island-dom-dirty", "true")
        .ok();
    request_paint(canvas);
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
        assert_eq!(snapshot.transform, "translate(8px, 13px)");
        assert_eq!(snapshot.transform_origin, "0 0");
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
        assert_eq!(snapshot.opacity, "1");
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

    #[cfg(feature = "rayx_diagnostics")]
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

    #[cfg(feature = "rayx_diagnostics")]
    #[test]
    fn registry_records_renderer_upload_diagnostics() {
        let canvas: web_sys::HtmlCanvasElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let mut registry = WebHtmlIslandRegistry::new(canvas);
        registry.sync_roots_for_testing(&[test_island("upload-card")]);

        let uploaded_dirty_root = registry.record_upload_success("upload-card");
        registry.record_upload_diagnostics_success(
            uploaded_dirty_root,
            "copyElementImageToTexture",
            true,
        );
        let diagnostics = registry.diagnostics();
        assert_eq!(diagnostics.upload_count, 1);
        assert_eq!(diagnostics.upload_failure_count, 0);
        assert_eq!(diagnostics.texture_reallocation_count, 1);
        assert_eq!(diagnostics.renderer_path, "copyElementImageToTexture");
        assert_eq!(diagnostics.renderer_last_error, None);

        registry.record_texture_cache_state(1, 0, 1);
        let diagnostics = registry.diagnostics();
        assert_eq!(diagnostics.texture_count, 1);
        assert_eq!(diagnostics.texture_pruned_count, 0);
        assert_eq!(diagnostics.texture_draw_count, 1);
        registry.record_upload_failure("upload-card");
        registry.record_upload_diagnostics_failure("direct upload failed".to_string());
        let diagnostics = registry.diagnostics();
        assert_eq!(diagnostics.upload_failure_count, 1);
        assert_eq!(
            diagnostics.renderer_last_error.as_deref(),
            Some("direct upload failed")
        );
    }

    fn test_island(id: &'static str) -> HtmlIslandPrimitive {
        HtmlIslandPrimitive {
            order: 0,
            id: ElementId::from(id),
            bounds: Default::default(),
            logical_bounds: Default::default(),
            content_mask: ContentMask::rectangular(Default::default()),
            input_mode: HtmlIslandInputMode::GpuiOwned,
            opacity: 1.,
            content_revision: 1,
            trusted_html: "".into(),
        }
    }
}
