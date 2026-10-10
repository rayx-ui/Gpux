//! Window composition on DirectComposition: native views between GPUI's base
//! content and its overlays (zed-industries/zed#62379).
//!
//! gpux's own, with gpui-fast's `gpui_windows/src/fast/composition.rs` as the
//! model: gpui-fast moves Zed's DirectX renderer onto a composition swap
//! chain, while gpux renders through wgpu's DX12 backend and keeps the
//! window's HWND flip swap chain, with its `DXGI_SCALING_NONE` live-resize
//! behaviour. Composition adds a topmost DirectComposition target on the
//! window, owned by gpux, whose root holds, in composition order, the visuals
//! of native surfaces (such as a WebView2 controller's root visual target)
//! and a visual per further GPUI surface. Each GPUI overlay visual holds a
//! premultiplied swap chain that wgpu creates from
//! `SurfaceTargetUnsafe::CompositionVisual`, drawn by a renderer sharing the
//! window renderer's GPU context, atlas and pipelines. A native surface is a
//! visual with a rectangle clip, placed at its bounds and hidden through its
//! opacity. gpux commits the composition after every composed frame, so a
//! producer's changes to a native visual show with the next frame.
//!
//! Visuals take no input: pointer and keyboard input reach the window, and
//! GPUI. An app embedding a native view forwards input to it itself (WebView2
//! composition hosting's `SendMouseInput`), and key downs of other windows,
//! such as WebView2's child windows, are left to their own window procedures.
//!
//! A lost device, or the DirectX device-lost message, rebuilds the tree on a
//! new DirectComposition device: new overlay visuals and renderers, new
//! native visuals (whose owners are told through `compositor_recreated`) and
//! the order last set.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use anyhow::{Context as _, Result};
use collections::{FxHashMap, FxHashSet};
use gpui_util::ResultExt as _;
use gpui_wgpu::{WgpuRenderer, WgpuSurfaceConfig, wgpu};
use gpux::{
    Bounds, ComposedScene, CompositionSurfaceId, DevicePixels, PlatformCompositionSurface,
    PlatformCompositionSurfaceContent, PlatformSurfaceAttachment, PlatformWindow as _, Point,
    Scene, Size,
};
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::DirectComposition::{
            DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionRectangleClip,
            IDCompositionTarget, IDCompositionVisual, IDCompositionVisual2, IDCompositionVisual3,
        },
        UI::WindowsAndMessaging::GetClassNameW,
    },
    core::{IUnknown, Interface as _},
};

use crate::WindowsWindow;

/// A window's composition state, held by `WindowsWindowState`.
#[derive(Default)]
pub(crate) struct Composition {
    /// The DirectComposition device, target and root, once composition is
    /// enabled.
    tree: Option<Tree>,
    /// The GPUI surface drawn into the window's own swap chain, once GPUI
    /// has ordered the window's surfaces.
    base: Option<CompositionSurfaceId>,
    /// The surfaces in the order last set.
    surfaces: Vec<PlatformCompositionSurface>,
    /// The GPUI surfaces above the base one.
    overlays: FxHashMap<CompositionSurfaceId, Overlay>,
    /// The native surfaces created for the window that may still be alive.
    native_surfaces: Vec<Weak<PortalState>>,
    /// Whether the tree must be rebuilt on a new device before the next
    /// composed frame.
    rebuild: bool,
}

struct Tree {
    device: IDCompositionDesktopDevice,
    /// Kept alive: dropping the target removes the visuals from the window.
    _target: IDCompositionTarget,
    root: IDCompositionVisual2,
}

struct Overlay {
    visual: IDCompositionVisual2,
    renderer: WgpuRenderer,
}

impl Tree {
    fn new(hwnd: HWND) -> Result<Self> {
        unsafe {
            let device: IDCompositionDesktopDevice =
                DCompositionCreateDevice2(None).context("creating a DirectComposition device")?;
            // Topmost: the visuals compose above the window's own swap chain.
            let target = device
                .CreateTargetForHwnd(hwnd, true)
                .context("creating the window's DirectComposition target")?;
            let root = device.CreateVisual()?;
            target.SetRoot(&root)?;
            device.Commit()?;
            Ok(Self {
                device,
                _target: target,
                root,
            })
        }
    }
}

