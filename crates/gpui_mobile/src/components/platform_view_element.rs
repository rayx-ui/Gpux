//! GPUI element for embedding native platform views.
//!
//! `PlatformViewElement` places a native view (Android `View` or iOS
//! `UIView`) through window composition: the first time it is laid out in a
//! window, it enables the window's composition, makes a native surface for
//! the view and attaches the view to it ([`PlatformView::attach`]); then it
//! places that surface as [`gpui::native_surface`] does, at the part of its
//! bounds inside the bounds of its content mask: the view keeps its full
//! size, offset inside the surface, so a scroll area clips it. GPUI's base
//! content draws below the view and its overlay content above it; the
//! surface is hidden in frames that do not draw the element, or that clip all
//! of it, and leaves the composition when the view is disposed.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use gpui_mobile::components::platform_view_element::platform_view_element;
//! use gpui_mobile::platform_view::PlatformViewHandle;
//! use gpui::{IntoElement, ParentElement, Styled, div};
//! use std::sync::Arc;
//!
//! fn my_component(handle: Arc<PlatformViewHandle>) -> impl IntoElement {
//!     div()
//!         .flex()
//!         .child(platform_view_element(handle).size_full())
//! }
//! ```
//!
//! [`PlatformView::attach`]: crate::platform_view::PlatformView::attach

use crate::platform_view::{PlatformViewBounds, PlatformViewHandle, PlatformViewId};
use futures::channel::oneshot;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, ParentElement, Pixels, Refineable, Style,
    StyleRefinement, Styled, Window, WindowCompositionSurface, WindowId, div, native_surface, px,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A platform view's native surface, and the frame last given to the view
/// inside it with the scale factor it was given at.
struct Placed {
    surface: WindowCompositionSurface,
    frame: Option<([f32; 4], f32)>,
}

thread_local! {
    /// Each placed platform view's native surface, per window.
    static SURFACES: RefCell<HashMap<(WindowId, PlatformViewId), Placed>> =
        RefCell::new(HashMap::new());
}

/// What tells each placed view's watcher that the view was disposed; a
/// handle may be disposed on any thread.
static DISPOSED: Mutex<Option<HashMap<PlatformViewId, Vec<oneshot::Sender<()>>>>> =
    Mutex::new(None);

/// Called as a platform view is disposed: its native surfaces leave their
/// windows' compositions, on GPUI's thread.
pub(crate) fn forget(id: PlatformViewId) {
    let watchers = DISPOSED
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|watchers| watchers.remove(&id));
    for watcher in watchers.into_iter().flatten() {
        watcher.send(()).ok();
    }
}

/// Removes `handle`'s surface from the composition of the window it was
/// placed in once the view is disposed, whether or not a platform view
/// element is ever laid out again.
fn remove_when_disposed(
    handle: &PlatformViewHandle,
    key: (WindowId, PlatformViewId),
    window: &Window,
    cx: &mut App,
) {
    let (disposed, on_disposed) = oneshot::channel();
    DISPOSED
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .entry(key.1)
        .or_default()
        .push(disposed);
    let window_handle = window.window_handle();
    cx.spawn(async move |cx| {
        if on_disposed.await.is_err() {
            return;
        }
        let Some(placed) = SURFACES.with_borrow_mut(|surfaces| surfaces.remove(&key)) else {
            return;
        };
        window_handle
            .update(cx, |_, window, _| {
                if let Ok(composition) = window.enable_window_composition() {
                    composition.remove_surface(placed.surface.id()).ok();
                }
            })
            .ok();
    })
    .detach();
    // Disposed on another thread while this was set up.
    if handle.inner().is_disposed() {
        forget(key.1);
    }
}

/// The native surface `handle`'s view is attached to in `window`, made the
/// first time.
fn surface(
    handle: &PlatformViewHandle,
    window: &Window,
    cx: &mut App,
) -> Result<WindowCompositionSurface, String> {
    let key = (window.window_handle().window_id(), handle.id());
    if let Some(surface) =
        SURFACES.with_borrow(|surfaces| surfaces.get(&key).map(|placed| placed.surface.clone()))
    {
        return Ok(surface);
    }
    let composition = window
        .enable_window_composition()
        .map_err(|error| format!("{error:#}"))?;
    let surface = composition
        .create_native_surface()
        .map_err(|error| format!("{error:#}"))?;
    let attached = surface
        .platform_surface()
        .and_then(|platform_surface| platform_surface.platform_handle())
        .map_err(|error| format!("{error:#}"))
        .and_then(|container| handle.inner().attach(container.as_ref()));
    if let Err(error) = attached {
        composition.remove_surface(surface.id()).ok();
        return Err(error);
    }
    SURFACES.with_borrow_mut(|surfaces| {
        surfaces.insert(
            key,
            Placed {
                surface: surface.clone(),
                frame: None,
            },
        )
    });
    remove_when_disposed(handle, key, window, cx);
    Ok(surface)
}

