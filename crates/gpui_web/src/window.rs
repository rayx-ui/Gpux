use crate::display::WebDisplay;
use crate::events::{ClickState, WebEventListeners, is_mac_platform};
use crate::html_element::{
    WebHtmlElementRegistry, create_overlay_root, write_html_element_diagnostics_to_global,
};
use crate::html_in_canvas::{
    HtmlInCanvasDiagnostics, HtmlInCanvasPaintObserver, detect_html_in_canvas,
    enable_layout_subtree, enable_layout_subtree_if_requested,
    write_html_in_canvas_diagnostics_to_global,
};
use crate::html_island::WebHtmlIslandRegistry;
use crate::media_query_diagnostics::MediaQueryPipelineDiagnostics;
use crate::web_viewport_scheduler::{WebViewportObservation, WebViewportScheduler};
use std::sync::Arc;
use std::{cell::Cell, cell::RefCell, rc::Rc};

use gpui_wgpu::{WgpuContext, WgpuRenderer, WgpuSurfaceConfig};
use gpux::{
    AnyWindowHandle, Bounds, Capslock, Decorations, DevicePixels, DispatchEventResult,
    EffectCapabilities, GpuSpecs, Modifiers, MouseButton, Pixels, PlatformAtlas, PlatformDisplay,
    PlatformInput, PlatformInputHandler, PlatformWindow, Point, PromptButton, PromptLevel,
    RequestFrameOptions, ResizeEdge, Scene, Size, WindowAppearance, WindowBackgroundAppearance,
    WindowBounds, WindowControlArea, WindowControls, WindowDecorations, WindowParams, px,
};
use wasm_bindgen::prelude::*;

#[derive(Default)]
pub(crate) struct WebWindowCallbacks {
    pub(crate) request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    pub(crate) input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    pub(crate) active_status_change: Option<Box<dyn FnMut(bool)>>,
    pub(crate) hover_status_change: Option<Box<dyn FnMut(bool)>>,
    pub(crate) resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    pub(crate) moved: Option<Box<dyn FnMut()>>,
    pub(crate) should_close: Option<Box<dyn FnMut() -> bool>>,
    pub(crate) close: Option<Box<dyn FnOnce()>>,
    pub(crate) appearance_changed: Option<Box<dyn FnMut()>>,
    pub(crate) hit_test_window_control: Option<Box<dyn FnMut() -> Option<WindowControlArea>>>,
}

pub(crate) struct WebWindowMutableState {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) scale_factor: f32,
    pub(crate) max_texture_dimension: u32,
    pub(crate) title: String,
    pub(crate) input_handler: Option<PlatformInputHandler>,
    pub(crate) is_fullscreen: bool,
    pub(crate) is_active: bool,
    pub(crate) is_hovered: bool,
    pub(crate) mouse_position: Point<Pixels>,
    pub(crate) modifiers: Modifiers,
    pub(crate) capslock: Capslock,
}

pub(crate) struct WebWindowInner {
    pub(crate) browser_window: web_sys::Window,
    pub(crate) canvas: web_sys::HtmlCanvasElement,
    pub(crate) input_element: web_sys::HtmlInputElement,
    pub(crate) has_device_pixel_support: bool,
    pub(crate) is_mac: bool,
    pub(crate) renderer: RefCell<WgpuRenderer>,
    pub(crate) html_element_registry: Rc<RefCell<WebHtmlElementRegistry>>,
    pub(crate) html_island_registry: Rc<RefCell<WebHtmlIslandRegistry>>,
    pub(crate) state: RefCell<WebWindowMutableState>,
    pub(crate) callbacks: RefCell<WebWindowCallbacks>,
    pub(crate) click_state: RefCell<ClickState>,
    pub(crate) pressed_button: Cell<Option<MouseButton>>,
    pub(crate) last_physical_size: Cell<(u32, u32)>,
    pub(crate) notify_scale: Cell<bool>,
    pub(crate) is_composing: Cell<bool>,
    html_island_force_frame: Cell<bool>,
    mql_handle: RefCell<Option<MqlHandle>>,
    viewport_scheduler: WebViewportScheduler,
    pending_physical_size: Cell<Option<(u32, u32)>>,
    media_query_diagnostics: MediaQueryPipelineDiagnostics,
}

