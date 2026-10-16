use std::{collections::HashMap, rc::Weak};

#[cfg(feature = "diagnostics")]
use std::{cell::Cell, rc::Rc};

use gpux::{
    Bounds, ElementId, HtmlElementContentFit, HtmlElementInputMode, HtmlElementLifetime,
    HtmlElementPrimitive, HtmlElementWheelBehavior, Pixels, Size, px,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

use crate::window::WebWindowInner;

const HTML_ELEMENT_STYLES: &str = r#"
.rayx-html-element-overlay {
  position: fixed;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
  z-index: 2147483647;
}
.rayx-html-element-root {
  position: absolute;
  display: block;
  box-sizing: border-box;
  contain: layout style paint;
  overflow: hidden;
  pointer-events: none;
  transform-origin: 0 0;
}
.rayx-html-element-content {
  position: absolute;
  left: 0;
  top: 0;
  box-sizing: border-box;
  transform-origin: 0 0;
  z-index: 0;
}
.rayx-html-element-content.rayx-html-element-fit-fill > * {
  width: 100%;
  height: 100%;
  box-sizing: border-box;
  display: block;
}
.rayx-html-element-wheel-capture {
  position: absolute;
  inset: 0;
  z-index: 1;
  display: none;
  background: transparent;
  pointer-events: none;
}
"#;

pub(crate) struct WebHtmlElementRegistry {
    overlay_root: web_sys::HtmlElement,
    roots: HashMap<String, WebHtmlElementRoot>,
    window: Option<Weak<WebWindowInner>>,
    #[cfg(feature = "diagnostics")]
    forwarded_wheel_count: Rc<Cell<u32>>,
    #[cfg(feature = "diagnostics")]
    created_root_count: u32,
    #[cfg(feature = "diagnostics")]
    pruned_root_count: u32,
    #[cfg(feature = "diagnostics")]
    content_update_count: u32,
    #[cfg(feature = "diagnostics")]
    geometry_update_count: u32,
    #[cfg(feature = "diagnostics")]
    geometry_skip_count: u32,
    #[cfg(feature = "diagnostics")]
    restored_root_count: u32,
    #[cfg(feature = "diagnostics")]
    extra_style_update_count: u32,
    #[cfg(feature = "diagnostics")]
    last_geometry_sample: Option<HtmlElementGeometrySample>,
    #[cfg(feature = "diagnostics")]
    last_error: Option<String>,
}

#[cfg(feature = "diagnostics")]
#[derive(Clone, Debug)]
pub(crate) struct WebHtmlElementDiagnostics {
    pub(crate) source: &'static str,
    pub(crate) overlay_root_present: bool,
    pub(crate) active_root_count: u32,
    pub(crate) parked_root_count: u32,
    pub(crate) created_root_count: u32,
    pub(crate) pruned_root_count: u32,
    pub(crate) restored_root_count: u32,
    pub(crate) content_update_count: u32,
    pub(crate) geometry_update_count: u32,
    pub(crate) geometry_skip_count: u32,
    pub(crate) dom_owned_count: u32,
    pub(crate) gpui_owned_count: u32,
    pub(crate) keep_alive_count: u32,
    pub(crate) browser_wheel_count: u32,
    pub(crate) forward_if_observable_wheel_count: u32,
    pub(crate) capture_forward_wheel_count: u32,
    pub(crate) forwarded_wheel_count: u32,
    pub(crate) extra_style_update_count: u32,
    pub(crate) last_geometry_sample: Option<HtmlElementGeometrySample>,
    pub(crate) last_error: Option<String>,
}

#[cfg(feature = "diagnostics")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HtmlElementGeometrySample {
    pub(crate) id: String,
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) clip_left: f32,
    pub(crate) clip_top: f32,
    pub(crate) clip_width: f32,
    pub(crate) clip_height: f32,
    pub(crate) visible: bool,
    pub(crate) opacity: f32,
    pub(crate) input_mode: &'static str,
    pub(crate) lifetime: &'static str,
}

