//! Painting a scroll container's children into its layer: the scene swap,
//! the cull/clip split and inserting the tile quads (M3).
//!
//! A `div` that scrolls prepaints and paints its children through
//! [`begin_children`], [`prepaint_children`], [`end_children`] and
//! [`paint_children`]. They ask [`policy::decide`] what to do and do it:
//!
//! - `Bypass`: today's path.
//! - `Repaint`: the children are prepainted and painted as today, in window
//!   space at the current offset, but culled against the painted region (the
//!   viewport and its overscan) instead of the viewport, and painted into the
//!   layer's own scene, which is then stored in content space, diffed into
//!   tiles, and composited.
//! - `Composite`: the children are neither prepainted nor painted; the layer's
//!   tiles are composited at the new offset.

use crate::{
    App, AtlasTile, Bounds, ContentMask, Corners, DevicePixels, GlobalElementId, Overflow,
    PaintColor, Pixels, Point, PolychromeSprite, PrepaintStateIndex, ScaledPixels, Scene, Size,
    Style, TileCoord, TransformationMatrix, Window, WindowBackgroundAppearance,
    fast::{
        composition::CompositionSurfaceId,
        dependencies::{DependencyRecording, RenderDependencies},
        layers::{
            COMPILED, Layer, active, background,
            policy::{self, Decision},
            record::LayerRecord,
            scene::{
                LAYER_TILE_PREMULTIPLIED, LayerContent, LayerFrame, LayerKey, decode_layer_tile,
                layer_tile_id, layer_tile_texture_id, translate_primitive, visible_bounds,
            },
            tiles::{dirty_tiles, tile_hashes},
        },
    },
    point,
    scene::{PaintOperation, Primitive},
    size as size2,
};
use collections::FxHashMap;
use smallvec::SmallVec;
use std::{mem, ops::Range, rc::Rc};

/// The side of a tile, in device pixels.
pub(crate) const TILE_SIZE: u32 = 512;

/// The layer whose content is being prepainted or painted.
pub(crate) struct Painting {
    /// The scroll container.
    pub(crate) id: GlobalElementId,
    /// The container's clip rect, in window space.
    pub(crate) viewport: Bounds<Pixels>,
    /// The part of the content painted, in window space: the viewport and
    /// the overscan around it, within the content.
    pub(crate) painted_region: Bounds<Pixels>,
    /// Where the content lies, in window space: from the container's origin,
    /// where its padding and its first child's margin begin, to the far
    /// corner of its children.
    pub(crate) content_bounds: Bounds<Pixels>,
    /// While the content paints, the frame's scene, swapped out for the
    /// layer's; otherwise the layer's scene, in window space.
    pub(crate) scene: Scene,
    /// The offset the content is painted at, snapped.
    pub(crate) scroll_offset: Point<Pixels>,
    /// The layer's translation, `scroll_offset` in whole device pixels.
    pub(crate) translation: Point<ScaledPixels>,
    /// What prepainting the content added to the frame.
    pub(crate) prepaint_range: Range<PrepaintStateIndex>,
    /// The recording of what prepainting the content reads, while it runs.
    pub(crate) recording: Option<DependencyRecording>,
    /// What prepainting the content read.
    pub(crate) dependencies: RenderDependencies,
    /// What routing input into the content takes from painting it.
    pub(crate) input: crate::fast::layers::input::PaintingInput,
    /// How the views prepainted inside the content were laid out.
    pub(crate) view_layouts: FxHashMap<GlobalElementId, crate::fast::layers::reuse::KeptLayout>,
    /// The composition surfaces the content switched to while it painted
    /// into the layer's scene, each with the length that scene had: content
    /// on another surface is no tile's, so the content is drawn into the
    /// frame with these switches (see [`paint_onto_surface`]).
    pub(crate) surface_switches: Vec<(usize, CompositionSurfaceId)>,
    /// Where the content paints, while it paints into the layer's scene.
    pub(crate) onto: Option<PaintingOnto>,
}

/// What the frame's scene and surface starts tell of where content paints,
/// kept for content painting into a layer's scene instead.
#[derive(Clone, Copy)]
pub(crate) struct PaintingOnto {
    /// The composition surface painted onto, if the window composes.
    pub(crate) surface: Option<CompositionSurfaceId>,
    /// Whether the container is painted in the frame's top-layer band.
    pub(crate) in_top_layer: bool,
}

/// What a layer's content inherited from where it was painted that its
/// prepaint does not see: the opacity and transform of the elements around
/// it, and whether its text blends over native content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintedWith {
    context: crate::window::ElementPaintContext,
    over_native_content: bool,
}

impl PaintedWith {
    pub(crate) fn current(window: &Window) -> Self {
        PaintedWith {
            context: window.element_paint_context(),
            over_native_content: crate::fast::composition::paints_over_native_content(window),
        }
    }
}

/// Where the layer being painted into its own scene paints, if one is.
#[inline]
pub(crate) fn painting_onto(window: &Window) -> Option<PaintingOnto> {
    window.fast_layers.painting.as_ref()?.onto
}

/// Paints `paint` onto `surface` for content painting into a layer's scene,
/// where the frame's surface starts do not reach: the switches are kept for
/// the content to be drawn into the frame with. `None` when no layer's
/// scene is being painted into.
pub(crate) fn paint_onto_surface<R, F: FnOnce(&mut Window) -> R>(
    window: &mut Window,
    surface: CompositionSurfaceId,
    paint: F,
) -> Result<R, F> {
    let scene_len = window.next_frame.scene.len();
    let Some(painting) = window.fast_layers.painting.as_mut() else {
        return Err(paint);
    };
    let Some(onto) = painting.onto.as_mut() else {
        return Err(paint);
    };
    let Some(previous) = onto.surface else {
        return Err(paint);
    };
    onto.surface = Some(surface);
    painting.surface_switches.push((scene_len, surface));
    let result = paint(window);
    let scene_len = window.next_frame.scene.len();
    if let Some(painting) = window.fast_layers.painting.as_mut() {
        if let Some(onto) = painting.onto.as_mut() {
            onto.surface = Some(previous);
        }
        painting.surface_switches.push((scene_len, previous));
    }
    Ok(result)
}

/// What a container's prepaint decided, for its paint to carry out.
pub(crate) enum Prepainted {
    /// The content was prepainted into the layer, and is painted into it.
    Repaint(Painting),
    /// The content was not prepainted; the layer's tiles are composited.
    #[allow(dead_code, reason = "the input stream reads the viewport")]
    Composite {
        viewport: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
    },
}