impl Overlay {
    fn new(tree: &Tree, window_renderer: &WgpuRenderer) -> Result<Self> {
        let visual = unsafe { tree.device.CreateVisual() }?;
        // SAFETY: the visual lives in `Overlay` next to the renderer, which is
        // destroyed first (see `Drop`); wgpu holds its own reference as well.
        let renderer = unsafe {
            window_renderer.new_sharing_target(
                wgpu::SurfaceTargetUnsafe::CompositionVisual(visual.as_raw()),
                surface_config(window_renderer.viewport_size()),
                true,
            )
        }
        .context("creating a GPUI composition renderer")?;
        Ok(Self { visual, renderer })
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        // Release the swap chain before the visual it is the content of.
        self.renderer.unconfigure_surface();
        self.renderer.destroy();
    }
}

fn surface_config(size: Size<DevicePixels>) -> WgpuSurfaceConfig {
    WgpuSurfaceConfig {
        size,
        transparent: true,
        preferred_present_mode: None,
    }
}

/// A native surface: a visual with a rectangle clip on the window's
/// DirectComposition device, replaced in place when the tree is rebuilt.
struct PortalState {
    device: RefCell<IDCompositionDesktopDevice>,
    visual: RefCell<IDCompositionVisual2>,
    clip: RefCell<IDCompositionRectangleClip>,
    bounds: Cell<Bounds<DevicePixels>>,
    parent_origin: Cell<Point<DevicePixels>>,
    visible: Cell<bool>,
    compositor_recreated_callback: RefCell<Option<Rc<dyn Fn(Box<dyn Any>) -> Result<()>>>>,
}

impl PortalState {
    fn new(device: &IDCompositionDesktopDevice) -> Result<Self> {
        let (visual, clip) = Self::create(device)?;
        Ok(Self {
            device: RefCell::new(device.clone()),
            visual: RefCell::new(visual),
            clip: RefCell::new(clip),
            bounds: Cell::new(Bounds::default()),
            parent_origin: Cell::new(Point::default()),
            visible: Cell::new(true),
            compositor_recreated_callback: RefCell::new(None),
        })
    }

    fn create(
        device: &IDCompositionDesktopDevice,
    ) -> Result<(IDCompositionVisual2, IDCompositionRectangleClip)> {
        unsafe {
            let visual = device.CreateVisual()?;
            let clip = device.CreateRectangleClip()?;
            visual.SetClip(&clip)?;
            Ok((visual, clip))
        }
    }

    /// Places the visual at its bounds relative to its parent, clipped to
    /// them, shown or hidden; committed by the caller.
    fn apply(&self) -> Result<()> {
        let bounds = self.bounds.get();
        let parent_origin = self.parent_origin.get();
        let visual = self.visual.borrow();
        let clip = self.clip.borrow();
        unsafe {
            visual.SetOffsetX2((bounds.origin.x - parent_origin.x).0 as f32)?;
            visual.SetOffsetY2((bounds.origin.y - parent_origin.y).0 as f32)?;
            clip.SetLeft2(0.)?;
            clip.SetTop2(0.)?;
            clip.SetRight2(bounds.size.width.0.max(0) as f32)?;
            clip.SetBottom2(bounds.size.height.0.max(0) as f32)?;
            visual
                .cast::<IDCompositionVisual3>()?
                .SetOpacity2(if self.visible.get() { 1. } else { 0. })?;
        }
        Ok(())
    }

    fn commit(&self) -> Result<()> {
        unsafe { self.device.borrow().Commit() }.map_err(Into::into)
    }

    /// Moves the surface onto a new device, placed as before.
    fn rebind(&self, device: &IDCompositionDesktopDevice) -> Result<()> {
        let (visual, clip) = Self::create(device)?;
        *self.device.borrow_mut() = device.clone();
        *self.visual.borrow_mut() = visual;
        *self.clip.borrow_mut() = clip;
        self.apply()
    }

