use crate::display::WebDisplay;
use crate::events::{
    ClickState, EventListenerHandle, TouchIds, WebEventListeners, is_mac_platform,
};
#[cfg(feature = "diagnostics")]
use crate::html_element::write_html_element_diagnostics_to_global;
use crate::html_element::{WebHtmlElementRegistry, create_overlay_root};
#[cfg(feature = "diagnostics")]
use crate::html_in_canvas::write_html_in_canvas_diagnostics_to_global;
use crate::html_in_canvas::{
    HtmlInCanvasCapabilities, HtmlInCanvasPaintObserver, detect_html_in_canvas,
    enable_layout_subtree, enable_layout_subtree_if_requested,
};
use crate::html_island::WebHtmlIslandRegistry;
use crate::ime_mirror::ImeMirror;
#[cfg(feature = "diagnostics")]
use crate::media_query_diagnostics::MediaQueryPipelineDiagnostics;
use crate::viewport::WebViewport;
use crate::web_viewport_scheduler::{WebViewportObservation, WebViewportScheduler};
use std::sync::Arc;
use std::{cell::Cell, cell::RefCell, rc::Rc};

use gpui_wgpu::{WgpuContext, WgpuRenderer, WgpuSurfaceConfig, wgpu};
use gpux::{
    AnyWindowHandle, Bounds, Capslock, ClipboardItem, Decorations, DevicePixels,
    DispatchEventResult, EffectCapabilities, FrameRequestSource, GpuSpecs, Modifiers, MouseButton,
    Pixels, PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow,
    Point, PromptButton, PromptLevel, RequestFrameOptions, ResizeEdge, Scene, Size,
    TextInputConfiguration, TextInputStateChange, WindowAppearance, WindowBackgroundAppearance,
    WindowBounds, WindowControlArea, WindowControls, WindowDecorations, WindowInsets, WindowParams,
    WindowVisibility, px,
};
use wasm_bindgen::prelude::*;

#[derive(Default)]
pub(crate) struct WebWindowCallbacks {
    pub(crate) request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    pub(crate) input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    pub(crate) active_status_change: Option<Box<dyn FnMut(bool)>>,
    pub(crate) visibility_change: Option<Box<dyn FnMut(WindowVisibility)>>,
    pub(crate) hover_status_change: Option<Box<dyn FnMut(bool)>>,
    pub(crate) resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    pub(crate) visual_viewport_changed: Option<Box<dyn FnMut()>>,
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
    pub(crate) visibility: WindowVisibility,
    pub(crate) is_hovered: bool,
    pub(crate) mouse_position: Point<Pixels>,
    pub(crate) modifiers: Modifiers,
    pub(crate) capslock: Capslock,
}

#[cfg(feature = "diagnostics")]
struct WasmTextScaleDiagnostics {
    generation: Cell<u64>,
    published_generation: Cell<Option<u64>>,
    logical_width: Cell<f32>,
    logical_height: Cell<f32>,
    scale_factor: Cell<f32>,
    physical_width: Cell<u32>,
    physical_height: Cell<u32>,
}

#[cfg(feature = "diagnostics")]
impl WasmTextScaleDiagnostics {
    fn new(
        logical_size: Size<Pixels>,
        scale_factor: f32,
        physical_width: u32,
        physical_height: u32,
    ) -> Self {
        Self {
            generation: Cell::new(0),
            published_generation: Cell::new(None),
            logical_width: Cell::new(f32::from(logical_size.width)),
            logical_height: Cell::new(f32::from(logical_size.height)),
            scale_factor: Cell::new(scale_factor),
            physical_width: Cell::new(physical_width),
            physical_height: Cell::new(physical_height),
        }
    }

    fn record_generation(
        &self,
        logical_size: Size<Pixels>,
        scale_factor: f32,
        physical_width: u32,
        physical_height: u32,
    ) {
        self.generation.set(self.generation.get().saturating_add(1));
        self.logical_width.set(f32::from(logical_size.width));
        self.logical_height.set(f32::from(logical_size.height));
        self.scale_factor.set(scale_factor);
        self.physical_width.set(physical_width);
        self.physical_height.set(physical_height);
    }

    fn publish(&self, canvas: &web_sys::HtmlCanvasElement, closed: bool) {
        let generation = self.generation.get();
        if !closed && self.published_generation.get() == Some(generation) {
            return;
        }
        let target = js_sys::Object::new();
        let set = |key: &str, value: JsValue| {
            let _ = js_sys::Reflect::set(&target, &JsValue::from_str(key), &value);
        };
        set("version", JsValue::from_f64(1.0));
        set("generation", JsValue::from_f64(generation as f64));
        set(
            "logicalWidth",
            JsValue::from_f64(self.logical_width.get() as f64),
        );
        set(
            "logicalHeight",
            JsValue::from_f64(self.logical_height.get() as f64),
        );
        set(
            "scaleFactor",
            JsValue::from_f64(self.scale_factor.get() as f64),
        );
        set(
            "physicalWidth",
            JsValue::from_f64(self.physical_width.get() as f64),
        );
        set(
            "physicalHeight",
            JsValue::from_f64(self.physical_height.get() as f64),
        );
        set("canvasWidth", JsValue::from_f64(canvas.width() as f64));
        set("canvasHeight", JsValue::from_f64(canvas.height() as f64));
        set(
            "surfaceWidth",
            JsValue::from_f64(self.physical_width.get() as f64),
        );
        set(
            "surfaceHeight",
            JsValue::from_f64(self.physical_height.get() as f64),
        );
        set("closed", JsValue::from_bool(closed));
        gpui_wgpu::web_globals::publish("WasmTextScaleDiagnostics", &target);
        self.published_generation.set(Some(generation));
    }
}