struct WebHtmlElementRoot {
    clip_element: web_sys::HtmlElement,
    content_element: web_sys::HtmlElement,
    wheel_capture_element: web_sys::HtmlElement,
    _observable_wheel_listener: Closure<dyn FnMut(JsValue)>,
    _capture_wheel_listener: Closure<dyn FnMut(JsValue)>,
    input_mode: HtmlElementInputMode,
    lifetime: HtmlElementLifetime,
    content_fit: HtmlElementContentFit,
    wheel_behavior: HtmlElementWheelBehavior,
    extra_styles: gpux::SharedString,
    content_revision: u64,
    last_geometry: Option<HtmlElementGeometry>,
    last_nonzero_size: Size<Pixels>,
    seen_this_frame: bool,
    parked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HtmlElementGeometry {
    clip: Bounds<Pixels>,
    element: Bounds<Pixels>,
    canvas_left: Pixels,
    canvas_top: Pixels,
    visible: bool,
    opacity: f32,
    input_mode: HtmlElementInputMode,
    lifetime: HtmlElementLifetime,
}

impl WebHtmlElementRegistry {
    pub(crate) fn new(overlay_root: web_sys::HtmlElement) -> Self {
        Self {
            overlay_root,
            roots: HashMap::default(),
            window: None,
            #[cfg(feature = "diagnostics")]
            forwarded_wheel_count: Rc::new(Cell::new(0)),
            #[cfg(feature = "diagnostics")]
            created_root_count: 0,
            #[cfg(feature = "diagnostics")]
            pruned_root_count: 0,
            #[cfg(feature = "diagnostics")]
            content_update_count: 0,
            #[cfg(feature = "diagnostics")]
            geometry_update_count: 0,
            #[cfg(feature = "diagnostics")]
            geometry_skip_count: 0,
            #[cfg(feature = "diagnostics")]
            restored_root_count: 0,
            #[cfg(feature = "diagnostics")]
            extra_style_update_count: 0,
            #[cfg(feature = "diagnostics")]
            last_geometry_sample: None,
            #[cfg(feature = "diagnostics")]
            last_error: None,
        }
    }

    pub(crate) fn set_window(&mut self, window: Weak<WebWindowInner>) {
        self.window = Some(window);
    }

    #[cfg(feature = "diagnostics")]
    pub(crate) fn diagnostics(&self) -> WebHtmlElementDiagnostics {
        let active_root_count = self.roots.values().filter(|root| !root.parked).count() as u32;
        let parked_root_count = self.roots.values().filter(|root| root.parked).count() as u32;
        let dom_owned_count = self
            .roots
            .values()
            .filter(|root| root.input_mode == HtmlElementInputMode::DomOwned)
            .count() as u32;
        let gpui_owned_count = self
            .roots
            .values()
            .filter(|root| root.input_mode == HtmlElementInputMode::GpuiOwned)
            .count() as u32;
        let keep_alive_count = self
            .roots
            .values()
            .filter(|root| root.lifetime == HtmlElementLifetime::KeepAliveHidden)
            .count() as u32;
        let browser_wheel_count = self
            .roots
            .values()
            .filter(|root| root.wheel_behavior == HtmlElementWheelBehavior::Browser)
            .count() as u32;
        let forward_if_observable_wheel_count = self
            .roots
            .values()
            .filter(|root| root.wheel_behavior == HtmlElementWheelBehavior::ForwardIfObservable)
            .count() as u32;
        let capture_forward_wheel_count = self
            .roots
            .values()
            .filter(|root| root.wheel_behavior == HtmlElementWheelBehavior::CaptureForward)
            .count() as u32;

        WebHtmlElementDiagnostics {
            source: "gpui_web",
            overlay_root_present: true,
            active_root_count,
            parked_root_count,
            created_root_count: self.created_root_count,
            pruned_root_count: self.pruned_root_count,
            restored_root_count: self.restored_root_count,
            content_update_count: self.content_update_count,
            geometry_update_count: self.geometry_update_count,
            geometry_skip_count: self.geometry_skip_count,
            dom_owned_count,
            gpui_owned_count,
            keep_alive_count,
            browser_wheel_count,
            forward_if_observable_wheel_count,
            capture_forward_wheel_count,
            forwarded_wheel_count: self.forwarded_wheel_count.get(),
            extra_style_update_count: self.extra_style_update_count,
            last_geometry_sample: self.last_geometry_sample.clone(),
            last_error: self.last_error.clone(),
        }
    }