pub struct WebWindow {
    inner: Rc<WebWindowInner>,
    display: Rc<dyn PlatformDisplay>,
    #[allow(dead_code)]
    handle: AnyWindowHandle,
    _raf_closure: Closure<dyn FnMut()>,
    _resize_observer: Option<web_sys::ResizeObserver>,
    _resize_observer_closure: Closure<dyn FnMut(js_sys::Array)>,
    _event_listeners: WebEventListeners,
    _html_in_canvas_paint_observer: HtmlInCanvasPaintObserver,
    html_in_canvas_diagnostics: HtmlInCanvasDiagnostics,
    layout_subtree_enabled: Cell<bool>,
}

impl WebWindow {
    pub fn new(
        handle: AnyWindowHandle,
        params: WindowParams,
        context: &WgpuContext,
        browser_window: web_sys::Window,
    ) -> anyhow::Result<Self> {
        let document = browser_window
            .document()
            .ok_or_else(|| anyhow::anyhow!("No `document` found on window"))?;

        let canvas: web_sys::HtmlCanvasElement = document
            .create_element("canvas")
            .map_err(|e| anyhow::anyhow!("Failed to create canvas element: {e:?}"))?
            .dyn_into()
            .map_err(|e| anyhow::anyhow!("Created element is not a canvas: {e:?}"))?;

        let dpr = device_pixel_ratio(&browser_window);
        let max_texture_dimension = context.device.limits().max_texture_dimension_2d;
        let has_device_pixel_support = check_device_pixel_support();
        let initial_logical_size = initial_logical_size(&browser_window, params.bounds.size);
        let (initial_physical_width, initial_physical_height) =
            physical_canvas_size(initial_logical_size, dpr, max_texture_dimension);

        canvas.set_tab_index(-1);
        canvas.set_width(initial_physical_width);
        canvas.set_height(initial_physical_height);

        let style = canvas.style();
        style
            .set_property("width", "100%")
            .map_err(|e| anyhow::anyhow!("Failed to set canvas width style: {e:?}"))?;
        style
            .set_property("height", "100%")
            .map_err(|e| anyhow::anyhow!("Failed to set canvas height style: {e:?}"))?;
        style
            .set_property("display", "block")
            .map_err(|e| anyhow::anyhow!("Failed to set canvas display style: {e:?}"))?;
        style
            .set_property("outline", "none")
            .map_err(|e| anyhow::anyhow!("Failed to set canvas outline style: {e:?}"))?;
        style
            .set_property("touch-action", "none")
            .map_err(|e| anyhow::anyhow!("Failed to set touch-action style: {e:?}"))?;

        let body = document
            .body()
            .ok_or_else(|| anyhow::anyhow!("No `body` found on document"))?;
        body.append_child(&canvas)
            .map_err(|e| anyhow::anyhow!("Failed to append canvas to body: {e:?}"))?;
        let html_element_overlay_root = create_overlay_root(&document, &body)?;

        let input_element: web_sys::HtmlInputElement = document
            .create_element("input")
            .map_err(|e| anyhow::anyhow!("Failed to create input element: {e:?}"))?
            .dyn_into()
            .map_err(|e| anyhow::anyhow!("Created element is not an input: {e:?}"))?;
        let input_style = input_element.style();
        input_style.set_property("position", "fixed").ok();
        input_style.set_property("top", "0").ok();
        input_style.set_property("left", "0").ok();
        input_style.set_property("width", "1px").ok();
        input_style.set_property("height", "1px").ok();
        input_style.set_property("opacity", "0").ok();
        body.append_child(&input_element)
            .map_err(|e| anyhow::anyhow!("Failed to append input to body: {e:?}"))?;
        input_element.focus().ok();

        let device_size = Size {
            width: DevicePixels(initial_physical_width as i32),
            height: DevicePixels(initial_physical_height as i32),
        };

        let renderer_config = WgpuSurfaceConfig {
            size: device_size,
            transparent: false,
            preferred_present_mode: None,
        };

        let renderer = WgpuRenderer::new_from_canvas(context, &canvas, renderer_config)?;
        let html_in_canvas_diagnostics = detect_html_in_canvas();
        let layout_subtree_enabled = enable_layout_subtree_if_requested(
            &browser_window,
            &canvas,
            &html_in_canvas_diagnostics,
        );
        let html_island_registry =
            Rc::new(RefCell::new(WebHtmlIslandRegistry::new(canvas.clone())));
        let html_element_registry = Rc::new(RefCell::new(WebHtmlElementRegistry::new(
            html_element_overlay_root,
        )));
        write_html_element_diagnostics_to_global(&html_element_registry.borrow().diagnostics());
        let html_in_canvas_paint_observer = HtmlInCanvasPaintObserver::start(
            &html_in_canvas_diagnostics,
            &canvas,
            layout_subtree_enabled,
            Some(Rc::clone(&html_island_registry)),
        );
        write_html_in_canvas_diagnostics_to_global(
            &html_in_canvas_diagnostics,
            &canvas,
            layout_subtree_enabled,
            &html_in_canvas_paint_observer.snapshot(),
            Some(&html_island_registry.borrow().diagnostics()),
        );

        let display: Rc<dyn PlatformDisplay> = Rc::new(WebDisplay::new(browser_window.clone()));

        let initial_bounds = Bounds {
            origin: Point::default(),
            size: initial_logical_size,
        };

        let mutable_state = WebWindowMutableState {
            bounds: initial_bounds,
            scale_factor: dpr,
            max_texture_dimension,
            title: String::new(),
            input_handler: None,
            is_fullscreen: false,
            is_active: true,
            is_hovered: false,
            mouse_position: Point::default(),
            modifiers: Modifiers::default(),
            capslock: Capslock::default(),
        };

        let is_mac = is_mac_platform(&browser_window);

        let inner = Rc::new(WebWindowInner {
            browser_window,
            html_element_registry,
            html_island_registry,
            canvas,
            input_element,
            has_device_pixel_support,
            is_mac,
            renderer: RefCell::new(renderer),
            state: RefCell::new(mutable_state),
            callbacks: RefCell::new(WebWindowCallbacks::default()),
            click_state: RefCell::new(ClickState::default()),
            pressed_button: Cell::new(None),
            last_physical_size: Cell::new((initial_physical_width, initial_physical_height)),
            notify_scale: Cell::new(false),
            is_composing: Cell::new(false),
            html_island_force_frame: Cell::new(false),
            mql_handle: RefCell::new(None),
            viewport_scheduler: WebViewportScheduler::default(),
            pending_physical_size: Cell::new(None),
            media_query_diagnostics: MediaQueryPipelineDiagnostics::default(),
        });
        inner
            .html_element_registry
            .borrow_mut()
            .set_window(Rc::downgrade(&inner));

        let raf_closure = inner.create_raf_closure();
        inner.schedule_raf(&raf_closure);

        let resize_observer_closure = Self::create_resize_observer_closure(Rc::clone(&inner));
        let resize_observer =
            web_sys::ResizeObserver::new(resize_observer_closure.as_ref().unchecked_ref()).ok();
        let use_browser_resize_fallback = resize_observer.is_none();

        if let Some(ref observer) = resize_observer {
            inner.observe_canvas(observer);
            inner.watch_dpr_changes(observer);
        }

        let event_listeners = inner.register_event_listeners(use_browser_resize_fallback);

        Ok(Self {
            inner,
            display,
            handle,
            _raf_closure: raf_closure,
            _resize_observer: resize_observer,
            _resize_observer_closure: resize_observer_closure,
            _event_listeners: event_listeners,
            _html_in_canvas_paint_observer: html_in_canvas_paint_observer,
            html_in_canvas_diagnostics,
            layout_subtree_enabled: Cell::new(layout_subtree_enabled),
        })
    }

