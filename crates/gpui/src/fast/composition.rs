//! Window composition: native views between GPUI's content and its
//! overlays.
//!
//! Adapted from gpui-fast's `fast/composition.rs`, itself a port of
//! zed-industries/zed#62379. A window paints one scene, as always, but can
//! present it on several ordered surfaces, so that a platform-native view (a
//! `WKWebView`, a WebView2, a Wayland subsurface fed by another GPU producer)
//! sits above GPUI's base content and below its overlays: popovers, dialogs,
//! menus, tooltips.
//!
//! ```text
//! front  GPUI overlay    the top-layer band, then deferred and window-level draws
//!        native surface  WKWebView / WebView2 / wl_subsurface / X11 child
//! back   GPUI base       the root view
//! ```
//!
//! [`CompositionTree`] holds each surface's identity, parent and sibling
//! order. The frame records, as it paints, which GPUI surface each stretch of
//! the scene targets ([`SurfaceStarts`]); presenting splits the scene along
//! those stretches into a [`ComposedScene`], and the platform draws each
//! surface's part onto its own layer, visual or subsurface.
//!
//! gpux adds to the upstream split: the primitives of its top-layer band
//! (`Window::paint_top_layer`) go to the overlay wherever they were painted,
//! since they draw above everything; a range replayed on its own restores the
//! clip, positioned and top layers it began inside; text painted onto the
//! overlay is grayscale, since it blends over native content; and a
//! [`native_surface`] element places a native surface where it is laid out.
//!
//! Composition is opt-in: until [`Window::enable_window_composition`] is
//! called, a window presents its scene with `PlatformWindow::draw`, as
//! upstream does, and paints exactly what it painted before.
//!
//! Unlike the upstream pull request, a view drawn again from last frame (a
//! cached, retained or spliced view) carries the surface switches it made with
//! it: [`reuse_starts`] copies them along with its scene.

use crate::window::{PaintIndex, PrepaintStateIndex};
use crate::{
    App, Bounds, DevicePixels, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, PlatformWindow, Point, Primitive, ScaledPixels, Scene, Style,
    StyleRefinement, Styled, Window, scene::PaintOperation,
};
use anyhow::{Context as _, Result, anyhow, bail};
use collections::{FxHashMap, FxHashSet};
use gpui_util::ResultExt as _;
use refineable::Refineable as _;
use slotmap::SlotMap;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    mem,
    ops::Range,
    rc::Rc,
};

slotmap::new_key_type! {
    /// Stable identity for a surface in a window composition tree.
    pub struct CompositionSurfaceId;
}

/// The renderer or platform facility responsible for a composition surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositionSurfaceKind {
    /// A surface rendered from a range of GPUI scene operations.
    Gpui,
    /// A surface whose content is supplied by a platform-native view or visual.
    Native,
    /// A surface whose content is supplied by an external GPU producer.
    ExternalGpu,
}

#[derive(Clone)]
struct CompositionSurfaceNode {
    kind: CompositionSurfaceKind,
    parent: Option<CompositionSurfaceId>,
    children: Vec<CompositionSurfaceId>,
}

/// The surfaces of a window, as a forest ordered bottom to top.
#[derive(Clone)]
pub(crate) struct CompositionTree {
    surfaces: SlotMap<CompositionSurfaceId, CompositionSurfaceNode>,
    roots: Vec<CompositionSurfaceId>,
}

impl CompositionTree {
    pub(crate) fn new() -> Self {
        Self {
            surfaces: SlotMap::with_key(),
            roots: Vec::new(),
        }
    }

    /// Adds a surface above its future siblings.
    pub(crate) fn insert(
        &mut self,
        kind: CompositionSurfaceKind,
        parent: Option<CompositionSurfaceId>,
    ) -> Result<CompositionSurfaceId> {
        if let Some(parent) = parent {
            self.surface(parent)?;
        }

        let surface = self.surfaces.insert(CompositionSurfaceNode {
            kind,
            parent,
            children: Vec::new(),
        });
        self.siblings_mut(parent)?.push(surface);
        Ok(surface)
    }

    /// Removes a surface, putting its children where it was.
    pub(crate) fn remove(&mut self, surface: CompositionSurfaceId) -> Result<()> {
        let parent = self.surface(surface)?.parent;
        let children = self.surface(surface)?.children.clone();
        let siblings = self.siblings_mut(parent)?;
        let index = siblings
            .iter()
            .position(|candidate| *candidate == surface)
            .ok_or_else(|| anyhow!("composition surface is missing from its parent"))?;
        siblings.splice(index..=index, children.iter().copied());
        for child in children {
            self.surface_mut(child)?.parent = parent;
        }
        self.surfaces.remove(surface);
        Ok(())
    }

    pub(crate) fn place_above(
        &mut self,
        surface: CompositionSurfaceId,
        sibling: CompositionSurfaceId,
    ) -> Result<()> {
        self.place_relative(surface, sibling, true)
    }

    pub(crate) fn place_below(
        &mut self,
        surface: CompositionSurfaceId,
        sibling: CompositionSurfaceId,
    ) -> Result<()> {
        self.place_relative(surface, sibling, false)
    }

    /// Moves a surface under `parent`, above the children it has.
    pub(crate) fn reparent(
        &mut self,
        surface: CompositionSurfaceId,
        parent: Option<CompositionSurfaceId>,
    ) -> Result<()> {
        self.surface(surface)?;
        if let Some(parent) = parent {
            self.surface(parent)?;
            if parent == surface || self.is_descendant(parent, surface)? {
                bail!("composition surfaces cannot contain themselves");
            }
        }

        let previous_parent = self.surface(surface)?.parent;
        self.siblings_mut(previous_parent)?
            .retain(|candidate| *candidate != surface);
        self.surface_mut(surface)?.parent = parent;
        self.siblings_mut(parent)?.push(surface);
        Ok(())
    }

    pub(crate) fn kind(&self, surface: CompositionSurfaceId) -> Result<CompositionSurfaceKind> {
        Ok(self.surface(surface)?.kind)
    }

    pub(crate) fn parent(
        &self,
        surface: CompositionSurfaceId,
    ) -> Result<Option<CompositionSurfaceId>> {
        Ok(self.surface(surface)?.parent)
    }

    /// The children of `parent`, or the roots when it is `None`, bottom to top.
    pub(crate) fn children(
        &self,
        parent: Option<CompositionSurfaceId>,
    ) -> Result<&[CompositionSurfaceId]> {
        match parent {
            Some(parent) => Ok(&self.surface(parent)?.children),
            None => Ok(&self.roots),
        }
    }

    /// Every surface, parents before their children, bottom to top.
    pub(crate) fn flattened(&self) -> Vec<CompositionSurfaceId> {
        let mut surfaces = Vec::with_capacity(self.surfaces.len());
        for root in &self.roots {
            self.flatten_into(*root, &mut surfaces);
        }
        surfaces
    }