    fn handle(&self) -> Result<IUnknown> {
        self.visual.borrow().cast::<IUnknown>().map_err(Into::into)
    }
}

/// The attachment GPUI holds for a native surface. Its `platform_handle` is
/// the visual as an `IUnknown`, castable to `IDCompositionVisual` (WebView2's
/// `SetRootVisualTarget` takes it); content set on it shows from the next
/// composed frame, which gpux commits.
struct Portal {
    state: Rc<PortalState>,
}

impl PlatformSurfaceAttachment for Portal {
    fn set_bounds(&self, bounds: Bounds<DevicePixels>) -> Result<()> {
        self.state.bounds.set(bounds);
        self.state.apply()?;
        self.state.commit()
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        self.state.bounds.get()
    }

    fn set_parent_origin(&self, origin: Point<DevicePixels>) -> Result<()> {
        self.state.parent_origin.set(origin);
        self.state.apply()
    }

    fn set_visible(&self, visible: bool) -> Result<()> {
        if self.state.visible.replace(visible) != visible {
            self.state.apply()?;
            self.state.commit()?;
        }
        Ok(())
    }

    fn compositor_recreated(&self, _platform_context: &dyn Any) -> Result<()> {
        let callback = self.state.compositor_recreated_callback.borrow().clone();
        if let Some(callback) = callback {
            callback(Box::new(self.state.handle()?))?;
        }
        Ok(())
    }

    fn set_compositor_recreated_callback(
        &self,
        callback: Rc<dyn Fn(Box<dyn Any>) -> Result<()>>,
    ) -> Result<()> {
        *self.state.compositor_recreated_callback.borrow_mut() = Some(callback);
        Ok(())
    }

    fn platform_handle(&self) -> Result<Box<dyn Any>> {
        Ok(Box::new(self.state.handle()?))
    }
}

impl Composition {
    fn tree(&mut self, hwnd: HWND) -> Result<&Tree> {
        if self.tree.is_none() {
            self.tree = Some(Tree::new(hwnd)?);
        }
        self.tree
            .as_ref()
            .context("DirectComposition tree is missing")
    }

    /// Creates the visuals and renderers of GPUI surfaces new in `surfaces`,
    /// drops those of surfaces gone, and restacks every visual.
    fn set_order(
        &mut self,
        hwnd: HWND,
        window_renderer: &WgpuRenderer,
        surfaces: &[PlatformCompositionSurface],
    ) -> Result<()> {
        let base = surfaces
            .iter()
            .find_map(|surface| match surface.content {
                PlatformCompositionSurfaceContent::Gpui => Some(surface.id),
                PlatformCompositionSurfaceContent::Native(_)
                | PlatformCompositionSurfaceContent::ExternalGpu(_) => None,
            })
            .context("composition has no GPUI base surface")?;
        self.tree(hwnd)?;
        let active = surfaces
            .iter()
            .filter(|surface| {
                surface.id != base
                    && matches!(surface.content, PlatformCompositionSurfaceContent::Gpui)
            })
            .map(|surface| surface.id)
            .collect::<FxHashSet<_>>();
        self.overlays.retain(|id, _| active.contains(id));
        let tree = self
            .tree
            .as_ref()
            .context("DirectComposition tree is missing")?;
        for id in &active {
            if !self.overlays.contains_key(id) {
                self.overlays
                    .insert(*id, Overlay::new(tree, window_renderer)?);
            }
        }
        restack(tree, &self.overlays, surfaces, base)?;
        self.base = Some(base);
        self.surfaces = surfaces.to_vec();
        Ok(())
    }

