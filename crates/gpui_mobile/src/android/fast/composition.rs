//! Window composition on Android (zed-industries/zed#62379): native views
//! between GPUI's base content and its overlay.
//!
//! gpux's own. GPUI draws its base content into the activity's window (a
//! NativeActivity's surface, or the embedded host's `SurfaceView`), where
//! Android views cannot draw, so gpux's Java helper (`dev.gpux.mobile.
//! Composition`, under `crates/gpui_mobile/java`) puts each native surface's
//! container in a panel window attached to the activity's window, and the
//! overlay in a translucent sub-panel window above them, holding a
//! `SurfaceView` (the spike's evidence is in `docs/upstream/gpui-fast.md`).
//! The overlay renders through a renderer sharing the window renderer's GPU
//! context, atlas and pipelines, on the overlay's own `ANativeWindow`.
//!
//! The helper's callbacks arrive on the UI thread: surface changes are held
//! in a shared slot the next composed frame reads (an overlay draw holds the
//! slot, so a destroyed surface waits for it to finish), and touches are
//! handed to GPUI's thread as raw touches. The overlay takes touches while it
//! drew something and lets them through to the native views otherwise.
//! Android composes one GPUI overlay, the composition's default one.
//!
//! wgpu's GLES backend presents only opaque surfaces (an `R8G8B8X8` EGL
//! config here), so with GL (emulators, whose software Vulkan gpux skips)
//! there is no overlay window: the whole scene is drawn into the activity's
//! window, as X11 does without a compositing manager, and each native
//! container clips the overlay's input region out of itself and hands the
//! touches starting there to GPUI. Shadows and translucent content then
//! cannot blend over native views.

use std::{
    any::Any,
    cell::Cell,
    ffi::c_void,
    ptr::NonNull,
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result};
use gpui::{
    Bounds, ComposedScene, CompositionSurfaceId, DevicePixels, PlatformCompositionSurface,
    PlatformCompositionSurfaceContent, PlatformSurfaceAttachment, Point, Scene,
};
use gpui_wgpu::{WgpuRenderer, WgpuSurfaceConfig, wgpu};
use jni::objects::{JObject, JValue};

/// A JNI global reference to a Java object.
type GlobalRef = jni::refs::Global<JObject<'static>>;
use ndk::native_window::NativeWindow;
use parking_lot::Mutex;
use raw_window_handle::{AndroidDisplayHandle, AndroidNdkWindowHandle};

use crate::android::{
    TouchPoint, dispatcher::AndroidDispatcher, jni as android_jni, window::AndroidWindow,
};

/// The helper class, in dot notation for the activity's class loader.
const HELPER: &str = "dev.gpux.mobile.Composition";

/// What the helper's UI-thread callbacks share with GPUI's thread.
struct Slot {
    /// The overlay's surface and its size, or `None` once destroyed.
    surface: Mutex<Option<(NativeWindow, i32, i32)>>,
    /// Bumped on every surface change.
    generation: AtomicU64,
    window: std::sync::Weak<AndroidWindow>,
    dispatcher: Arc<AndroidDispatcher>,
}

/// A window's composition state, held by `AndroidPlatformWindow`.
#[derive(Default)]
pub(crate) struct Composition {
    java: Option<Rc<GlobalRef>>,
    /// The slot the helper's callbacks reach; its `Arc` is the helper's
    /// handle, released with the composition.
    slot: Option<Arc<Slot>>,
    base: Option<CompositionSurfaceId>,
    overlay: Option<CompositionSurfaceId>,
    /// Whether the overlay has a window of its own (Vulkan), or is cut out
    /// of the native containers (GLES).
    overlay_window: bool,
    renderer: Option<(u64, WgpuRenderer)>,
    touchable: Cell<bool>,
    native_surfaces: Vec<Weak<NativeState>>,
}