    pub(crate) fn sync_roots(
        &mut self,
        elements: &[HtmlElementPrimitive],
        canvas_rect: &web_sys::DomRect,
    ) {
        for root in self.roots.values_mut() {
            root.seen_this_frame = false;
        }

        let canvas_left = px(canvas_rect.left() as f32);
        let canvas_top = px(canvas_rect.top() as f32);

        for element in elements {
            let key = registry_key(&element.id);
            let is_new_root = !self.roots.contains_key(&key);
            if is_new_root {
                #[cfg(feature = "diagnostics")]
                let root_result = create_root(
                    &self.overlay_root,
                    &key,
                    self.window.clone(),
                    Rc::clone(&self.forwarded_wheel_count),
                );
                #[cfg(not(feature = "diagnostics"))]
                let root_result = create_root(&self.overlay_root, &key, self.window.clone());
                match root_result {
                    Ok(root) => {
                        self.roots.insert(key.clone(), root);
                        #[cfg(feature = "diagnostics")]
                        {
                            self.created_root_count = self.created_root_count.saturating_add(1);
                        }
                    }
                    Err(error) => {
                        #[cfg(feature = "diagnostics")]
                        {
                            self.last_error = Some(error.to_string());
                        }
                        #[cfg(not(feature = "diagnostics"))]
                        let _ = error;
                        continue;
                    }
                }
            }

            let Some(root) = self.roots.get_mut(&key) else {
                continue;
            };

            root.seen_this_frame = true;
            #[cfg(feature = "diagnostics")]
            if root.sync_properties(element) {
                self.extra_style_update_count = self.extra_style_update_count.saturating_add(1);
            }
            #[cfg(not(feature = "diagnostics"))]
            let _ = root.sync_properties(element);

            if is_new_root || root.content_revision != element.content_revision {
                root.content_element.set_inner_html(&element.trusted_html);
                root.content_revision = element.content_revision;
                root.sync_revision_attribute();
                #[cfg(feature = "diagnostics")]
                {
                    self.content_update_count = self.content_update_count.saturating_add(1);
                }
            }

            let geometry = HtmlElementGeometry::from_primitive(element, canvas_left, canvas_top);
            if geometry.element.size.width > px(0.) && geometry.element.size.height > px(0.) {
                root.last_nonzero_size = geometry.element.size;
            }

            #[cfg(feature = "diagnostics")]
            let restored = root.parked && geometry.visible;
            #[cfg(feature = "diagnostics")]
            if root.sync_geometry(&geometry) {
                self.geometry_update_count = self.geometry_update_count.saturating_add(1);
            } else {
                self.geometry_skip_count = self.geometry_skip_count.saturating_add(1);
            }
            #[cfg(not(feature = "diagnostics"))]
            let _ = root.sync_geometry(&geometry);

            #[cfg(feature = "diagnostics")]
            if restored && !root.parked {
                self.restored_root_count = self.restored_root_count.saturating_add(1);
            }

            #[cfg(feature = "diagnostics")]
            {
                self.last_geometry_sample =
                    Some(HtmlElementGeometrySample::from_geometry(key, &geometry));
            }
        }

        let stale_ids = self
            .roots
            .iter()
            .filter_map(|(id, root)| (!root.seen_this_frame).then_some(id.clone()))
            .collect::<Vec<_>>();
        for id in stale_ids {
            let Some(root) = self.roots.get_mut(&id) else {
                continue;
            };
            if root.lifetime == HtmlElementLifetime::KeepAliveHidden {
                let _ = root.park();
            } else if let Some(root) = self.roots.remove(&id) {
                root.clip_element.remove();
                #[cfg(feature = "diagnostics")]
                {
                    self.pruned_root_count = self.pruned_root_count.saturating_add(1);
                }
            }
        }
    }

    pub(crate) fn input_mode_for_event_target(
        &self,
        target: Option<&web_sys::EventTarget>,
    ) -> Option<HtmlElementInputMode> {
        let element = target?.dyn_ref::<web_sys::Element>()?.clone();
        let id = find_element_root_id(element)?;
        self.roots.get(&id).map(|root| root.input_mode)
    }

    #[cfg(test)]
    fn root_count_for_testing(&self) -> usize {
        self.roots.len()
    }
}

impl Drop for WebHtmlElementRegistry {
    fn drop(&mut self) {
        self.overlay_root.remove();
    }
}

pub(crate) fn create_overlay_root(
    document: &web_sys::Document,
    body: &web_sys::HtmlElement,
) -> anyhow::Result<web_sys::HtmlElement> {
    install_stylesheet(document, body)?;

    let overlay_root = document
        .create_element("div")
        .map_err(|e| anyhow::anyhow!("Failed to create HTML element overlay root: {e:?}"))?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|e| anyhow::anyhow!("HTML element overlay root is not an HtmlElement: {e:?}"))?;

    overlay_root
        .set_attribute("data-rayx-html-element-overlay", "true")
        .ok();
    overlay_root
        .set_attribute("class", "rayx-html-element-overlay")
        .ok();

    body.append_child(&overlay_root)
        .map_err(|e| anyhow::anyhow!("Failed to append HTML element overlay root: {e:?}"))?;

    Ok(overlay_root)
}

fn install_stylesheet(
    document: &web_sys::Document,
    body: &web_sys::HtmlElement,
) -> anyhow::Result<()> {
    if document
        .query_selector("[data-rayx-html-element-styles]")
        .ok()
        .flatten()
        .is_some()
    {
        return Ok(());
    }

    let style_element = document
        .create_element("style")
        .map_err(|e| anyhow::anyhow!("Failed to create HTML element stylesheet: {e:?}"))?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|e| anyhow::anyhow!("HTML element stylesheet is not an HtmlElement: {e:?}"))?;
    style_element
        .set_attribute("data-rayx-html-element-styles", "true")
        .ok();
    style_element.set_inner_html(HTML_ELEMENT_STYLES);
    body.append_child(&style_element)
        .map_err(|e| anyhow::anyhow!("Failed to append HTML element stylesheet: {e:?}"))?;
    Ok(())
}

fn registry_key(id: &ElementId) -> String {
    id.to_string()
}

/// The DOM id of the overlay root an `html_element` with this registry key paints into: what
/// the screen-reader mirror's `aria-owns` names for a node whose `accessibility_id` is the key.
pub(crate) fn overlay_dom_id(key: &str) -> String {
    let key: String = key
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect();
    format!("gpux-html-element-{key}")
}