    /// Rebuilds the tree on a new device: new overlay visuals and renderers,
    /// native visuals rebound (their owners told), the order restored.
    fn rebuild(&mut self, hwnd: HWND, window_renderer: &WgpuRenderer) -> Result<()> {
        self.rebuild = false;
        self.overlays.clear();
        self.tree = None;
        let tree = Tree::new(hwnd)?;
        self.native_surfaces
            .retain(|surface| surface.strong_count() > 0);
        for surface in self.native_surfaces.iter().filter_map(Weak::upgrade) {
            surface
                .rebind(&tree.device)
                .context("rebinding a DirectComposition native surface")?;
        }
        self.tree = Some(tree);
        let Some(base) = self.base else {
            return Ok(());
        };
        let surfaces = self.surfaces.clone();
        let tree = self
            .tree
            .as_ref()
            .context("DirectComposition tree is missing")?;
        for surface in &surfaces {
            if surface.id != base
                && matches!(surface.content, PlatformCompositionSurfaceContent::Gpui)
            {
                self.overlays
                    .insert(surface.id, Overlay::new(tree, window_renderer)?);
            }
        }
        let platform_context = tree.device.cast::<IUnknown>()?;
        for surface in &surfaces {
            if let PlatformCompositionSurfaceContent::Native(platform_surface)
            | PlatformCompositionSurfaceContent::ExternalGpu(platform_surface) = &surface.content
            {
                platform_surface
                    .compositor_recreated(&platform_context)
                    .context("rebinding a composition surface")?;
            }
        }
        restack(tree, &self.overlays, &surfaces, base)
    }
}

/// Makes the root hold every surface's visual but the base's in `surfaces`'
/// order, each nested in its parent's visual, and commits.
fn restack(
    tree: &Tree,
    overlays: &FxHashMap<CompositionSurfaceId, Overlay>,
    surfaces: &[PlatformCompositionSurface],
    base: CompositionSurfaceId,
) -> Result<()> {
    let mut visuals = FxHashMap::<CompositionSurfaceId, IDCompositionVisual>::default();
    for surface in surfaces {
        if surface.id == base {
            continue;
        }
        let visual = match &surface.content {
            PlatformCompositionSurfaceContent::Gpui => overlays
                .get(&surface.id)
                .context("GPUI composition visual is missing")?
                .visual
                .cast::<IDCompositionVisual>()?,
            PlatformCompositionSurfaceContent::Native(platform_surface)
            | PlatformCompositionSurfaceContent::ExternalGpu(platform_surface) => {
                platform_surface.set_parent_origin(surface.parent_origin)?;
                platform_surface
                    .platform_handle()?
                    .downcast::<IUnknown>()
                    .map_err(|_| {
                        anyhow::anyhow!("native surface is not a DirectComposition visual")
                    })?
                    .cast::<IDCompositionVisual>()?
            }
        };
        visuals.insert(surface.id, visual);
    }
    unsafe {
        tree.root.RemoveAllVisuals()?;
        for visual in visuals.values() {
            visual.RemoveAllVisuals()?;
        }
        let root = tree.root.cast::<IDCompositionVisual>()?;
        let mut previous =
            FxHashMap::<Option<CompositionSurfaceId>, IDCompositionVisual>::default();
        for surface in surfaces {
            let Some(visual) = visuals.get(&surface.id) else {
                continue;
            };
            // A surface whose parent is the base composes over the window.
            let parent = surface.parent.filter(|parent| *parent != base);
            let container = parent
                .and_then(|parent| visuals.get(&parent))
                .unwrap_or(&root);
            if matches!(surface.content, PlatformCompositionSurfaceContent::Gpui) {
                visual.SetOffsetX2((surface.window_origin.x - surface.parent_origin.x).0 as f32)?;
                visual.SetOffsetY2((surface.window_origin.y - surface.parent_origin.y).0 as f32)?;
            }
            container.AddVisual(visual, true, previous.get(&parent))?;
            previous.insert(parent, visual.clone());
        }
        tree.device.Commit()?;
    }
    Ok(())
}