pub(crate) struct WebWindowInner {
    pub(crate) browser_window: web_sys::Window,
    pub(crate) canvas: web_sys::HtmlCanvasElement,
    pub(crate) ime_mirror: ImeMirror,
    pub(crate) viewport: RefCell<WebViewport>,
    pub(crate) has_device_pixel_support: bool,
    pub(crate) is_mac: bool,
    pub(crate) touch_input: bool,
    pub(crate) renderer: RefCell<WgpuRenderer>,
    pub(crate) html_element_registry: Rc<RefCell<WebHtmlElementRegistry>>,
    pub(crate) fast_composition: RefCell<crate::fast::composition::Composition>,
    pub(crate) html_island_registry: Rc<RefCell<WebHtmlIslandRegistry>>,
    pub(crate) state: RefCell<WebWindowMutableState>,
    pub(crate) callbacks: RefCell<WebWindowCallbacks>,
    pub(crate) click_state: RefCell<ClickState>,
    pub(crate) touch_ids: RefCell<TouchIds>,
    pub(crate) pressed_button: Cell<Option<MouseButton>>,
    pub(crate) closed: Cell<bool>,
    pub(crate) last_physical_size: Cell<(u32, u32)>,
    pub(crate) is_composing: Cell<bool>,
    pub(crate) clipboard: Rc<RefCell<Option<ClipboardItem>>>,
    /// Set while `show_virtual_keyboard` blur/focus-cycles the hidden input.
    /// The cycle is a keyboard-visibility hint, not a real activity change;
    /// letting the focus/blur listeners report it would re-enter GPUI
    /// synchronously from inside an input dispatch, and a `RefCell`
    /// double-borrow panic on wasm never unwinds, wedging the app.
    pub(crate) suppress_focus_status_events: Cell<bool>,
    /// The visual viewport height when the current pointer gesture began.
    /// A mid-gesture change means the software keyboard opened or closed and
    /// reflowed the layout, so the release position no longer refers to what
    /// the user aimed at.
    pub(crate) gesture_start_visual_viewport_height: Cell<f64>,
    /// A touch that may still resolve into a tap: its pointer id and starting
    /// position, cleared once it travels beyond touch slop. Virtual keyboard
    /// and IME focus may only change when a touch release completes a tap;
    /// pans must leave them untouched, or scrolling over editable content
    /// flickers the keyboard and drags the caret around.
    pub(crate) touch_tap_candidate: Cell<Option<(i32, Point<Pixels>)>>,
    html_island_force_frame: Cell<bool>,
    mql_handle: RefCell<Option<MqlHandle>>,
    viewport_scheduler: WebViewportScheduler,
    pending_physical_size: Cell<Option<(u32, u32)>>,
    raf_id: Cell<Option<i32>>,
    raf_function: RefCell<Option<js_sys::Function>>,
    #[cfg(feature = "diagnostics")]
    media_query_diagnostics: MediaQueryPipelineDiagnostics,
    #[cfg(feature = "diagnostics")]
    wasm_text_scale_diagnostics: WasmTextScaleDiagnostics,
}

pub struct WebWindow {
    pub(crate) inner: Rc<WebWindowInner>,
    display: Rc<dyn PlatformDisplay>,
    #[allow(dead_code)]
    handle: AnyWindowHandle,
    _raf_closure: Closure<dyn FnMut()>,
    _resize_observer: Option<web_sys::ResizeObserver>,
    _resize_observer_closure: Closure<dyn FnMut(js_sys::Array)>,
    _safe_area_observer: Option<web_sys::ResizeObserver>,
    _safe_area_observer_closure: Closure<dyn FnMut(js_sys::Array)>,
    _event_listeners: WebEventListeners,
    _html_in_canvas_paint_observer: HtmlInCanvasPaintObserver,
    html_in_canvas_capabilities: HtmlInCanvasCapabilities,
    layout_subtree_enabled: Cell<bool>,
}