    fn create_resize_observer_closure(
        inner: Rc<WebWindowInner>,
    ) -> Closure<dyn FnMut(js_sys::Array)> {
        Closure::new(move |entries: js_sys::Array| {
            inner.media_query_diagnostics.raw_viewport_signal();
            let entry: web_sys::ResizeObserverEntry = match entries.get(0).dyn_into().ok() {
                Some(entry) => entry,
                None => return,
            };

            // Use CSS content bounds as the logical size and scale explicitly by DPR.
            // `devicePixelContentBoxSize` is not consistently physical in Chromium
            // when Playwright emulates high-DPI, so deriving physical size here keeps
            // the canvas backing store aligned with browser scale.
            let rect = entry.content_rect();
            let logical_width = rect.width() as f32;
            let logical_height = rect.height() as f32;
            inner.enqueue_logical_size(
                Size {
                    width: px(logical_width),
                    height: px(logical_height),
                },
                device_pixel_ratio(&inner.browser_window),
                true,
            );
            // A ResizeObserver callback is the final, canvas-authoritative
            // browser layout signal. Publish it immediately so a busy browser
            // render cadence cannot defer the MediaQuery snapshot by an
            // unbounded number of animation frames. Updating the backing-store
            // size does not change the CSS content box, so this does not create
            // a ResizeObserver feedback loop.
            inner.flush_viewport_scheduler();
        })
    }
}