/// Create a GPUI element that hosts a native platform view.
///
/// The returned `Div` reserves layout space and places the view's native
/// surface there on every frame that draws it. Style it with `.w()`, `.h()`,
/// `.size()`, `.flex_grow()`, etc. to control how much space it occupies.
pub fn platform_view_element(handle: Arc<PlatformViewHandle>) -> gpui::Div {
    div().child(
        PlatformViewElement {
            handle,
            style: StyleRefinement::default(),
        }
        .size_full(),
    )
}

/// The element a platform view is placed at, filling the `Div` that
/// [`platform_view_element`] returns.
struct PlatformViewElement {
    handle: Arc<PlatformViewHandle>,
    style: StyleRefinement,
}

impl Styled for PlatformViewElement {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for PlatformViewElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl PlatformViewElement {
    /// Gives the view its `frame` inside its surface, when it changed.
    fn set_frame(&self, window: &Window, frame: PlatformViewBounds) {
        let key = (window.window_handle().window_id(), self.handle.id());
        let scale_factor = window.scale_factor();
        let value = ([frame.x, frame.y, frame.width, frame.height], scale_factor);
        let changed = SURFACES.with_borrow_mut(|surfaces| {
            surfaces.get_mut(&key).is_some_and(|placed| {
                let changed = placed.frame != Some(value);
                placed.frame = Some(value);
                changed
            })
        });
        if changed {
            self.handle.set_frame(frame, scale_factor);
        }
    }
}

impl Element for PlatformViewElement {
    type RequestLayoutState = ();
    /// The native surface's element, placed at the visible part of the
    /// bounds; none when the view is clipped away or could not be placed.
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.refine(&self.style);
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let visible = bounds.intersect(&window.content_mask().bounds);
        if self.handle.inner().is_disposed()
            || visible.size.width <= px(0.)
            || visible.size.height <= px(0.)
        {
            return None;
        }
        let surface = match surface(&self.handle, window, cx) {
            Ok(surface) => surface,
            Err(error) => {
                log::error!("placing {}: {error}", self.handle.id());
                return None;
            }
        };
        // The registry tracks where the view shows, for hit testing.
        self.handle.set_bounds(PlatformViewBounds {
            x: visible.origin.x.as_f32(),
            y: visible.origin.y.as_f32(),
            width: visible.size.width.as_f32(),
            height: visible.size.height.as_f32(),
        });
        self.set_frame(
            window,
            PlatformViewBounds {
                x: (bounds.origin.x - visible.origin.x).as_f32(),
                y: (bounds.origin.y - visible.origin.y).as_f32(),
                width: bounds.size.width.as_f32(),
                height: bounds.size.height.as_f32(),
            },
        );
        let mut child = native_surface(surface)
            .w(visible.size.width)
            .h(visible.size.height)
            .into_any_element();
        child.prepaint_as_root(
            visible.origin,
            visible.size.map(AvailableSpace::Definite),
            window,
            cx,
        );
        Some(child)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        child: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(child) = child {
            child.paint(window, cx);
        }
    }
}

/// A higher-level wrapper that creates the platform view from the registry
/// and manages its lifecycle.
///
/// The view is created on first render and disposed when dropped.
pub struct ManagedPlatformView {
    handle: Option<Arc<PlatformViewHandle>>,
    view_type: String,
    creation_params: std::collections::HashMap<String, String>,
}

impl ManagedPlatformView {
    /// Create a new managed platform view for the given type.
    pub fn new(view_type: impl Into<String>) -> Self {
        Self {
            handle: None,
            view_type: view_type.into(),
            creation_params: std::collections::HashMap::new(),
        }
    }

    /// Add a creation parameter.
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.creation_params.insert(key.into(), value.into());
        self
    }

    /// Get or create the platform view handle.
    pub fn ensure_created(&mut self) -> Result<Arc<PlatformViewHandle>, String> {
        if let Some(handle) = &self.handle {
            return Ok(handle.clone());
        }

        let registry = crate::platform_view::PlatformViewRegistry::global();
        let params = crate::platform_view::PlatformViewParams {
            bounds: PlatformViewBounds::default(),
            creation_params: self.creation_params.clone(),
        };

        let handle = Arc::new(registry.create_view(&self.view_type, params)?);
        self.handle = Some(handle.clone());
        Ok(handle)
    }

    /// Get the handle if the view has been created.
    pub fn handle(&self) -> Option<&Arc<PlatformViewHandle>> {
        self.handle.as_ref()
    }

    /// Render the platform view element.
    ///
    /// Returns a div with an error message if the view fails to create.
    pub fn render(&mut self) -> gpui::Div {
        match self.ensure_created() {
            Ok(handle) => platform_view_element(handle),
            Err(e) => {
                log::error!("Failed to create platform view '{}': {}", self.view_type, e);
                div()
            }
        }
    }
}

impl Drop for ManagedPlatformView {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.dispose();
        }
    }
}