/// What a container decided for its children this frame, from
/// [`begin_children`] to [`end_children`].
#[derive(Clone, Copy)]
pub(crate) struct Children {
    decision: Decision,
    scroll_offset: Point<Pixels>,
}

impl Children {
    /// The offset the children are prepainted at: the container's, snapped
    /// to whole device pixels while its window's scroll layers are on.
    #[inline(always)]
    pub(crate) fn scroll_offset(&self) -> Point<Pixels> {
        self.scroll_offset
    }

    /// Whether the children are prepainted: not when their layer is
    /// composited.
    #[inline(always)]
    pub(crate) fn prepainted(&self) -> bool {
        self.decision != Decision::Composite
    }
}

/// Whether the content of a layer is being prepainted or painted: nested
/// retained views then record nothing and reuse nothing, the layer being
/// their retention (spec §6.3).
#[inline]
pub(crate) fn inside_layer(window: &Window) -> bool {
    window.fast_layers.painting.is_some()
}

/// Decides what the container `id`, prepainting its children at
/// `scroll_offset`, does with them, and sets up their prepaint for it.
/// `bounds` are the container's, `child_min` and `content_size` where its
/// children lie before scrolling. `displaced` is whether sticky or fixed
/// positioning moved the container from where it was laid out, which moves
/// its content with it besides the scroll.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn begin_children(
    window: &mut Window,
    cx: &mut App,
    id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    child_min: Point<Pixels>,
    content_size: Size<Pixels>,
    scroll_offset: Point<Pixels>,
    style: &Style,
    displaced: bool,
) -> Children {
    // Nearly every div: it does not scroll, and has no offset to snap.
    if !scrolls(style) && scroll_offset == Point::default() {
        return Children {
            decision: Decision::Bypass,
            scroll_offset,
        };
    }
    begin_scrolling_children(
        window,
        cx,
        id,
        bounds,
        child_min,
        content_size,
        scroll_offset,
        style,
        displaced,
    )
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn begin_scrolling_children(
    window: &mut Window,
    cx: &mut App,
    id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    child_min: Point<Pixels>,
    content_size: Size<Pixels>,
    scroll_offset: Point<Pixels>,
    style: &Style,
    displaced: bool,
) -> Children {
    // With the switch off nothing is snapped: output is today's.
    if !window.fast_mode.snapping() {
        return Children {
            decision: Decision::Bypass,
            scroll_offset,
        };
    }
    let scroll_offset = snap_scroll_offset(window, scroll_offset);
    let bypass = Children {
        decision: Decision::Bypass,
        scroll_offset,
    };
    if !window.fast_mode.scroll_layers() {
        return bypass;
    }
    let Some(id) = id else {
        return bypass;
    };
    if !scrolls(style) {
        return bypass;
    }
    if inside_layer(window) {
        policy::count_bypass(window, policy::Bypass::Nested);
        return bypass;
    }
    if !active(window, cx) {
        policy::count_bypass(window, policy::Bypass::Inactive);
        return bypass;
    }
    // A container sticky or fixed positioning displaced moves with what
    // displaced it, not only with its scroll offset.
    if displaced {
        policy::count_bypass(window, policy::Bypass::Moved);
        return bypass;
    }
    // A layer's tiles are composited clipped to a rectangle, and only under
    // the unit transform.
    let mask = window.content_mask();
    if mask.corner_radii != Corners::default()
        || window.current_element_transform() != TransformationMatrix::unit()
        || !on_device_pixels(mask.bounds, window.scale_factor())
    {
        policy::count_bypass(window, policy::Bypass::Ineligible);
        return bypass;
    }
    let viewport = mask.bounds;
    let decision = policy::decide(window, cx, id, bounds, content_size, scroll_offset);
    let mut decision = crate::fast::layers::input::decide(window, id, decision, scroll_offset);
    if decision == Decision::Composite
        && window
            .fast_layers
            .layers
            .get(id)
            .is_none_or(|layer| layer.record.is_none())
    {
        decision = Decision::Repaint;
    }
    match decision {
        Decision::Bypass => return bypass,
        Decision::Composite => {
            layer_mut(window, id).prepainted = Some(Prepainted::Composite {
                viewport,
                scroll_offset,
            });
            crate::fast::layers::reuse::carry_prepaint(window, id, viewport, scroll_offset);
        }
        Decision::Repaint => {
            // The content is everything the container scrolls over: its
            // padding and its first child's margin scroll with the children,
            // and the layer covers them like the rest (`policy::covers`).
            let scroll_max = crate::Interactivity::scroll_max_for_bounds(
                content_size,
                bounds.size,
                style,
                window.rem_size(),
                window.scale_factor(),
            );
            let scrolled_over = bounds.bottom_right() + scroll_max;
            let content = if child_min.x == Pixels::MAX {
                Bounds::from_corners(
                    bounds.origin + scroll_offset,
                    scrolled_over
                        .max(&(bounds.origin + point(content_size.width, content_size.height)))
                        + scroll_offset,
                )
            } else {
                let children_end = child_min + point(content_size.width, content_size.height);
                Bounds::from_corners(
                    bounds.origin.min(&child_min) + scroll_offset,
                    scrolled_over.max(&children_end) + scroll_offset,
                )
            };
            let painted_region = painted_region(viewport, content, style.overflow);
            let start = window.prepaint_index();
            window.fast_layers.painting = Some(Painting {
                id: id.clone(),
                viewport,
                painted_region,
                content_bounds: content,
                scene: Scene::default(),
                scroll_offset,
                translation: translation(window, scroll_offset),
                prepaint_range: start.clone()..start,
                recording: Some(cx.begin_recording_dependencies()),
                dependencies: RenderDependencies::default(),
                input: Default::default(),
                view_layouts: FxHashMap::default(),
                surface_switches: Vec::new(),
                onto: None,
            });
            // Culling works in the painted region, not in the viewport and
            // whatever clips it; the composite clips to those.
            window
                .content_mask_stack
                .push(ContentMask::rectangular(painted_region));
        }
    }
    Children {
        decision,
        scroll_offset,
    }
}

/// The content mask as it is without a layer: while a layer's content is
/// prepainted or painted the mask is the painted region, the viewport and
/// its overscan, for culling; what reads the mask for geometry or keeps it
/// for later (a fixed element's containing block, a deferred draw's clip)
/// takes the viewport's.
#[inline]
pub(crate) fn unlayered_content_mask(window: &Window) -> ContentMask<Pixels> {
    let mask = window.content_mask();
    match window.fast_layers.painting.as_ref() {
        Some(painting) => ContentMask {
            bounds: mask.bounds.intersect(&painting.viewport),
            corner_radii: mask.corner_radii,
        },
        None => mask,
    }
}

/// Whether the edges of `bounds` lie on device pixels at `scale_factor`.
///
/// gpux clips quads to the device pixels covering a content mask and
/// glyphs, underlines and paths to the mask itself. The two are one clip,
/// which a layer's tile quads can stand in for, only where the mask lies on
/// device pixels: laid-out bounds do, a window whose size does not (as
/// while it is resized by a fraction of a pixel) clips what reaches its
/// edge between them.
///
/// An edge a few units in the last place off a device pixel is on it: a
/// window's size in pixels is its size in device pixels divided by the scale
/// factor, and scaling it back can miss by as much.
pub(crate) fn on_device_pixels(bounds: Bounds<Pixels>, scale_factor: f32) -> bool {
    let whole = |value: Pixels| {
        let device = value.0 * scale_factor;
        (device - device.round()).abs() <= device.abs().max(1.) * f32::EPSILON * 4.
    };
    let corner = bounds.bottom_right();
    whole(bounds.origin.x) && whole(bounds.origin.y) && whole(corner.x) && whole(corner.y)
}

/// Ends what [`begin_children`] began, once the children are prepainted.
#[inline(always)]
pub(crate) fn end_children(window: &mut Window, cx: &mut App, children: Children) {
    if children.decision != Decision::Repaint {
        return;
    }
    end_repainted_children(window, cx);
}

#[inline(never)]
fn end_repainted_children(window: &mut Window, cx: &mut App) {
    window.content_mask_stack.pop();
    let Some(mut painting) = window.fast_layers.painting.take() else {
        debug_assert!(false, "a layer's prepaint ended without beginning");
        return;
    };
    painting.prepaint_range.end = window.prepaint_index();
    if let Some(recording) = painting.recording.take() {
        painting.dependencies = cx.finish_recording_dependencies(recording).all;
    }
    let id = painting.id.clone();
    layer_mut(window, &id).prepainted = Some(Prepainted::Repaint(painting));
}

/// Paints a container's children as its prepaint decided: `f` is the
/// children's paint.
#[inline(always)]
pub(crate) fn paint_children(
    window: &mut Window,
    cx: &mut App,
    id: Option<&GlobalElementId>,
    f: impl FnOnce(&mut Window, &mut App),
) {
    if window.fast_layers.layers.is_empty() {
        return f(window, cx);
    }
    paint_children_of_layers(window, cx, id, f)
}

#[inline(never)]
fn paint_children_of_layers(
    window: &mut Window,
    cx: &mut App,
    id: Option<&GlobalElementId>,
    f: impl FnOnce(&mut Window, &mut App),
) {
    let prepainted = match id {
        Some(id) if !window.fast_layers.layers.is_empty() => window
            .fast_layers
            .layers
            .get_mut(id)
            .and_then(|layer| layer.prepainted.take()),
        _ => None,
    };
    match prepainted {
        None => f(window, cx),
        // A transform applied only while painting is not seen by the
        // container's prepaint: its content is painted straight into the
        // frame, as content whose background cannot be baked is.
        Some(Prepainted::Repaint(painting))
            if window.current_element_transform() != TransformationMatrix::unit() =>
        {
            policy::count_bypass(window, policy::Bypass::Ineligible);
            paint_unbaked(window, cx, painting, f)
        }
        Some(Prepainted::Repaint(painting)) => match bake_background(window) {
            Some(background) => repaint(window, cx, painting, background, f),
            None => {
                policy::count_bypass(window, policy::Bypass::Ineligible);
                paint_unbaked(window, cx, painting, f)
            }
        },
        Some(Prepainted::Composite { scroll_offset, .. }) => {
            if let Some(id) = id {
                crate::fast::layers::reuse::carry_paint(window, id);
                note_composited_hovers(window, id);
                composite(window, id, scroll_offset);
                repaint_if_painted_otherwise(window, id);
            }
        }
    }
}

/// Adds the hovers the content of `id` was painted by to the recordings
/// around the container compositing it, which depend on them too.
fn note_composited_hovers(window: &Window, id: &GlobalElementId) {
    if let Some(record) = window
        .fast_layers
        .layers
        .get(id)
        .and_then(|layer| layer.record.as_ref())
    {
        crate::fast::dependencies::note_replayed_hovers(window, &record.hovers);
        // Prepaint composited it only with the pointer outside its viewport.
        debug_assert!(
            crate::fast::dependencies::hovers_unchanged(window, &record.hovers),
            "a composited layer's hovers changed after prepaint"
        );
    }
}

/// An ancestor that applies opacity or a transform only while painting, or
/// moves the container onto the overlay, can change after the container's
/// prepaint decided to composite its layer, so this frame shows what the
/// layer painted before: it is painted afresh on the next, as a view drawn
/// from last frame is built on the next (`retained::reuse_paint`).
pub(crate) fn repaint_if_painted_otherwise(window: &mut Window, id: &GlobalElementId) {
    let now = PaintedWith::current(window);
    let Some(layer) = window.fast_layers.layers.get_mut(id) else {
        return;
    };
    if layer
        .record
        .as_ref()
        .is_none_or(|record| record.painted_with == Some(now))
    {
        return;
    }
    layer.input.rebuild = true;
    let owner = layer
        .input
        .owner
        .as_ref()
        .and_then(crate::fast::retained::view_entity);
    match owner {
        Some(owner) => window.on_next_frame(move |_, cx| cx.notify(owner)),
        None => window.on_next_frame(|window, _| window.refresh()),
    }
}

/// The opaque colour a layer's tiles are cleared with: what the frame has
/// painted so far under the viewport, the current content mask, if it is
/// one solid quad covering it (spec §5.2).
pub(crate) fn bake_background(window: &Window) -> Option<PaintColor> {
    let window_opaque =
        window.platform_window.background_appearance() == WindowBackgroundAppearance::Opaque;
    let viewport = window.snapped_content_mask().bounds;
    background::bake(&window.next_frame.scene, viewport, window_opaque)
}

/// Paints content prepainted for a layer straight into the frame, as
/// without one, because the layer's tiles could not be cleared with what is
/// under them. The layer keeps no content, so that it is painted afresh.
fn paint_unbaked(
    window: &mut Window,
    cx: &mut App,
    painting: Painting,
    f: impl FnOnce(&mut Window, &mut App),
) {
    let id = painting.id.clone();
    // Nested views were prepainted inside the layer, and are painted so.
    window.fast_layers.painting = Some(painting);
    f(window, cx);
    window.fast_layers.painting = None;
    crate::fast::layers::policy::defer_unbaked(window, &id);
}

/// Carries out a `Composite` decision for the container `id`, scrolled to
/// `scroll_offset`: the layer's content stands, cleared with the background
/// now under it, and is composited at the offset.
fn composite(window: &mut Window, id: &GlobalElementId, scroll_offset: Point<Pixels>) {
    let translation = translation(window, scroll_offset);
    composite_at(window, id, translation);
}

/// Composites the layer of the container `id` with its content moved by
/// `translation` into window space, its content standing, cleared with the
/// background now under it.
pub(crate) fn composite_at(
    window: &mut Window,
    id: &GlobalElementId,
    translation: Point<ScaledPixels>,
) {
    let background = bake_background(window);
    let layer = layer_mut(window, id);
    let Some(current) = layer.record.as_ref().map(|record| record.background) else {
        return;
    };
    if background.is_some_and(|background| background != current) {
        layer.next_generation();
    }
    let generation = layer.generation;
    let record = layer.record.as_mut().expect("checked above");
    let mut dirtied = 0;
    match background {
        Some(background) if background == record.background => {}
        Some(background) => {
            // Every tile is cleared with another colour: a new generation of
            // the same content, all of it dirty.
            record.background = background;
            record.generation = generation;
            record.dirty_tiles = all_tiles(&record.tile_hashes);
            dirtied = record.dirty_tiles.len();
        }
        None => {
            // The tiles cannot be cleared with what is under them now: the
            // content is drawn into the frame, and painted afresh next time.
            let record = layer.record.take().expect("checked above");
            crate::fast::layers::policy::defer_unbaked(window, id);
            policy::count_bypass(window, policy::Bypass::Ineligible);
            draw_into_frame(window, record.content.operations(), translation);
            let viewport = window.snapped_content_mask().bounds;
            insert_paths(
                &mut window.next_frame.scene,
                &record.paths,
                translation,
                viewport,
            );
            return;
        }
    }
    if insert_layer(window, id, translation, dirtied) {
        crate::fast::fast_stat!(window.fast_stats.frame, layers_composited += 1);
    }
}

/// Composites the layer of the container `id` into the frame at
/// `translation`, `dirtied` of its tiles new this frame: its tile quads
/// over the viewport, the current content mask, and its [`LayerFrame`].
/// Content holding paths is drawn into the frame instead. Returns whether
/// the tiles were inserted.
pub(crate) fn insert_layer(
    window: &mut Window,
    id: &GlobalElementId,
    translation: Point<ScaledPixels>,
    _dirtied: usize,
) -> bool {
    let viewport = window.snapped_content_mask().bounds;
    let Some(layer) = window.fast_layers.layers.get(id) else {
        return false;
    };
    let Some(record) = layer.record.as_ref() else {
        return false;
    };
    if record.has_paths {
        let content = record.content.clone();
        draw_into_frame(window, content.operations(), translation);
        return false;
    }
    let _composited = insert_tile_quads(&mut window.next_frame.scene, layer, viewport, translation);
    insert_paths(
        &mut window.next_frame.scene,
        &record.paths,
        translation,
        viewport,
    );
    crate::fast::fast_stat!(window.fast_stats.frame, tiles_rasterized += _dirtied);
    crate::fast::fast_stat!(window.fast_stats.frame, tiles_composited += _composited);
    policy::note_work(window, id, 0.);
    true
}

/// Inserts into `scene` the quads of the tiles of `layer` that show in
/// `viewport` with its content moved by `translation`, all at one draw
/// order, and the layer's [`LayerFrame`] they are rasterized from (spec
/// §5.1). Returns how many quads that is.
pub(crate) fn insert_tile_quads(
    scene: &mut Scene,
    layer: &Layer,
    viewport: Bounds<ScaledPixels>,
    translation: Point<ScaledPixels>,
) -> usize {
    let Some(record) = layer.record.as_ref() else {
        return 0;
    };
    if viewport.is_empty() {
        return 0;
    }
    let frame = LayerFrame {
        key: layer.key,
        generation: record.generation,
        background: record.background,
        tile_size: TILE_SIZE,
        content: record.content.clone(),
        dirty_tiles: record.dirty_tiles.clone(),
    };
    let size = TILE_SIZE as f32;
    let content_viewport = Bounds {
        origin: viewport.origin - translation,
        size: viewport.size,
    };
    let min = content_viewport.origin;
    let max = content_viewport.bottom_right();
    let (x0, y0) = (
        (min.x.0 / size).floor() as i32,
        (min.y.0 / size).floor() as i32,
    );
    let (x1, y1) = (
        (max.x.0 / size).ceil() as i32,
        (max.y.0 / size).ceil() as i32,
    );
    scene.push_layer(viewport);
    let texture_id = layer_tile_texture_id(layer.key);
    for y in y0..y1 {
        for x in x0..x1 {
            let coord = TileCoord { x, y };
            let tile = frame.tile_bounds(coord);
            scene.insert_primitive(PolychromeSprite {
                order: 0,
                pad: LAYER_TILE_PREMULTIPLIED,
                grayscale: false,
                opacity: 1.,
                bounds: Bounds {
                    origin: tile.origin + translation,
                    size: tile.size,
                },
                content_mask: ContentMask::rectangular(viewport),
                corner_radii: Corners::default(),
                transformation: TransformationMatrix::unit(),
                tile: AtlasTile {
                    texture_id,
                    tile_id: layer_tile_id(coord),
                    padding: 0,
                    bounds: Bounds {
                        origin: Point::default(),
                        size: size2(
                            DevicePixels(TILE_SIZE as i32),
                            DevicePixels(TILE_SIZE as i32),
                        ),
                    },
                },
            });
        }
    }
    scene.pop_layer();
    scene.layers.frames.push(frame);
    ((x1 - x0).max(0) * (y1 - y0).max(0)) as usize
}

/// Inserts into `scene` the `paths` a layer keeps apart from its tiles,
/// moved by `translation` and clipped to `viewport`, after its tile quads,
/// so that they are drawn over them (see [`lift_paths`]).
fn insert_paths(
    scene: &mut Scene,
    paths: &[crate::Path<ScaledPixels>],
    translation: Point<ScaledPixels>,
    viewport: Bounds<ScaledPixels>,
) {
    for path in paths {
        let mut primitive = translate_primitive(&Primitive::Path(path.clone()), translation);
        clip_primitive(&mut primitive, &viewport);
        scene.insert_primitive(primitive);
    }
}

/// `content`, a layer's content scene, split into the scene its tiles are
/// rasterized from and the paths it painted, in drawing order, when nothing
/// it draws after a path overlaps that path: the paths can then be drawn
/// over the tiles, in the frame, and every pixel is drawn over in the order
/// it would be without a layer. Paths are never rasterized into tiles (spec
/// §5.6). `Err(content)` when something is drawn over a path.
pub(crate) fn lift_paths(
    content: Scene,
) -> Result<(Scene, Rc<[crate::Path<ScaledPixels>]>), Box<Scene>> {
    if content.paths.is_empty() {
        return Ok((content, Rc::from([])));
    }
    // Every primitive in the order the renderer draws them: by draw order,
    // then by kind, as `BatchIterator` does, then as the scene sorted them.
    let mut sequence: Vec<((u32, u8, usize), Bounds<ScaledPixels>, bool)> = Vec::new();
    macro_rules! gather {
        ($field:ident, $rank:expr) => {
            sequence.extend(content.$field.iter().enumerate().map(|(ix, p)| {
                (
                    (p.order, $rank, ix),
                    p.bounds.intersect(&p.content_mask.bounds),
                    false,
                )
            }))
        };
    }
    gather!(shadows, 0);
    gather!(quads, 1);
    gather!(underlines, 3);
    gather!(monochrome_sprites, 4);
    gather!(subpixel_sprites, 5);
    gather!(polychrome_sprites, 6);
    gather!(surfaces, 7);
    // A path's antialiased edge may reach into the pixels around its bounds.
    let edge = ScaledPixels(1.);
    sequence.extend(content.paths.iter().enumerate().map(|(ix, p)| {
        let clipped = p.bounds.intersect(&p.content_mask.bounds);
        let reach = Bounds::from_corners(
            clipped.origin - point(edge, edge),
            clipped.bottom_right() + point(edge, edge),
        );
        ((p.order, 2, ix), reach, true)
    }));
    sequence.sort_by_key(|(key, _, _)| *key);
    let mut paths_so_far: Vec<Bounds<ScaledPixels>> = Vec::new();
    for (_, bounds, is_path) in &sequence {
        if *is_path {
            paths_so_far.push(*bounds);
        } else if paths_so_far.iter().any(|path| path.intersects(bounds)) {
            return Err(Box::new(content));
        }
    }
    let paths: Rc<[crate::Path<ScaledPixels>]> = content.paths.iter().cloned().collect();
    let mut rest = Scene::default();
    for operation in &content.paint_operations {
        match operation {
            PaintOperation::Primitive(Primitive::Path(_)) => {}
            PaintOperation::Primitive(primitive) => rest.insert_primitive(primitive.clone()),
            PaintOperation::FastDropped(dropped) => {
                crate::fast::edge_shadows::dropped(&mut rest, *dropped)
            }
            PaintOperation::FastClip(clip) => crate::fast::edge_shadows::clip_op(&mut rest, *clip),
            PaintOperation::StartLayer(bounds) => rest.push_layer(*bounds),
            PaintOperation::StartPositionedLayer(bounds) => rest.push_positioned_layer(*bounds),
            PaintOperation::StartTopLayer(bounds) => rest.push_top_layer(*bounds),
            PaintOperation::EndLayer => rest.pop_layer(),
        }
    }
    rest.finish();
    Ok((rest, paths))
}

/// How finely [`snap_path`] places path vertices: 256 steps a device pixel,
/// as fine as a GPU rasterizer resolves a vertex's position.
const PATH_GRID: f32 = 256.;

/// `path`, in device pixels, with its vertices and bounds moved to the
/// nearest 1/256 of a device pixel where layers are compiled.
///
/// A path painted into a layer is drawn moved by whole device pixels, while
/// drawn without a layer it is painted where it is now: at a fractional
/// scale factor the two differ by rounding error in the last bits of each
/// position (a snapped scroll offset is not a whole number of logical
/// pixels). On the grid they land on the same positions.
pub(crate) fn snap_path(mut path: crate::Path<ScaledPixels>) -> crate::Path<ScaledPixels> {
    if !COMPILED {
        return path;
    }
    // Adding and taking away 1.5 × 2^23 rounds an `f32` below 2^22 in
    // magnitude to a whole number, ties to even, which a shift by whole
    // pixels (a multiple of 256 steps) keeps; `floor` would call into libm.
    const ROUNDER: f32 = 12_582_912.;
    let snap = |value: ScaledPixels| {
        let steps = value.0 * PATH_GRID;
        if steps.abs() < 4_194_304. {
            ScaledPixels(((steps + ROUNDER) - ROUNDER) / PATH_GRID)
        } else {
            value
        }
    };
    let snap_point = |point: Point<ScaledPixels>| Point {
        x: snap(point.x),
        y: snap(point.y),
    };
    for vertex in &mut path.vertices {
        vertex.xy_position = snap_point(vertex.xy_position);
    }
    path.start = snap_point(path.start);
    path.current = snap_point(path.current);
    path.bounds = Bounds::from_corners(
        snap_point(path.bounds.origin),
        snap_point(path.bounds.bottom_right()),
    );
    path
}

/// Whether `scene`, a layer's content as it was painted, holds something no
/// tile can hold, which is drawn into the frame instead: gpux's visual
/// effects (which sample what is under them), screen-effect regions and
/// requests, HTML islands and elements (which the DOM draws), surfaces
/// (whose pixels come from outside the scene), and whatever is painted in
/// the positioned or top layer (fixed and sticky content, which does not
/// scroll with the rest, and chrome above it).
pub(crate) fn uncompositable(scene: &Scene) -> bool {
    !scene.visual_effects.is_empty()
        || !scene.html_islands.is_empty()
        || !scene.html_elements.is_empty()
        || !scene.screen_effect_regions.is_empty()
        || !scene.surfaces.is_empty()
        || !scene.active_screen_effects.is_empty()
        || !scene.active_screen_effect_snapshots.is_empty()
        || scene.paint_operations.iter().any(|operation| {
            matches!(
                operation,
                PaintOperation::StartPositionedLayer(_) | PaintOperation::StartTopLayer(_)
            )
        })
}

/// Moves what `from`, a layer's scene, holds besides its paint operations
/// (the screen effects requested while the content was painted) to `to`,
/// the frame's scene, where painting without a layer would have put it.
pub(crate) fn take_scene_requests(from: &mut Scene, to: &mut Scene) {
    to.active_screen_effects
        .append(&mut from.active_screen_effects);
    to.active_screen_effect_snapshots
        .append(&mut from.active_screen_effect_snapshots);
}

/// `path` as `window` paints it: snapped ([`snap_path`]) while its scroll
/// layers are on, and as it is otherwise.
#[inline]
pub(crate) fn snapped_path(
    window: &Window,
    path: crate::Path<ScaledPixels>,
) -> crate::Path<ScaledPixels> {
    if window.fast_mode.snapping() {
        snap_path(path)
    } else {
        path
    }
}

/// Carries into `scene` the [`LayerFrame`]s of the tile quads that
/// `scene.replay` copies from the operations `range` of `previous`, so
/// that a retained subtree holding a layer composites it when it is reused.
pub(crate) fn replay_layers(scene: &mut Scene, range: Range<usize>, previous: &Scene) {
    if previous.layers.frames.is_empty() {
        return;
    }
    for operation in &previous.paint_operations[range] {
        let PaintOperation::Primitive(Primitive::PolychromeSprite(sprite)) = operation else {
            continue;
        };
        let Some((key, _)) = decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id) else {
            continue;
        };
        if scene.layers.frames.iter().any(|frame| frame.key == key) {
            continue;
        }
        if let Some(frame) = previous.layers.frames.iter().find(|frame| frame.key == key) {
            scene.layers.frames.push(frame.clone());
        }
    }
}