impl WebWindow {
    pub(crate) fn prepare_canvas(
        browser_window: &web_sys::Window,
    ) -> anyhow::Result<web_sys::HtmlCanvasElement> {
        let document = browser_window
            .document()
            .ok_or_else(|| anyhow::anyhow!("No `document` found on window"))?;
        let canvas: web_sys::HtmlCanvasElement = document
            .create_element("canvas")
            .map_err(|error| anyhow::anyhow!("Failed to create canvas element: {error:?}"))?
            .dyn_into()
            .map_err(|error| anyhow::anyhow!("Created element is not a canvas: {error:?}"))?;
        canvas.set_tab_index(-1);

        let style = canvas.style();
        for (property, value) in [
            ("width", "100%"),
            ("height", "100%"),
            ("display", "block"),
            ("outline", "none"),
            ("touch-action", "none"),
            ("-webkit-touch-callout", "none"),
            ("user-select", "none"),
            ("-webkit-user-select", "none"),
        ] {
            style.set_property(property, value).map_err(|error| {
                anyhow::anyhow!("Failed to set canvas {property} style: {error:?}")
            })?;
        }

        let body = document
            .body()
            .ok_or_else(|| anyhow::anyhow!("No `body` found on document"))?;
        body.append_child(&canvas)
            .map_err(|error| anyhow::anyhow!("Failed to append canvas to body: {error:?}"))?;
        Ok(canvas)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        handle: AnyWindowHandle,
        params: WindowParams,
        context: &WgpuContext,
        canvas: web_sys::HtmlCanvasElement,
        surface: wgpu::Surface<'static>,
        browser_window: web_sys::Window,
        clipboard: Rc<RefCell<Option<ClipboardItem>>>,
    ) -> anyhow::Result<Self> {
        let document = browser_window
            .document()
            .ok_or_else(|| anyhow::anyhow!("No `document` found on window"))?;

        let dpr = device_pixel_ratio(&browser_window);
        let max_texture_dimension = context.device.limits().max_texture_dimension_2d;
        let has_device_pixel_support = check_device_pixel_support();
        let initial_logical_size = initial_logical_size(&browser_window, params.bounds.size);
        let (initial_physical_width, initial_physical_height) =
            physical_canvas_size(initial_logical_size, dpr, max_texture_dimension);

        canvas.set_width(initial_physical_width);
        canvas.set_height(initial_physical_height);

        let body = document
            .body()
            .ok_or_else(|| anyhow::anyhow!("No `body` found on document"))?;
        let html_element_overlay_root = create_overlay_root(&document, &body)?;

        let touch_input = browser_window
            .match_media("(pointer: coarse)")
            .ok()
            .flatten()
            .is_some_and(|query| query.matches());
        let ime_mirror = ImeMirror::new(&document, &body, touch_input)?;
        let mut viewport = WebViewport::new(&document)?;
        viewport.update(&browser_window, &canvas)?;

        let device_size = Size {
            width: DevicePixels(initial_physical_width as i32),
            height: DevicePixels(initial_physical_height as i32),
        };

        let renderer_config = WgpuSurfaceConfig {
            size: device_size,
            transparent: false,
            preferred_present_mode: None,
        };

        let renderer = WgpuRenderer::new_from_surface(context, surface, renderer_config)?;
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
        #[cfg(feature = "diagnostics")]
        write_html_element_diagnostics_to_global(&html_element_registry.borrow().diagnostics());
        let html_in_canvas_paint_observer = HtmlInCanvasPaintObserver::start(
            &html_in_canvas_diagnostics,
            &canvas,
            layout_subtree_enabled,
            Some(Rc::clone(&html_island_registry)),
        );
        #[cfg(feature = "diagnostics")]
        write_html_in_canvas_diagnostics_to_global(
            &html_in_canvas_diagnostics.diagnostics(),
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
            is_active: document_is_active(&browser_window),
            visibility: document_visibility(&browser_window),
            is_hovered: false,
            mouse_position: Point::default(),
            modifiers: Modifiers::default(),
            capslock: Capslock::default(),
        };

        let is_mac = is_mac_platform(&browser_window);

        let inner = Rc::new(WebWindowInner {
            browser_window,
            html_element_registry,
            fast_composition: RefCell::default(),
            html_island_registry,
            canvas,
            ime_mirror,
            viewport: RefCell::new(viewport),
            has_device_pixel_support,
            is_mac,
            touch_input,
            renderer: RefCell::new(renderer),
            state: RefCell::new(mutable_state),
            callbacks: RefCell::new(WebWindowCallbacks::default()),
            click_state: RefCell::new(ClickState::default()),
            touch_ids: RefCell::new(TouchIds::default()),
            pressed_button: Cell::new(None),
            closed: Cell::new(false),
            last_physical_size: Cell::new((initial_physical_width, initial_physical_height)),
            is_composing: Cell::new(false),
            clipboard,
            suppress_focus_status_events: Cell::new(false),
            gesture_start_visual_viewport_height: Cell::new(0.0),
            touch_tap_candidate: Cell::new(None),
            html_island_force_frame: Cell::new(false),
            mql_handle: RefCell::new(None),
            viewport_scheduler: WebViewportScheduler::default(),
            pending_physical_size: Cell::new(None),
            raf_id: Cell::new(None),
            raf_function: RefCell::new(None),
            #[cfg(feature = "diagnostics")]
            media_query_diagnostics: MediaQueryPipelineDiagnostics::default(),
            #[cfg(feature = "diagnostics")]
            wasm_text_scale_diagnostics: WasmTextScaleDiagnostics::new(
                initial_logical_size,
                dpr,
                initial_physical_width,
                initial_physical_height,
            ),
        });
        inner
            .html_element_registry
            .borrow_mut()
            .set_window(Rc::downgrade(&inner));

        let raf_closure = inner.create_raf_closure();
        inner.wake_frame_loop();

        let resize_observer_closure = Self::create_resize_observer_closure(Rc::clone(&inner));
        let resize_observer =
            web_sys::ResizeObserver::new(resize_observer_closure.as_ref().unchecked_ref()).ok();
        let use_browser_resize_fallback = resize_observer.is_none();

        if let Some(ref observer) = resize_observer {
            inner.observe_canvas(observer);
            inner.watch_dpr_changes(observer);
        }

        let mut event_listeners = inner.register_event_listeners();
        // The canvas ResizeObserver is the authoritative logical viewport: it
        // measures the actual surface GPUI renders into. A browser `resize`
        // event can arrive before that surface has been laid out and would
        // otherwise race it with window.innerWidth/innerHeight.
        if use_browser_resize_fallback {
            event_listeners.push(inner.register_browser_resize_fallback());
        }
        let safe_area_observer_closure = Closure::wrap(Box::new({
            let inner = Rc::downgrade(&inner);
            move |_: js_sys::Array| {
                if let Some(inner) = inner.upgrade() {
                    inner.notify_viewport_changed();
                }
            }
        }) as Box<dyn FnMut(js_sys::Array)>);
        let safe_area_observer =
            match web_sys::ResizeObserver::new(safe_area_observer_closure.as_ref().unchecked_ref())
            {
                Ok(observer) => {
                    inner.viewport.borrow().observe_safe_area(&observer);
                    Some(observer)
                }
                Err(error) => {
                    log::warn!("Failed to observe safe area: {error:?}");
                    None
                }
            };

        Ok(Self {
            inner,
            display,
            handle,
            _raf_closure: raf_closure,
            _resize_observer: resize_observer,
            _resize_observer_closure: resize_observer_closure,
            _safe_area_observer: safe_area_observer,
            _safe_area_observer_closure: safe_area_observer_closure,
            _event_listeners: event_listeners,
            _html_in_canvas_paint_observer: html_in_canvas_paint_observer,
            html_in_canvas_capabilities: html_in_canvas_diagnostics,
            layout_subtree_enabled: Cell::new(layout_subtree_enabled),
        })
    }

    fn create_resize_observer_closure(
        inner: Rc<WebWindowInner>,
    ) -> Closure<dyn FnMut(js_sys::Array)> {
        Closure::new(move |entries: js_sys::Array| {
            if inner.closed.get() {
                return;
            }
            #[cfg(feature = "diagnostics")]
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

            // ResizeObserver runs after layout but before the browser paints.
            // Render synchronously here so the newly resized CSS canvas is
            // never presented with its previous backing image stretched into
            // the new viewport dimensions.
            if inner.pending_physical_size.get().is_some() {
                inner.with_callback(
                    |callbacks| &mut callbacks.request_frame,
                    |callback| {
                        callback(RequestFrameOptions {
                            require_presentation: true,
                            force_render: true,
                            signal_at: None,
                            signal_source: FrameRequestSource::NativeCallback,
                        })
                    },
                );
            }
        })
    }
}

impl WebWindowInner {
    fn sample_viewport(&self) -> bool {
        match self
            .viewport
            .borrow_mut()
            .update(&self.browser_window, &self.canvas)
        {
            Ok(changed) => changed,
            Err(error) => {
                log::warn!("Failed to update browser viewport: {error:#}");
                false
            }
        }
    }

    pub(crate) fn notify_viewport_changed(&self) {
        self.with_callback(
            |callbacks| &mut callbacks.visual_viewport_changed,
            |callback| callback(),
        );
    }

    pub(crate) fn refresh_active_status(&self) {
        let active = document_is_active(&self.browser_window);
        if std::mem::replace(&mut self.state.borrow_mut().is_active, active) != active {
            self.with_callback(
                |callbacks| &mut callbacks.active_status_change,
                |callback| callback(active),
            );
        }
    }