    fn place_relative(
        &mut self,
        surface: CompositionSurfaceId,
        sibling: CompositionSurfaceId,
        above: bool,
    ) -> Result<()> {
        if surface == sibling {
            bail!("a composition surface cannot be ordered relative to itself");
        }
        let parent = self.surface(surface)?.parent;
        if self.surface(sibling)?.parent != parent {
            bail!("composition surfaces must share a parent to be reordered");
        }

        let siblings = self.siblings_mut(parent)?;
        siblings.retain(|candidate| *candidate != surface);
        let sibling_index = siblings
            .iter()
            .position(|candidate| *candidate == sibling)
            .ok_or_else(|| anyhow!("composition sibling is missing from its parent"))?;
        let index = sibling_index + usize::from(above);
        siblings.insert(index, surface);
        Ok(())
    }

    fn is_descendant(
        &self,
        candidate: CompositionSurfaceId,
        ancestor: CompositionSurfaceId,
    ) -> Result<bool> {
        let mut parent = Some(candidate);
        while let Some(surface) = parent {
            if surface == ancestor {
                return Ok(true);
            }
            parent = self.surface(surface)?.parent;
        }
        Ok(false)
    }

    fn surface(&self, surface: CompositionSurfaceId) -> Result<&CompositionSurfaceNode> {
        self.surfaces
            .get(surface)
            .ok_or_else(|| anyhow!("composition surface does not exist"))
    }

    fn surface_mut(
        &mut self,
        surface: CompositionSurfaceId,
    ) -> Result<&mut CompositionSurfaceNode> {
        self.surfaces
            .get_mut(surface)
            .ok_or_else(|| anyhow!("composition surface does not exist"))
    }

    fn siblings_mut(
        &mut self,
        parent: Option<CompositionSurfaceId>,
    ) -> Result<&mut Vec<CompositionSurfaceId>> {
        match parent {
            Some(parent) => Ok(&mut self.surface_mut(parent)?.children),
            None => Ok(&mut self.roots),
        }
    }

    fn flatten_into(
        &self,
        surface: CompositionSurfaceId,
        flattened: &mut Vec<CompositionSurfaceId>,
    ) {
        flattened.push(surface);
        if let Some(surface) = self.surfaces.get(surface) {
            for child in &surface.children {
                self.flatten_into(*child, flattened);
            }
        }
    }
}
/// Splits a scene along `starts`, the points where painting switched GPUI
/// surface, into one layer per GPUI surface of `tree`, in composition order.
/// The stretches of the top-layer band that `overlay` does not hold already
/// move to it from whichever surface they were painted on.
pub(crate) fn composed_scene_layers(
    tree: &CompositionTree,
    starts: &[(CompositionSurfaceId, usize)],
    scene: &Scene,
    overlay: CompositionSurfaceId,
) -> Result<Vec<ComposedSceneLayer>> {
    let scene_len = scene.len();
    let mut ranges_by_surface = FxHashMap::<_, Vec<Range<usize>>>::default();
    for (index, (surface, start)) in starts.iter().enumerate() {
        if tree.kind(*surface)? != CompositionSurfaceKind::Gpui {
            bail!("scene operations target a non-GPUI composition surface");
        }
        let end = starts
            .get(index + 1)
            .map_or(scene_len, |(_, next_start)| *next_start);
        if *start > end || end > scene_len {
            bail!("composition scene ranges are not monotonic");
        }
        if *start >= end {
            continue;
        }
        if *surface == overlay {
            ranges_by_surface
                .entry(*surface)
                .or_default()
                .push(*start..end);
            continue;
        }
        let (kept, top) = split_top_layers(&scene.paint_operations, *start..end);
        ranges_by_surface.entry(*surface).or_default().extend(kept);
        ranges_by_surface.entry(overlay).or_default().extend(top);
    }

    let mut layers = Vec::new();
    for surface in tree.flattened() {
        if tree.kind(surface)? == CompositionSurfaceKind::Gpui {
            let mut ranges = ranges_by_surface.remove(&surface).unwrap_or_default();
            ranges.sort_by_key(|range| range.start);
            layers.push(ComposedSceneLayer { surface, ranges });
        }
    }
    Ok(layers)
}

/// `range` of `operations` split into what stays where it was painted and
/// the stretches of the top-layer band in it, each from its opening to its
/// closing operation.
fn split_top_layers(
    operations: &[PaintOperation],
    range: Range<usize>,
) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let mut kept = Vec::new();
    let mut top = Vec::new();
    let mut cursor = range.start;
    let mut index = range.start;
    while index < range.end {
        if let PaintOperation::StartTopLayer(_) = operations[index] {
            let mut depth = 0usize;
            let mut end = index;
            while end < range.end {
                match operations[end] {
                    PaintOperation::StartLayer(_)
                    | PaintOperation::StartPositionedLayer(_)
                    | PaintOperation::StartTopLayer(_) => depth += 1,
                    PaintOperation::EndLayer => depth -= 1,
                    PaintOperation::Primitive(_)
                    | PaintOperation::FastDropped(_)
                    | PaintOperation::FastClip(_) => {}
                }
                end += 1;
                if depth == 0 {
                    break;
                }
            }
            if cursor < index {
                kept.push(cursor..index);
            }
            top.push(index..end);
            cursor = end;
            index = end;
        } else {
            index += 1;
        }
    }
    if cursor < range.end {
        kept.push(cursor..range.end);
    }
    (kept, top)
}

/// A platform attachment participating in a window composition tree.
pub trait PlatformSurfaceAttachment {
    /// Updates the native surface geometry in window-content device pixels.
    fn set_bounds(&self, bounds: Bounds<DevicePixels>) -> Result<()>;
    /// Returns the native surface geometry in window-content device pixels.
    fn bounds(&self) -> Bounds<DevicePixels>;
    /// Updates the window-content origin of the surface's composition parent.
    /// Platform adapters use this to preserve window-coordinate geometry when
    /// a surface is nested.
    fn set_parent_origin(&self, origin: Point<DevicePixels>) -> Result<()>;
    /// Rebinds externally owned content after the platform compositor has been
    /// recreated. Native surfaces managed by GPUI may use the default no-op.
    fn compositor_recreated(&self, _platform_context: &dyn Any) -> Result<()> {
        Ok(())
    }
    /// Registers a callback that reattaches native content when the platform
    /// attachment object changes after compositor recovery.
    fn set_compositor_recreated_callback(
        &self,
        _callback: Rc<dyn Fn(Box<dyn Any>) -> Result<()>>,
    ) -> Result<()> {
        bail!("compositor recreation callbacks are not supported")
    }
    /// Updates whether the native surface participates in composition.
    fn set_visible(&self, visible: bool) -> Result<()>;
    /// Returns the platform attachment object: an `NSView` pointer as `usize`
    /// on macOS, an `IDCompositionVisual` as `windows::core::IUnknown` on
    /// Windows, a `raw_window_handle::RawWindowHandle` on Linux.
    fn platform_handle(&self) -> Result<Box<dyn Any>>;
}