fn css_px(value: Pixels) -> String {
    let value = f32::from(value);
    if value.fract() == 0. {
        format!("{}px", value as i32)
    } else {
        format!("{}px", value)
    }
}

fn set_px(element: &web_sys::HtmlElement, property: &str, value: Pixels) -> Result<(), JsValue> {
    element.style().set_property(property, &css_px(value))
}

#[cfg(feature = "diagnostics")]
fn set_property(owner: &js_sys::Object, name: &str, value: JsValue) {
    js_sys::Reflect::set(owner, &JsValue::from_str(name), &value).ok();
}

fn input_mode_name(input_mode: HtmlElementInputMode) -> &'static str {
    match input_mode {
        HtmlElementInputMode::DomOwned => "dom-owned",
        HtmlElementInputMode::GpuiOwned => "gpui-owned",
    }
}

fn lifetime_name(lifetime: HtmlElementLifetime) -> &'static str {
    match lifetime {
        HtmlElementLifetime::PruneWhenAbsent => "prune-when-absent",
        HtmlElementLifetime::KeepAliveHidden => "keep-alive-hidden",
    }
}

fn content_fit_name(content_fit: HtmlElementContentFit) -> &'static str {
    match content_fit {
        HtmlElementContentFit::Natural => "natural",
        HtmlElementContentFit::Fill => "fill",
    }
}

fn wheel_behavior_name(wheel_behavior: HtmlElementWheelBehavior) -> &'static str {
    match wheel_behavior {
        HtmlElementWheelBehavior::Browser => "browser",
        HtmlElementWheelBehavior::ForwardIfObservable => "forward-if-observable",
        HtmlElementWheelBehavior::CaptureForward => "capture-forward",
    }
}

fn content_class(content_fit: HtmlElementContentFit) -> &'static str {
    match content_fit {
        HtmlElementContentFit::Natural => "rayx-html-element-content",
        HtmlElementContentFit::Fill => "rayx-html-element-content rayx-html-element-fit-fill",
    }
}

impl HtmlElementGeometry {
    fn from_primitive(
        element: &HtmlElementPrimitive,
        canvas_left: Pixels,
        canvas_top: Pixels,
    ) -> Self {
        let clip = element
            .logical_bounds
            .intersect(&element.logical_content_mask.bounds);
        let visible =
            !clip.is_empty() && !element.logical_bounds.is_empty() && element.opacity > 0.;
        Self {
            clip,
            element: element.logical_bounds,
            canvas_left,
            canvas_top,
            visible,
            opacity: element.opacity,
            input_mode: element.input_mode,
            lifetime: element.lifetime,
        }
    }
}

#[cfg(feature = "diagnostics")]
impl HtmlElementGeometrySample {
    fn from_geometry(id: String, geometry: &HtmlElementGeometry) -> Self {
        Self {
            id,
            left: f32::from(geometry.element.left()),
            top: f32::from(geometry.element.top()),
            width: f32::from(geometry.element.size.width),
            height: f32::from(geometry.element.size.height),
            clip_left: f32::from(geometry.clip.left()),
            clip_top: f32::from(geometry.clip.top()),
            clip_width: f32::from(geometry.clip.size.width),
            clip_height: f32::from(geometry.clip.size.height),
            visible: geometry.visible,
            opacity: geometry.opacity,
            input_mode: input_mode_name(geometry.input_mode),
            lifetime: lifetime_name(geometry.lifetime),
        }
    }
}

impl WebHtmlElementRoot {
    fn sync_properties(&mut self, element: &HtmlElementPrimitive) -> bool {
        self.input_mode = element.input_mode;
        self.lifetime = element.lifetime;

        if self.content_fit != element.content_fit {
            self.content_fit = element.content_fit;
            self.content_element
                .set_attribute("class", content_class(element.content_fit))
                .ok();
        }

        if self.wheel_behavior != element.wheel_behavior {
            self.wheel_behavior = element.wheel_behavior;
            self.sync_wheel_capture();
        }

        self.clip_element
            .set_attribute(
                "data-rayx-html-element-input-mode",
                input_mode_name(element.input_mode),
            )
            .ok();
        self.clip_element
            .set_attribute(
                "data-rayx-html-element-lifetime",
                lifetime_name(element.lifetime),
            )
            .ok();
        self.clip_element
            .set_attribute(
                "data-rayx-html-element-content-fit",
                content_fit_name(element.content_fit),
            )
            .ok();
        self.clip_element
            .set_attribute(
                "data-rayx-html-element-wheel-behavior",
                wheel_behavior_name(element.wheel_behavior),
            )
            .ok();

        if self.extra_styles != element.extra_styles {
            self.extra_styles = element.extra_styles.clone();
            self.clip_element
                .set_attribute("style", self.extra_styles.as_ref())
                .ok();
            self.last_geometry = None;
            return true;
        }

        false
    }

    fn sync_revision_attribute(&self) {
        self.clip_element
            .set_attribute(
                "data-rayx-html-element-content-revision",
                &self.content_revision.to_string(),
            )
            .ok();
    }

