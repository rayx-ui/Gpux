use std::collections::{HashMap, HashSet};

use gpux::{ElementId, HtmlIslandInputMode, HtmlIslandPrimitive, Pixels};
use wasm_bindgen::{JsCast, JsValue};

pub(crate) struct WebHtmlIslandRegistry {
    canvas: web_sys::HtmlCanvasElement,
    roots: HashMap<String, WebHtmlIslandRoot>,
    dirty_roots: HashSet<String>,
    created_root_count: u32,
    pruned_root_count: u32,
    content_update_count: u32,
    dirty_mark_count: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WebHtmlIslandSyncSummary {
    pub(crate) created_roots: u32,
    pub(crate) content_updates: u32,
    pub(crate) pruned_roots: u32,
    pub(crate) dirty_marks: u32,
    pub(crate) request_paint: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WebHtmlIslandDiagnostics {
    pub(crate) count: u32,
    pub(crate) dirty_count: u32,
    pub(crate) created_root_count: u32,
    pub(crate) pruned_root_count: u32,
    pub(crate) content_update_count: u32,
    pub(crate) dirty_mark_count: u32,
    pub(crate) upload_count: u32,
    pub(crate) texture_reallocation_count: u32,
}

struct WebHtmlIslandRoot {
    element: web_sys::HtmlElement,
    content_element: web_sys::HtmlElement,
    content_revision: u64,
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
        }
    }

    pub(crate) fn root_count(&self) -> usize {
        self.roots.len()
    }

    pub(crate) fn contains_root_for_id(&self, id: &ElementId) -> bool {
        self.roots.contains_key(&registry_key(id))
    }

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
            upload_count: 0,
            texture_reallocation_count: 0,
        }
    }

    pub(crate) fn sync_roots(&mut self, islands: &[HtmlIslandPrimitive]) -> WebHtmlIslandSyncSummary {
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
        self.pruned_root_count = self
            .pruned_root_count
            .saturating_add(summary.pruned_roots);
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
            if let Some(root) = self.roots.remove(&id) {
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
    fn sync_layout_and_mode(&self, island: &HtmlIslandPrimitive) {
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
        content_revision: 0,
    }
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
        island.logical_bounds = gpux::Bounds::new(gpux::point(gpux::px(8.), gpux::px(13.)), gpux::size(gpux::px(144.), gpux::px(55.)));
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