/// Draws the primitives of `content`, moved by `delta` into window space,
/// straight into the frame, clipped to the viewport, the current content
/// mask: what painting the content into the frame would have drawn.
pub(crate) fn draw_into_frame<'a>(
    window: &mut Window,
    operations: impl IntoIterator<Item = &'a PaintOperation>,
    delta: Point<ScaledPixels>,
) {
    let viewport = window.snapped_content_mask().bounds;
    let logical_viewport = window.content_mask().bounds;
    replay_content(
        &mut window.next_frame.scene,
        operations,
        delta,
        viewport,
        logical_viewport,
    );
}

/// Replays `operations`, a layer's content, into `scene`, moved by `delta`
/// and clipped to `viewport`, as painting the content there would have left
/// it: a layer the viewport clips away entirely is not begun, as
/// [`Window::paint_layer`] begins none, and what is painted in it is dropped
/// with every other primitive clipped away.
pub(crate) fn replay_content<'a>(
    scene: &mut Scene,
    operations: impl IntoIterator<Item = &'a PaintOperation>,
    delta: Point<ScaledPixels>,
    viewport: Bounds<ScaledPixels>,
    logical_viewport: Bounds<Pixels>,
) {
    // Whether each layer begun in the content was begun in the scene.
    let mut begun: SmallVec<[bool; 8]> = SmallVec::new();
    for operation in operations {
        match operation {
            PaintOperation::Primitive(primitive) => {
                let mut primitive = translate_primitive(primitive, delta);
                clip_primitive(&mut primitive, &viewport);
                // An HTML element is clipped by the DOM, in logical pixels.
                if let Primitive::HtmlElement(element) = &mut primitive {
                    element.logical_content_mask.bounds = element
                        .logical_content_mask
                        .bounds
                        .intersect(&logical_viewport);
                }
                scene.insert_primitive(primitive);
            }
            PaintOperation::FastDropped(dropped) => {
                // Dropped by the viewport now, as the primitives it clips
                // away are.
                let mut dropped = dropped.moved(delta);
                dropped.mask = dropped.mask.intersect(&viewport);
                crate::fast::edge_shadows::dropped(scene, dropped);
            }
            PaintOperation::FastClip(clip) => {
                crate::fast::edge_shadows::clip_op(scene, clip.moved(delta))
            }
            PaintOperation::StartLayer(bounds)
            | PaintOperation::StartPositionedLayer(bounds)
            | PaintOperation::StartTopLayer(bounds) => {
                let bounds = Bounds {
                    origin: bounds.origin + delta,
                    size: bounds.size,
                }
                .intersect(&viewport);
                begun.push(!bounds.is_empty());
                if !bounds.is_empty() {
                    match operation {
                        PaintOperation::StartLayer(_) => scene.push_layer(bounds),
                        PaintOperation::StartPositionedLayer(_) => {
                            scene.push_positioned_layer(bounds)
                        }
                        _ => scene.push_top_layer(bounds),
                    }
                }
            }
            PaintOperation::EndLayer => {
                if begun.pop().unwrap_or(true) {
                    scene.pop_layer();
                }
            }
        }
    }
}