    fn sync_wheel_capture(&self) {
        let capture_style = self.wheel_capture_element.style();
        match self.wheel_behavior {
            HtmlElementWheelBehavior::CaptureForward => {
                capture_style.set_property("display", "block").ok();
                capture_style.set_property("pointer-events", "auto").ok();
            }
            HtmlElementWheelBehavior::Browser | HtmlElementWheelBehavior::ForwardIfObservable => {
                capture_style.set_property("display", "none").ok();
                capture_style.set_property("pointer-events", "none").ok();
            }
        }
    }

    fn sync_geometry(&mut self, geometry: &HtmlElementGeometry) -> bool {
        if self.last_geometry == Some(*geometry) && !(self.parked && geometry.visible) {
            return false;
        }

        self.last_geometry = Some(*geometry);
        self.input_mode = geometry.input_mode;
        self.lifetime = geometry.lifetime;

        if !geometry.visible {
            if geometry.lifetime == HtmlElementLifetime::KeepAliveHidden {
                self.park();
            } else {
                self.hide_without_parking();
            }
            return true;
        }

        if self.parked {
            self.parked = false;
        }

        let clip_left = geometry.canvas_left + geometry.clip.left();
        let clip_top = geometry.canvas_top + geometry.clip.top();
        let content_x = geometry.element.left() - geometry.clip.left();
        let content_y = geometry.element.top() - geometry.clip.top();
        let style = self.clip_element.style();
        set_px(&self.clip_element, "left", clip_left).ok();
        set_px(&self.clip_element, "top", clip_top).ok();
        set_px(&self.clip_element, "width", geometry.clip.size.width).ok();
        set_px(&self.clip_element, "height", geometry.clip.size.height).ok();
        style.set_property("display", "block").ok();
        style.set_property("visibility", "visible").ok();
        style
            .set_property("opacity", &geometry.opacity.to_string())
            .ok();
        style.set_property("pointer-events", "none").ok();
        style.set_property("transform", "translate(0px, 0px)").ok();
        self.clip_element.remove_attribute("aria-hidden").ok();

        let content_style = self.content_element.style();
        set_px(&self.content_element, "width", geometry.element.size.width).ok();
        set_px(
            &self.content_element,
            "height",
            geometry.element.size.height,
        )
        .ok();
        content_style
            .set_property(
                "transform",
                &format!("translate({}, {})", css_px(content_x), css_px(content_y)),
            )
            .ok();
        content_style
            .set_property(
                "pointer-events",
                match geometry.input_mode {
                    HtmlElementInputMode::DomOwned => "auto",
                    HtmlElementInputMode::GpuiOwned => "none",
                },
            )
            .ok();
        content_style.set_property("visibility", "visible").ok();

        self.clip_element.remove_attribute("aria-hidden").ok();
        self.sync_revision_attribute();
        true
    }

    fn park(&mut self) -> bool {
        let was_visible = !self.parked;
        self.parked = true;
        let style = self.clip_element.style();
        style.set_property("display", "block").ok();
        style.set_property("visibility", "hidden").ok();
        style.set_property("opacity", "0").ok();
        style.set_property("pointer-events", "none").ok();
        style
            .set_property("transform", "translate(-100000px, -100000px)")
            .ok();
        set_px(&self.clip_element, "width", self.last_nonzero_size.width).ok();
        set_px(&self.clip_element, "height", self.last_nonzero_size.height).ok();
        self.clip_element.set_attribute("aria-hidden", "true").ok();

        let content_style = self.content_element.style();
        content_style.set_property("pointer-events", "none").ok();
        content_style.set_property("visibility", "hidden").ok();
        was_visible
    }

    fn hide_without_parking(&mut self) {
        self.parked = false;
        let style = self.clip_element.style();
        style.set_property("display", "block").ok();
        style.set_property("visibility", "hidden").ok();
        style.set_property("opacity", "0").ok();
        style.set_property("pointer-events", "none").ok();
        self.clip_element.set_attribute("aria-hidden", "true").ok();
        self.content_element
            .style()
            .set_property("pointer-events", "none")
            .ok();
    }
}