impl WebWindowInner {
    pub(crate) fn apply_browser_viewport_size(&self) {
        if let Some(logical_size) = viewport_size(&self.browser_window) {
            self.enqueue_logical_size(
                logical_size,
                device_pixel_ratio(&self.browser_window),
                false,
            );
        }
    }

    fn enqueue_logical_size(
        &self,
        logical_size: Size<Pixels>,
        scale_factor: f32,
        allow_zero_size: bool,
    ) {
        self.viewport_scheduler.enqueue(WebViewportObservation {
            logical_size,
            scale_factor,
            allow_zero_size,
        });
    }

    fn enqueue_current_logical_size(&self) {
        self.enqueue_logical_size(
            self.state.borrow().bounds.size,
            device_pixel_ratio(&self.browser_window),
            true,
        );
    }

    fn flush_viewport_scheduler(&self) {
        if let Some(observation) = self.viewport_scheduler.flush() {
            self.media_query_diagnostics.coalesced_flush();
            self.apply_logical_size(
                observation.logical_size,
                observation.scale_factor,
                observation.allow_zero_size,
            );
        }
    }

    fn apply_logical_size(&self, new_size: Size<Pixels>, scale_factor: f32, allow_zero_size: bool) {
        if allow_zero_size
            && (f32::from(new_size.width) <= 0.0 || f32::from(new_size.height) <= 0.0)
        {
            // A hidden canvas does not describe a new viewport. Keep the last
            // visible tuple so responsive consumers never observe a false
            // zero-sized (Phone) branch while the browser is temporarily hidden.
            return;
        }

        let max_texture_dimension = self.state.borrow().max_texture_dimension;
        let (physical_width, physical_height) =
            physical_canvas_size(new_size, scale_factor, max_texture_dimension);
        let logical_size = Size {
            width: px(physical_width as f32 / scale_factor),
            height: px(physical_height as f32 / scale_factor),
        };

        let scale_changed = self.notify_scale.replace(false);
        let prev = self.last_physical_size.get();
        let size_changed = prev != (physical_width, physical_height);

        if !scale_changed && !size_changed {
            return;
        }
        self.last_physical_size
            .set((physical_width, physical_height));

        self.pending_physical_size
            .set(Some((physical_width, physical_height)));

        {
            let mut s = self.state.borrow_mut();
            s.bounds.size = logical_size;
            s.scale_factor = scale_factor;
        }

        self.emit_resize(logical_size, scale_factor);
    }

    fn emit_resize(&self, new_size: Size<Pixels>, scale_factor: f32) {
        self.media_query_diagnostics.gpui_resize_emission();
        let mut callback = {
            let mut callbacks = self.callbacks.borrow_mut();
            callbacks.resize.take()
        };
        if let Some(ref mut callback) = callback {
            callback(new_size, scale_factor);
        }
        let mut callbacks = self.callbacks.borrow_mut();
        if callbacks.resize.is_none() {
            callbacks.resize = callback;
        }
    }

    fn apply_physical_size(&self, width: u32, height: u32) {
        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
        }

