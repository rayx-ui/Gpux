//! Window composition in the browser (zed-industries/zed#62379): GPUI's
//! overlays drawn above the DOM overlay of `html_element`.
//!
//! gpux's own; gpui-fast has no browser backend. The window's canvas draws the
//! base surface below the `html_element` overlay root, as without
//! composition. Every further GPUI surface gets a canvas of its own above that
//! root, drawn by a renderer sharing the window renderer's WebGPU device,
//! sprite atlas and pipelines, so a popover paints over a DOM element. Each
//! overlay canvas is followed by an input shield: a transparent element whose
//! `clip-path` is the overlay's input region (what it drew last frame, see
//! `ComposedScene::layer_input_region`), which takes the pointer there and
//! hands its events to the window's canvas, where GPUI listens. Elsewhere the
//! pointer reaches the DOM elements and the canvas below.
//!
//! WebGL2 cannot share a context between canvases, so composition reports
//! unsupported there and the window keeps the single canvas, below its DOM
//! overlay.

use std::{borrow::Cow, cell::RefCell, collections::HashMap, rc::Rc};

use anyhow::{Context as _, Result};
use gpui_wgpu::{WgpuBackend, WgpuContext, WgpuRenderer, WgpuSurfaceConfig, wgpu};
use gpux::{
    Bounds, ComposedScene, CompositionSurfaceId, DevicePixels, HtmlElementPrimitive,
    PlatformCompositionSurface, PlatformCompositionSurfaceContent, PlatformWindow as _,
    ScaledPixels, Scene, Size,
};
use wasm_bindgen::{JsCast as _, JsValue};

use crate::{events::EventListenerHandle, window::WebWindow};

/// The z-index of the `html_element` overlay root while the window composes:
/// one below the overlays' canvases.
const DOM_OVERLAY_Z_INDEX: &str = "2147483646";
const OVERLAY_Z_INDEX: &str = "2147483647";

/// The pointer events an input shield hands to the window's canvas.
const FORWARDED_EVENTS: &[&str] = &[
    "pointerdown",
    "pointerup",
    "pointermove",
    "pointercancel",
    "contextmenu",
];

/// A browser window's composition state, held by `WebWindowInner`.
#[derive(Default)]
pub(crate) struct Composition {
    /// The platform's WebGPU context, which overlay canvases make their
    /// surfaces on.
    context: Option<Rc<RefCell<Option<WgpuContext>>>>,
    enabled: bool,
    base: Option<CompositionSurfaceId>,
    overlays: HashMap<CompositionSurfaceId, Overlay>,
    /// The DOM elements of the whole composed frame being drawn, which the
    /// base surface's draw places, wherever in the composition they were
    /// painted.
    frame_html_elements: Option<Vec<HtmlElementPrimitive>>,
}

struct Overlay {
    canvas: web_sys::HtmlCanvasElement,
    shield: web_sys::HtmlElement,
    renderer: WgpuRenderer,
    /// The input region last set on the shield.
    region: Option<Vec<Bounds<ScaledPixels>>>,
    _listeners: Vec<EventListenerHandle>,
}

impl Drop for Overlay {
    fn drop(&mut self) {
        self.renderer.destroy();
        self.canvas.remove();
        self.shield.remove();
    }
}

impl Overlay {
    fn new(window: &WebWindow, context: &WgpuContext) -> Result<Self> {
        let inner = &window.inner;
        let document = inner
            .canvas
            .owner_document()
            .context("the canvas has no document")?;
        let body = document.body().context("the document has no body")?;
        let canvas = document
            .create_element("canvas")
            .map_err(|error| anyhow::anyhow!("creating an overlay canvas: {error:?}"))?
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .map_err(|_| anyhow::anyhow!("the overlay canvas is not a canvas"))?;
        canvas
            .set_attribute("data-gpux-composition-overlay", "true")
            .ok();
        let shield = document
            .create_element("div")
            .map_err(|error| anyhow::anyhow!("creating an overlay input shield: {error:?}"))?
            .dyn_into::<web_sys::HtmlElement>()
            .map_err(|_| anyhow::anyhow!("the input shield is not an element"))?;
        shield
            .set_attribute("data-gpux-composition-shield", "true")
            .ok();
        for (element, pointer_events) in [
            (canvas.unchecked_ref::<web_sys::HtmlElement>(), "none"),
            (&shield, "auto"),
        ] {
            let style = element.style();
            for (property, value) in [
                ("position", "fixed"),
                ("margin", "0"),
                ("padding", "0"),
                ("border", "0"),
                ("background", "transparent"),
                ("pointer-events", pointer_events),
                ("z-index", OVERLAY_Z_INDEX),
                ("touch-action", "none"),
            ] {
                style.set_property(property, value).ok();
            }
        }
        shield.style().set_property("display", "none").ok();
        body.append_child(&canvas)
            .map_err(|error| anyhow::anyhow!("appending the overlay canvas: {error:?}"))?;
        body.append_child(&shield)
            .map_err(|error| anyhow::anyhow!("appending the input shield: {error:?}"))?;

        let size = window_device_size(window);
        canvas.set_width(size.width.0.max(1) as u32);
        canvas.set_height(size.height.0.max(1) as u32);
        let renderer = context
            .instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|error| anyhow::anyhow!("creating an overlay surface: {error}"))
            .and_then(|surface| {
                inner.renderer.borrow().new_sharing_with_context(
                    context,
                    surface,
                    WgpuSurfaceConfig {
                        size,
                        transparent: true,
                        preferred_present_mode: None,
                    },
                    true,
                )
            });
        let renderer = match renderer {
            Ok(renderer) => renderer,
            Err(error) => {
                canvas.remove();
                shield.remove();
                return Err(error);
            }
        };