    /// Invokes a registered callback with take/call/restore semantics.
    ///
    /// The callback is removed from the slot for the duration of the call, so
    /// the `RefCell` is not borrowed while user code runs: a callback that
    /// re-enters the platform window (dispatching input, registering
    /// handlers) would otherwise panic with a `BorrowMutError`. A re-entrant
    /// invocation of the same callback finds the slot empty and is a no-op.
    pub(crate) fn with_callback<C, R>(
        &self,
        select: impl Fn(&mut WebWindowCallbacks) -> &mut Option<C>,
        invoke: impl FnOnce(&mut C) -> R,
    ) -> Option<R> {
        let mut callback = select(&mut self.callbacks.borrow_mut()).take()?;
        let result = invoke(&mut callback);
        *select(&mut self.callbacks.borrow_mut()) = Some(callback);
        Some(result)
    }

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
            #[cfg(feature = "diagnostics")]
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

        let previous_scale_factor = self.state.borrow().scale_factor;
        let scale_changed = previous_scale_factor.to_bits() != scale_factor.to_bits();
        let prev = self.last_physical_size.get();
        let size_changed = prev != (physical_width, physical_height);

        if !scale_changed && !size_changed {
            return;
        }
        self.last_physical_size
            .set((physical_width, physical_height));

        self.pending_physical_size
            .set(Some((physical_width, physical_height)));

        #[cfg(feature = "diagnostics")]
        self.wasm_text_scale_diagnostics.record_generation(
            logical_size,
            scale_factor,
            physical_width,
            physical_height,
        );

        {
            let mut s = self.state.borrow_mut();
            s.bounds.size = logical_size;
            s.scale_factor = scale_factor;
        }

