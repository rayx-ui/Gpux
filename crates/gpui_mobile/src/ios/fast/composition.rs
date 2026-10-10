//! Window composition on iOS (zed-industries/zed#62379): native views
//! between GPUI's base content and its overlays.
//!
//! gpux's own; the shape follows the macOS backend. The window's
//! `GPUIMetalView` draws the base surface. Native surfaces are `UIView`
//! containers (clipping their content, handed out as a `UIView*`) placed as
//! subviews of the Metal view; every further GPUI surface is a
//! `GPUIOverlayView`, a transparent `CAMetalLayer` view drawn by a renderer
//! sharing the window renderer's Metal context, atlas and pipelines. An
//! overlay's `hitTest:withEvent:` takes a touch only within its input region
//! (what it drew last frame), so elsewhere touches reach the native views and
//! the Metal view below; the touches it takes go into the window's touch path
//! as raw `PlatformInput::Touch`, like the Metal view's.
//!
//! Going to the background drops the overlays' surfaces; they are made again
//! for the first composed frame after. Geometry follows rotation and safe-area
//! layout through the overlays' autoresizing and each frame's drawable size.
//! The window releases its overlay and native views before it is freed.

use std::{
    any::Any,
    cell::Cell,
    collections::HashMap,
    ffi::c_void,
    ptr,
    rc::{Rc, Weak},
    sync::{Arc, Once},
};

use anyhow::{Context as _, Result};
use gpui::{
    Bounds, ComposedScene, CompositionSurfaceId, DevicePixels, Pixels, PlatformCompositionSurface,
    PlatformCompositionSurfaceContent, PlatformSurfaceAttachment, PlatformWindow as _, Point,
    Scene, WindowVisibility, point, px, size,
};
use gpui_wgpu::{WgpuRenderer, WgpuSurfaceConfig, wgpu};
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{class, msg_send, sel};
use parking_lot::Mutex;
use raw_window_handle::{HasDisplayHandle as _, HasWindowHandle as _};

use crate::ios::cg_types::{ObjcCGPoint, ObjcCGRect};
use crate::ios::window::{GPUI_WINDOW_IVAR, IosWindow, RawIosWindow, handle_touches};

/// The ivar of a `GPUIOverlayView` holding its [`InputRegion`] as a raw
/// `Arc` pointer.
const INPUT_REGION_IVAR: &str = "gpux_input_region";

/// Where an overlay takes touches, in points from the window's top left.
type InputRegion = Mutex<Vec<Bounds<Pixels>>>;

static OVERLAY_VIEW_CLASS: Once = Once::new();

/// Registers `GPUIOverlayView`: a `CAMetalLayer` view whose hit testing is
/// gated on its input region and whose touches go to its window.
fn overlay_view_class() -> &'static AnyClass {
    OVERLAY_VIEW_CLASS.call_once(|| {
        let mut decl = ClassBuilder::new(c"GPUIOverlayView", class!(UIView))
            .expect("GPUIOverlayView is registered once");
        decl.add_ivar::<*mut c_void>(c"gpui_window_ptr");
        decl.add_ivar::<*mut c_void>(c"gpux_input_region");

        extern "C" fn layer_class(_: *const AnyClass, _: Sel) -> *const AnyClass {
            class!(CAMetalLayer) as *const AnyClass
        }

        extern "C" fn hit_test(
            this: *mut AnyObject,
            _: Sel,
            location: ObjcCGPoint,
            _event: *mut AnyObject,
        ) -> *mut AnyObject {
            let region = unsafe {
                #[allow(deprecated)]
                let raw: *mut c_void = *(*this).get_ivar(INPUT_REGION_IVAR);
                if raw.is_null() {
                    return ptr::null_mut();
                }
                &*(raw as *const InputRegion)
            };
            let position = point(px(location.x as f32), px(location.y as f32));
            if region
                .lock()
                .iter()
                .any(|bounds| bounds.contains(&position))
            {
                this
            } else {
                ptr::null_mut()
            }
        }

        extern "C" fn touches(
            this: *mut AnyObject,
            _: Sel,
            touches: *mut AnyObject,
            event: *mut AnyObject,
        ) {
            handle_touches(this, touches, event);
        }

        unsafe {
            decl.add_class_method(
                sel!(layerClass),
                layer_class as extern "C" fn(*const AnyClass, Sel) -> *const AnyClass,
            );
            decl.add_method(
                sel!(hitTest:withEvent:),
                hit_test
                    as extern "C" fn(
                        *mut AnyObject,
                        Sel,
                        ObjcCGPoint,
                        *mut AnyObject,
                    ) -> *mut AnyObject,
            );
            for selector in [
                sel!(touchesBegan:withEvent:),
                sel!(touchesMoved:withEvent:),
                sel!(touchesEnded:withEvent:),
                sel!(touchesCancelled:withEvent:),
            ] {
                decl.add_method(
                    selector,
                    touches as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
                );
            }
        }
        decl.register();
    });
    class!(GPUIOverlayView)
}