        self.renderer.borrow_mut().update_drawable_size(Size {
            width: DevicePixels(width as i32),
            height: DevicePixels(height as i32),
        });
    }

    fn create_raf_closure(self: &Rc<Self>) -> Closure<dyn FnMut()> {
        let raf_handle: Rc<RefCell<Option<js_sys::Function>>> = Rc::new(RefCell::new(None));
        let raf_handle_inner = Rc::clone(&raf_handle);

        let this = Rc::clone(self);
        let closure = Closure::new(move || {
            this.flush_viewport_scheduler();
            let force_html_island_frame = this
                .html_island_registry
                .try_borrow_mut()
                .map(|mut registry| registry.mark_dirty_from_dom_state())
                .unwrap_or(false);
            let mut callback = {
                let mut callbacks = this.callbacks.borrow_mut();
                callbacks.request_frame.take()
            };
            if let Some(ref mut callback) = callback {
                let resize_frame_pending = this.pending_physical_size.get().is_some();
                this.media_query_diagnostics.animation_frame_request();
                callback(RequestFrameOptions {
                    require_presentation: true,
                    force_render: resize_frame_pending
                        || this.html_island_force_frame.take()
                        || force_html_island_frame,
                });
            }
            let mut callbacks = this.callbacks.borrow_mut();
            if callbacks.request_frame.is_none() {
                callbacks.request_frame = callback;
            }

            // Re-schedule for the next frame
            if let Some(ref func) = *raf_handle_inner.borrow() {
                this.browser_window.request_animation_frame(func).ok();
            }
        });

        let js_func: js_sys::Function =
            closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        *raf_handle.borrow_mut() = Some(js_func);

        closure
    }

    fn schedule_raf(&self, closure: &Closure<dyn FnMut()>) {
        self.browser_window
            .request_animation_frame(closure.as_ref().unchecked_ref())
            .ok();
    }

    fn observe_canvas(&self, observer: &web_sys::ResizeObserver) {
        observer.unobserve(&self.canvas);
        if self.has_device_pixel_support {
            let options = web_sys::ResizeObserverOptions::new();
            options.set_box(web_sys::ResizeObserverBoxOptions::DevicePixelContentBox);
            observer.observe_with_options(&self.canvas, &options);
        } else {
            observer.observe(&self.canvas);
        }
    }

    fn watch_dpr_changes(self: &Rc<Self>, observer: &web_sys::ResizeObserver) {
        let current_dpr = self.browser_window.device_pixel_ratio();
        let media_query =
            format!("(resolution: {current_dpr}dppx), (-webkit-device-pixel-ratio: {current_dpr})");
        let Some(mql) = self.browser_window.match_media(&media_query).ok().flatten() else {
            return;
        };

        let this = Rc::clone(self);
        let observer = observer.clone();

        let closure = Closure::<dyn FnMut(JsValue)>::new(move |_event: JsValue| {
            this.notify_scale.set(true);
            this.enqueue_current_logical_size();
            this.observe_canvas(&observer);
            this.watch_dpr_changes(&observer);
        });

        mql.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
            .ok();

        *self.mql_handle.borrow_mut() = Some(MqlHandle {
            mql,
            _closure: closure,
        });
    }

    pub(crate) fn register_visibility_change(
        self: &Rc<Self>,
    ) -> Option<Closure<dyn FnMut(JsValue)>> {
        let document = self.browser_window.document()?;
        let this = Rc::clone(self);

        let closure = Closure::<dyn FnMut(JsValue)>::new(move |_event: JsValue| {
            let is_visible = this
                .browser_window
                .document()
                .map(|doc| {
                    let state_str: String = js_sys::Reflect::get(&doc, &"visibilityState".into())
                        .ok()
                        .and_then(|v| v.as_string())
                        .unwrap_or_default();
                    state_str == "visible"
                })
                .unwrap_or(true);

            {
                let mut state = this.state.borrow_mut();
                state.is_active = is_visible;
            }
            let mut callback = {
                let mut callbacks = this.callbacks.borrow_mut();
                callbacks.active_status_change.take()
            };
            if let Some(ref mut callback) = callback {
                callback(is_visible);
            }
            let mut callbacks = this.callbacks.borrow_mut();
            if callbacks.active_status_change.is_none() {
                callbacks.active_status_change = callback;
            }
        });

        document
            .add_event_listener_with_callback("visibilitychange", closure.as_ref().unchecked_ref())
            .ok();

        Some(closure)
    }

    pub(crate) fn with_input_handler<R>(
        &self,
        f: impl FnOnce(&mut PlatformInputHandler) -> R,
    ) -> Option<R> {
        let mut handler = self.state.borrow_mut().input_handler.take()?;
        let result = f(&mut handler);
        self.state.borrow_mut().input_handler = Some(handler);
        Some(result)
    }

    pub(crate) fn register_appearance_change(
        self: &Rc<Self>,
    ) -> Option<Closure<dyn FnMut(JsValue)>> {
        let mql = self
            .browser_window
            .match_media("(prefers-color-scheme: dark)")
            .ok()??;

        let this = Rc::clone(self);
        let closure = Closure::<dyn FnMut(JsValue)>::new(move |_event: JsValue| {
            let mut callback = {
                let mut callbacks = this.callbacks.borrow_mut();
                callbacks.appearance_changed.take()
            };
            if let Some(ref mut callback) = callback {
                callback();
            }
            let mut callbacks = this.callbacks.borrow_mut();
            if callbacks.appearance_changed.is_none() {
                callbacks.appearance_changed = callback;
            }
        });

        mql.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
            .ok();

        Some(closure)
    }
}