        self.emit_resize(logical_size, scale_factor);
    }

    fn emit_resize(&self, new_size: Size<Pixels>, scale_factor: f32) {
        #[cfg(feature = "diagnostics")]
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
        let this = Rc::clone(self);
        let closure = Closure::new(move || {
            // The request that fired is no longer pending; clear it before
            // running the frame so wakeups issued while the frame executes
            // schedule the next request instead of being swallowed.
            this.raf_id.set(None);
            if this.closed.get() {
                return;
            }
            this.flush_viewport_scheduler();
            let force_html_island_frame = this
                .html_island_registry
                .try_borrow_mut()
                .map(|mut registry| registry.mark_dirty_from_dom_state())
                .unwrap_or(false);
            let resize_frame_pending = this.pending_physical_size.get().is_some();
            #[cfg(feature = "diagnostics")]
            this.media_query_diagnostics.animation_frame_request();
            this.with_callback(
                |callbacks| &mut callbacks.request_frame,
                |callback| {
                    callback(RequestFrameOptions {
                        require_presentation: true,
                        force_render: resize_frame_pending
                            || this.html_island_force_frame.take()
                            || force_html_island_frame,
                        signal_at: None,
                        signal_source: FrameRequestSource::NativeCallback,
                    })
                },
            );

            // gpux keeps a continuous browser frame loop: HTML-island paint
            // readiness, viewport coalescing, and pending physical-size presents
            // are observed per animation frame rather than through wakeups.
            this.wake_frame_loop();
        });

        let js_func: js_sys::Function =
            closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        *self.raf_function.borrow_mut() = Some(js_func);

        closure
    }

    pub(crate) fn wake_frame_loop(&self) {
        if self.raf_id.get().is_some() {
            return;
        }
        let raf_function = self.raf_function.borrow();
        if let Some(func) = raf_function.as_ref() {
            self.raf_id
                .set(self.browser_window.request_animation_frame(func).ok());
        }
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

    pub(crate) fn register_visibility_change(self: &Rc<Self>) -> Option<EventListenerHandle> {
        let document = self.browser_window.document()?;
        let this = Rc::clone(self);

        Some(EventListenerHandle::add(
            document.as_ref(),
            "visibilitychange",
            move |_event: JsValue| {
                let visibility = document_visibility(&this.browser_window);
                let visibility_changed = {
                    let mut state = this.state.borrow_mut();
                    std::mem::replace(&mut state.visibility, visibility) != visibility
                };
                this.refresh_active_status();
                if visibility_changed {
                    this.with_callback(
                        |callbacks| &mut callbacks.visibility_change,
                        |callback| callback(visibility),
                    );
                }
            },
        ))
    }

    /// Tracks `fullscreenchange` instead of toggling a local flag: the user
    /// can exit fullscreen with Esc, and `requestFullscreen` can be rejected,
    /// so the document is the only reliable source of truth.
    pub(crate) fn register_fullscreen_change(self: &Rc<Self>) -> Option<EventListenerHandle> {
        let document = self.browser_window.document()?;
        let this = Rc::clone(self);

        Some(EventListenerHandle::add(
            document.as_ref(),
            "fullscreenchange",
            move |_event: JsValue| {
                let is_fullscreen = this
                    .browser_window
                    .document()
                    .is_some_and(|document| document.fullscreen_element().is_some());
                this.state.borrow_mut().is_fullscreen = is_fullscreen;
            },
        ))
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

    pub(crate) fn register_appearance_change(self: &Rc<Self>) -> Option<EventListenerHandle> {
        let mql = self
            .browser_window
            .match_media("(prefers-color-scheme: dark)")
            .ok()??;

        let this = Rc::clone(self);
        Some(EventListenerHandle::add(
            mql.as_ref(),
            "change",
            move |_event: JsValue| {
                this.with_callback(
                    |callbacks| &mut callbacks.appearance_changed,
                    |callback| callback(),
                );
            },
        ))
    }
}

impl Drop for WebWindow {
    fn drop(&mut self) {
        self.inner.closed.set(true);
        #[cfg(feature = "diagnostics")]
        self.inner
            .wasm_text_scale_diagnostics
            .publish(&self.inner.canvas, true);
        // Cancel the pending requestAnimationFrame callback before
        // `_raf_closure` is freed, and disconnect the resize observers before
        // their closures are freed; a late invocation of either would throw
        // "closure invoked after being dropped".
        if let Some(raf_id) = self.inner.raf_id.take() {
            self.inner
                .browser_window
                .cancel_animation_frame(raf_id)
                .ok();
        }
        // A frame waker that outlives this window must not re-schedule the
        // freed closure; without a stored function, `wake_frame_loop` no-ops.
        self.inner.raf_function.borrow_mut().take();
        if let Some(observer) = &self._resize_observer {
            observer.disconnect();
        }
        if let Some(observer) = &self._safe_area_observer {
            observer.disconnect();
        }

        // The DPR media-query closure captures an `Rc<WebWindowInner>` and is
        // stored inside the inner itself, forming a reference cycle; take it
        // out so the inner can actually be freed.
        self.inner.mql_handle.borrow_mut().take();

        self.inner.canvas.remove();
        self.inner.ime_mirror.remove();
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

/// The Page Visibility API: `hidden` when the tab is in the background or the
/// browser window is minimized. A missing document reads as visible.
fn document_visibility(browser_window: &web_sys::Window) -> WindowVisibility {
    let is_visible = browser_window
        .document()
        .and_then(|document| js_sys::Reflect::get(&document, &"visibilityState".into()).ok())
        .and_then(|value| value.as_string())
        .is_none_or(|state| state == "visible");

    if is_visible {
        WindowVisibility::Visible
    } else {
        WindowVisibility::Hidden
    }
}

fn document_is_active(browser_window: &web_sys::Window) -> bool {
    document_visibility(browser_window).is_visible()
        && browser_window
            .document()
            .and_then(|document| document.has_focus().ok())
            .unwrap_or(true)
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

    fn set_text_input_configuration(&mut self, configuration: TextInputConfiguration) {
        self.inner.ime_mirror.apply_configuration(&configuration);
    }

    fn show_soft_keyboard(&self) {
        self.inner.show_virtual_keyboard();
    }

    fn hide_soft_keyboard(&self) {
        if !self.inner.touch_input {
            return;
        }
        self.inner.ime_mirror.set_virtual_keyboard_enabled(false);
        // Restart the browser input session without a software keyboard, but
        // keep receiving hardware keys that can move the caret back into an
        // editable region. GPUI focus, not this DOM element, owns their target.
        self.inner.suppress_focus_status_events.set(true);
        self.inner.ime_mirror.blur();
        self.inner.suppress_focus_status_events.set(false);
        // Chrome can coalesce a same-task blur/focus and leave the keyboard
        // visible. Restore the hardware-key receiver after this task instead.
        let callback = Closure::once_into_js({
            let inner = Rc::downgrade(&self.inner);
            move || {
                if let Some(inner) = inner.upgrade()
                    && !inner.ime_mirror.virtual_keyboard_enabled()
                    && inner.state.borrow().is_active
                {
                    inner.suppress_focus_status_events.set(true);
                    inner.ime_mirror.focus();
                    inner.suppress_focus_status_events.set(false);
                }
            }
        });
        if let Err(error) = self
            .inner
            .browser_window
            .set_timeout_with_callback(callback.unchecked_ref())
        {
            log::warn!("failed to restore keyboard event receiver: {error:?}");
        }
    }

    fn visual_viewport_bounds(&self) -> Bounds<Pixels> {
        self.inner.viewport.borrow().visible_bounds
    }

    fn prepare_frame(&self) -> bool {
        self.inner.sample_viewport()
    }

    fn on_visual_viewport_changed(&self, callback: Box<dyn FnMut()>) {
        self.inner.callbacks.borrow_mut().visual_viewport_changed = Some(callback);
    }

    fn insets(&self) -> WindowInsets {
        self.inner.viewport.borrow().insets.clone()
    }

    fn text_input_state_changed(&self, change: TextInputStateChange) {
        // React to editability transitions, not each frame: manually dismissing
        // the keyboard must remain effective until another editing gesture.
        // On desktop, the DOM input remains the keyboard event receiver even
        // when GPUI focuses a read-only surface. Blurring it would also disable
        // hardware shortcuts, unlike a native window.
        if self.inner.touch_input {
            match change {
                TextInputStateChange::FocusGained => self.inner.show_virtual_keyboard(),
                TextInputStateChange::FocusLost => self.hide_soft_keyboard(),
                TextInputStateChange::SelectionChanged | TextInputStateChange::ContentChanged => {}
            }
            return;
        }
        match change {
            TextInputStateChange::FocusGained => {
                self.inner.ime_mirror.set_read_only(false);
                if !self.inner.ime_mirror.is_focused() {
                    self.inner.focus_ime_mirror();
                }
            }
            TextInputStateChange::FocusLost => {
                self.inner.ime_mirror.set_read_only(true);
            }
            TextInputStateChange::SelectionChanged | TextInputStateChange::ContentChanged => {}
        }
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

    fn visibility(&self) -> WindowVisibility {
        self.inner.state.borrow().visibility
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
        let Some(document) = self.inner.browser_window.document() else {
            return;
        };

        // `is_fullscreen` is updated by the `fullscreenchange` listener once
        // the transition actually happens (or not, if the request fails).
        if document.fullscreen_element().is_some() {
            document.exit_fullscreen();
        } else {
            let canvas: &web_sys::Element = self.inner.canvas.as_ref();
            canvas.request_fullscreen().ok();
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.inner.state.borrow().is_fullscreen
    }

    fn frame_waker(&self) -> Option<Rc<dyn Fn()>> {
        // Hold the inner window weakly: the waker is stored in the window's
        // invalidator, and `callbacks.request_frame` captures a clone of that
        // invalidator, so a strong reference here would form a cycle and leak
        // the window on close.
        let inner = Rc::downgrade(&self.inner);
        Some(Rc::new(move || {
            if let Some(inner) = inner.upgrade() {
                inner.wake_frame_loop();
            }
        }))
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

    fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) {
        self.inner.callbacks.borrow_mut().visibility_change = Some(callback);
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

    fn rasterizes_layer_tiles(&self) -> bool {
        // The wgpu renderer does, on WebGPU and on the WebGL2 transport alike.
        true
    }

    fn release_layer_tiles(&self) {
        self.inner.renderer.borrow_mut().release_layer_tiles();
    }

    #[cfg(feature = "diagnostics")]
    fn layer_tile_counters(&self) -> Option<gpux::LayerTileCounters> {
        Some(self.inner.renderer.borrow().layer_tile_counters())
    }

    #[cfg(feature = "diagnostics")]
    fn publish_fast_stats(&self, frame: &[(String, u64)], run: &[(String, u64)]) {
        write_fast_stats_to_global(frame, run, self.layer_tile_counters());
    }

    fn draw(&self, scene: &Scene) {
        if let Some((width, height)) = self.inner.pending_physical_size.take() {
            self.inner.apply_physical_size(width, height);
        }
        #[cfg(feature = "diagnostics")]
        self.inner
            .wasm_text_scale_diagnostics
            .publish(&self.inner.canvas, false);

        let canvas_rect = self.inner.canvas.get_bounding_client_rect();
        self.inner.html_element_registry.borrow_mut().sync_roots(
            &crate::fast::composition::html_elements(self, scene),
            &canvas_rect,
        );
        #[cfg(feature = "diagnostics")]
        write_html_element_diagnostics_to_global(
            &self.inner.html_element_registry.borrow().diagnostics(),
        );

        if !scene.html_islands.is_empty()
            && !self.layout_subtree_enabled.get()
            && enable_layout_subtree(&self.inner.canvas, &self.html_in_canvas_capabilities)
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
                Ok(_result) => {
                    let mut registry = self.inner.html_island_registry.borrow_mut();
                    let uploaded_dirty_root = registry.record_upload_success(&job.id);
                    #[cfg(feature = "diagnostics")]
                    registry.record_upload_diagnostics_success(
                        uploaded_dirty_root,
                        _result.path.as_str(),
                        _result.texture_reallocated,
                    );
                    #[cfg(not(feature = "diagnostics"))]
                    let _ = uploaded_dirty_root;
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
                        let mut registry = self.inner.html_island_registry.borrow_mut();
                        registry.record_upload_failure(&job.id);
                        #[cfg(feature = "diagnostics")]
                        registry.record_upload_diagnostics_failure(error);
                    }
                }
            }
        }
        let _needs_redraw = {
            let mut renderer = self.inner.renderer.borrow_mut();
            renderer.draw(scene);
            renderer.needs_redraw()
        };
        #[cfg(feature = "diagnostics")]
        write_wgpu_frame_telemetry_to_global(&self.inner.renderer.borrow());
        #[cfg(feature = "diagnostics")]
        write_text_renderer_diagnostics_to_global(&self.inner.renderer.borrow(), scene);
        #[cfg(feature = "diagnostics")]
        let (texture_count, texture_pruned_count, texture_draw_count) = self
            .inner
            .renderer
            .borrow()
            .html_island_texture_diagnostics();
        #[cfg(feature = "diagnostics")]
        self.inner
            .html_island_registry
            .borrow_mut()
            .record_texture_cache_state(texture_count, texture_pruned_count, texture_draw_count);
        #[cfg(feature = "diagnostics")]
        write_html_in_canvas_diagnostics_to_global(
            &self.html_in_canvas_capabilities.diagnostics(),
            &self.inner.canvas,
            layout_subtree_enabled,
            &self._html_in_canvas_paint_observer.snapshot(),
            Some(&self.inner.html_island_registry.borrow().diagnostics()),
        );
    }

    fn draw_composed(&self, scene: gpux::ComposedScene<'_>) {
        crate::fast::composition::draw_composed(self, scene)
    }

    fn enable_window_composition(&self) -> anyhow::Result<()> {
        crate::fast::composition::enable_window_composition(self)
    }

    fn set_composition_order(
        &self,
        fast_surfaces: &[gpux::PlatformCompositionSurface],
    ) -> anyhow::Result<()> {
        crate::fast::composition::set_composition_order(self, fast_surfaces)
    }

    fn effect_capabilities(&self) -> EffectCapabilities {
        self.inner.renderer.borrow().effect_capabilities()
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

/// Publishes the fast-mode counters of the last frame and of the run as `__gpuxFastStats`:
/// `{ frame: { <counter>: n, .. }, run: { <counter>: n, .. }, tiles: { held, rasterized,
/// released, budget } }`, the last what the renderer did with scroll-layer tiles and the bytes
/// of tiles a window may keep.
#[cfg(feature = "diagnostics")]
fn write_fast_stats_to_global(
    frame: &[(String, u64)],
    run: &[(String, u64)],
    tiles: Option<gpux::LayerTileCounters>,
) {
    let object = |counters: &[(String, u64)]| {
        let object = js_sys::Object::new();
        for (name, value) in counters {
            let _ = js_sys::Reflect::set(&object, &name.as_str().into(), &(*value as f64).into());
        }
        object
    };
    let target = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&target, &"frame".into(), &object(frame));
    let _ = js_sys::Reflect::set(&target, &"run".into(), &object(run));
    if let Some(tiles) = tiles {
        let tiles = object(&[
            ("held".to_string(), tiles.held),
            ("rasterized".to_string(), tiles.rasterized),
            ("released".to_string(), tiles.released),
            ("budget".to_string(), gpux::LAYER_TILE_BUDGET_BYTES),
        ]);
        let _ = js_sys::Reflect::set(&target, &"tiles".into(), &tiles);
    }
    gpui_wgpu::web_globals::publish("FastStats", &target);
}

#[cfg(feature = "diagnostics")]
fn write_wgpu_frame_telemetry_to_global(renderer: &WgpuRenderer) {
    let telemetry = renderer.frame_telemetry();
    let target = js_sys::Object::new();
    let set_number = |key: &str, value: u64| {
        let _ = js_sys::Reflect::set(&target, &key.into(), &(value as f64).into());
    };

    let _ = js_sys::Reflect::set(
        &target,
        &"diagnosticVersion".into(),
        &"wgpu-frame/v1".into(),
    );
    set_number("schemaVersion", telemetry.schema_version as u64);
    set_number("frameIndex", telemetry.frame_index);
    set_number("quadCount", telemetry.quad_count);
    set_number("shadowCount", telemetry.shadow_count);
    set_number("visualEffectCount", telemetry.visual_effect_count);
    set_number("htmlIslandCount", telemetry.html_island_count);
    set_number("pathCount", telemetry.path_count);
    set_number("underlineCount", telemetry.underline_count);
    set_number("monochromeSpriteCount", telemetry.monochrome_sprite_count);
    set_number("subpixelSpriteCount", telemetry.subpixel_sprite_count);
    set_number("polychromeSpriteCount", telemetry.polychrome_sprite_count);
    set_number(
        "monochromeSpriteSortDisorderCount",
        telemetry.monochrome_sprite_sort_disorder_count,
    );
    set_number(
        "subpixelSpriteSortDisorderCount",
        telemetry.subpixel_sprite_sort_disorder_count,
    );
    set_number(
        "polychromeSpriteSortDisorderCount",
        telemetry.polychrome_sprite_sort_disorder_count,
    );
    set_number(
        "boundsTreeInsertionCount",
        telemetry.bounds_tree_insertion_count,
    );
    set_number(
        "boundsTreeMaxLeafHitCount",
        telemetry.bounds_tree_max_leaf_hit_count,
    );
    set_number(
        "boundsTreeSlowSearchCount",
        telemetry.bounds_tree_slow_search_count,
    );
    set_number(
        "boundsTreeSearchNodeVisitCount",
        telemetry.bounds_tree_search_node_visit_count,
    );
    set_number(
        "boundsTreeInsertionChildEvaluationCount",
        telemetry.bounds_tree_insertion_child_evaluation_count,
    );
    set_number(
        "boundsTreeRetainedNodeCount",
        telemetry.bounds_tree_retained_node_count,
    );
    set_number(
        "globalElementIdCreationCount",
        telemetry.global_element_id_creation_count,
    );
    set_number(
        "globalElementIdPathElementCount",
        telemetry.global_element_id_path_element_count,
    );
    set_number(
        "globalElementIdMaxPathLength",
        telemetry.global_element_id_max_path_length,
    );
    set_number(
        "globalElementIdPathAtMost4Count",
        telemetry.global_element_id_path_at_most_4_count,
    );
    set_number(
        "globalElementIdPathAtMost8Count",
        telemetry.global_element_id_path_at_most_8_count,
    );
    set_number(
        "globalElementIdPathAtMost16Count",
        telemetry.global_element_id_path_at_most_16_count,
    );
    set_number(
        "globalElementIdSameFrameDuplicateCount",
        telemetry.global_element_id_same_frame_duplicate_count,
    );
    set_number(
        "globalElementIdPreviousFrameHitCount",
        telemetry.global_element_id_previous_frame_hit_count,
    );
    set_number("taffyNodeCount", telemetry.taffy_node_count);
    set_number(
        "taffyMeasuredLeafCount",
        telemetry.taffy_measured_leaf_count,
    );
    set_number("taffyLeafCount", telemetry.taffy_leaf_count);
    set_number("taffyContainerCount", telemetry.taffy_container_count);
    set_number("taffyChildEdgeCount", telemetry.taffy_child_edge_count);
    set_number("taffyMaxChildCount", telemetry.taffy_max_child_count);
    set_number(
        "taffyContainerAtMost1Count",
        telemetry.taffy_container_at_most_1_count,
    );
    set_number(
        "taffyContainerAtMost2Count",
        telemetry.taffy_container_at_most_2_count,
    );
    set_number(
        "taffyContainerAtMost4Count",
        telemetry.taffy_container_at_most_4_count,
    );
    set_number(
        "taffyContainerAtMost8Count",
        telemetry.taffy_container_at_most_8_count,
    );
    set_number(
        "taffyFlexContainerCount",
        telemetry.taffy_flex_container_count,
    );
    set_number(
        "taffyBlockContainerCount",
        telemetry.taffy_block_container_count,
    );
    set_number(
        "taffyGridContainerCount",
        telemetry.taffy_grid_container_count,
    );
    set_number(
        "taffyComputeLayoutCount",
        telemetry.taffy_compute_layout_count,
    );
    set_number(
        "taffyRepeatedComputeLayoutCount",
        telemetry.taffy_repeated_compute_layout_count,
    );
    set_number(
        "taffyInvalidatedDescendantCount",
        telemetry.taffy_invalidated_descendant_count,
    );
    set_number(
        "taffyLayoutBoundsCallCount",
        telemetry.taffy_layout_bounds_call_count,
    );
    set_number(
        "taffyLayoutBoundsCacheHitCount",
        telemetry.taffy_layout_bounds_cache_hit_count,
    );
    set_number(
        "taffyParentOriginCacheHitCount",
        telemetry.taffy_parent_origin_cache_hit_count,
    );
    set_number(
        "taffyParentOriginCacheMissCount",
        telemetry.taffy_parent_origin_cache_miss_count,
    );
    set_number(
        "taffyMeasurementCallCount",
        telemetry.taffy_measurement_call_count,
    );
    set_number(
        "taffyPreviousMeasurementCallCount",
        telemetry.taffy_previous_measurement_call_count,
    );
    set_number(
        "taffySameInputMeasurementCount",
        telemetry.taffy_same_input_measurement_count,
    );
    set_number(
        "taffySameInputSameOutputMeasurementCount",
        telemetry.taffy_same_input_same_output_measurement_count,
    );
    set_number(
        "taffySameInputChangedOutputMeasurementCount",
        telemetry.taffy_same_input_changed_output_measurement_count,
    );
    set_number(
        "taffyCustomMeasurementCallCount",
        telemetry.taffy_custom_measurement_call_count,
    );
    set_number(
        "taffyTextMeasurementCallCount",
        telemetry.taffy_text_measurement_call_count,
    );
    set_number(
        "taffyListMeasurementCallCount",
        telemetry.taffy_list_measurement_call_count,
    );
    set_number(
        "taffyUniformListMeasurementCallCount",
        telemetry.taffy_uniform_list_measurement_call_count,
    );
    set_number(
        "taffyTextLocalMeasurementReuseCount",
        telemetry.taffy_text_local_measurement_reuse_count,
    );
    set_number(
        "taffyTextFullMeasurementLayoutCount",
        telemetry.taffy_text_full_measurement_layout_count,
    );
    set_number("taffyNodeRequestCount", telemetry.taffy_node_request_count);
    set_number(
        "taffyPooledNodeReuseCount",
        telemetry.taffy_pooled_node_reuse_count,
    );
    set_number("taffyNewNodeCount", telemetry.taffy_new_node_count);
    set_number(
        "taffyUnchangedStyleCount",
        telemetry.taffy_unchanged_style_count,
    );
    set_number("taffyStyleWriteCount", telemetry.taffy_style_write_count);
    set_number(
        "taffyUnchangedTopologyCount",
        telemetry.taffy_unchanged_topology_count,
    );
    set_number(
        "taffyTopologyWriteCount",
        telemetry.taffy_topology_write_count,
    );
    set_number(
        "taffyMeasuredContextRefillCount",
        telemetry.taffy_measured_context_refill_count,
    );
    set_number(
        "taffyMeasuredContextInstallCount",
        telemetry.taffy_measured_context_install_count,
    );
    set_number(
        "taffyMeasuredContextDirtyCount",
        telemetry.taffy_measured_context_dirty_count,
    );
    set_number(
        "taffyUniqueChildInsertionCount",
        telemetry.taffy_unique_child_insertion_count,
    );
    set_number(
        "textMeasurementMissingCacheCount",
        telemetry.text_measurement_missing_cache_count,
    );
    set_number(
        "textMeasurementWrapMismatchCount",
        telemetry.text_measurement_wrap_mismatch_count,
    );
    set_number(
        "textMeasurementTruncationRequestCount",
        telemetry.text_measurement_truncation_request_count,
    );
    set_number(
        "textMeasurementPreviousTruncationCount",
        telemetry.text_measurement_previous_truncation_count,
    );
    set_number("textShapeCallCount", telemetry.text_shape_call_count);
    set_number("textShapeLineCount", telemetry.text_shape_line_count);
    set_number(
        "textShapeDecorationRunCount",
        telemetry.text_shape_decoration_run_count,
    );
    set_number(
        "textShapeMaxDecorationRunCount",
        telemetry.text_shape_max_decoration_run_count,
    );
    set_number(
        "textShapeAtMostOneDecorationRunLineCount",
        telemetry.text_shape_at_most_one_decoration_run_line_count,
    );
    set_number(
        "textShapeDecorationRunCapacity",
        telemetry.text_shape_decoration_run_capacity,
    );
    set_number(
        "textShapeFontRunPoolTakeCount",
        telemetry.text_shape_font_run_pool_take_count,
    );
    set_number(
        "textShapeFontRunPoolHitCount",
        telemetry.text_shape_font_run_pool_hit_count,
    );
    set_number(
        "textShapeFontRunPoolMissCount",
        telemetry.text_shape_font_run_pool_miss_count,
    );
    set_number(
        "textShapeFontRunPoolReturnCount",
        telemetry.text_shape_font_run_pool_return_count,
    );
    set_number(
        "textWrappedLayoutRequestCount",
        telemetry.text_wrapped_layout_request_count,
    );
    set_number(
        "textWrappedLayoutCurrentFrameHitCount",
        telemetry.text_wrapped_layout_current_frame_hit_count,
    );
    set_number(
        "textWrappedLayoutPreviousFrameHitCount",
        telemetry.text_wrapped_layout_previous_frame_hit_count,
    );
    set_number(
        "textWrappedLayoutMissCount",
        telemetry.text_wrapped_layout_miss_count,
    );
    set_number(
        "textLineLayoutRequestCount",
        telemetry.text_line_layout_request_count,
    );
    set_number(
        "textLineLayoutCurrentFrameHitCount",
        telemetry.text_line_layout_current_frame_hit_count,
    );
    set_number(
        "textLineLayoutPreviousFrameHitCount",
        telemetry.text_line_layout_previous_frame_hit_count,
    );
    set_number(
        "textLineLayoutMissCount",
        telemetry.text_line_layout_miss_count,
    );
    set_number(
        "textLayoutCacheKeyHashCount",
        telemetry.text_layout_cache_key_hash_count,
    );
    set_number(
        "textLayoutCacheKeyTextBytesHashed",
        telemetry.text_layout_cache_key_text_bytes_hashed,
    );
    set_number(
        "textLayoutCacheKeyRunItemsHashed",
        telemetry.text_layout_cache_key_run_items_hashed,
    );
    set_number(
        "textLayoutCacheKeyEqualityCount",
        telemetry.text_layout_cache_key_equality_count,
    );
    set_number(
        "styleRequestLayoutCount",
        telemetry.style_request_layout_count,
    );
    set_number(
        "stylePrepaintInitialCount",
        telemetry.style_prepaint_initial_count,
    );
    set_number(
        "stylePrepaintRecomputeCount",
        telemetry.style_prepaint_recompute_count,
    );
    set_number("stylePaintCount", telemetry.style_paint_count);
    set_number(
        "stylePaintStaticEligibleCount",
        telemetry.style_paint_static_eligible_count,
    );
    set_number(
        "stylePaintReuseEligibleCount",
        telemetry.style_paint_reuse_eligible_count,
    );
    set_number(
        "styleRequestStaticEligibleCount",
        telemetry.style_request_static_eligible_count,
    );
    set_number(
        "stylePrepaintRequestCandidateCount",
        telemetry.style_prepaint_request_candidate_count,
    );
    set_number(
        "styleRequestGpuxOwnedStaticEligibleCount",
        telemetry.style_request_gpux_owned_static_eligible_count,
    );
    set_number(
        "styleRequestExternalStaticEligibleCount",
        telemetry.style_request_external_static_eligible_count,
    );
    set_number(
        "stylePrepaintGpuxOwnedRequestCandidateCount",
        telemetry.style_prepaint_gpux_owned_request_candidate_count,
    );
    set_number(
        "stylePrepaintExternalRequestCandidateCount",
        telemetry.style_prepaint_external_request_candidate_count,
    );
    set_number(
        "stylePrepaintRequestReuseCount",
        telemetry.style_prepaint_request_reuse_count,
    );
    set_number(
        "styleRequestDefaultBaseCount",
        telemetry.style_request_default_base_count,
    );
    set_number(
        "activeScreenEffectCount",
        telemetry.active_screen_effect_count,
    );
    set_number("batchCount", telemetry.batch_count);
    set_number("instanceBytesWritten", telemetry.instance_bytes_written);
    set_number("auxiliaryBytesWritten", telemetry.auxiliary_bytes_written);
    set_number("queueBufferWriteCount", telemetry.queue_buffer_write_count);
    set_number("queueBufferWriteBytes", telemetry.queue_buffer_write_bytes);
    set_number(
        "atlasTextureWriteCount",
        telemetry.atlas_texture_write_count,
    );
    set_number(
        "atlasTextureWriteBytes",
        telemetry.atlas_texture_write_bytes,
    );
    set_number("bindGroupCount", telemetry.bind_group_count);
    set_number("renderPassCount", telemetry.render_pass_count);
    set_number("bufferRestartCount", telemetry.buffer_restart_count);
    set_number("submissionCount", telemetry.submission_count);
    set_number("publishedAtMs", js_sys::Date::now() as u64);
    gpui_wgpu::web_globals::publish("WgpuFrameDiagnostics", &target);
}

#[cfg(feature = "diagnostics")]
fn text_rendering_diagnostics_enabled() -> bool {
    gpui_wgpu::web_globals::flag("TextRenderingDiagnosticsEnabled")
}

#[cfg(feature = "diagnostics")]
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
    gpui_wgpu::web_globals::publish("TextRendererDiagnostics", target.as_ref());
}