/// The ranges of a GPUI scene rendered onto one composition surface.
#[derive(Clone, Debug)]
pub struct ComposedSceneLayer {
    /// The GPUI composition surface receiving these scene ranges.
    pub surface: CompositionSurfaceId,
    /// Paint-operation ranges belonging to the surface.
    pub ranges: Vec<Range<usize>>,
}

/// A scene split across GPUI surfaces in composition order.
pub struct ComposedScene<'a> {
    scene: &'a Scene,
    layers: Vec<ComposedSceneLayer>,
}

impl<'a> ComposedScene<'a> {
    pub(crate) fn new(scene: &'a Scene, layers: Vec<ComposedSceneLayer>) -> Self {
        Self { scene, layers }
    }

    /// Returns the complete scene containing all GPUI planes.
    pub fn scene(&self) -> &'a Scene {
        self.scene
    }

    /// Returns the GPUI surface layers in composition order.
    pub fn layers(&self) -> &[ComposedSceneLayer] {
        &self.layers
    }

    /// Returns the part of the scene `layer` covers as a finished scene of
    /// its own, ready to draw onto the layer's surface.
    pub fn layer_scene(&self, layer: &ComposedSceneLayer) -> Result<Scene> {
        let mut scene = Scene::default();
        for range in &layer.ranges {
            scene.replay_balanced(range.clone(), self.scene)?;
        }
        scene.finish();
        Ok(scene)
    }

    /// Where `layer` drew, in window-content device pixels: the bounds of
    /// its primitives clipped by their content masks, overlapping ones
    /// merged. A surface above native content takes pointer input only
    /// there, so a menu or a scrollbar on the overlay leaves the rest of a
    /// native view clickable. Shadows are left out, as GPUI's hit testing
    /// leaves them out.
    pub fn layer_input_region(&self, layer: &ComposedSceneLayer) -> Vec<Bounds<ScaledPixels>> {
        let mut region = Vec::<Bounds<ScaledPixels>>::new();
        for range in &layer.ranges {
            for operation in &self.scene.paint_operations[range.clone()] {
                let PaintOperation::Primitive(primitive) = operation else {
                    continue;
                };
                if matches!(primitive, Primitive::Shadow(_)) {
                    continue;
                }
                let mut bounds = primitive
                    .bounds()
                    .intersect(&primitive.content_mask().bounds);
                if bounds.is_empty() {
                    continue;
                }
                while let Some(index) = region.iter().position(|other| other.intersects(&bounds)) {
                    bounds = bounds.union(&region.swap_remove(index));
                }
                region.push(bounds);
                if region.len() > INPUT_REGION_RECTS {
                    let merged = region
                        .drain(..)
                        .reduce(|merged, bounds| merged.union(&bounds));
                    region.extend(merged);
                }
            }
        }
        region
    }
}

/// The most rectangles a layer's input region keeps before it is merged
/// into one, which bounds the cost of a hit test on it.
const INPUT_REGION_RECTS: usize = 32;

/// A platform surface participating in window composition.
#[derive(Clone)]
pub struct PlatformCompositionSurface {
    /// Stable identity in the window composition tree.
    pub id: CompositionSurfaceId,
    /// Parent surface, or `None` for a root surface.
    pub parent: Option<CompositionSurfaceId>,
    /// This surface's origin in window-content device pixels. GPUI surfaces
    /// use the window origin because their backing scenes remain window-based.
    pub window_origin: Point<DevicePixels>,
    /// The parent surface's origin in window-content device pixels.
    pub parent_origin: Point<DevicePixels>,
    /// Native or external surface bounds in window-content device pixels.
    /// GPUI surfaces use the full window and therefore have no explicit bounds.
    pub window_bounds: Option<Bounds<DevicePixels>>,
    /// Renderer or attachment supplying this surface's content.
    pub content: PlatformCompositionSurfaceContent,
}

/// Content attached to a platform composition surface.
#[derive(Clone)]
pub enum PlatformCompositionSurfaceContent {
    /// A surface rendered by GPUI.
    Gpui,
    /// A surface backed by a platform-native view or visual.
    Native(Rc<dyn PlatformSurfaceAttachment>),
    /// A surface backed by an external GPU producer.
    ExternalGpu(Rc<dyn PlatformSurfaceAttachment>),
}

/// What `PlatformWindow::draw_composed` does on a platform without
/// composition: draws the whole scene on the window's one surface.
pub(crate) fn draw_uncomposed<W: PlatformWindow + ?Sized>(window: &W, scene: ComposedScene<'_>) {
    window.draw(scene.scene());
}

/// What the `PlatformWindow` composition methods return on a platform
/// without composition.
pub(crate) fn unsupported<T>(what: &str) -> Result<T> {
    bail!("{what} is not supported on this platform")
}

impl crate::Background {
    /// Returns whether the background covers its bounds with no translucency,
    /// so a platform compositing native content below it can cut that
    /// content out instead of blending it.
    pub fn is_opaque(&self) -> bool {
        match self.tag {
            crate::color::BackgroundTag::Solid => self.solid.is_opaque(),
            crate::color::BackgroundTag::LinearGradient => {
                self.stops.iter().all(|stop| stop.color.is_opaque())
            }
            crate::color::BackgroundTag::PatternSlash
            | crate::color::BackgroundTag::Checkerboard => false,
        }
    }
}

impl crate::PreparedBackground {
    /// [`crate::Background::is_opaque`] for a background prepared for the GPU,
    /// as a scene's quads hold it: premultiplied, so a stop is opaque when its
    /// alpha is one.
    pub fn is_opaque(&self) -> bool {
        match self.tag {
            crate::color::BackgroundTag::Solid => self.solid.a >= 1.0,
            crate::color::BackgroundTag::LinearGradient => {
                self.stops.iter().all(|stop| stop.color[3] >= 1.0)
            }
            crate::color::BackgroundTag::PatternSlash
            | crate::color::BackgroundTag::Checkerboard => false,
        }
    }
}