/// `scene` as a frame painting the content of every layer it composites
/// straight into it would have left it: its paint operations replayed, each
/// run of a layer's tile sprites (and the layer [`insert_tile_quads`] wraps
/// them in) replaced by the layer's content, moved to where the tiles put it
/// and clipped to their viewport. Draw orders come out as painting assigns
/// them, so the scene describes like the one drawn without layers.
///
#[cfg(any(test, feature = "test-support", feature = "diagnostics"))]
pub(crate) fn expanded_scene(scene: &Scene) -> Scene {
    let tile_of = |operation: Option<&PaintOperation>| match operation {
        Some(PaintOperation::Primitive(Primitive::PolychromeSprite(sprite))) => {
            decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id)
                .map(|(key, tile)| (key, tile, sprite.bounds.origin))
        }
        _ => None,
    };
    let operations = &scene.paint_operations;
    let mut expanded = Scene::default();
    expanded.scale_factor = scene.scale_factor;
    let mut index = 0;
    while index < operations.len() {
        let operation = &operations[index];
        if let PaintOperation::StartLayer(viewport) = operation
            && let Some((key, tile, origin)) = tile_of(operations.get(index + 1))
        {
            let layer = scene
                .layers
                .frames
                .iter()
                .find(|layer| layer.key == key)
                .unwrap_or_else(|| {
                    panic!("a tile of layer {key:?}, which the scene has no frame of")
                });
            let tile_origin = layer.tile_bounds(tile).origin;
            let translation = point(origin.x - tile_origin.x, origin.y - tile_origin.y);
            replay_content(
                &mut expanded,
                layer.content.operations(),
                translation,
                *viewport,
                Bounds::default(),
            );
            index += 1;
            while tile_of(operations.get(index)).is_some_and(|(other, ..)| other == key) {
                index += 1;
            }
            assert!(
                matches!(operations.get(index), Some(PaintOperation::EndLayer)),
                "the tiles of layer {key:?} are not composited in a layer of their own"
            );
            index += 1;
            continue;
        }
        match operation {
            PaintOperation::Primitive(primitive) => expanded.insert_primitive(primitive.clone()),
            PaintOperation::FastDropped(dropped) => {
                crate::fast::edge_shadows::dropped(&mut expanded, *dropped)
            }
            PaintOperation::FastClip(clip) => {
                crate::fast::edge_shadows::clip_op(&mut expanded, *clip)
            }
            PaintOperation::StartLayer(bounds) => expanded.push_layer(*bounds),
            PaintOperation::StartPositionedLayer(bounds) => expanded.push_positioned_layer(*bounds),
            PaintOperation::StartTopLayer(bounds) => expanded.push_top_layer(*bounds),
            PaintOperation::EndLayer => expanded.pop_layer(),
        }
        index += 1;
    }
    expanded
        .active_screen_effects
        .clone_from(&scene.active_screen_effects);
    expanded
        .active_screen_effect_snapshots
        .clone_from(&scene.active_screen_effect_snapshots);
    expanded.finish();
    expanded
}