#[cfg(feature = "diagnostics")]
pub(crate) fn write_html_element_diagnostics_to_global(diagnostics: &WebHtmlElementDiagnostics) {
    let object = js_sys::Object::new();
    set_property(&object, "version", JsValue::from_f64(1.0));
    set_property(&object, "source", JsValue::from_str(diagnostics.source));
    set_property(
        &object,
        "overlayRootPresent",
        JsValue::from_bool(diagnostics.overlay_root_present),
    );
    set_property(
        &object,
        "activeRootCount",
        JsValue::from_f64(diagnostics.active_root_count as f64),
    );
    set_property(
        &object,
        "parkedRootCount",
        JsValue::from_f64(diagnostics.parked_root_count as f64),
    );
    set_property(
        &object,
        "createdRootCount",
        JsValue::from_f64(diagnostics.created_root_count as f64),
    );
    set_property(
        &object,
        "prunedRootCount",
        JsValue::from_f64(diagnostics.pruned_root_count as f64),
    );
    set_property(
        &object,
        "restoredRootCount",
        JsValue::from_f64(diagnostics.restored_root_count as f64),
    );
    set_property(
        &object,
        "contentUpdateCount",
        JsValue::from_f64(diagnostics.content_update_count as f64),
    );
    set_property(
        &object,
        "geometryUpdateCount",
        JsValue::from_f64(diagnostics.geometry_update_count as f64),
    );
    set_property(
        &object,
        "geometrySkipCount",
        JsValue::from_f64(diagnostics.geometry_skip_count as f64),
    );
    set_property(
        &object,
        "domOwnedCount",
        JsValue::from_f64(diagnostics.dom_owned_count as f64),
    );
    set_property(
        &object,
        "gpuiOwnedCount",
        JsValue::from_f64(diagnostics.gpui_owned_count as f64),
    );
    set_property(
        &object,
        "keepAliveCount",
        JsValue::from_f64(diagnostics.keep_alive_count as f64),
    );
    set_property(
        &object,
        "browserWheelCount",
        JsValue::from_f64(diagnostics.browser_wheel_count as f64),
    );
    set_property(
        &object,
        "forwardIfObservableWheelCount",
        JsValue::from_f64(diagnostics.forward_if_observable_wheel_count as f64),
    );
    set_property(
        &object,
        "captureForwardWheelCount",
        JsValue::from_f64(diagnostics.capture_forward_wheel_count as f64),
    );
    set_property(
        &object,
        "forwardedWheelCount",
        JsValue::from_f64(diagnostics.forwarded_wheel_count as f64),
    );
    set_property(
        &object,
        "extraStyleUpdateCount",
        JsValue::from_f64(diagnostics.extra_style_update_count as f64),
    );
    set_property(
        &object,
        "lastGeometrySample",
        diagnostics
            .last_geometry_sample
            .as_ref()
            .map(HtmlElementGeometrySample::to_js_object)
            .unwrap_or(JsValue::NULL),
    );
    set_property(
        &object,
        "lastError",
        diagnostics
            .last_error
            .as_deref()
            .map(JsValue::from_str)
            .unwrap_or(JsValue::NULL),
    );

    gpui_wgpu::web_globals::publish("HtmlElementDiagnostics", &object.into());
}

#[cfg(feature = "diagnostics")]
impl HtmlElementGeometrySample {
    fn to_js_object(&self) -> JsValue {
        let object = js_sys::Object::new();
        set_property(&object, "id", JsValue::from_str(&self.id));
        set_property(&object, "left", JsValue::from_f64(self.left as f64));
        set_property(&object, "top", JsValue::from_f64(self.top as f64));
        set_property(&object, "width", JsValue::from_f64(self.width as f64));
        set_property(&object, "height", JsValue::from_f64(self.height as f64));
        set_property(
            &object,
            "clipLeft",
            JsValue::from_f64(self.clip_left as f64),
        );
        set_property(&object, "clipTop", JsValue::from_f64(self.clip_top as f64));
        set_property(
            &object,
            "clipWidth",
            JsValue::from_f64(self.clip_width as f64),
        );
        set_property(
            &object,
            "clipHeight",
            JsValue::from_f64(self.clip_height as f64),
        );
        set_property(&object, "visible", JsValue::from_bool(self.visible));
        set_property(&object, "opacity", JsValue::from_f64(self.opacity as f64));
        set_property(&object, "inputMode", JsValue::from_str(self.input_mode));
        set_property(&object, "lifetime", JsValue::from_str(self.lifetime));
        object.into()
    }
}

fn add_non_passive_event_listener(
    element: &web_sys::HtmlElement,
    event_name: &str,
    callback: &js_sys::Function,
) {
    let element_js: &JsValue = element.as_ref();
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"passive".into(), &false.into()).ok();
    if let Ok(add_fn_val) = js_sys::Reflect::get(element_js, &"addEventListener".into())
        && let Ok(add_fn) = add_fn_val.dyn_into::<js_sys::Function>()
    {
        add_fn
            .call3(element_js, &event_name.into(), callback, &options)
            .ok();
    }
}

fn create_wheel_forward_listener(
    window: Option<Weak<WebWindowInner>>,
    root_element: web_sys::HtmlElement,
    expected_behavior: HtmlElementWheelBehavior,
    #[cfg(feature = "diagnostics")] forwarded_wheel_count: Rc<Cell<u32>>,
) -> Closure<dyn FnMut(JsValue)> {
    Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        if root_element
            .get_attribute("data-rayx-html-element-wheel-behavior")
            .as_deref()
            != Some(wheel_behavior_name(expected_behavior))
        {
            return;
        }

        let event: web_sys::WheelEvent = event.unchecked_into();
        event.prevent_default();
        event.stop_propagation();

        let Some(window) = window.as_ref().and_then(Weak::upgrade) else {
            return;
        };

        window.dispatch_wheel_event_from_dom(&event);
        #[cfg(feature = "diagnostics")]
        forwarded_wheel_count.set(forwarded_wheel_count.get().saturating_add(1));
    })
}