impl Scene {
    /// Returns whether the scene contains no drawable primitives.
    ///
    /// A scene may have paint operations that only open and close empty
    /// layers, so `len() == 0` is not equivalent to having no visible or
    /// input-relevant overlay content.
    pub fn is_empty(&self) -> bool {
        self.shadows.is_empty()
            && self.visual_effects.is_empty()
            && self.quads.is_empty()
            && self.html_islands.is_empty()
            && self.html_elements.is_empty()
            && self.screen_effect_regions.is_empty()
            && self.paths.is_empty()
            && self.underlines.is_empty()
            && self.monochrome_sprites.is_empty()
            && self.subpixel_sprites.is_empty()
            && self.polychrome_sprites.is_empty()
            && self.surfaces.is_empty()
    }

    /// Replays a range as a self-contained scene, restoring the clip,
    /// positioned and top layers that were open at its start and closing
    /// those still open at its end.
    pub fn replay_balanced(&mut self, range: Range<usize>, previous_scene: &Scene) -> Result<()> {
        anyhow::ensure!(
            range.start <= range.end,
            "scene replay range starts after its end"
        );
        let prefix = previous_scene
            .paint_operations
            .get(..range.start)
            .context("scene replay range starts past the scene")?;
        let operations = previous_scene
            .paint_operations
            .get(range.clone())
            .context("scene replay range ends past the scene")?;
        let mut active_layers = Vec::new();
        // The clips open at the range's start, which what it paints is
        // ordered by (see `crate::fast::edge_shadows`).
        let mut active_clips = Vec::new();
        for operation in prefix {
            match operation {
                PaintOperation::FastClip(clip) => match clip {
                    crate::fast::edge_shadows::ClipOp::Push(_) => active_clips.push(*clip),
                    crate::fast::edge_shadows::ClipOp::Pop => {
                        active_clips.pop();
                    }
                },
                PaintOperation::StartLayer(_)
                | PaintOperation::StartPositionedLayer(_)
                | PaintOperation::StartTopLayer(_) => active_layers.push(operation),
                PaintOperation::EndLayer => {
                    anyhow::ensure!(
                        active_layers.pop().is_some(),
                        "source scene closes a layer that is not open"
                    );
                }
                PaintOperation::Primitive(_) | PaintOperation::FastDropped(_) => {}
            }
        }
        crate::fast::layers::paint::replay_layers(self, range, previous_scene);
        for layer in &active_layers {
            self.open_layer(layer);
        }
        let mut open_clips = active_clips.len();
        for clip in active_clips {
            crate::fast::edge_shadows::clip_op(self, clip);
        }
        let mut open = active_layers.len();
        for operation in operations {
            match operation {
                PaintOperation::FastClip(clip) => {
                    match clip {
                        crate::fast::edge_shadows::ClipOp::Push(_) => open_clips += 1,
                        crate::fast::edge_shadows::ClipOp::Pop => {
                            open_clips = open_clips.saturating_sub(1)
                        }
                    }
                    crate::fast::edge_shadows::clip_op(self, *clip);
                }
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::FastDropped(dropped) => {
                    crate::fast::edge_shadows::dropped(self, *dropped)
                }
                PaintOperation::StartLayer(_)
                | PaintOperation::StartPositionedLayer(_)
                | PaintOperation::StartTopLayer(_) => {
                    open += 1;
                    self.open_layer(operation);
                }
                PaintOperation::EndLayer => {
                    anyhow::ensure!(
                        open > 0,
                        "scene replay range closes a layer that is not open"
                    );
                    open -= 1;
                    self.pop_layer();
                }
            }
        }
        for _ in 0..open {
            self.pop_layer();
        }
        for _ in 0..open_clips {
            crate::fast::edge_shadows::clip_op(self, crate::fast::edge_shadows::ClipOp::Pop);
        }
        Ok(())
    }

    fn open_layer(&mut self, operation: &PaintOperation) {
        match operation {
            PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
            PaintOperation::StartPositionedLayer(bounds) => self.push_positioned_layer(*bounds),
            PaintOperation::StartTopLayer(bounds) => self.push_top_layer(*bounds),
            PaintOperation::EndLayer
            | PaintOperation::Primitive(_)
            | PaintOperation::FastDropped(_)
            | PaintOperation::FastClip(_) => {}
        }
    }

    /// Whether what is painted now is painted in the top-layer band.
    pub(crate) fn in_top_layer(&self) -> bool {
        self.layer_stack
            .iter()
            .any(|layer| matches!(layer, crate::scene::LayerOrder::Top))
    }
}

/// Where each stretch of a frame's scene is presented: the GPUI surface
/// painting switched to, and the scene length at the switch.
#[derive(Default)]
pub(crate) struct SurfaceStarts(Vec<(CompositionSurfaceId, usize)>);

impl SurfaceStarts {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    fn current(&self) -> Option<CompositionSurfaceId> {
        self.0.last().map(|(surface, _)| *surface)
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The switches, in scene order.
    #[cfg(test)]
    pub(crate) fn switches(&self) -> &[(CompositionSurfaceId, usize)] {
        &self.0
    }

    fn switch_to(&mut self, surface: CompositionSurfaceId, scene_len: usize) {
        if self.current() == Some(surface) {
            return;
        }
        self.0.push((surface, scene_len));
    }
}

/// A window's composition: its surface tree, the platform surfaces in it and
/// whether the window presents through it.
pub(crate) struct WindowCompositionState {
    tree: CompositionTree,
    platform_surfaces: FxHashMap<CompositionSurfaceId, Rc<dyn PlatformSurfaceAttachment>>,
    base_surface: CompositionSurfaceId,
    overlay_surface: CompositionSurfaceId,
    geometry_dirty: Rc<Cell<bool>>,
    enabled: bool,
    /// Whether the next frame is drawn afresh: one enabling composition, so
    /// that nothing painted before is replayed onto the overlay with
    /// subpixel text.
    refresh_pending: bool,
    /// The order and geometry last handed to the platform, so that only a
    /// change is handed over again.
    synchronized: Option<Vec<SynchronizedSurface>>,
    /// The native and external surfaces a [`native_surface`] element has
    /// placed: from then on they are shown only in frames that place them.
    placed_by_elements: FxHashSet<CompositionSurfaceId>,
    /// The surfaces of `placed_by_elements` hidden because the last frame
    /// presented placed them nowhere.
    hidden_unplaced: FxHashSet<CompositionSurfaceId>,
}

/// What [`WindowComposition::synchronize_platform_order`] last handed to the
/// platform about one surface.
#[derive(Clone, PartialEq)]
struct SynchronizedSurface {
    id: CompositionSurfaceId,
    parent: Option<CompositionSurfaceId>,
    window_bounds: Option<Bounds<DevicePixels>>,
}

/// The [`Window`] field holding its composition.
pub(crate) struct WindowCompositionHandle(Rc<RefCell<WindowCompositionState>>);

impl Default for WindowCompositionHandle {
    fn default() -> Self {
        let mut tree = CompositionTree::new();
        let base_surface = tree
            .insert(CompositionSurfaceKind::Gpui, None)
            .expect("a root can always be inserted");
        let overlay_surface = tree
            .insert(CompositionSurfaceKind::Gpui, None)
            .expect("a root can always be inserted");
        Self(Rc::new(RefCell::new(WindowCompositionState {
            tree,
            platform_surfaces: FxHashMap::default(),
            base_surface,
            overlay_surface,
            geometry_dirty: Rc::new(Cell::new(false)),
            enabled: false,
            refresh_pending: false,
            synchronized: None,
            placed_by_elements: FxHashSet::default(),
            hidden_unplaced: FxHashSet::default(),
        })))
    }
}

/// A platform surface whose bounds changes mark the window's composition
/// geometry for synchronization at the next present.
struct ManagedPlatformSurface {
    platform_surface: Rc<dyn PlatformSurfaceAttachment>,
    bounds: Cell<Bounds<DevicePixels>>,
    geometry_dirty: Rc<Cell<bool>>,
}

impl PlatformSurfaceAttachment for ManagedPlatformSurface {
    fn set_bounds(&self, bounds: Bounds<DevicePixels>) -> Result<()> {
        if self.bounds.get() == bounds {
            return Ok(());
        }
        self.platform_surface.set_bounds(bounds)?;
        self.bounds.set(bounds);
        self.geometry_dirty.set(true);
        Ok(())
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        self.bounds.get()
    }