/// Narrows every content mask `primitive` carries to `mask`.
fn clip_primitive(primitive: &mut Primitive, mask: &Bounds<ScaledPixels>) {
    let clip = |content_mask: &mut ContentMask<ScaledPixels>| {
        content_mask.bounds = content_mask.bounds.intersect(mask);
    };
    match primitive {
        Primitive::Shadow(p) => clip(&mut p.content_mask),
        Primitive::VisualEffect(p) => clip(&mut p.content_mask),
        Primitive::HtmlIsland(p) => clip(&mut p.content_mask),
        Primitive::HtmlElement(p) => clip(&mut p.content_mask),
        Primitive::ScreenEffectRegion(p) => clip(&mut p.content_mask),
        Primitive::Quad(p) => clip(&mut p.content_mask),
        Primitive::Path(p) => {
            clip(&mut p.content_mask);
            for vertex in &mut p.vertices {
                clip(&mut vertex.content_mask);
            }
        }
        Primitive::Underline(p) => clip(&mut p.content_mask),
        Primitive::MonochromeSprite(p) => clip(&mut p.content_mask),
        Primitive::SubpixelSprite(p) => clip(&mut p.content_mask),
        Primitive::PolychromeSprite(p) => clip(&mut p.content_mask),
        Primitive::Surface(p) => clip(&mut p.content_mask),
    }
}