/// `PlatformWindow::draw_composed`: draws the base content into the window's
/// own swap chain and each overlay's part into its visual's, then commits the
/// composition. Until GPUI orders the surfaces, the whole scene is drawn as
/// `draw` draws it.
pub(crate) fn draw_composed(window: &WindowsWindow, scene: ComposedScene<'_>) {
    let composition = &window.state.fast_composition;
    let Some(base) = composition.borrow().base else {
        window.draw(scene.scene());
        return;
    };

    let mut base_scene = Scene::default();
    let mut overlay_scenes = Vec::new();
    for layer in scene.layers() {
        match scene.layer_scene(layer) {
            Ok(surface_scene) if layer.surface == base => base_scene = surface_scene,
            Ok(surface_scene) => overlay_scenes.push((layer.surface, surface_scene)),
            Err(error) => {
                log::error!("replaying DirectComposition scene: {error:#}");
                return;
            }
        }
    }
    // Presents the base, recovering the window's renderer from a lost
    // device first if it has to.
    window.draw(&base_scene);

    let renderer = window.state.renderer.borrow();
    if renderer.device_lost() {
        return;
    }
    let mut composition = composition.borrow_mut();
    let lost = composition
        .overlays
        .values()
        .any(|overlay| overlay.renderer.device_lost());
    if composition.rebuild || lost {
        if let Err(error) = composition.rebuild(window.hwnd, &renderer) {
            log::error!("rebuilding the DirectComposition tree: {error:#}");
            composition.rebuild = true;
            return;
        }
    }
    let size = renderer.viewport_size();
    for (surface, surface_scene) in &overlay_scenes {
        let Some(overlay) = composition.overlays.get_mut(surface) else {
            log::error!("missing DirectComposition GPUI surface {surface:?}");
            continue;
        };
        if overlay.renderer.viewport_size() != size {
            overlay.renderer.update_drawable_size(size);
        }
        overlay.renderer.draw(surface_scene);
    }
    if let Some(tree) = &composition.tree {
        unsafe { tree.device.Commit() }.log_err();
    }
}

/// `PlatformWindow::enable_window_composition`: creates the window's
/// DirectComposition target.
pub(crate) fn enable_window_composition(window: &WindowsWindow) -> Result<()> {
    window
        .state
        .fast_composition
        .borrow_mut()
        .tree(window.hwnd)
        .map(|_| ())
}

/// `PlatformWindow::create_native_surface`: a clipped visual on the window's
/// DirectComposition device, placed once the composition order holds it.
pub(crate) fn create_native_surface(
    window: &WindowsWindow,
) -> Result<Rc<dyn PlatformSurfaceAttachment>> {
    let mut composition = window.state.fast_composition.borrow_mut();
    let state = Rc::new(PortalState::new(&composition.tree(window.hwnd)?.device)?);
    composition.native_surfaces.push(Rc::downgrade(&state));
    Ok(Rc::new(Portal { state }))
}

/// `PlatformWindow::set_composition_order`: creates and drops the overlays'
/// visuals and renderers and restacks every visual in `surfaces`' order.
pub(crate) fn set_composition_order(
    window: &WindowsWindow,
    surfaces: &[PlatformCompositionSurface],
) -> Result<()> {
    let renderer = window.state.renderer.borrow();
    window
        .state
        .fast_composition
        .borrow_mut()
        .set_order(window.hwnd, &renderer, surfaces)
}

/// Called when the window's renderer recovered from a lost device, or the
/// DirectX device-lost message arrives: the tree is rebuilt on a new device
/// before the next composed frame.
pub(crate) fn device_lost(window: &crate::window::WindowsWindowInner) {
    let mut composition = window.state.fast_composition.borrow_mut();
    if composition.tree.is_some() {
        composition.rebuild = true;
    }
}

/// `translate_accelerator`'s guard: whether `hwnd` is one of GPUI's windows,
/// whose key downs GPUI handles before they are translated. Key downs of
/// other windows, such as a WebView2 controller's child windows, are left to
/// their own window procedures.
pub(crate) fn is_gpui_window(hwnd: HWND) -> Option<()> {
    let class_name = unsafe { crate::window::WINDOW_CLASS_NAME.as_wide() };
    let mut buffer = [0u16; 64];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    let length = usize::try_from(length).ok()?;
    (buffer.get(..length)? == class_name).then_some(())
}