    fn set_parent_origin(&self, origin: Point<DevicePixels>) -> Result<()> {
        self.platform_surface.set_parent_origin(origin)
    }

    fn compositor_recreated(&self, platform_context: &dyn Any) -> Result<()> {
        self.platform_surface.compositor_recreated(platform_context)
    }

    fn set_compositor_recreated_callback(
        &self,
        callback: Rc<dyn Fn(Box<dyn Any>) -> Result<()>>,
    ) -> Result<()> {
        self.platform_surface
            .set_compositor_recreated_callback(callback)
    }

    fn set_visible(&self, visible: bool) -> Result<()> {
        self.platform_surface.set_visible(visible)
    }

    fn platform_handle(&self) -> Result<Box<dyn Any>> {
        self.platform_surface.platform_handle()
    }
}

struct WindowCompositionSurfaceInner {
    id: CompositionSurfaceId,
    platform_surface: Option<Rc<dyn PlatformSurfaceAttachment>>,
}

/// A stable handle to a surface in a window composition tree.
#[derive(Clone)]
pub struct WindowCompositionSurface(Rc<WindowCompositionSurfaceInner>);

impl WindowCompositionSurface {
    /// Returns this surface's stable identity.
    pub fn id(&self) -> CompositionSurfaceId {
        self.0.id
    }

    /// Returns the platform surface used to attach native content.
    pub fn platform_surface(&self) -> Result<&dyn PlatformSurfaceAttachment> {
        self.0
            .platform_surface
            .as_deref()
            .context("this composition surface is rendered by GPUI")
    }
}

/// Controls the surface tree composited into a window.
pub struct WindowComposition<'a> {
    platform_window: &'a dyn PlatformWindow,
    state: Rc<RefCell<WindowCompositionState>>,
}