/// Every tile of `hashes`, sorted.
pub(crate) fn all_tiles(hashes: &FxHashMap<TileCoord, u64>) -> Vec<TileCoord> {
    let mut all: Vec<_> = hashes.keys().copied().collect();
    all.sort();
    all
}

/// Paints the content into the layer's scene and records it.
fn repaint(
    window: &mut Window,
    cx: &mut App,
    mut painting: Painting,
    background: PaintColor,
    f: impl FnOnce(&mut Window, &mut App),
) {
    // Culling works in the painted region; gpux keeps a second mask for the
    // paint phase, in device pixels, which glyphs, underlines and paths clip
    // to, and which follows the first.
    window
        .content_mask_stack
        .push(ContentMask::rectangular(painting.painted_region));
    window
        .paint_content_mask_stack
        .push(ContentMask::rectangular(
            painting.painted_region.scale(window.scale_factor()),
        ));
    let painted_with = PaintedWith::current(window);
    painting.onto = Some(PaintingOnto {
        surface: crate::fast::composition::current_surface(window),
        in_top_layer: window.next_frame.scene.in_top_layer(),
    });
    mem::swap(&mut window.next_frame.scene, &mut painting.scene);
    // The painted region is the content's clip, as the viewport is the
    // clip of content painted without a layer: what it cuts and drops is
    // ordered as with it not there (see `crate::fast::edge_shadows`).
    let region = window.cover_bounds(
        window
            .content_mask_stack
            .last()
            .map_or(Bounds::default(), |mask| mask.bounds),
    );
    crate::fast::edge_shadows::clip(
        &mut window.next_frame.scene,
        crate::fast::edge_shadows::ClipOp::Push(region),
    );
    let paint_start = window.paint_index();
    let hovers_start = crate::fast::dependencies::begin_recording(window);
    let recording = cx.begin_recording_dependencies();
    window.fast_layers.painting = Some(painting);

    f(window, cx);

    let mut painting = window
        .fast_layers
        .painting
        .take()
        .expect("the layer being painted");
    let paint_dependencies = cx.finish_recording_dependencies(recording).all;
    // The reads stay in the log for the view holding the container, whose
    // hovers they are too.
    let hovers: Rc<[_]> = crate::fast::dependencies::finish_recording(window, hovers_start).into();
    let paint_end = window.paint_index();
    crate::fast::edge_shadows::clip(
        &mut window.next_frame.scene,
        crate::fast::edge_shadows::ClipOp::Pop,
    );
    mem::swap(&mut window.next_frame.scene, &mut painting.scene);
    window.paint_content_mask_stack.pop();
    window.content_mask_stack.pop();

    let scale_factor = window.scale_factor();
    let translation = painting.translation;
    let to_content = point(
        ScaledPixels(-translation.x.0),
        ScaledPixels(-translation.y.0),
    );
    let content = translated_scene(&painting.scene, to_content);
    let region = painting.painted_region.scale(scale_factor);
    let region = Bounds {
        origin: region.origin + to_content,
        size: region.size,
    };
    // What no tile can hold is drawn into the frame as it was painted, and
    // the container goes back to today's path (`policy::eligible`).
    let uncompositable = !painting.surface_switches.is_empty() || uncompositable(&painting.scene);
    take_scene_requests(&mut painting.scene, &mut window.next_frame.scene);
    let (content, paths, has_paths) = if uncompositable {
        (content, Rc::from([]), true)
    } else {
        match lift_paths(content) {
            Ok((content, paths)) => (content, paths, false),
            Err(content) => (*content, Rc::from([]), true),
        }
    };
    let mut visible = painting.viewport.scale(scale_factor);
    visible.origin += to_content;
    let mut rendered_work = 0;
    let mut visible_work = 0;
    for operation in &content.paint_operations {
        if let PaintOperation::Primitive(primitive) = operation {
            rendered_work += 1;
            visible_work += usize::from(visible_bounds(primitive).intersects(&visible));
        }
    }
    policy::note_work(
        window,
        &painting.id,
        rendered_work as f32 / visible_work.max(1) as f32,
    );
    let hashes = tile_hashes(&content, TILE_SIZE, region);
    if has_paths {
        let operations = &painting.scene.paint_operations;
        let mut drawn = 0;
        for (at, surface) in &painting.surface_switches {
            draw_into_frame(window, &operations[drawn..*at], Point::default());
            crate::fast::composition::switch_surface(window, *surface);
            drawn = *at;
        }
        draw_into_frame(window, &operations[drawn..], Point::default());
    }

    let views = crate::fast::layers::invalidate::content_views(window, &painting.prepaint_range);
    let layer = layer_mut(window, &painting.id);
    let generation = layer.next_generation();
    let dirty = match &layer.record {
        Some(old) if old.background == background => dirty_tiles(&old.tile_hashes, &hashes),
        _ => all_tiles(&hashes),
    };
    layer.record = Some(LayerRecord {
        content: LayerContent::from(content),
        generation,
        painted_region: painting.painted_region,
        content_bounds: painting.content_bounds,
        viewport: painting.viewport,
        scroll_offset: painting.scroll_offset,
        translation,
        prepaint_range: painting.prepaint_range,
        paint_range: paint_start..paint_end,
        tile_hashes: hashes,
        dirty_tiles: dirty,
        background,
        hovers,
        dependencies: painting.dependencies.union(&paint_dependencies),
        views,
        has_paths: has_paths && !uncompositable,
        uncompositable,
        painted_with: Some(painted_with),
        paths,
        view_layouts: Rc::new(mem::take(&mut painting.view_layouts)),
    });
    let dirtied = layer
        .record
        .as_ref()
        .map_or(0, |record| record.dirty_tiles.len());
    crate::fast::layers::input::painted(window, &painting.id, painting.input);
    // A frame is counted once: painted into its layer and shown from its
    // tiles, or drawn into the frame as without one.
    if !has_paths && insert_layer(window, &painting.id, translation, dirtied) {
        crate::fast::fast_stat!(window.fast_stats.frame, layers_repainted += 1);
    } else {
        policy::count_bypass(window, policy::Bypass::Ineligible);
    }
}