        let target: web_sys::EventTarget = inner.canvas.clone().into();
        let listeners = FORWARDED_EVENTS
            .iter()
            .map(|name| forward(&shield, name, &target, false))
            .chain([forward(&shield, "wheel", &target, true)])
            .collect();
        Ok(Self {
            canvas,
            shield,
            renderer,
            region: None,
            _listeners: listeners,
        })
    }

    /// Places the canvas and the shield over the window's canvas, sized to
    /// `size` device pixels.
    fn place(&mut self, rect: &web_sys::DomRect, size: Size<DevicePixels>) {
        for element in [
            self.canvas.unchecked_ref::<web_sys::HtmlElement>(),
            &self.shield,
        ] {
            let style = element.style();
            style
                .set_property("left", &format!("{}px", rect.left()))
                .ok();
            style.set_property("top", &format!("{}px", rect.top())).ok();
            style
                .set_property("width", &format!("{}px", rect.width()))
                .ok();
            style
                .set_property("height", &format!("{}px", rect.height()))
                .ok();
        }
        let (width, height) = (size.width.0.max(1) as u32, size.height.0.max(1) as u32);
        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
        }
        if self.renderer.viewport_size() != size {
            self.renderer.update_drawable_size(size);
        }
    }

    /// Clips the shield to `region`, in device pixels at `scale`, and hides
    /// it while the overlay drew nothing.
    fn set_region(&mut self, region: Vec<Bounds<ScaledPixels>>, scale: f32) {
        if self.region.as_ref() == Some(&region) {
            return;
        }
        let style = self.shield.style();
        if region.is_empty() {
            style.set_property("display", "none").ok();
        } else {
            let path = region
                .iter()
                .map(|bounds| {
                    let left = bounds.origin.x.0 / scale;
                    let top = bounds.origin.y.0 / scale;
                    let width = bounds.size.width.0 / scale;
                    let height = bounds.size.height.0 / scale;
                    format!("M{left} {top}h{width}v{height}h{}z", -width)
                })
                .collect::<String>();
            style
                .set_property("clip-path", &format!("path('{path}')"))
                .ok();
            style.set_property("display", "block").ok();
        }
        self.region = Some(region);
    }
}

/// Hands every `name` event on `shield` to `target` as a copy, and stops the
/// original's default when the copy's was prevented.
fn forward(
    shield: &web_sys::HtmlElement,
    name: &'static str,
    target: &web_sys::EventTarget,
    non_passive: bool,
) -> EventListenerHandle {
    let target = target.clone();
    let handler = move |event: JsValue| {
        let event: web_sys::Event = event.unchecked_into();
        let Some(copy) = copy_event(&event) else {
            return;
        };
        target.dispatch_event(&copy).ok();
        if copy.default_prevented() {
            event.prevent_default();
        }
        event.stop_propagation();
    };
    if non_passive {
        EventListenerHandle::add_non_passive(shield.as_ref(), name, handler)
    } else {
        EventListenerHandle::add(shield.as_ref(), name, handler)
    }
}

/// A new event of `event`'s own class and type, initialized from it.
fn copy_event(event: &web_sys::Event) -> Option<web_sys::Event> {
    let constructor = js_sys::Reflect::get(event, &"constructor".into())
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    let arguments = js_sys::Array::of2(&event.type_().into(), event);
    js_sys::Reflect::construct(&constructor, &arguments)
        .ok()?
        .dyn_into::<web_sys::Event>()
        .ok()
}

fn window_device_size(window: &WebWindow) -> Size<DevicePixels> {
    let canvas = &window.inner.canvas;
    Size {
        width: DevicePixels(canvas.width() as i32),
        height: DevicePixels(canvas.height() as i32),
    }
}

/// Lets the window's overlays make surfaces on the platform's WebGPU
/// context; called once the window is open.
pub(crate) fn attach_context(window: &WebWindow, context: &Rc<RefCell<Option<WgpuContext>>>) {
    window.inner.fast_composition.borrow_mut().context = Some(context.clone());
}