impl Drop for Composition {
    fn drop(&mut self) {
        // The overlay's surface draws into the helper's window: drop it first.
        self.renderer = None;
        if let Some(java) = self.java.take() {
            android_jni::with_env(|env| {
                env.call_method(
                    java.as_obj(),
                    jni::jni_str!("release"),
                    jni::jni_sig!("()V"),
                    &[],
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            })
            .ok();
        }
        // The helper keeps calling back until its windows are gone; its
        // handle stays valid by leaking this one reference.
        if let Some(slot) = self.slot.take() {
            std::mem::forget(slot);
        }
    }
}

/// A native surface: the helper's container `FrameLayout`.
struct NativeState {
    container: GlobalRef,
    java: Rc<GlobalRef>,
    bounds: Cell<Bounds<DevicePixels>>,
    /// The holes last handed to the container, in cut-out mode.
    holes: std::cell::RefCell<Vec<f32>>,
}

fn call_helper(
    java: &GlobalRef,
    name: &'static str,
    f: impl FnOnce(&mut jni::Env, &JObject) -> jni::errors::Result<()>,
) {
    let result = android_jni::with_env(|env| {
        f(env, java.as_obj()).map_err(|error| {
            env.exception_describe();
            env.exception_clear();
            error.to_string()
        })
    });
    if let Err(error) = result {
        log::error!("dev.gpux.mobile.Composition.{name}: {error}");
    }
}

impl NativeState {
    /// Clips `holes` (window rectangles as x, y, width, height) out of the
    /// container, when they changed.
    fn set_holes(&self, holes: Vec<f32>) {
        if *self.holes.borrow() == holes {
            return;
        }
        let container = &self.container;
        call_helper(&self.java, "setHoles", |env, java| {
            let array = env.new_float_array(holes.len())?;
            array.set_region(env, 0, &holes)?;
            env.call_method(
                java,
                jni::jni_str!("setHoles"),
                jni::jni_sig!("(Landroid/widget/FrameLayout;[F)V"),
                &[JValue::Object(container.as_obj()), JValue::Object(&array)],
            )
            .map(|_| ())
        });
        *self.holes.borrow_mut() = holes;
    }

    fn place(&self) {
        let bounds = self.bounds.get();
        let container = &self.container;
        call_helper(&self.java, "setBounds", |env, java| {
            env.call_method(
                java,
                jni::jni_str!("setBounds"),
                jni::jni_sig!("(Landroid/widget/FrameLayout;IIII)V"),
                &[
                    JValue::Object(container.as_obj()),
                    JValue::Int(bounds.origin.x.0),
                    JValue::Int(bounds.origin.y.0),
                    JValue::Int(bounds.size.width.0),
                    JValue::Int(bounds.size.height.0),
                ],
            )
            .map(|_| ())
        });
    }
}

/// The attachment GPUI holds for a native surface. Its `platform_handle` is
/// a JNI global reference ([`GlobalRef`]) to the container `ViewGroup`.
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

    /// Panel windows are placed in window coordinates whatever the parent.
    fn set_parent_origin(&self, _: Point<DevicePixels>) -> Result<()> {
        Ok(())
    }

    fn set_visible(&self, visible: bool) -> Result<()> {
        let container = &self.state.container;
        call_helper(&self.state.java, "setVisible", |env, java| {
            env.call_method(
                java,
                jni::jni_str!("setVisible"),
                jni::jni_sig!("(Landroid/widget/FrameLayout;Z)V"),
                &[JValue::Object(container.as_obj()), JValue::Bool(visible)],
            )
            .map(|_| ())
        });
        Ok(())
    }

    /// A new global reference, the caller's to keep.
    fn platform_handle(&self) -> Result<Box<dyn Any>> {
        let container = android_jni::with_env(|env| {
            env.new_global_ref(self.state.container.as_obj())
                .map_err(|error| error.to_string())
        })
        .map_err(|error| anyhow::anyhow!("referencing a native surface: {error}"))?;
        Ok(Box::new(container))
    }
}