/// Ends the layers' part of the frame being drawn, before it becomes the
/// rendered frame.
pub(crate) fn finish_frame(window: &mut Window) {
    debug_assert!(window.fast_layers.painting.is_none());
    for layer in window.fast_layers.layers.values_mut() {
        layer.prepainted = None;
        layer.rows.finish_frame();
    }
}

/// The layer of the container `id`, made if it has none.
pub(crate) fn layer_mut<'a>(window: &'a mut Window, id: &GlobalElementId) -> &'a mut Layer {
    let layers = &mut window.fast_layers;
    if !layers.layers.contains_key(id) {
        let key = LayerKey(layers.next_key % crate::fast::layers::scene::LAYER_KEY_LIMIT);
        layers.next_key = (layers.next_key + 1) % crate::fast::layers::scene::LAYER_KEY_LIMIT;
        layers.layers.insert(
            id.clone(),
            Layer {
                key,
                generation: 0,
                record: None,
                policy: Default::default(),
                input: Default::default(),
                rows: Default::default(),
                last_composited_frame: layers.frame,
                prepainted: None,
            },
        );
    }
    layers.layers.get_mut(id).unwrap()
}

/// Whether `style` scrolls on some axis.
fn scrolls(style: &Style) -> bool {
    style.overflow.x == Overflow::Scroll || style.overflow.y == Overflow::Scroll
}