fn current_appearance(browser_window: &web_sys::Window) -> WindowAppearance {
    let is_dark = browser_window
        .match_media("(prefers-color-scheme: dark)")
        .ok()
        .flatten()
        .map(|mql| mql.matches())
        .unwrap_or(false);

    if is_dark {
        WindowAppearance::Dark
    } else {
        WindowAppearance::Light
    }
}

fn device_pixel_ratio(browser_window: &web_sys::Window) -> f32 {
    let dpr = browser_window.device_pixel_ratio() as f32;
    if dpr.is_finite() && dpr > 0.0 {
        dpr
    } else {
        1.0
    }
}

fn initial_logical_size(
    browser_window: &web_sys::Window,
    fallback_size: Size<Pixels>,
) -> Size<Pixels> {
    viewport_size(browser_window).unwrap_or_else(|| {
        if f32::from(fallback_size.width) > 0.0 && f32::from(fallback_size.height) > 0.0 {
            fallback_size
        } else {
            Size {
                width: px(1.0),
                height: px(1.0),
            }
        }
    })
}

fn viewport_size(browser_window: &web_sys::Window) -> Option<Size<Pixels>> {
    let width = browser_window.inner_width().ok()?.as_f64()? as f32;
    let height = browser_window.inner_height().ok()?.as_f64()? as f32;

    if width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0 {
        Some(Size {
            width: px(width),
            height: px(height),
        })
    } else {
        None
    }
}

fn physical_canvas_size(
    logical_size: Size<Pixels>,
    scale_factor: f32,
    max_texture_dimension: u32,
) -> (u32, u32) {
    let width = physical_canvas_axis(logical_size.width, scale_factor, max_texture_dimension);
    let height = physical_canvas_axis(logical_size.height, scale_factor, max_texture_dimension);
    (width, height)
}

fn physical_canvas_axis(size: Pixels, scale_factor: f32, max_texture_dimension: u32) -> u32 {
    let physical_size = (f32::from(size) * scale_factor).round();
    let clamped_size = physical_size.clamp(1.0, max_texture_dimension as f32);
    clamped_size as u32
}

struct MqlHandle {
    mql: web_sys::MediaQueryList,
    _closure: Closure<dyn FnMut(JsValue)>,
}

impl Drop for MqlHandle {
    fn drop(&mut self) {
        self.mql
            .remove_event_listener_with_callback("change", self._closure.as_ref().unchecked_ref())
            .ok();
    }
}

// Safari does not support `devicePixelContentBoxSize`, so detect whether it's available.
fn check_device_pixel_support() -> bool {
    let global: JsValue = js_sys::global().into();
    let Ok(constructor) = js_sys::Reflect::get(&global, &"ResizeObserverEntry".into()) else {
        return false;
    };
    let Ok(prototype) = js_sys::Reflect::get(&constructor, &"prototype".into()) else {
        return false;
    };
    let descriptor = js_sys::Object::get_own_property_descriptor(
        &prototype.unchecked_into::<js_sys::Object>(),
        &"devicePixelContentBoxSize".into(),
    );
    !descriptor.is_undefined()
}

impl raw_window_handle::HasWindowHandle for WebWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let canvas_ref: &JsValue = self.inner.canvas.as_ref();
        let obj = std::ptr::NonNull::from(canvas_ref).cast::<std::ffi::c_void>();
        let handle = raw_window_handle::WebCanvasWindowHandle::new(obj);
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(handle.into()) })
    }
}

impl raw_window_handle::HasDisplayHandle for WebWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::web())
    }
}