impl WindowComposition<'_> {
    /// Returns the GPUI surface initially rendered below native content.
    pub fn base_surface(&self) -> CompositionSurfaceId {
        self.state.borrow().base_surface
    }

    /// Returns the GPUI surface initially rendered above native content.
    pub fn overlay_surface(&self) -> CompositionSurfaceId {
        self.state.borrow().overlay_surface
    }

    /// Creates a platform-native surface as a root of the composition tree,
    /// below the overlay surface.
    pub fn create_native_surface(&self) -> Result<WindowCompositionSurface> {
        let platform_surface = self.platform_window.create_native_surface()?;
        self.register_platform_surface(CompositionSurfaceKind::Native, platform_surface)
    }

    /// Registers an externally produced GPU surface as a root of the
    /// composition tree, below the overlay surface.
    ///
    /// The producer owns the platform attachment and its synchronization. GPUI
    /// owns its placement in the window composition tree.
    pub fn create_external_gpu_surface(
        &self,
        platform_surface: Rc<dyn PlatformSurfaceAttachment>,
    ) -> Result<WindowCompositionSurface> {
        self.register_platform_surface(CompositionSurfaceKind::ExternalGpu, platform_surface)
    }

    /// Creates another GPUI-rendered surface in the composition tree. Paint
    /// onto it with [`Window::with_composition_surface`].
    pub fn create_gpui_surface(
        &self,
        parent: Option<CompositionSurfaceId>,
    ) -> Result<WindowCompositionSurface> {
        let id = self
            .state
            .borrow_mut()
            .tree
            .insert(CompositionSurfaceKind::Gpui, parent)?;
        if let Err(error) = self.synchronize_platform_order() {
            self.state.borrow_mut().tree.remove(id)?;
            self.synchronize_platform_order().log_err();
            return Err(error);
        }
        Ok(surface_handle(id, None))
    }

    /// Places `surface` immediately above `sibling`.
    pub fn place_above(
        &self,
        surface: CompositionSurfaceId,
        sibling: CompositionSurfaceId,
    ) -> Result<()> {
        anyhow::ensure!(
            surface != self.state.borrow().base_surface,
            "the GPUI base surface cannot be reordered"
        );
        self.change_tree(|tree| tree.place_above(surface, sibling))
    }

    /// Places `surface` immediately below `sibling`.
    pub fn place_below(
        &self,
        surface: CompositionSurfaceId,
        sibling: CompositionSurfaceId,
    ) -> Result<()> {
        let base_surface = self.state.borrow().base_surface;
        anyhow::ensure!(
            surface != base_surface && sibling != base_surface,
            "surfaces cannot be placed below or move the GPUI base surface"
        );
        self.change_tree(|tree| tree.place_below(surface, sibling))
    }

    /// Moves `surface` under a new parent, placing it above existing children.
    pub fn reparent(
        &self,
        surface: CompositionSurfaceId,
        parent: Option<CompositionSurfaceId>,
    ) -> Result<()> {
        anyhow::ensure!(
            surface != self.state.borrow().base_surface,
            "the GPUI base surface cannot be reparented"
        );
        self.change_tree(|tree| tree.reparent(surface, parent))
    }

    /// Removes a surface while preserving its children at the removed
    /// surface's previous position.
    pub fn remove_surface(&self, surface: CompositionSurfaceId) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if surface == state.base_surface || surface == state.overlay_surface {
            bail!("the default GPUI composition surfaces cannot be removed");
        }
        let previous_tree = state.tree.clone();
        let platform_surface = state.platform_surfaces.get(&surface).cloned();
        if let Some(platform_surface) = &platform_surface {
            platform_surface.set_visible(false)?;
        }
        state.tree.remove(surface)?;
        state.platform_surfaces.remove(&surface);
        state.placed_by_elements.remove(&surface);
        state.hidden_unplaced.remove(&surface);
        drop(state);
        if let Err(error) = self.synchronize_platform_order() {
            let mut state = self.state.borrow_mut();
            state.tree = previous_tree;
            if let Some(platform_surface) = platform_surface {
                platform_surface.set_visible(true).log_err();
                state.platform_surfaces.insert(surface, platform_surface);
            }
            drop(state);
            self.synchronize_platform_order().log_err();
            return Err(error);
        }
        Ok(())
    }

    /// Returns the renderer kind for a surface.
    pub fn surface_kind(&self, surface: CompositionSurfaceId) -> Result<CompositionSurfaceKind> {
        self.state.borrow().tree.kind(surface)
    }

    /// Returns a surface's parent.
    pub fn parent(&self, surface: CompositionSurfaceId) -> Result<Option<CompositionSurfaceId>> {
        self.state.borrow().tree.parent(surface)
    }

    /// Returns the ordered children of a surface, or the root surfaces when
    /// `parent` is `None`.
    pub fn children(
        &self,
        parent: Option<CompositionSurfaceId>,
    ) -> Result<Vec<CompositionSurfaceId>> {
        Ok(self.state.borrow().tree.children(parent)?.to_vec())
    }

    /// Applies `change` to the tree and the platform, or neither.
    fn change_tree(&self, change: impl FnOnce(&mut CompositionTree) -> Result<()>) -> Result<()> {
        let previous_tree = self.state.borrow().tree.clone();
        change(&mut self.state.borrow_mut().tree)?;
        if let Err(error) = self.synchronize_platform_order() {
            self.state.borrow_mut().tree = previous_tree;
            self.synchronize_platform_order().log_err();
            return Err(error);
        }
        Ok(())
    }

    fn register_platform_surface(
        &self,
        kind: CompositionSurfaceKind,
        platform_surface: Rc<dyn PlatformSurfaceAttachment>,
    ) -> Result<WindowCompositionSurface> {
        let initial_bounds = platform_surface.bounds();
        let geometry_dirty = self.state.borrow().geometry_dirty.clone();
        let platform_surface: Rc<dyn PlatformSurfaceAttachment> = Rc::new(ManagedPlatformSurface {
            platform_surface,
            bounds: Cell::new(initial_bounds),
            geometry_dirty,
        });
        let mut state = self.state.borrow_mut();
        let id = state.tree.insert(kind, None)?;
        let overlay_surface = state.overlay_surface;
        state.tree.place_below(id, overlay_surface)?;
        state.platform_surfaces.insert(id, platform_surface.clone());
        drop(state);
        if let Err(error) = self.synchronize_platform_order() {
            platform_surface.set_visible(false).log_err();
            let mut state = self.state.borrow_mut();
            state.platform_surfaces.remove(&id);
            state.tree.remove(id)?;
            drop(state);
            self.synchronize_platform_order().log_err();
            return Err(error);
        }
        Ok(surface_handle(id, Some(platform_surface)))
    }

    /// Hands the platform the tree's order and every surface's geometry.
    fn synchronize_platform_order(&self) -> Result<()> {
        let state = self.state.borrow();
        let mut origins = FxHashMap::default();
        let mut surfaces = Vec::new();
        for id in state.tree.flattened() {
            let parent = state.tree.parent(id)?;
            let parent_origin = parent
                .and_then(|parent| origins.get(&parent).copied())
                .unwrap_or_default();
            let content = match state.tree.kind(id)? {
                CompositionSurfaceKind::Gpui => PlatformCompositionSurfaceContent::Gpui,
                CompositionSurfaceKind::Native => PlatformCompositionSurfaceContent::Native(
                    state
                        .platform_surfaces
                        .get(&id)
                        .context("native composition surface is missing")?
                        .clone(),
                ),
                CompositionSurfaceKind::ExternalGpu => {
                    PlatformCompositionSurfaceContent::ExternalGpu(
                        state
                            .platform_surfaces
                            .get(&id)
                            .context("external GPU composition surface is missing")?
                            .clone(),
                    )
                }
            };
            let window_bounds = match &content {
                PlatformCompositionSurfaceContent::Gpui => None,
                PlatformCompositionSurfaceContent::Native(platform_surface)
                | PlatformCompositionSurfaceContent::ExternalGpu(platform_surface) => {
                    Some(platform_surface.bounds())
                }
            };
            let window_origin = window_bounds.map_or_else(Point::default, |bounds| bounds.origin);
            origins.insert(id, window_origin);
            surfaces.push(PlatformCompositionSurface {
                id,
                parent,
                window_origin,
                parent_origin,
                window_bounds,
                content,
            });
        }
        let synchronized: Vec<SynchronizedSurface> = surfaces
            .iter()
            .map(|surface| SynchronizedSurface {
                id: surface.id,
                parent: surface.parent,
                window_bounds: surface.window_bounds,
            })
            .collect();
        state.geometry_dirty.set(false);
        if state.synchronized.as_ref() == Some(&synchronized) {
            return Ok(());
        }
        self.platform_window.set_composition_order(&surfaces)?;
        drop(state);
        self.state.borrow_mut().synchronized = Some(synchronized);
        Ok(())
    }
}

fn surface_handle(
    id: CompositionSurfaceId,
    platform_surface: Option<Rc<dyn PlatformSurfaceAttachment>>,
) -> WindowCompositionSurface {
    WindowCompositionSurface(Rc::new(WindowCompositionSurfaceInner {
        id,
        platform_surface,
    }))
}

impl Window {
    /// Enables native window composition and returns its controller.
    ///
    /// GPUI's root scene is rendered below surfaces created by the controller,
    /// while deferred and window-level overlays are rendered above them.
    pub fn enable_window_composition(&self) -> Result<WindowComposition<'_>> {
        self.platform_window.enable_window_composition()?;
        let composition = WindowComposition {
            platform_window: self.platform_window.as_ref(),
            state: self.fast_composition.0.clone(),
        };
        composition.synchronize_platform_order()?;
        let mut state = composition.state.borrow_mut();
        if !state.enabled {
            state.enabled = true;
            state.refresh_pending = true;
            self.invalidator.set_dirty(true);
        }
        drop(state);
        Ok(composition)
    }

    /// Paints a scope of scene operations onto a GPUI surface in this window's
    /// composition tree, then restores the previous surface.
    pub fn with_composition_surface<R>(
        &mut self,
        surface: CompositionSurfaceId,
        paint: impl FnOnce(&mut Self) -> R,
    ) -> Result<R> {
        self.invalidator.debug_assert_paint();
        anyhow::ensure!(
            self.fast_composition.0.borrow().tree.kind(surface)? == CompositionSurfaceKind::Gpui,
            "paint operations can only target GPUI composition surfaces"
        );
        // Content painting into a scroll layer's scene switches there.
        let paint = match crate::fast::layers::paint::paint_onto_surface(self, surface, paint) {
            Ok(result) => return Ok(result),
            Err(paint) => paint,
        };
        let previous_surface = self
            .next_frame
            .fast_composition_starts
            .current()
            .context("composition paint scope has no base surface")?;
        switch_surface(self, surface);
        let result = paint(self);
        switch_surface(self, previous_surface);
        Ok(result)
    }
}