/// The helper's `nativeSurfaceChanged(long, Surface, int, int)`.
extern "system" fn native_surface_changed(
    env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    handle: jni::sys::jlong,
    surface: jni::sys::jobject,
    width: jni::sys::jint,
    height: jni::sys::jint,
) {
    let Some(slot) = (unsafe { (handle as *const Slot).as_ref() }) else {
        return;
    };
    let native_window = if surface.is_null() {
        None
    } else {
        // SAFETY: a live JNIEnv and Surface from the JVM's callback.
        unsafe { NativeWindow::from_surface(env.cast(), surface.cast()) }
            .map(|window| (window, width, height))
    };
    // Waits for an overlay draw in progress before the surface goes away.
    *slot.surface.lock() = native_window;
    slot.generation.fetch_add(1, Ordering::AcqRel);
    if let Some(window) = slot.window.upgrade() {
        window
            .force_next_frame_render
            .store(true, Ordering::Release);
    }
}

/// The helper's `nativeTouch(long, int, int, float, float)`.
extern "system" fn native_touch(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    handle: jni::sys::jlong,
    action: jni::sys::jint,
    pointer_id: jni::sys::jint,
    x: jni::sys::jfloat,
    y: jni::sys::jfloat,
) {
    let Some(slot) = (unsafe { (handle as *const Slot).as_ref() }) else {
        return;
    };
    let window = slot.window.clone();
    let point = TouchPoint {
        id: pointer_id,
        x,
        y,
        action: action as u32,
        timestamp: Some(std::time::Instant::now()),
    };
    slot.dispatcher.dispatch_on_main_thread(move || {
        if let Some(window) = window.upgrade() {
            window.handle_touch(point);
        }
    });
}

/// The platform's dispatcher, which runs closures on GPUI's thread.
fn dispatcher() -> Result<Arc<AndroidDispatcher>> {
    let platform = android_jni::platform()
        .or_else(|| crate::android::embedded::PLATFORM.get())
        .context("the Android platform is not running")?;
    Ok(platform.state.lock().dispatcher.clone())
}

/// `PlatformWindow::enable_window_composition`: loads the helper, registers
/// its native methods and makes it for the activity.
pub(crate) fn enable_window_composition(
    window: &Arc<AndroidWindow>,
    composition: &mut Composition,
) -> Result<()> {
    if composition.java.is_some() {
        return Ok(());
    }
    let slot = Arc::new(Slot {
        surface: Mutex::new(None),
        generation: AtomicU64::new(0),
        window: Arc::downgrade(window),
        dispatcher: dispatcher()?,
    });
    let handle = Arc::as_ptr(&slot) as i64;
    let overlay_window = window
        .state
        .lock()
        .renderer
        .as_ref()
        .map(|renderer| renderer.gpu_specs().backend == "vulkan")
        .context("the window has no renderer")?;
    let java = android_jni::with_env(|env| {
        let class = android_jni::find_app_class(env, HELPER)?;
        // SAFETY: the function pointers match the Java declarations.
        let methods = unsafe {
            [
                jni::NativeMethod::from_raw_parts(
                    jni::jni_str!("nativeSurfaceChanged"),
                    jni::jni_str!("(JLandroid/view/Surface;II)V"),
                    native_surface_changed as *mut c_void,
                ),
                jni::NativeMethod::from_raw_parts(
                    jni::jni_str!("nativeTouch"),
                    jni::jni_str!("(JIIFF)V"),
                    native_touch as *mut c_void,
                ),
            ]
        };
        // SAFETY: as above.
        unsafe { env.register_native_methods(&class, &methods) }
            .map_err(|error| format!("registering the helper's native methods: {error}"))?;
        let activity = android_jni::activity(env)?;
        let object = env
            .new_object(
                &class,
                jni::jni_sig!("(Landroid/app/Activity;JZ)V"),
                &[
                    JValue::Object(&activity),
                    JValue::Long(handle),
                    JValue::Bool(overlay_window),
                ],
            )
            .map_err(|error| {
                env.exception_describe();
                env.exception_clear();
                format!("making {HELPER}: {error}")
            })?;
        env.new_global_ref(&object)
            .map_err(|error| error.to_string())
    })
    .map_err(|error| anyhow::anyhow!("window composition: {error}"))?;
    composition.java = Some(Rc::new(java));
    composition.slot = Some(slot);
    composition.overlay_window = overlay_window;
    Ok(())
}