/// A window's composition state, held by `IosWindow`.
#[derive(Default)]
pub(crate) struct Composition {
    base: Option<CompositionSurfaceId>,
    overlays: HashMap<CompositionSurfaceId, Overlay>,
    native_surfaces: Vec<Weak<NativeState>>,
    /// Views in the order last mounted, with their superviews.
    mounted: Vec<(usize, usize)>,
}

struct Overlay {
    view: *mut AnyObject,
    /// Dropped in the background and made again on the next composed frame.
    renderer: Option<WgpuRenderer>,
    region: Arc<InputRegion>,
}

impl Overlay {
    fn new(window: &IosWindow) -> Result<Self> {
        let region = Arc::new(InputRegion::default());
        unsafe {
            let class = overlay_view_class();
            let bounds: ObjcCGRect = msg_send![window.view, bounds];
            let view: *mut AnyObject = msg_send![class, alloc];
            let view: *mut AnyObject = msg_send![view, initWithFrame: bounds];
            anyhow::ensure!(!view.is_null(), "creating a GPUIOverlayView failed");
            #[allow(deprecated)]
            {
                *(*view).get_mut_ivar::<*mut c_void>(GPUI_WINDOW_IVAR) =
                    window as *const IosWindow as *mut c_void;
                *(*view).get_mut_ivar::<*mut c_void>(INPUT_REGION_IVAR) =
                    Arc::into_raw(region.clone()) as *mut c_void;
            }
            let _: () = msg_send![view, setOpaque: false];
            let clear: *mut AnyObject = msg_send![class!(UIColor), clearColor];
            let _: () = msg_send![view, setBackgroundColor: clear];
            let _: () = msg_send![view, setMultipleTouchEnabled: true];
            // Flexible width and height: the overlay follows the Metal view.
            let _: () = msg_send![view, setAutoresizingMask: 18_usize];
            let layer: *mut AnyObject = msg_send![view, layer];
            let _: () = msg_send![layer, setOpaque: false];
            let _: () = msg_send![layer, setContentsScale: window.scale_factor.get() as f64];
            Ok(Self {
                view,
                renderer: None,
                region,
            })
        }
    }

    /// The overlay's renderer, made on the window renderer's context.
    fn renderer(&mut self, window_renderer: &WgpuRenderer) -> Result<&mut WgpuRenderer> {
        if self.renderer.is_none() {
            // The window's Metal instance has no display handle of its own:
            // name UIKit's with the view, as the window's renderer does.
            let raw = RawIosWindow {
                view: self.view as *mut c_void,
            };
            let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw.display_handle()?.as_raw()),
                raw_window_handle: raw.window_handle()?.as_raw(),
            };
            // SAFETY: the view outlives the renderer, which `release` drops
            // first.
            let renderer = unsafe {
                window_renderer.new_sharing_target(
                    target,
                    WgpuSurfaceConfig {
                        size: window_renderer.viewport_size(),
                        transparent: true,
                        preferred_present_mode: None,
                    },
                    true,
                )
            }?;
            self.renderer = Some(renderer);
        }
        self.renderer
            .as_mut()
            .context("the overlay has no renderer")
    }

    /// Releases the view: no touch reaches the window through it afterwards.
    fn release(&mut self) {
        // The surface draws into the view's layer: drop it first.
        self.renderer = None;
        unsafe {
            #[allow(deprecated)]
            {
                *(*self.view).get_mut_ivar::<*mut c_void>(GPUI_WINDOW_IVAR) = ptr::null_mut();
                let raw = std::mem::replace(
                    (*self.view).get_mut_ivar::<*mut c_void>(INPUT_REGION_IVAR),
                    ptr::null_mut(),
                );
                if !raw.is_null() {
                    drop(Arc::from_raw(raw as *const InputRegion));
                }
            }
            let _: () = msg_send![self.view, removeFromSuperview];
            let _: () = msg_send![self.view, release];
        }
    }
}

/// A native surface: a `UIView` container clipping its content.
struct NativeState {
    view: *mut AnyObject,
    bounds: Cell<Bounds<DevicePixels>>,
    parent_origin: Cell<Point<DevicePixels>>,
    scale: Cell<f32>,
}