/// Begins a frame: the first one composed is drawn afresh, so that no view
/// drawn from an uncomposed frame brings subpixel text onto the overlay.
#[inline]
pub(crate) fn begin_frame(window: &mut Window) {
    let pending = mem::take(&mut window.fast_composition.0.borrow_mut().refresh_pending);
    if pending {
        window.refreshing = true;
    }
}

/// The surface painted onto now, if the window composes.
#[inline]
pub(crate) fn current_surface(window: &Window) -> Option<CompositionSurfaceId> {
    window.next_frame.fast_composition_starts.current()
}

pub(crate) fn switch_surface(window: &mut Window, surface: CompositionSurfaceId) {
    let scene_len = window.next_frame.scene.len();
    window
        .next_frame
        .fast_composition_starts
        .switch_to(surface, scene_len);
}

/// Paints what follows, the root view, onto the base surface.
#[inline]
pub(crate) fn begin_base(window: &mut Window) {
    if !window.fast_composition.0.borrow().enabled {
        return;
    }
    let surface = window.fast_composition.0.borrow().base_surface;
    switch_surface(window, surface);
}

/// Paints what follows, deferred and window-level draws, onto the overlay
/// surface.
#[inline]
pub(crate) fn begin_overlay(window: &mut Window) {
    if !window.fast_composition.0.borrow().enabled {
        return;
    }
    let surface = window.fast_composition.0.borrow().overlay_surface;
    switch_surface(window, surface);
}

/// Whether text painted now blends over native content and so is drawn in
/// grayscale: it is painted onto the overlay, or in the top-layer band that
/// goes to it.
#[inline]
pub(crate) fn paints_over_native_content(window: &Window) -> bool {
    let state = window.fast_composition.0.borrow();
    if !state.enabled {
        return false;
    }
    // Content painting into a scroll layer's scene is where its container is.
    let (surface, in_top_layer) = match crate::fast::layers::paint::painting_onto(window) {
        Some(onto) => (onto.surface, onto.in_top_layer),
        None => (window.next_frame.fast_composition_starts.current(), false),
    };
    surface == Some(state.overlay_surface) || in_top_layer || window.next_frame.scene.in_top_layer()
}

/// Carries the surface switches last frame made within `range` into the
/// frame being drawn, as [`Window::reuse_paint`] replays its scene, and
/// leaves painting on the surface it was on before.
#[inline]
pub(crate) fn reuse_starts(window: &mut Window, range: &Range<PaintIndex>) {
    let starts = &window.rendered_frame.fast_composition_starts.0;
    if starts.is_empty() {
        return;
    }
    let scenes = range.start.scene_index..range.end.scene_index;
    let first = starts.partition_point(|(_, start)| *start < scenes.start);
    let last = starts.partition_point(|(_, start)| *start < scenes.end);
    if first == last {
        return;
    }
    let Some(surface) = window.next_frame.fast_composition_starts.current() else {
        return;
    };
    let offset = window.next_frame.scene.len();
    for index in first..last {
        let (reused, start) = window.rendered_frame.fast_composition_starts.0[index];
        window
            .next_frame
            .fast_composition_starts
            .switch_to(reused, start - scenes.start + offset);
    }
    let end = offset + scenes.len();
    window
        .next_frame
        .fast_composition_starts
        .switch_to(surface, end);
}

/// Presents the frame last drawn: on its one surface, or split along the
/// composition tree once the window composes.
pub(crate) fn present(window: &mut Window) {
    let state = window.fast_composition.0.clone();
    if !state.borrow().enabled {
        window.platform_window.draw(&window.rendered_frame.scene);
        return;
    }
    show_placed_surfaces(
        &mut state.borrow_mut(),
        &window.rendered_frame.fast_native_surfaces.0,
    );
    if state.borrow().geometry_dirty.get() {
        let composition = WindowComposition {
            platform_window: window.platform_window.as_ref(),
            state: state.clone(),
        };
        if let Err(error) = composition.synchronize_platform_order() {
            log::error!("updating window composition geometry: {error:#}");
        }
    }
    let state = state.borrow();
    let scene = &window.rendered_frame.scene;
    let layers = composed_scene_layers(
        &state.tree,
        &window.rendered_frame.fast_composition_starts.0,
        scene,
        state.overlay_surface,
    )
    .unwrap_or_else(|error| {
        log::error!("invalid window composition scene: {error:#}");
        vec![ComposedSceneLayer {
            surface: state.base_surface,
            ranges: vec![0..scene.len()],
        }]
    });
    drop(state);
    window
        .platform_window
        .draw_composed(ComposedScene::new(scene, layers));
}

/// Shows the surfaces `placed` this frame and hides the others an element
/// placed before, so a native surface whose element is no longer drawn does
/// not stay on screen at its last bounds.
fn show_placed_surfaces(state: &mut WindowCompositionState, placed: &[CompositionSurfaceId]) {
    if placed.is_empty() && state.placed_by_elements.is_empty() {
        return;
    }
    let WindowCompositionState {
        platform_surfaces,
        placed_by_elements,
        hidden_unplaced,
        ..
    } = state;
    placed_by_elements.extend(
        placed
            .iter()
            .copied()
            .filter(|surface| platform_surfaces.contains_key(surface)),
    );
    for surface in placed_by_elements.iter() {
        let Some(platform_surface) = platform_surfaces.get(surface) else {
            continue;
        };
        let shown = placed.contains(surface);
        if shown == hidden_unplaced.contains(surface) {
            platform_surface.set_visible(shown).log_err();
            if shown {
                hidden_unplaced.remove(surface);
            } else {
                hidden_unplaced.insert(*surface);
            }
        }
    }
}

/// The native and external surfaces placed in a frame by [`native_surface`]
/// elements, in prepaint order. A view drawn from last frame carries the
/// placements it made, as [`reuse_placements`] copies them.
#[derive(Default)]
pub(crate) struct NativeSurfacePlacements(pub(crate) Vec<CompositionSurfaceId>);