fn create_root(
    overlay_root: &web_sys::HtmlElement,
    id: &str,
    window: Option<Weak<WebWindowInner>>,
    #[cfg(feature = "diagnostics")] forwarded_wheel_count: Rc<Cell<u32>>,
) -> anyhow::Result<WebHtmlElementRoot> {
    let document = overlay_root
        .owner_document()
        .ok_or_else(|| anyhow::anyhow!("HTML element overlay root has no owner document"))?;
    let clip_element = document
        .create_element("div")
        .map_err(|e| anyhow::anyhow!("Failed to create HTML element root: {e:?}"))?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|e| anyhow::anyhow!("HTML element root is not an HtmlElement: {e:?}"))?;
    clip_element
        .set_attribute("data-rayx-html-element-id", id)
        .ok();
    clip_element.set_id(&overlay_dom_id(id));
    clip_element.set_attribute("aria-hidden", "true").ok();
    clip_element
        .set_attribute("class", "rayx-html-element-root")
        .ok();
    clip_element
        .set_attribute("data-rayx-html-element-input-mode", "gpui-owned")
        .ok();
    clip_element
        .set_attribute("data-rayx-html-element-lifetime", "prune-when-absent")
        .ok();
    clip_element
        .set_attribute("data-rayx-html-element-content-fit", "natural")
        .ok();
    clip_element
        .set_attribute("data-rayx-html-element-wheel-behavior", "browser")
        .ok();
    clip_element
        .set_attribute("data-rayx-html-element-content-revision", "0")
        .ok();

    let content_element = document
        .create_element("div")
        .map_err(|e| anyhow::anyhow!("Failed to create HTML element content host: {e:?}"))?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|e| anyhow::anyhow!("HTML element content host is not an HtmlElement: {e:?}"))?;
    content_element
        .set_attribute("data-rayx-html-element-content", "true")
        .ok();
    content_element
        .set_attribute("class", "rayx-html-element-content")
        .ok();

    let wheel_capture_element = document
        .create_element("div")
        .map_err(|e| anyhow::anyhow!("Failed to create HTML element wheel capture layer: {e:?}"))?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|e| {
            anyhow::anyhow!("HTML element wheel capture layer is not an HtmlElement: {e:?}")
        })?;
    wheel_capture_element
        .set_attribute("data-rayx-html-element-wheel-capture", "true")
        .ok();
    wheel_capture_element
        .set_attribute("class", "rayx-html-element-wheel-capture")
        .ok();

    #[cfg(feature = "diagnostics")]
    let observable_wheel_listener = create_wheel_forward_listener(
        window.clone(),
        clip_element.clone(),
        HtmlElementWheelBehavior::ForwardIfObservable,
        Rc::clone(&forwarded_wheel_count),
    );
    #[cfg(not(feature = "diagnostics"))]
    let observable_wheel_listener = create_wheel_forward_listener(
        window.clone(),
        clip_element.clone(),
        HtmlElementWheelBehavior::ForwardIfObservable,
    );
    add_non_passive_event_listener(
        &content_element,
        "wheel",
        observable_wheel_listener.as_ref().unchecked_ref(),
    );

    #[cfg(feature = "diagnostics")]
    let capture_wheel_listener = create_wheel_forward_listener(
        window,
        clip_element.clone(),
        HtmlElementWheelBehavior::CaptureForward,
        forwarded_wheel_count,
    );
    #[cfg(not(feature = "diagnostics"))]
    let capture_wheel_listener = create_wheel_forward_listener(
        window,
        clip_element.clone(),
        HtmlElementWheelBehavior::CaptureForward,
    );
    add_non_passive_event_listener(
        &wheel_capture_element,
        "wheel",
        capture_wheel_listener.as_ref().unchecked_ref(),
    );

    clip_element.append_child(&content_element).ok();
    clip_element.append_child(&wheel_capture_element).ok();
    overlay_root.append_child(&clip_element).map_err(|e| {
        anyhow::anyhow!("Failed to append HTML element root to overlay root: {e:?}")
    })?;

    Ok(WebHtmlElementRoot {
        clip_element,
        content_element,
        wheel_capture_element,
        _observable_wheel_listener: observable_wheel_listener,
        _capture_wheel_listener: capture_wheel_listener,
        input_mode: HtmlElementInputMode::GpuiOwned,
        lifetime: HtmlElementLifetime::PruneWhenAbsent,
        content_fit: HtmlElementContentFit::Natural,
        wheel_behavior: HtmlElementWheelBehavior::Browser,
        extra_styles: gpux::SharedString::default(),
        content_revision: 0,
        last_geometry: None,
        last_nonzero_size: Size {
            width: px(1.),
            height: px(1.),
        },
        seen_this_frame: false,
        parked: false,
    })
}