impl NativeState {
    fn place(&self) {
        let bounds = self.bounds.get();
        let parent_origin = self.parent_origin.get();
        let scale = self.scale.get() as f64;
        let frame = ObjcCGRect {
            x: (bounds.origin.x.0 - parent_origin.x.0) as f64 / scale,
            y: (bounds.origin.y.0 - parent_origin.y.0) as f64 / scale,
            width: bounds.size.width.0.max(0) as f64 / scale,
            height: bounds.size.height.0.max(0) as f64 / scale,
        };
        unsafe {
            let _: () = msg_send![self.view, setFrame: frame];
        }
    }
}

impl Drop for NativeState {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.view, removeFromSuperview];
            let _: () = msg_send![self.view, release];
        }
    }
}

/// The attachment GPUI holds for a native surface. Its `platform_handle` is
/// the container `UIView*` as a `usize`; add native views to it.
struct NativeSurface {
    state: Rc<NativeState>,
}

impl PlatformSurfaceAttachment for NativeSurface {
    fn set_bounds(&self, bounds: Bounds<DevicePixels>) -> Result<()> {
        self.state.bounds.set(bounds);
        self.state.place();
        Ok(())
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        self.state.bounds.get()
    }

    fn set_parent_origin(&self, origin: Point<DevicePixels>) -> Result<()> {
        self.state.parent_origin.set(origin);
        self.state.place();
        Ok(())
    }

    fn set_visible(&self, visible: bool) -> Result<()> {
        unsafe {
            let _: () = msg_send![self.state.view, setHidden: !visible];
        }
        Ok(())
    }

    fn platform_handle(&self) -> Result<Box<dyn Any>> {
        Ok(Box::new(self.state.view as usize))
    }
}

/// `PlatformWindow::draw_composed`: the base on the Metal view, each overlay
/// on its own view, its input region set from what it drew.
pub(crate) fn draw_composed(window: &IosWindow, scene: ComposedScene<'_>) {
    let Some(base) = window.fast_composition.borrow().base else {
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
                log::error!("replaying iOS composition scene: {error:#}");
                return;
            }
        }
    }
    window.draw(&base_scene);
    if !window.visible.get() {
        return;
    }

    let guard = window.renderer.lock();
    let Some(window_renderer) = guard.as_ref() else {
        return;
    };
    let size = window_renderer.viewport_size();
    let scale = window.scale_factor.get();
    let mut composition = window.fast_composition.borrow_mut();
    for (surface, surface_scene, region) in overlay_scenes {
        let Some(overlay) = composition.overlays.get_mut(&surface) else {
            log::error!("missing iOS composition surface {surface:?}");
            continue;
        };
        *overlay.region.lock() = region
            .iter()
            .map(|bounds| {
                Bounds::new(
                    point(px(bounds.origin.x.0 / scale), px(bounds.origin.y.0 / scale)),
                    size_px(bounds.size.width.0 / scale, bounds.size.height.0 / scale),
                )
            })
            .collect();
        match overlay.renderer(window_renderer) {
            Ok(renderer) => {
                if renderer.viewport_size() != size {
                    renderer.update_drawable_size(size);
                }
                renderer.draw(&surface_scene);
            }
            Err(error) => log::error!("creating an iOS composition renderer: {error:#}"),
        }
    }
}

fn size_px(width: f32, height: f32) -> gpui::Size<Pixels> {
    size(px(width), px(height))
}

/// `PlatformWindow::enable_window_composition`: needs the window's Metal
/// renderer, which the overlays share.
pub(crate) fn enable_window_composition(window: &IosWindow) -> Result<()> {
    anyhow::ensure!(
        window.renderer.lock().is_some(),
        "the window has no renderer: Metal is unavailable here"
    );
    Ok(())
}

/// `PlatformWindow::create_native_surface`: a container `UIView`, mounted
/// once the composition order holds it.
pub(crate) fn create_native_surface(
    window: &IosWindow,
) -> Result<Rc<dyn PlatformSurfaceAttachment>> {
    let view = unsafe {
        let view: *mut AnyObject = msg_send![class!(UIView), alloc];
        let view: *mut AnyObject = msg_send![view, initWithFrame: ObjcCGRect::default()];
        anyhow::ensure!(
            !view.is_null(),
            "creating a native surface container failed"
        );
        let _: () = msg_send![view, setClipsToBounds: true];
        view
    };
    let state = Rc::new(NativeState {
        view,
        bounds: Cell::new(Bounds::default()),
        parent_origin: Cell::new(Point::default()),
        scale: Cell::new(window.scale_factor.get()),
    });
    window
        .fast_composition
        .borrow_mut()
        .native_surfaces
        .push(Rc::downgrade(&state));
    Ok(Rc::new(NativeSurface { state }))
}