impl NativeSurfacePlacements {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

/// Carries the native surfaces last frame placed within `range` into the
/// frame being drawn, as its prepaint is copied rather than run.
#[inline]
pub(crate) fn reuse_placements(window: &mut Window, range: &Range<PrepaintStateIndex>) {
    let placed = &window.rendered_frame.fast_native_surfaces.0
        [range.start.fast_native_surfaces_index..range.end.fast_native_surfaces_index];
    if !placed.is_empty() {
        window
            .next_frame
            .fast_native_surfaces
            .0
            .extend_from_slice(placed);
    }
}

/// Places `surface`, a native or external surface of the window's
/// composition, where the element is laid out.
///
/// The surface is shown in the frames that place it and hidden in the others.
/// GPUI's content masks do not clip it: a native surface inside a scroll
/// area shows past the area's edges unless its content clips itself.
pub fn native_surface(surface: WindowCompositionSurface) -> NativeSurface {
    NativeSurface {
        surface,
        style: StyleRefinement::default(),
    }
}

/// An element a native surface is placed at: see [`native_surface`]. Its
/// bounds are handed to the surface, in device pixels, as it is prepainted,
/// and only when they change; a view drawn from last frame carries the
/// placement.
pub struct NativeSurface {
    surface: WindowCompositionSurface,
    style: StyleRefinement,
}

impl Styled for NativeSurface {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for NativeSurface {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for NativeSurface {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        _: &mut App,
    ) {
        let scale_factor = window.scale_factor();
        let bounds = window
            .current_element_transform()
            .apply_to_scaled_bounds(bounds.scale(scale_factor))
            .map_origin(|origin| origin.round())
            .map_size(|size| size.round());
        let bounds = Bounds::new(
            Point::new(
                DevicePixels(bounds.origin.x.0 as i32),
                DevicePixels(bounds.origin.y.0 as i32),
            ),
            crate::size(
                DevicePixels(bounds.size.width.0 as i32),
                DevicePixels(bounds.size.height.0 as i32),
            ),
        );
        if let Ok(platform_surface) = self.surface.platform_surface() {
            platform_surface.set_bounds(bounds).log_err();
        }
        window
            .next_frame
            .fast_native_surfaces
            .0
            .push(self.surface.id());
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

/// The test platform's composition: what it was handed, recorded for tests
/// to read, with native surfaces that record their geometry.
#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
#[derive(Clone, Default)]
pub(crate) struct TestComposition(Rc<RefCell<TestCompositionState>>);

#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
#[derive(Default)]
pub(crate) struct TestCompositionState {
    pub(crate) enabled: bool,
    /// Each order handed over, bottom to top.
    pub(crate) orders: Vec<Vec<(CompositionSurfaceId, Option<Bounds<DevicePixels>>)>>,
    /// Each frame presented: every GPUI surface's scene, as its primitives
    /// in drawing order described without their draw order.
    pub(crate) presented: Vec<Vec<(CompositionSurfaceId, Vec<String>)>>,
    /// Each frame presented: every GPUI surface's input region.
    pub(crate) input_regions: Vec<Vec<(CompositionSurfaceId, Vec<Bounds<ScaledPixels>>)>>,
    /// Native surfaces created, in order.
    pub(crate) native_surfaces: Vec<Rc<TestNativeSurface>>,
    /// Whether the next native surface fails to be created.
    pub(crate) fail_next_surface: bool,
    /// Whether the platform reports composition unsupported, as a platform
    /// without it does.
    pub(crate) unsupported: bool,
}

#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
impl TestComposition {
    #[cfg(test)]
    pub(crate) fn state(&self) -> std::cell::RefMut<'_, TestCompositionState> {
        self.0.borrow_mut()
    }

    pub(crate) fn enable(&self) -> Result<()> {
        if self.0.borrow().unsupported {
            return unsupported("window composition");
        }
        self.0.borrow_mut().enabled = true;
        Ok(())
    }

    pub(crate) fn create_native_surface(&self) -> Result<Rc<dyn PlatformSurfaceAttachment>> {
        let mut state = self.0.borrow_mut();
        if std::mem::take(&mut state.fail_next_surface) {
            bail!("the test platform was asked to fail this surface");
        }
        let surface = Rc::new(TestNativeSurface::default());
        state.native_surfaces.push(surface.clone());
        Ok(surface)
    }

    pub(crate) fn record_order(&self, surfaces: &[PlatformCompositionSurface]) -> Result<()> {
        self.0.borrow_mut().orders.push(
            surfaces
                .iter()
                .map(|surface| (surface.id, surface.window_bounds))
                .collect(),
        );
        Ok(())
    }

    pub(crate) fn record_presented(&self, scene: &ComposedScene<'_>) {
        let layers = scene
            .layers()
            .iter()
            .map(|layer| {
                let lines = scene
                    .layer_scene(layer)
                    .map(|scene| described(&scene))
                    .unwrap_or_default();
                (layer.surface, lines)
            })
            .collect();
        let regions = scene
            .layers()
            .iter()
            .map(|layer| (layer.surface, scene.layer_input_region(layer)))
            .collect();
        let mut state = self.0.borrow_mut();
        state.presented.push(layers);
        state.input_regions.push(regions);
    }
}

/// `scene`'s primitives in drawing order, described without their draw order.
#[cfg(any(test, feature = "test-support"))]
fn described(scene: &crate::Scene) -> Vec<String> {
    crate::fast::verify::drawing_sequence(scene)
        .iter()
        .map(|drawn| crate::fast::verify::describe_primitive(&drawn.primitive))
        .collect()
}

/// A bench window presents frames nothing reads back.
#[cfg(all(feature = "bench-support", not(any(test, feature = "test-support"))))]
fn described(_scene: &crate::Scene) -> Vec<String> {
    Vec::new()
}

/// A native surface on the test platform.
#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
#[derive(Default)]
pub(crate) struct TestNativeSurface {
    pub(crate) bounds: Cell<Bounds<DevicePixels>>,
    pub(crate) bounds_writes: Cell<usize>,
    pub(crate) visible: Cell<bool>,
    /// Each visibility handed to the surface, in order.
    pub(crate) visibility_changes: RefCell<Vec<bool>>,
}

#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
impl PlatformSurfaceAttachment for TestNativeSurface {
    fn set_bounds(&self, bounds: Bounds<DevicePixels>) -> Result<()> {
        self.bounds_writes.set(self.bounds_writes.get() + 1);
        self.bounds.set(bounds);
        Ok(())
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        self.bounds.get()
    }

    fn set_parent_origin(&self, _: Point<DevicePixels>) -> Result<()> {
        Ok(())
    }

    fn set_visible(&self, visible: bool) -> Result<()> {
        self.visible.set(visible);
        self.visibility_changes.borrow_mut().push(visible);
        Ok(())
    }

    fn platform_handle(&self) -> Result<Box<dyn Any>> {
        Ok(Box::new(()))
    }
}