impl PlatformWindow for WebWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.inner.state.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        false
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.inner.state.borrow().bounds.size
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let style = self.inner.canvas.style();
        style
            .set_property("width", &format!("{}px", f32::from(size.width)))
            .ok();
        style
            .set_property("height", &format!("{}px", f32::from(size.height)))
            .ok();
    }

    fn scale_factor(&self) -> f32 {
        self.inner.state.borrow().scale_factor
    }

    fn appearance(&self) -> WindowAppearance {
        current_appearance(&self.inner.browser_window)
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.display.clone())
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.inner.state.borrow().mouse_position
    }

    fn modifiers(&self) -> Modifiers {
        self.inner.state.borrow().modifiers
    }

    fn capslock(&self) -> Capslock {
        self.inner.state.borrow().capslock
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.inner.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.inner.state.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        None
    }

    fn activate(&self) {
        self.inner.state.borrow_mut().is_active = true;
    }

    fn is_active(&self) -> bool {
        self.inner.state.borrow().is_active
    }

    fn is_hovered(&self) -> bool {
        self.inner.state.borrow().is_hovered
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }

    fn set_title(&mut self, title: &str) {
        self.inner.state.borrow_mut().title = title.to_owned();
        if let Some(document) = self.inner.browser_window.document() {
            document.set_title(title);
        }
    }

    fn set_background_appearance(&self, _background: WindowBackgroundAppearance) {}

    fn minimize(&self) {
        log::warn!("WebWindow::minimize is not supported in the browser");
    }

    fn zoom(&self) {
        log::warn!("WebWindow::zoom is not supported in the browser");
    }

    fn toggle_fullscreen(&self) {
        let mut state = self.inner.state.borrow_mut();
        state.is_fullscreen = !state.is_fullscreen;

        if state.is_fullscreen {
            let canvas: &web_sys::Element = self.inner.canvas.as_ref();
            canvas.request_fullscreen().ok();
        } else {
            if let Some(document) = self.inner.browser_window.document() {
                document.exit_fullscreen();
            }
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.inner.state.borrow().is_fullscreen
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.inner.callbacks.borrow_mut().request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.inner.callbacks.borrow_mut().input = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.inner.callbacks.borrow_mut().active_status_change = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.inner.callbacks.borrow_mut().hover_status_change = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.inner.callbacks.borrow_mut().resize = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.inner.callbacks.borrow_mut().moved = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.inner.callbacks.borrow_mut().should_close = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.inner.callbacks.borrow_mut().close = Some(callback);
    }

    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
        self.inner.callbacks.borrow_mut().hit_test_window_control = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.inner.callbacks.borrow_mut().appearance_changed = Some(callback);
    }

    fn draw(&self, scene: &Scene) {
        if let Some((width, height)) = self.inner.pending_physical_size.take() {
            self.inner.apply_physical_size(width, height);
        }

        let canvas_rect = self.inner.canvas.get_bounding_client_rect();
        self.inner
            .html_element_registry
            .borrow_mut()
            .sync_roots(&scene.html_elements, &canvas_rect);
        write_html_element_diagnostics_to_global(
            &self.inner.html_element_registry.borrow().diagnostics(),
        );

        if !scene.html_islands.is_empty()
            && !self.layout_subtree_enabled.get()
            && enable_layout_subtree(&self.inner.canvas, &self.html_in_canvas_diagnostics)
        {
            self.layout_subtree_enabled.set(true);
            self._html_in_canvas_paint_observer
                .request_paint(&self.inner.canvas);
        }

        let layout_subtree_enabled = self.layout_subtree_enabled.get();
        let html_island_sync = if layout_subtree_enabled {
            self.inner
                .html_island_registry
                .borrow_mut()
                .sync_roots(&scene.html_islands)
        } else {
            self.inner.html_island_registry.borrow_mut().sync_roots(&[])
        };
        if html_island_sync.request_paint {
            self._html_in_canvas_paint_observer
                .request_paint(&self.inner.canvas);
        }
        let upload_jobs = if layout_subtree_enabled {
            self.inner
                .html_island_registry
                .borrow_mut()
                .dirty_upload_jobs()
        } else {
            Vec::new()
        };
        if self
            .inner
            .html_island_registry
            .borrow()
            .has_pending_upload_work()
        {
            self.inner.html_island_force_frame.set(true);
        }
        for job in upload_jobs {
            let upload_result = {
                self.inner
                    .renderer
                    .borrow_mut()
                    .upload_html_island_element_to_texture(&job.id, &job.element, job.physical_size)
            };
            match upload_result {
                Ok(result) => {
                    self.inner
                        .html_island_registry
                        .borrow_mut()
                        .record_upload_success(
                            &job.id,
                            result.path.as_str(),
                            result.texture_reallocated,
                            result.source_sample,
                        );
                }
                Err(error) => {
                    let error = error.to_string();
                    if error.contains("No cached paint record for element") {
                        self.inner
                            .html_island_registry
                            .borrow_mut()
                            .defer_upload_until_next_paint(&job.id);
                    } else {
                        self.inner
                            .renderer
                            .borrow_mut()
                            .discard_html_island_texture(&job.id);
                        self.inner
                            .html_island_registry
                            .borrow_mut()
                            .record_upload_failure(&job.id, error);
                    }
                }
            }
        }
        let _needs_redraw = {
            let mut renderer = self.inner.renderer.borrow_mut();
            renderer.draw(scene);
            renderer.needs_redraw()
        };
        write_text_renderer_diagnostics_to_global(&self.inner.renderer.borrow(), scene);
        let (texture_count, texture_pruned_count, texture_draw_count) = self
            .inner
            .renderer
            .borrow()
            .html_island_texture_diagnostics();
        self.inner
            .html_island_registry
            .borrow_mut()
            .record_texture_cache_state(texture_count, texture_pruned_count, texture_draw_count);
        write_html_in_canvas_diagnostics_to_global(
            &self.html_in_canvas_diagnostics,
            &self.inner.canvas,
            layout_subtree_enabled,
            &self._html_in_canvas_paint_observer.snapshot(),
            Some(&self.inner.html_island_registry.borrow().diagnostics()),
        );
    }

    fn effect_capabilities(&self) -> EffectCapabilities {
        self.inner.renderer.borrow().effect_capabilities()
    }

    fn completed_frame(&self) {
        // On web, presentation happens automatically via wgpu surface present
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.inner.renderer.borrow().sprite_atlas().clone()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        Some(self.inner.renderer.borrow().gpu_specs())
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn request_decorations(&self, _decorations: WindowDecorations) {}

    fn show_window_menu(&self, _position: Point<Pixels>) {}

    fn start_window_move(&self) {}

    fn start_window_resize(&self, _edge: ResizeEdge) {}

    fn window_decorations(&self) -> Decorations {
        Decorations::Server
    }

    fn set_app_id(&mut self, _app_id: &str) {}

    fn window_controls(&self) -> WindowControls {
        WindowControls {
            fullscreen: true,
            maximize: false,
            minimize: false,
            window_menu: false,
        }
    }

    fn set_client_inset(&self, _inset: Pixels) {}
}