/// `PlatformWindow::create_native_surface`: the helper's container, in a
/// panel window once an order mounts it.
pub(crate) fn create_native_surface(
    composition: &mut Composition,
) -> Result<Rc<dyn PlatformSurfaceAttachment>> {
    let java = composition
        .java
        .clone()
        .context("window composition is not enabled")?;
    let container = android_jni::with_env(|env| {
        let container = env
            .call_method(
                java.as_obj(),
                jni::jni_str!("createNativeSurface"),
                jni::jni_sig!("()Landroid/widget/FrameLayout;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| {
                env.exception_describe();
                env.exception_clear();
                error.to_string()
            })?;
        env.new_global_ref(&container)
            .map_err(|error| error.to_string())
    })
    .map_err(|error| anyhow::anyhow!("creating a native surface: {error}"))?;
    let state = Rc::new(NativeState {
        container,
        java,
        bounds: Cell::new(Bounds::default()),
        holes: Default::default(),
    });
    composition.native_surfaces.push(Rc::downgrade(&state));
    Ok(Rc::new(NativeSurface { state }))
}

/// `PlatformWindow::set_composition_order`: mounts the native surfaces'
/// panel windows in order, under the overlay.
pub(crate) fn set_composition_order(
    composition: &mut Composition,
    surfaces: &[PlatformCompositionSurface],
) -> Result<()> {
    let java = composition
        .java
        .clone()
        .context("window composition is not enabled")?;
    let gpui = surfaces
        .iter()
        .filter(|surface| matches!(surface.content, PlatformCompositionSurfaceContent::Gpui))
        .map(|surface| surface.id)
        .collect::<Vec<_>>();
    anyhow::ensure!(
        gpui.len() <= 2,
        "Android composes one GPUI overlay above the base surface"
    );
    composition.base = gpui.first().copied();
    composition.overlay = gpui.get(1).copied();
    composition
        .native_surfaces
        .retain(|surface| surface.strong_count() > 0);
    let mut containers = Vec::new();
    for surface in surfaces {
        if let PlatformCompositionSurfaceContent::Native(platform_surface)
        | PlatformCompositionSurfaceContent::ExternalGpu(platform_surface) = &surface.content
        {
            let container = platform_surface
                .platform_handle()?
                .downcast::<GlobalRef>()
                .map_err(|_| anyhow::anyhow!("native surface is not an Android view"))?;
            containers.push(*container);
        }
    }
    let result = android_jni::with_env(|env| {
        let class = env
            .find_class(jni::jni_str!("android/widget/FrameLayout"))
            .map_err(|error| error.to_string())?;
        let array = env
            .new_object_array(containers.len() as i32, &class, JObject::null())
            .map_err(|error| error.to_string())?;
        for (index, container) in containers.iter().enumerate() {
            array
                .set_element(env, index, container.as_obj())
                .map_err(|error| error.to_string())?;
        }
        env.call_method(
            java.as_obj(),
            jni::jni_str!("setOrder"),
            jni::jni_sig!("([Landroid/widget/FrameLayout;)V"),
            &[JValue::Object(&array)],
        )
        .map(|_| ())
        .map_err(|error| {
            env.exception_describe();
            env.exception_clear();
            error.to_string()
        })
    });
    result.map_err(|error| anyhow::anyhow!("ordering the composition: {error}"))
}