/// The part of the content a layer paints: the viewport, and one viewport's
/// extent on each side along the scrolled axes as far as the content goes.
/// How far past the viewport a layer paints on each scrolled side, in
/// viewport extents. Measured on `gpui_perf`'s scroll scenarios: 2 cut the
/// mean frame of scroll-child-view by 44 % and of scroll-same-view by 21 %
/// against 1, at the cost of repaint frames about 1.7 times as long (one per
/// two viewports scrolled); 0.5 more than doubled the mean.
pub(crate) const OVERSCAN_VIEWPORTS: f32 = 2.;

pub(crate) fn painted_region(
    viewport: Bounds<Pixels>,
    content: Bounds<Pixels>,
    overflow: Point<Overflow>,
) -> Bounds<Pixels> {
    fn extend(
        min: Pixels,
        extent: Pixels,
        content_min: Pixels,
        content_extent: Pixels,
    ) -> (Pixels, Pixels) {
        let max = min + extent;
        let over = extent * OVERSCAN_VIEWPORTS;
        let low = (min - over).max(content_min).min(min);
        let high = (max + over).min(content_min + content_extent).max(max);
        (low, high - low)
    }
    let mut region = viewport;
    if overflow.x == Overflow::Scroll {
        (region.origin.x, region.size.width) = extend(
            viewport.origin.x,
            viewport.size.width,
            content.origin.x,
            content.size.width,
        );
    }
    if overflow.y == Overflow::Scroll {
        (region.origin.y, region.size.height) = extend(
            viewport.origin.y,
            viewport.size.height,
            content.origin.y,
            content.size.height,
        );
    }
    region
}

/// A layer's translation at `scroll_offset`: the offset in whole device
/// pixels.
pub(crate) fn translation(window: &Window, scroll_offset: Point<Pixels>) -> Point<ScaledPixels> {
    let scale_factor = window.scale_factor();
    point(
        ScaledPixels((scroll_offset.x.0 * scale_factor).round()),
        ScaledPixels((scroll_offset.y.0 * scale_factor).round()),
    )
}

/// `scene` moved by `delta`, finished.
fn translated_scene(scene: &Scene, delta: Point<ScaledPixels>) -> Scene {
    let mut translated = Scene::default();
    for operation in &scene.paint_operations {
        match operation {
            PaintOperation::Primitive(primitive) => {
                translated.insert_primitive(translate_primitive(primitive, delta))
            }
            PaintOperation::FastDropped(dropped) => {
                crate::fast::edge_shadows::dropped(&mut translated, dropped.moved(delta))
            }
            PaintOperation::FastClip(clip) => {
                crate::fast::edge_shadows::clip_op(&mut translated, clip.moved(delta))
            }
            PaintOperation::StartLayer(bounds) => translated.push_layer(Bounds {
                origin: bounds.origin + delta,
                size: bounds.size,
            }),
            PaintOperation::StartPositionedLayer(bounds) => {
                translated.push_positioned_layer(Bounds {
                    origin: bounds.origin + delta,
                    size: bounds.size,
                })
            }
            PaintOperation::StartTopLayer(bounds) => translated.push_top_layer(Bounds {
                origin: bounds.origin + delta,
                size: bounds.size,
            }),
            PaintOperation::EndLayer => translated.pop_layer(),
        }
    }
    translated.finish();
    translated
}

/// `offset`, a scroll offset about to be applied, moved to whole device
/// pixels where layers are compiled, with or without a layer, so that a
/// layer's content and content drawn without one land on the same pixels
/// (spec §5.3).
pub(crate) fn snap_scroll_offset(window: &Window, offset: Point<Pixels>) -> Point<Pixels> {
    snap_offset(offset, window.scale_factor(), COMPILED)
}

/// [`snap_scroll_offset`] at `scale_factor`, snapping only when `compiled`,
/// rounding as [`Window::pixel_snap`] does.
pub(crate) fn snap_offset(
    offset: Point<Pixels>,
    scale_factor: f32,
    compiled: bool,
) -> Point<Pixels> {
    if !compiled {
        return offset;
    }
    offset.map(|value| {
        crate::px(crate::util::round_half_toward_zero(value.0 * scale_factor) / scale_factor)
    })
}