/// `PlatformWindow::set_composition_order`: creates and releases overlay
/// views and mounts every view under its parent's, in order.
pub(crate) fn set_composition_order(
    window: &IosWindow,
    surfaces: &[PlatformCompositionSurface],
) -> Result<()> {
    let base = surfaces
        .iter()
        .find_map(|surface| match surface.content {
            PlatformCompositionSurfaceContent::Gpui => Some(surface.id),
            _ => None,
        })
        .context("composition has no GPUI base surface")?;
    let mut composition = window.fast_composition.borrow_mut();
    let active = surfaces
        .iter()
        .filter(|surface| {
            surface.id != base && matches!(surface.content, PlatformCompositionSurfaceContent::Gpui)
        })
        .map(|surface| surface.id)
        .collect::<Vec<_>>();
    let gone = composition
        .overlays
        .keys()
        .filter(|id| !active.contains(id))
        .copied()
        .collect::<Vec<_>>();
    for id in gone {
        if let Some(mut overlay) = composition.overlays.remove(&id) {
            overlay.release();
        }
    }
    for id in &active {
        if !composition.overlays.contains_key(id) {
            let overlay = Overlay::new(window)?;
            composition.overlays.insert(*id, overlay);
        }
    }

    let scale = window.scale_factor.get();
    let mut views = HashMap::<CompositionSurfaceId, *mut AnyObject>::default();
    for surface in surfaces {
        match &surface.content {
            PlatformCompositionSurfaceContent::Gpui if surface.id == base => {}
            PlatformCompositionSurfaceContent::Gpui => {
                if let Some(overlay) = composition.overlays.get(&surface.id) {
                    views.insert(surface.id, overlay.view);
                }
            }
            PlatformCompositionSurfaceContent::Native(platform_surface)
            | PlatformCompositionSurfaceContent::ExternalGpu(platform_surface) => {
                let view = *platform_surface
                    .platform_handle()?
                    .downcast::<usize>()
                    .map_err(|_| anyhow::anyhow!("native surface is not a UIView"))?;
                platform_surface.set_parent_origin(surface.parent_origin)?;
                if let Some(state) = composition
                    .native_surfaces
                    .iter()
                    .filter_map(Weak::upgrade)
                    .find(|state| state.view as usize == view)
                {
                    state.scale.set(scale);
                    if let Some(bounds) = surface.window_bounds {
                        state.bounds.set(bounds);
                    }
                    state.place();
                }
                views.insert(surface.id, view as *mut AnyObject);
            }
        }
    }
    let order = surfaces
        .iter()
        .filter_map(|surface| {
            let view = *views.get(&surface.id)?;
            let parent = surface
                .parent
                .filter(|parent| *parent != base)
                .and_then(|parent| views.get(&parent).copied())
                .unwrap_or(window.view);
            Some((view as usize, parent as usize))
        })
        .collect::<Vec<_>>();
    if composition.mounted != order {
        unsafe {
            // Adding a subview again brings it to the front: in order, each
            // ends up above the ones before it.
            for (view, parent) in &order {
                let _: () =
                    msg_send![*parent as *mut AnyObject, addSubview: *view as *mut AnyObject];
            }
        }
        composition.mounted = order;
    }
    composition.base = Some(base);
    composition
        .native_surfaces
        .retain(|surface| surface.strong_count() > 0);
    Ok(())
}

/// Going to the background drops the overlays' surfaces; the next composed
/// frame in the foreground makes them again.
pub(crate) fn visibility_changed(window: &IosWindow, visibility: WindowVisibility) {
    if visibility == WindowVisibility::Hidden {
        for overlay in window.fast_composition.borrow_mut().overlays.values_mut() {
            overlay.renderer = None;
        }
    }
}

/// Releases the overlay views and unmounts the native containers before the
/// window they point to is freed.
pub(crate) fn release(window: &IosWindow) {
    let mut composition = window.fast_composition.borrow_mut();
    for (_, mut overlay) in composition.overlays.drain() {
        overlay.release();
    }
    for state in composition
        .native_surfaces
        .drain(..)
        .filter_map(|state| state.upgrade())
    {
        unsafe {
            let _: () = msg_send![state.view, removeFromSuperview];
        }
    }
    composition.mounted.clear();
    composition.base = None;
}