/// `PlatformWindow::draw_composed`: the base into the activity's window and
/// the overlay into the helper's surface, which takes touches while it drew
/// something.
pub(crate) fn draw_composed(
    window: &Arc<AndroidWindow>,
    composition: &mut Composition,
    scene: ComposedScene<'_>,
) {
    let (Some(base), Some(slot)) = (composition.base, composition.slot.clone()) else {
        window.draw(scene.scene());
        return;
    };
    if !composition.overlay_window {
        draw_cut_out(window, composition, scene);
        return;
    }
    let mut base_scene = Scene::default();
    let mut overlay_scene = None;
    let mut region = Vec::new();
    for layer in scene.layers() {
        match scene.layer_scene(layer) {
            Ok(surface_scene) if layer.surface == base => base_scene = surface_scene,
            Ok(surface_scene) if Some(layer.surface) == composition.overlay => {
                region = scene.layer_input_region(layer);
                overlay_scene = Some(surface_scene);
            }
            Ok(_) => {}
            Err(error) => {
                log::error!("replaying Android composition scene: {error:#}");
                return;
            }
        }
    }
    window.draw(&base_scene);

    let touchable = !region.is_empty();
    if composition.touchable.replace(touchable) != touchable
        && let Some(java) = &composition.java
    {
        call_helper(java, "setOverlayTouchable", |env, java| {
            env.call_method(
                java,
                jni::jni_str!("setOverlayTouchable"),
                jni::jni_sig!("(Z)V"),
                &[JValue::Bool(touchable)],
            )
            .map(|_| ())
        });
    }

    let Some(overlay_scene) = overlay_scene else {
        return;
    };
    // Held while drawing: a destroyed surface waits until the frame is done.
    let surface = slot.surface.lock();
    let generation = slot.generation.load(Ordering::Acquire);
    if composition
        .renderer
        .as_ref()
        .is_some_and(|(made_at, _)| *made_at != generation)
    {
        composition.renderer = None;
    }
    let Some((native_window, width, height)) = surface.as_ref() else {
        return;
    };
    if composition.renderer.is_none() {
        let made = window
            .state
            .lock()
            .renderer
            .as_ref()
            .map(|window_renderer| {
                overlay_renderer(window_renderer, native_window, *width, *height)
            });
        match made {
            Some(Ok(renderer)) => composition.renderer = Some((generation, renderer)),
            Some(Err(error)) => {
                log::error!("creating the Android composition renderer: {error:#}");
                return;
            }
            None => return,
        }
    }
    if let Some((_, renderer)) = composition.renderer.as_mut() {
        renderer.draw(&overlay_scene);
    }
}

/// Cut-out mode (GLES): the whole scene goes into the activity's window, and
/// the overlay's input region is clipped out of every native container.
fn draw_cut_out(
    window: &Arc<AndroidWindow>,
    composition: &mut Composition,
    scene: ComposedScene<'_>,
) {
    window.draw(scene.scene());
    let region = scene
        .layers()
        .iter()
        .find(|layer| Some(layer.surface) == composition.overlay)
        .map(|layer| scene.layer_input_region(layer))
        .unwrap_or_default();
    composition
        .native_surfaces
        .retain(|surface| surface.strong_count() > 0);
    for state in composition.native_surfaces.iter().filter_map(Weak::upgrade) {
        let bounds = state.bounds.get();
        let holes = region
            .iter()
            .filter(|hole| {
                hole.origin.x.0 < (bounds.origin.x.0 + bounds.size.width.0) as f32
                    && hole.origin.y.0 < (bounds.origin.y.0 + bounds.size.height.0) as f32
                    && hole.origin.x.0 + hole.size.width.0 > bounds.origin.x.0 as f32
                    && hole.origin.y.0 + hole.size.height.0 > bounds.origin.y.0 as f32
            })
            .flat_map(|hole| {
                [
                    hole.origin.x.0.floor(),
                    hole.origin.y.0.floor(),
                    hole.size.width.0.ceil(),
                    hole.size.height.0.ceil(),
                ]
            })
            .collect();
        state.set_holes(holes);
    }
}

/// A renderer for the overlay's `ANativeWindow`, sharing the window
/// renderer's context (its backend, Vulkan or GL), atlas and pipelines.
fn overlay_renderer(
    window_renderer: &WgpuRenderer,
    native_window: &NativeWindow,
    width: i32,
    height: i32,
) -> Result<WgpuRenderer> {
    let pointer = NonNull::new(native_window.ptr().as_ptr().cast::<c_void>())
        .context("the overlay's native window is null")?;
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(AndroidDisplayHandle::new().into()),
        raw_window_handle: AndroidNdkWindowHandle::new(pointer).into(),
    };
    // SAFETY: the slot keeps the native window until the renderer is dropped
    // (a new surface generation drops it first).
    unsafe {
        window_renderer.new_sharing_target(
            target,
            WgpuSurfaceConfig {
                size: gpui::size(DevicePixels(width), DevicePixels(height)),
                transparent: true,
                preferred_present_mode: Some(wgpu::PresentMode::Mailbox),
            },
            true,
        )
    }
}