fn find_element_root_id(mut element: web_sys::Element) -> Option<String> {
    loop {
        if let Some(id) = element.get_attribute("data-rayx-html-element-id") {
            return Some(id);
        }
        element = element.parent_element()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen::JsCast;

    #[cfg(feature = "diagnostics")]
    #[test]
    fn registry_diagnostics_start_at_zero() {
        let overlay_root: web_sys::HtmlElement = wasm_bindgen::JsValue::NULL.unchecked_into();
        let registry = WebHtmlElementRegistry::new(overlay_root);

        let diagnostics = registry.diagnostics();

        assert_eq!(diagnostics.source, "gpui_web");
        assert!(diagnostics.overlay_root_present);
        assert_eq!(diagnostics.active_root_count, 0);
        assert_eq!(diagnostics.parked_root_count, 0);
        assert_eq!(diagnostics.pruned_root_count, 0);
        assert_eq!(diagnostics.content_update_count, 0);
        assert_eq!(diagnostics.geometry_update_count, 0);
        assert_eq!(diagnostics.geometry_skip_count, 0);
        assert_eq!(diagnostics.dom_owned_count, 0);
        assert_eq!(diagnostics.gpui_owned_count, 0);
        assert_eq!(diagnostics.keep_alive_count, 0);
        assert_eq!(diagnostics.browser_wheel_count, 0);
        assert_eq!(diagnostics.forward_if_observable_wheel_count, 0);
        assert_eq!(diagnostics.capture_forward_wheel_count, 0);
        assert_eq!(diagnostics.forwarded_wheel_count, 0);
        assert_eq!(diagnostics.extra_style_update_count, 0);
        assert!(diagnostics.last_geometry_sample.is_none());
        assert!(diagnostics.last_error.is_none());
        assert_eq!(registry.root_count_for_testing(), 0);
    }

    #[test]
    fn geometry_intersects_logical_bounds_with_content_mask() {
        let primitive = test_element("provider");
        let geometry = HtmlElementGeometry::from_primitive(&primitive, px(10.), px(20.));

        assert_eq!(geometry.clip.left(), px(12.));
        assert_eq!(geometry.clip.top(), px(14.));
        assert_eq!(geometry.clip.size.width, px(40.));
        assert_eq!(geometry.clip.size.height, px(30.));
        assert!(geometry.visible);

        #[cfg(feature = "diagnostics")]
        {
            let sample =
                HtmlElementGeometrySample::from_geometry("provider".to_string(), &geometry);
            assert_eq!(sample.left, 8.);
            assert_eq!(sample.top, 9.);
            assert_eq!(sample.clip_left, 12.);
            assert_eq!(sample.input_mode, "dom-owned");
            assert_eq!(sample.lifetime, "keep-alive-hidden");
        }
    }

    #[cfg(feature = "diagnostics")]
    #[test]
    fn diagnostics_writer_accepts_zero_counts() {
        write_html_element_diagnostics_to_global(&WebHtmlElementDiagnostics {
            source: "gpui_web",
            overlay_root_present: true,
            active_root_count: 0,
            parked_root_count: 0,
            created_root_count: 0,
            pruned_root_count: 0,
            restored_root_count: 0,
            content_update_count: 0,
            geometry_update_count: 0,
            geometry_skip_count: 0,
            dom_owned_count: 0,
            gpui_owned_count: 0,
            keep_alive_count: 0,
            browser_wheel_count: 0,
            forward_if_observable_wheel_count: 0,
            capture_forward_wheel_count: 0,
            forwarded_wheel_count: 0,
            extra_style_update_count: 0,
            last_geometry_sample: None,
            last_error: None,
        });
    }

    #[test]
    fn overlay_root_creation_helper_is_available() {
        let _create: fn(
            &web_sys::Document,
            &web_sys::HtmlElement,
        ) -> anyhow::Result<web_sys::HtmlElement> = create_overlay_root;
    }

    fn test_element(id: &'static str) -> HtmlElementPrimitive {
        HtmlElementPrimitive {
            order: 0,
            id: ElementId::from(id),
            bounds: Default::default(),
            logical_bounds: Bounds::new(
                gpux::point(px(8.), px(9.)),
                Size {
                    width: px(100.),
                    height: px(50.),
                },
            ),
            logical_content_mask: gpux::ContentMask::rectangular(Bounds::new(
                gpux::point(px(12.), px(14.)),
                Size {
                    width: px(40.),
                    height: px(30.),
                },
            )),
            content_mask: Default::default(),
            input_mode: HtmlElementInputMode::DomOwned,
            lifetime: HtmlElementLifetime::KeepAliveHidden,
            content_fit: HtmlElementContentFit::Natural,
            wheel_behavior: HtmlElementWheelBehavior::Browser,
            opacity: 1.,
            content_revision: 1,
            trusted_html: "<iframe title=\"Provider\"></iframe>".into(),
            extra_styles: gpux::SharedString::default(),
        }
    }
}