fn text_rendering_diagnostics_enabled() -> bool {
    js_sys::Reflect::get(
        &js_sys::global(),
        &wasm_bindgen::JsValue::from_str("__rayxTextRenderingDiagnosticsEnabled"),
    )
    .ok()
    .and_then(|value| value.as_bool())
    .unwrap_or(false)
}

fn write_text_renderer_diagnostics_to_global(renderer: &WgpuRenderer, scene: &Scene) {
    if !text_rendering_diagnostics_enabled() {
        return;
    }

    let diagnostics = renderer.text_renderer_diagnostics(scene);
    let target = js_sys::Object::new();
    let set = |key: &str, value: wasm_bindgen::JsValue| {
        let _ = js_sys::Reflect::set(&target, &key.into(), &value);
    };
    set("diagnosticVersion", "text-renderer-wasm/v1".into());
    set("antialiasingMode", diagnostics.antialiasing_mode.into());
    set(
        "dualSourceBlendingSupported",
        renderer.supports_dual_source_blending().into(),
    );
    set("glyphSpriteKind", diagnostics.glyph_sprite_kind.into());
    set(
        "monochromeCoverageKind",
        "separate-glyph-and-image-mask".into(),
    );
    set(
        "glyphMaskCount",
        (diagnostics.glyph_mask_count as f64).into(),
    );
    set(
        "imageMaskCount",
        (diagnostics.image_mask_count as f64).into(),
    );
    set(
        "monochromeSpriteCount",
        (diagnostics.monochrome_sprite_count as f64).into(),
    );
    set(
        "subpixelSpriteCount",
        (diagnostics.subpixel_sprite_count as f64).into(),
    );
    set(
        "polychromeSpriteCount",
        (diagnostics.polychrome_sprite_count as f64).into(),
    );
    set(
        "devicePixelRatio",
        (diagnostics.device_pixel_ratio as f64).into(),
    );
    set("surfaceFormat", diagnostics.surface_format.into());
    set("publishedAtMs", js_sys::Date::now().into());
    let _ = js_sys::Reflect::set(
        &js_sys::global(),
        &wasm_bindgen::JsValue::from_str("__rayxTextRendererDiagnostics"),
        target.as_ref(),
    );
}