/// The DOM elements the base surface's draw places: those of the whole
/// composed frame while composing, otherwise the scene's own.
pub(crate) fn html_elements<'a>(
    window: &WebWindow,
    scene: &'a Scene,
) -> Cow<'a, [HtmlElementPrimitive]> {
    match window
        .inner
        .fast_composition
        .borrow_mut()
        .frame_html_elements
        .take()
    {
        Some(elements) => Cow::Owned(elements),
        None => Cow::Borrowed(&scene.html_elements),
    }
}

/// `PlatformWindow::enable_window_composition`: needs WebGPU, whose device
/// the overlays' canvases share.
pub(crate) fn enable_window_composition(window: &WebWindow) -> Result<()> {
    let inner = &window.inner;
    let mut composition = inner.fast_composition.borrow_mut();
    let context = composition
        .context
        .clone()
        .context("the window has no WebGPU context")?;
    let backend = context
        .borrow()
        .as_ref()
        .map(|context| context.backend())
        .context("the WebGPU context is not initialized")?;
    if backend != WgpuBackend::BrowserWebGpu {
        anyhow::bail!("window composition needs WebGPU; WebGL2 keeps a single canvas");
    }
    if !composition.enabled {
        composition.enabled = true;
        // The overlays' canvases go above the DOM overlay.
        if let Some(root) = inner
            .canvas
            .owner_document()
            .and_then(|document| {
                document
                    .query_selector("[data-rayx-html-element-overlay]")
                    .ok()
                    .flatten()
            })
            .and_then(|root| root.dyn_into::<web_sys::HtmlElement>().ok())
        {
            root.style()
                .set_property("z-index", DOM_OVERLAY_Z_INDEX)
                .ok();
        }
    }
    Ok(())
}

/// `PlatformWindow::set_composition_order`: a canvas and shield per GPUI
/// surface above the base, stacked in `surfaces`' order. Native surfaces are
/// `html_element`'s business in the browser.
pub(crate) fn set_composition_order(
    window: &WebWindow,
    surfaces: &[PlatformCompositionSurface],
) -> Result<()> {
    anyhow::ensure!(
        surfaces
            .iter()
            .all(|surface| matches!(surface.content, PlatformCompositionSurfaceContent::Gpui)),
        "native surfaces are not supported in the browser; use html_element"
    );
    let base = surfaces
        .first()
        .map(|surface| surface.id)
        .context("composition has no GPUI base surface")?;
    let inner = &window.inner;
    let context = inner
        .fast_composition
        .borrow()
        .context
        .clone()
        .context("the window has no WebGPU context")?;
    let context = context.borrow();
    let context = context
        .as_ref()
        .context("the WebGPU context is not initialized")?;
    let mut composition = inner.fast_composition.borrow_mut();
    let ids = surfaces
        .iter()
        .skip(1)
        .map(|surface| surface.id)
        .collect::<Vec<_>>();
    composition.overlays.retain(|id, _| ids.contains(id));
    for id in &ids {
        if !composition.overlays.contains_key(id) {
            let overlay = Overlay::new(window, context)?;
            composition.overlays.insert(*id, overlay);
        }
    }
    // Re-append in order: later siblings stack above earlier ones.
    if let Some(body) = inner
        .canvas
        .owner_document()
        .and_then(|document| document.body())
    {
        for id in &ids {
            if let Some(overlay) = composition.overlays.get(id) {
                body.append_child(&overlay.canvas).ok();
                body.append_child(&overlay.shield).ok();
            }
        }
    }
    composition.base = Some(base);
    Ok(())
}

/// `PlatformWindow::draw_composed`: the base surface on the window's canvas,
/// below the DOM overlay, and each overlay on its canvas above it.
pub(crate) fn draw_composed(window: &WebWindow, scene: ComposedScene<'_>) {
    let inner = &window.inner;
    let Some(base) = inner.fast_composition.borrow().base else {
        window.draw(scene.scene());
        return;
    };
    let mut base_scene = Scene::default();
    let mut overlay_scenes = Vec::new();
    for layer in scene.layers() {
        match scene.layer_scene(layer) {
            Ok(surface_scene) if layer.surface == base => base_scene = surface_scene,
            Ok(surface_scene) => overlay_scenes.push((
                layer.surface,
                surface_scene,
                scene.layer_input_region(layer),
            )),
            Err(error) => {
                log::error!("replaying browser composition scene: {error:#}");
                return;
            }
        }
    }
    inner.fast_composition.borrow_mut().frame_html_elements =
        Some(scene.scene().html_elements.clone());
    window.draw(&base_scene);

    let rect = inner.canvas.get_bounding_client_rect();
    let size = window_device_size(window);
    let scale = window.scale_factor();
    let mut composition = inner.fast_composition.borrow_mut();
    for (surface, surface_scene, region) in overlay_scenes {
        let Some(overlay) = composition.overlays.get_mut(&surface) else {
            log::error!("missing browser composition surface {surface:?}");
            continue;
        };
        overlay.place(&rect, size);
        overlay.set_region(region, scale);
        overlay.renderer.draw(&surface_scene);
    }
}
