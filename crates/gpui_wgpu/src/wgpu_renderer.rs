#[cfg(feature = "diagnostics")]
use crate::WgpuFrameTelemetry;
#[cfg(all(target_family = "wasm", feature = "diagnostics"))]
use crate::html_island_texture::HtmlIslandTextureCacheStatus;
#[cfg(target_family = "wasm")]
use crate::html_island_texture::{HtmlIslandTextureCache, HtmlIslandTextureDescriptor};
#[cfg(target_family = "wasm")]
use crate::html_island_upload::{HtmlIslandUploadResult, upload_html_island_element_to_texture};
#[cfg(not(target_family = "wasm"))]
use crate::native_backend_policy;
use crate::{CompositorGpuHint, WgpuAtlas, WgpuContext};
use bytemuck::{Pod, Zeroable};
#[cfg(target_family = "wasm")]
use gpux::HtmlIslandPrimitive;
use gpux::{
    ActiveScreenEffect, ActiveScreenEffectSnapshot, AtlasTextureId, BackgroundKind, Bounds,
    ColorSpace, ContentMask, Corners, DevicePixels, Edges, EffectCapabilities, EffectQuality,
    ElementEffectCapabilities, GpuSpecs, MonochromeSprite, MonochromeSpriteKind, PaintColor, Path,
    Pixels, Point, PolychromeSprite, PreparedBackground, PreparedElementEffect, PrimitiveBatch,
    Quad, ScaledPixels, Scene, ScreenEffect, ScreenEffectCapabilities, ScreenEffectSnapshotId,
    ScreenEffectTarget, Shadow, Size, SnapshotTransitionKind, SubpixelSprite, TransformationMatrix,
    Underline, VisualEffectPrimitive, get_gamma_correction_ratios,
};
use image::RgbaImage;
use log::warn;
#[cfg(not(target_family = "wasm"))]
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::cell::RefCell;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::{mem::align_of, mem::offset_of, mem::size_of};

const SURFACE_FRAME_LATENCY: u32 = 2;
const INSTANCE_TEXTURE_TEXEL_SIZE: u64 = 16;

const STORAGE_BUFFER_SHADERS: &str = concat!(
    include_str!("shaders.wgsl"),
    include_str!("shaders_storage.wgsl"),
);
const WEBGL_SHADERS: &str = concat!(
    include_str!("shaders.wgsl"),
    include_str!("shaders_webgl.wgsl"),
);
const SUBPIXEL_SHADERS: &str = concat!(
    "enable dual_source_blending;\n",
    include_str!("shaders.wgsl"),
    include_str!("shaders_storage.wgsl"),
    include_str!("shaders_subpixel.wgsl"),
);

fn backend_policy_label(backends: wgpu::Backends) -> String {
    let mut labels = Vec::new();
    if backends.contains(wgpu::Backends::NOOP) {
        labels.push("NOOP");
    }
    if backends.contains(wgpu::Backends::VULKAN) {
        labels.push("VULKAN");
    }
    if backends.contains(wgpu::Backends::METAL) {
        labels.push("METAL");
    }
    if backends.contains(wgpu::Backends::DX12) {
        labels.push("DX12");
    }
    if backends.contains(wgpu::Backends::GL) {
        labels.push("GL");
    }
    if backends.contains(wgpu::Backends::BROWSER_WEBGPU) {
        labels.push("BROWSER_WEBGPU");
    }

    if labels.is_empty() {
        "NONE".to_string()
    } else {
        labels.join("|")
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalParams {
    viewport_size: [f32; 2],
    premultiplied_alpha: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct PodBounds {
    origin: [f32; 2],
    size: [f32; 2],
}

impl From<Bounds<ScaledPixels>> for PodBounds {
    fn from(bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            origin: [bounds.origin.x.0, bounds.origin.y.0],
            size: [bounds.size.width.0, bounds.size.height.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct PackedContentMask {
    bounds: PodBounds,
    corner_radii: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SurfaceParams {
    bounds: PodBounds,
    content_mask: PackedContentMask,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GammaParams {
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
    is_bgr: u32,
    _pad: u32,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PathSprite {
    bounds: PodBounds,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PackedGradientStop {
    prepared_color: [f32; 4],
    percentage: f32,
    _pad: [f32; 3],
}

// Keep this in sync with the WGSL GradientStop layout.
const MIN_GRADIENT_STOP_BINDING_SIZE: u64 = size_of::<PackedGradientStop>() as u64;
const _: [(); 32] = [(); size_of::<PackedGradientStop>()];

const VISUAL_EFFECT_KIND_BACKDROP_BLUR: u32 = 1;
const VISUAL_EFFECT_KIND_GLASS: u32 = 2;
const VISUAL_EFFECT_KIND_GLOW: u32 = 3;
const SCREEN_EFFECT_KIND_RIPPLE: u32 = 1;
const SCREEN_EFFECT_KIND_CROSSFADE: u32 = 2;
const SCREEN_EFFECT_KIND_BLUR_CROSSFADE: u32 = 3;
const SCREEN_EFFECT_KIND_RADIAL_REVEAL: u32 = 4;
const SCREEN_EFFECT_KIND_RIPPLE_REVEAL: u32 = 5;
const MAX_WGPU_FAST_BLUR_RADIUS: f32 = 64.0;
const MAX_WGPU_HIGH_BLUR_RADIUS: f32 = 128.0;

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PackedTransformationMatrix {
    rotation_scale: [[f32; 2]; 2],
    translation: [f32; 2],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PackedVisualEffectPrimitive {
    bounds: PodBounds,
    content_mask: PackedContentMask,
    corner_radii: [f32; 4],
    opacity: f32,
    effect_offset: u32,
    effect_count: u32,
    _pad: u32,
    transformation: PackedTransformationMatrix,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PackedElementEffect {
    kind: u32,
    _pad: [u32; 3],
    params0: [f32; 4],
    params1: [f32; 4],
    color0: [f32; 4],
    color1: [f32; 4],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PackedScreenEffect {
    kind: u32,
    uses_snapshot: u32,
    _pad0: [u32; 2],
    bounds: PodBounds,
    tint: [f32; 4],
    origin: [f32; 2],
    elapsed_ms: f32,
    duration_ms: f32,
    radius: f32,
    amplitude: f32,
    wavelength: f32,
    damping: f32,
    params: [f32; 4],
}

#[derive(Default)]
struct PackedScreenEffects {
    effects: Vec<PackedScreenEffect>,
    snapshot_id: Option<ScreenEffectSnapshotId>,
}

#[derive(Default)]
struct PackedVisualEffects {
    primitives: Vec<PackedVisualEffectPrimitive>,
    effects: Vec<PackedElementEffect>,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C, align(8))]
struct PackedBackground {
    tag: u32,
    color_space: u32,
    solid: [f32; 4],
    gradient_angle_or_pattern_height: f32,
    stop_offset: u32,
    stop_count: u32,
    _pad: [u32; 3],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PackedQuad {
    order: u32,
    border_style: u32,
    bounds: PodBounds,
    content_mask: PackedContentMask,
    background: PackedBackground,
    border_color: PackedBackground,
    corner_radii: [f32; 4],
    border_widths: [f32; 4],
    transformation: PackedTransformationMatrix,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PackedPathRasterizationVertex {
    xy_position: [f32; 2],
    st_position: [f32; 2],
    background: PackedBackground,
    content_mask: PackedContentMask,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct PackedHtmlIsland {
    bounds: PodBounds,
    content_mask: PackedContentMask,
    opacity: f32,
    _pad: [f32; 3],
}

const _: [(); 16] = [(); size_of::<PodBounds>()];
const _: [(); 32] = [(); size_of::<PackedContentMask>()];
const _: [(); 24] = [(); size_of::<PackedTransformationMatrix>()];
const _: [(); 48] = [(); size_of::<PackedBackground>()];
const _: [(); 8] = [(); align_of::<PackedBackground>()];
const _: [(); 208] = [(); size_of::<PackedQuad>()];
const _: [(); 56] = [(); offset_of!(PackedQuad, background)];
const _: [(); 104] = [(); offset_of!(PackedQuad, border_color)];
const _: [(); 184] = [(); offset_of!(PackedQuad, transformation)];
const _: [(); 96] = [(); size_of::<PackedPathRasterizationVertex>()];
const _: [(); 16] = [(); offset_of!(PackedPathRasterizationVertex, background)];
const _: [(); 64] = [(); offset_of!(PackedPathRasterizationVertex, content_mask)];
const _: [(); 64] = [(); size_of::<PackedHtmlIsland>()];
const _: [(); 128] = [(); size_of::<MonochromeSprite>()];
const _: [(); 104] = [(); size_of::<Underline>()];
const _: [(); 136] = [(); size_of::<PolychromeSprite>()];
const _: [(); 128] = [(); size_of::<SubpixelSprite>()];
const _: [(); 4] = [(); offset_of!(MonochromeSprite, kind)];
const _: [(); 8] = [(); offset_of!(MonochromeSprite, bounds)];
const _: [(); 24] = [(); offset_of!(MonochromeSprite, content_mask)];
// Keep the directly uploaded shadow record synchronized with `shaders.wgsl`.
const _: [(); 152] = [(); size_of::<Shadow>()];
const _: [(); 72] = [(); offset_of!(Shadow, color)];
const _: [(); 88] = [(); offset_of!(Shadow, element_bounds)];
const _: [(); 120] = [(); offset_of!(Shadow, inset)];
const _: [(); 128] = [(); offset_of!(Shadow, transformation)];
const _: [(); 56] = [(); offset_of!(MonochromeSprite, color)];
const _: [(); 72] = [(); offset_of!(MonochromeSprite, tile)];
const _: [(); 104] = [(); offset_of!(MonochromeSprite, transformation)];
const _: [(); 0] = [(); MonochromeSpriteKind::Glyph as usize];
const _: [(); 1] = [(); MonochromeSpriteKind::ImageMask as usize];
const _: [(); 104] = [(); size_of::<PackedVisualEffectPrimitive>()];
const _: [(); 16] = [(); offset_of!(PackedVisualEffectPrimitive, content_mask)];
const _: [(); 48] = [(); offset_of!(PackedVisualEffectPrimitive, corner_radii)];
const _: [(); 64] = [(); offset_of!(PackedVisualEffectPrimitive, opacity)];
const _: [(); 68] = [(); offset_of!(PackedVisualEffectPrimitive, effect_offset)];
const _: [(); 72] = [(); offset_of!(PackedVisualEffectPrimitive, effect_count)];
const _: [(); 80] = [(); offset_of!(PackedVisualEffectPrimitive, transformation)];
const _: [(); 80] = [(); size_of::<PackedElementEffect>()];
const _: [(); 16] = [(); offset_of!(PackedElementEffect, params0)];
const _: [(); 32] = [(); offset_of!(PackedElementEffect, params1)];
const _: [(); 48] = [(); offset_of!(PackedElementEffect, color0)];
const _: [(); 64] = [(); offset_of!(PackedElementEffect, color1)];
const _: [(); 96] = [(); size_of::<PackedScreenEffect>()];
const _: [(); 16] = [(); offset_of!(PackedScreenEffect, bounds)];
const _: [(); 32] = [(); offset_of!(PackedScreenEffect, tint)];
const _: [(); 48] = [(); offset_of!(PackedScreenEffect, origin)];
const _: [(); 56] = [(); offset_of!(PackedScreenEffect, elapsed_ms)];
const _: [(); 60] = [(); offset_of!(PackedScreenEffect, duration_ms)];
const _: [(); 64] = [(); offset_of!(PackedScreenEffect, radius)];
const _: [(); 68] = [(); offset_of!(PackedScreenEffect, amplitude)];
const _: [(); 72] = [(); offset_of!(PackedScreenEffect, wavelength)];
const _: [(); 76] = [(); offset_of!(PackedScreenEffect, damping)];
const _: [(); 80] = [(); offset_of!(PackedScreenEffect, params)];

#[cfg(any(target_family = "wasm", test))]
fn html_island_has_drawable_area(
    bounds: Bounds<ScaledPixels>,
    content_mask: Bounds<ScaledPixels>,
) -> bool {
    bounds.size.width.0 > 0.
        && bounds.size.height.0 > 0.
        && content_mask.size.width.0 > 0.
        && content_mask.size.height.0 > 0.
}

fn pack_quads(quads: &[Quad]) -> (Vec<PackedQuad>, Vec<PackedGradientStop>) {
    let mut gradient_stops = Vec::new();
    let packed_quads = quads
        .iter()
        .map(|quad| PackedQuad {
            order: quad.order,
            border_style: quad.border_style as u32,
            bounds: quad.bounds.into(),
            content_mask: pack_content_mask(quad.content_mask),
            background: pack_background(&quad.background, &mut gradient_stops),
            border_color: pack_background(&quad.border_color, &mut gradient_stops),
            corner_radii: pack_corners(quad.corner_radii),
            border_widths: pack_edges(quad.border_widths),
            transformation: pack_transformation(quad.transformation),
        })
        .collect();

    (packed_quads, gradient_stops)
}

fn pack_visual_effect_primitives(primitives: &[VisualEffectPrimitive]) -> PackedVisualEffects {
    let mut packed = PackedVisualEffects::default();
    for primitive in primitives {
        let effect_offset = packed.effects.len() as u32;
        packed
            .effects
            .extend(primitive.effects.iter().map(pack_element_effect));
        packed.primitives.push(PackedVisualEffectPrimitive {
            bounds: primitive.bounds.into(),
            content_mask: pack_content_mask(primitive.content_mask),
            corner_radii: pack_corners(primitive.corner_radii),
            opacity: primitive.opacity,
            effect_offset,
            effect_count: primitive.effects.len() as u32,
            _pad: 0,
            transformation: pack_transformation(primitive.transformation),
        });
    }

    packed
}

fn pack_element_effect(effect: &PreparedElementEffect) -> PackedElementEffect {
    match effect {
        PreparedElementEffect::BackdropBlur {
            radius,
            saturation,
            brightness,
            tint,
            quality,
        } => PackedElementEffect {
            kind: VISUAL_EFFECT_KIND_BACKDROP_BLUR,
            _pad: [0; 3],
            params0: [
                pack_blur_radius(*radius, *quality),
                *saturation,
                *brightness,
                0.0,
            ],
            params1: [0.0; 4],
            color0: tint.map(pack_paint_color).unwrap_or([0.0; 4]),
            color1: [0.0; 4],
        },
        PreparedElementEffect::Glass {
            blur_radius,
            saturation,
            brightness,
            tint,
            highlight,
            refraction,
            distortion,
            quality,
        } => PackedElementEffect {
            kind: VISUAL_EFFECT_KIND_GLASS,
            _pad: [0; 3],
            params0: [
                pack_blur_radius(*blur_radius, *quality),
                *saturation,
                *brightness,
                *refraction,
            ],
            params1: [*distortion, 0.0, 0.0, 0.0],
            color0: pack_paint_color(*tint),
            color1: pack_paint_color(*highlight),
        },
        PreparedElementEffect::Glow {
            color,
            radius,
            spread,
            intensity,
        } => PackedElementEffect {
            kind: VISUAL_EFFECT_KIND_GLOW,
            _pad: [0; 3],
            params0: [f32::from(*radius), f32::from(*spread), *intensity, 0.0],
            params1: [0.0; 4],
            color0: pack_paint_color(*color),
            color1: [0.0; 4],
        },
    }
}

fn pack_screen_effects(
    scene: &Scene,
    active_effects: &[ActiveScreenEffect],
    surface_size: Size<DevicePixels>,
    available_snapshot_id: Option<ScreenEffectSnapshotId>,
) -> PackedScreenEffects {
    let scale_factor = scene.scale_factor.max(1.0);
    let mut packed = PackedScreenEffects::default();

    for active in active_effects {
        if active.is_complete() {
            log::trace!(
                "skipping completed screen effect id={}",
                active.id().as_u64()
            );
            continue;
        }

        match active.effect() {
            ScreenEffect::Ripple(effect) => {
                let Some((bounds, origin)) = resolve_screen_ripple_target(
                    scene,
                    effect.target_ref(),
                    effect.origin(),
                    surface_size,
                ) else {
                    log::trace!(
                        "skipping screen ripple id={} because target is unavailable",
                        active.id().as_u64()
                    );
                    continue;
                };

                packed.effects.push(PackedScreenEffect {
                    kind: SCREEN_EFFECT_KIND_RIPPLE,
                    uses_snapshot: 0,
                    _pad0: [0; 2],
                    bounds,
                    tint: effect
                        .tint_color()
                        .map(|color| pack_paint_color(color.paint_color()))
                        .unwrap_or([0.0; 4]),
                    origin,
                    elapsed_ms: active.elapsed_ms() as f32,
                    duration_ms: effect.duration_ms_value().max(1) as f32,
                    radius: f32::from(effect.radius_value()) * scale_factor,
                    amplitude: f32::from(effect.amplitude_value()) * scale_factor,
                    wavelength: f32::from(effect.wavelength_value()) * scale_factor,
                    damping: effect.damping_value(),
                    params: [effect.chromatic_aberration_value(), 0.0, 0.0, 0.0],
                });
            }
            ScreenEffect::SnapshotTransition(effect) => {
                let Some(snapshot_id) = available_snapshot_id else {
                    log::trace!(
                        "skipping snapshot transition id={} snapshot={} because no captured texture is available",
                        active.id().as_u64(),
                        effect.snapshot_id().as_u64()
                    );
                    continue;
                };
                if snapshot_id != effect.snapshot_id() {
                    log::trace!(
                        "skipping snapshot transition id={} snapshot={} because renderer captured snapshot={}",
                        active.id().as_u64(),
                        effect.snapshot_id().as_u64(),
                        snapshot_id.as_u64()
                    );
                    continue;
                }

                let Some(bounds) =
                    resolve_screen_effect_target(scene, effect.target_ref(), surface_size)
                else {
                    log::trace!(
                        "skipping snapshot transition id={} because target is unavailable",
                        active.id().as_u64()
                    );
                    continue;
                };

                let kind = match effect.kind() {
                    SnapshotTransitionKind::Crossfade => SCREEN_EFFECT_KIND_CROSSFADE,
                    SnapshotTransitionKind::BlurCrossfade => SCREEN_EFFECT_KIND_BLUR_CROSSFADE,
                    SnapshotTransitionKind::RadialReveal => SCREEN_EFFECT_KIND_RADIAL_REVEAL,
                    SnapshotTransitionKind::RippleReveal => SCREEN_EFFECT_KIND_RIPPLE_REVEAL,
                };

                packed.snapshot_id = Some(snapshot_id);
                packed.effects.push(PackedScreenEffect {
                    kind,
                    uses_snapshot: 1,
                    _pad0: [0; 2],
                    bounds,
                    tint: effect
                        .tint_color()
                        .map(|color| pack_paint_color(color.paint_color()))
                        .unwrap_or([0.0; 4]),
                    origin: pack_scaled_pixel_origin(effect.origin_value(), scale_factor),
                    elapsed_ms: active.elapsed_ms() as f32,
                    duration_ms: effect.duration_ms_value().max(1) as f32,
                    radius: f32::from(effect.radius_value()) * scale_factor,
                    amplitude: f32::from(effect.amplitude_value()) * scale_factor,
                    wavelength: f32::from(effect.wavelength_value()) * scale_factor,
                    damping: effect.damping_value(),
                    params: [
                        0.0,
                        f32::from(effect.blur_radius_value()) * scale_factor,
                        0.0,
                        0.0,
                    ],
                });
            }
        }
    }

    packed
}

fn first_snapshot_transition_id(
    active_effects: &[ActiveScreenEffect],
) -> Option<ScreenEffectSnapshotId> {
    active_effects
        .iter()
        .filter(|active| !active.is_complete())
        .find_map(|active| active.effect().snapshot_id())
}

fn resolve_screen_ripple_target(
    scene: &Scene,
    target: &ScreenEffectTarget,
    origin: Point<Pixels>,
    surface_size: Size<DevicePixels>,
) -> Option<(PodBounds, [f32; 2])> {
    let scale_factor = scene.scale_factor.max(1.0);
    let bounds = resolve_screen_effect_target(scene, target, surface_size)?;
    let origin = match target {
        ScreenEffectTarget::Region(id) => {
            let region = scene.screen_effect_region(id.as_ref())?;
            pack_region_origin(origin, region.logical_bounds, region.bounds)
        }
        ScreenEffectTarget::Window | ScreenEffectTarget::Bounds(_) => {
            pack_scaled_pixel_origin(origin, scale_factor)
        }
    };
    Some((bounds, origin))
}

fn resolve_screen_effect_target(
    scene: &Scene,
    target: &ScreenEffectTarget,
    surface_size: Size<DevicePixels>,
) -> Option<PodBounds> {
    let scale_factor = scene.scale_factor.max(1.0);
    match target {
        ScreenEffectTarget::Window => Some(PodBounds {
            origin: [0.0, 0.0],
            size: [
                (surface_size.width.0.max(0)) as f32,
                (surface_size.height.0.max(0)) as f32,
            ],
        }),
        ScreenEffectTarget::Bounds(bounds) => Some(PodBounds::from(bounds.scale(scale_factor))),
        ScreenEffectTarget::Region(id) => {
            let region = scene.screen_effect_region(id.as_ref())?;
            Some(PodBounds::from(region.bounds))
        }
    }
}

fn pack_scaled_pixel_origin(origin: Point<Pixels>, scale_factor: f32) -> [f32; 2] {
    [
        f32::from(origin.x) * scale_factor,
        f32::from(origin.y) * scale_factor,
    ]
}

fn pack_region_origin(
    origin: Point<Pixels>,
    logical_bounds: Bounds<Pixels>,
    scaled_bounds: Bounds<ScaledPixels>,
) -> [f32; 2] {
    let scale_x = scaled_bounds.size.width.0 / f32::from(logical_bounds.size.width).max(1.0);
    let scale_y = scaled_bounds.size.height.0 / f32::from(logical_bounds.size.height).max(1.0);
    [
        scaled_bounds.origin.x.0
            + (f32::from(origin.x) - f32::from(logical_bounds.origin.x)) * scale_x,
        scaled_bounds.origin.y.0
            + (f32::from(origin.y) - f32::from(logical_bounds.origin.y)) * scale_y,
    ]
}

fn pack_background(
    background: &PreparedBackground,
    gradient_stops: &mut Vec<PackedGradientStop>,
) -> PackedBackground {
    let (stop_offset, stop_count) = match background.kind() {
        BackgroundKind::LinearGradient => {
            let stop_offset = gradient_stops.len() as u32;
            gradient_stops.extend(background.gradient_stops().iter().map(|stop| {
                PackedGradientStop {
                    prepared_color: stop.color,
                    percentage: stop.percentage,
                    _pad: [0.0; 3],
                }
            }));
            (stop_offset, background.gradient_stops().len() as u32)
        }
        BackgroundKind::Solid | BackgroundKind::PatternSlash | BackgroundKind::Checkerboard => {
            (0, 0)
        }
    };

    PackedBackground {
        tag: background_kind_to_u32(background.kind()),
        color_space: color_space_to_u32(background.interpolation_color_space()),
        solid: pack_paint_color(background.solid_color()),
        gradient_angle_or_pattern_height: background.angle_or_pattern_value(),
        stop_offset,
        stop_count,
        _pad: [0, 0, 0],
    }
}

fn pack_paint_color(color: PaintColor) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}

fn pack_blur_radius(radius: gpux::Pixels, quality: EffectQuality) -> f32 {
    let cap = match quality {
        EffectQuality::Fast => MAX_WGPU_FAST_BLUR_RADIUS,
        EffectQuality::High => MAX_WGPU_HIGH_BLUR_RADIUS,
    };
    f32::from(radius).min(cap)
}

fn pack_corners(corners: Corners<ScaledPixels>) -> [f32; 4] {
    [
        corners.top_left.0,
        corners.top_right.0,
        corners.bottom_right.0,
        corners.bottom_left.0,
    ]
}

fn pack_content_mask(content_mask: ContentMask<ScaledPixels>) -> PackedContentMask {
    PackedContentMask {
        bounds: content_mask.bounds.into(),
        corner_radii: pack_corners(content_mask.corner_radii),
    }
}

fn pack_edges(edges: Edges<ScaledPixels>) -> [f32; 4] {
    [edges.top.0, edges.right.0, edges.bottom.0, edges.left.0]
}

fn pack_point(point: Point<ScaledPixels>) -> [f32; 2] {
    [point.x.0, point.y.0]
}

fn pack_point_f32(point: Point<f32>) -> [f32; 2] {
    [point.x, point.y]
}

fn pack_transformation(transformation: TransformationMatrix) -> PackedTransformationMatrix {
    PackedTransformationMatrix {
        rotation_scale: transformation.rotation_scale,
        translation: transformation.translation,
    }
}

fn wgpu_effect_capabilities() -> EffectCapabilities {
    EffectCapabilities {
        element: ElementEffectCapabilities {
            backdrop_blur: true,
            glass: true,
            glow: true,
        },
        screen: ScreenEffectCapabilities {
            ripple: true,
            snapshot_transitions: true,
        },
    }
}

fn visual_effect_glow_blend_state() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

fn background_kind_to_u32(kind: BackgroundKind) -> u32 {
    match kind {
        BackgroundKind::Solid => 0,
        BackgroundKind::LinearGradient => 1,
        BackgroundKind::PatternSlash => 2,
        BackgroundKind::Checkerboard => 3,
    }
}

fn color_space_to_u32(color_space: ColorSpace) -> u32 {
    match color_space {
        ColorSpace::Srgb => 0,
        ColorSpace::Oklab => 1,
    }
}

pub struct WgpuSurfaceConfig {
    pub size: Size<DevicePixels>,
    pub transparent: bool,
    /// Preferred presentation mode. When `Some`, the renderer will use this
    /// mode if supported by the surface. `Auto*` modes are accepted directly
    /// because WGPU defines their own portable fallbacks.
    /// When `None`, defaults to `Fifo` (VSync).
    ///
    /// Mobile platforms may prefer `Mailbox` (triple-buffering) to avoid
    /// blocking in `get_current_texture()` during lifecycle transitions.
    pub preferred_present_mode: Option<wgpu::PresentMode>,
}

/// Paint colors are premultiplied encoded sRGB and the GPU blends them as stored, so the render
/// target is a non-sRGB format: an sRGB view would re-encode them and blend in linear light,
/// which makes translucent colors much brighter than in Figma or a browser.
fn choose_surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ]
    .into_iter()
    .find(|format| formats.contains(format))
    .or_else(|| formats.iter().find(|format| !format.is_srgb()).copied())
    .or_else(|| formats.first().copied())
}

fn choose_present_mode(
    surface_caps: &wgpu::SurfaceCapabilities,
    preferred: Option<wgpu::PresentMode>,
) -> wgpu::PresentMode {
    if let Some(preferred) = preferred {
        if is_auto_present_mode(preferred) || surface_caps.present_modes.contains(&preferred) {
            return preferred;
        }

        if matches!(
            preferred,
            wgpu::PresentMode::AutoNoVsync
                | wgpu::PresentMode::Immediate
                | wgpu::PresentMode::Mailbox
        ) {
            if surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
            {
                return wgpu::PresentMode::Mailbox;
            }
            if surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Immediate)
            {
                return wgpu::PresentMode::Immediate;
            }
            return wgpu::PresentMode::AutoNoVsync;
        }
    }

    surface_caps
        .present_modes
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::PresentMode::Fifo)
        .or_else(|| surface_caps.present_modes.first().copied())
        .unwrap_or(wgpu::PresentMode::Fifo)
}

fn is_auto_present_mode(mode: wgpu::PresentMode) -> bool {
    matches!(
        mode,
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::AutoNoVsync
    )
}

#[derive(Clone)]
struct WgpuPipelines {
    quads: wgpu::RenderPipeline,
    shadows: wgpu::RenderPipeline,
    path_rasterization: wgpu::RenderPipeline,
    paths: wgpu::RenderPipeline,
    underlines: wgpu::RenderPipeline,
    mono_sprites: wgpu::RenderPipeline,
    subpixel_sprites: Option<wgpu::RenderPipeline>,
    poly_sprites: wgpu::RenderPipeline,
    #[cfg_attr(not(target_family = "wasm"), allow(dead_code))]
    html_islands: wgpu::RenderPipeline,
    #[allow(dead_code)]
    surfaces: wgpu::RenderPipeline,
    post_process_pass_through: wgpu::RenderPipeline,
    post_process_gaussian_blur_horizontal: wgpu::RenderPipeline,
    post_process_gaussian_blur_vertical: wgpu::RenderPipeline,
    visual_effects: wgpu::RenderPipeline,
    visual_effect_glows: wgpu::RenderPipeline,
    screen_effects: wgpu::RenderPipeline,
}

#[derive(Clone)]
struct WgpuBindGroupLayouts {
    globals: wgpu::BindGroupLayout,
    instances: wgpu::BindGroupLayout,
    instances_with_texture: wgpu::BindGroupLayout,
    surfaces: wgpu::BindGroupLayout,
    post_process: wgpu::BindGroupLayout,
    visual_effects: wgpu::BindGroupLayout,
    screen_effects: wgpu::BindGroupLayout,
}

enum WgpuDataBinding<'a> {
    Buffer {
        buffer: &'a wgpu::Buffer,
        offset: u64,
        size: NonZeroU64,
    },
    Texture {
        _texture: wgpu::Texture,
        view: wgpu::TextureView,
    },
}

impl WgpuDataBinding<'_> {
    fn resource(&self) -> wgpu::BindingResource<'_> {
        match self {
            Self::Buffer {
                buffer,
                offset,
                size,
            } => wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: *offset,
                size: Some(*size),
            }),
            Self::Texture { view, .. } => wgpu::BindingResource::TextureView(view),
        }
    }
}

/// Shared GPU context reference, used to coordinate device recovery across multiple windows.
pub type GpuContext = Rc<RefCell<Option<WgpuContext>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WgpuEffectTargetKey {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
}

impl WgpuEffectTargetKey {
    fn new(width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            format,
        }
    }

    fn matches(self, width: u32, height: u32, format: wgpu::TextureFormat) -> bool {
        self == Self::new(width, height, format)
    }
}

#[allow(dead_code)]
struct WgpuEffectTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[allow(dead_code)]
struct WgpuScreenEffectSnapshot {
    id: ScreenEffectSnapshotId,
    target: ScreenEffectTarget,
    color: WgpuEffectTarget,
    captured: bool,
}

#[allow(dead_code)]
struct WgpuEffectTargets {
    key: WgpuEffectTargetKey,
    scene_color: WgpuEffectTarget,
    ping_color: WgpuEffectTarget,
    pong_color: WgpuEffectTarget,
    blur_color: WgpuEffectTarget,
    previous_scene_color: WgpuEffectTarget,
    has_previous_scene_color: bool,
    screen_effect_snapshots: Vec<WgpuScreenEffectSnapshot>,
}

impl WgpuEffectTargets {
    #[allow(dead_code)]
    fn new(device: &wgpu::Device, key: WgpuEffectTargetKey) -> Self {
        Self {
            key,
            scene_color: Self::create_target(device, key, "effect_scene_color"),
            ping_color: Self::create_target(device, key, "effect_ping_color"),
            pong_color: Self::create_target(device, key, "effect_pong_color"),
            blur_color: Self::create_target(device, key, "effect_blur_color"),
            previous_scene_color: Self::create_target(device, key, "effect_previous_scene_color"),
            has_previous_scene_color: false,
            screen_effect_snapshots: Vec::new(),
        }
    }

    fn destroy(self) {
        self.scene_color.texture.destroy();
        self.ping_color.texture.destroy();
        self.pong_color.texture.destroy();
        self.blur_color.texture.destroy();
        self.previous_scene_color.texture.destroy();
        for snapshot in self.screen_effect_snapshots {
            snapshot.color.texture.destroy();
        }
    }

    fn create_target(
        device: &wgpu::Device,
        key: WgpuEffectTargetKey,
        label: &'static str,
    ) -> WgpuEffectTarget {
        Self::create_sized_target(device, key, label)
    }

    fn create_sized_target(
        device: &wgpu::Device,
        key: WgpuEffectTargetKey,
        label: &'static str,
    ) -> WgpuEffectTarget {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: key.width,
                height: key.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: key.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        WgpuEffectTarget { texture, view }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WgpuEffectTargetSlot {
    SceneColor,
    PingColor,
    PongColor,
}

impl WgpuEffectTargetSlot {
    fn first_ping_pong() -> Self {
        Self::PingColor
    }

    fn next_ping_pong(self) -> Self {
        match self {
            Self::SceneColor | Self::PongColor => Self::PingColor,
            Self::PingColor => Self::PongColor,
        }
    }
}

impl WgpuEffectTargets {
    fn target(&self, slot: WgpuEffectTargetSlot) -> &WgpuEffectTarget {
        match slot {
            WgpuEffectTargetSlot::SceneColor => &self.scene_color,
            WgpuEffectTargetSlot::PingColor => &self.ping_color,
            WgpuEffectTargetSlot::PongColor => &self.pong_color,
        }
    }

    fn sync_screen_effect_snapshots(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        snapshots: &[ActiveScreenEffectSnapshot],
    ) -> Vec<ScreenEffectSnapshotId> {
        let mut captured_ids = Vec::new();
        self.screen_effect_snapshots
            .retain(|snapshot| snapshots.iter().any(|request| request.id() == snapshot.id));

        for request in snapshots {
            if let Some(snapshot) = self
                .screen_effect_snapshots
                .iter_mut()
                .find(|snapshot| snapshot.id == request.id())
            {
                if snapshot.target != *request.target() {
                    snapshot.target = request.target().clone();
                    snapshot.captured = false;
                }
            } else {
                log::trace!(
                    "allocating screen effect snapshot texture id={} target={:?}",
                    request.id().as_u64(),
                    request.target()
                );
                self.screen_effect_snapshots.push(WgpuScreenEffectSnapshot {
                    id: request.id(),
                    target: request.target().clone(),
                    color: Self::create_target(device, self.key, "effect_screen_snapshot_color"),
                    captured: false,
                });
            }
        }

        for snapshot in &mut self.screen_effect_snapshots {
            if snapshot.captured {
                continue;
            }

            if self.has_previous_scene_color {
                copy_effect_texture(
                    encoder,
                    &self.previous_scene_color.texture,
                    &snapshot.color.texture,
                    self.key,
                );
                snapshot.captured = true;
                captured_ids.push(snapshot.id);
                log::trace!(
                    "captured screen effect snapshot id={} target={:?}",
                    snapshot.id.as_u64(),
                    snapshot.target
                );
            } else {
                log::trace!(
                    "screen effect snapshot id={} is waiting for a previous scene texture",
                    snapshot.id.as_u64()
                );
            }
        }
        captured_ids
    }

    fn mark_snapshots_uncaptured(&mut self, snapshot_ids: &[ScreenEffectSnapshotId]) {
        for snapshot_id in snapshot_ids {
            if let Some(snapshot) = self
                .screen_effect_snapshots
                .iter_mut()
                .find(|snapshot| snapshot.id == *snapshot_id)
            {
                snapshot.captured = false;
            }
        }
    }

    fn captured_snapshot_view(&self, id: ScreenEffectSnapshotId) -> Option<&wgpu::TextureView> {
        self.screen_effect_snapshots
            .iter()
            .find(|snapshot| snapshot.id == id && snapshot.captured)
            .map(|snapshot| &snapshot.color.view)
    }

    fn remember_presented_scene(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        source_slot: WgpuEffectTargetSlot,
    ) {
        copy_effect_texture(
            encoder,
            &self.target(source_slot).texture,
            &self.previous_scene_color.texture,
            self.key,
        );
        self.has_previous_scene_color = true;
    }
}

fn copy_effect_texture(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    destination: &wgpu::Texture,
    key: WgpuEffectTargetKey,
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: source,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: destination,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: key.width,
            height: key.height,
            depth_or_array_layers: 1,
        },
    );
}

/// GPU resources that must be dropped together during device recovery.
struct WgpuResources {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    /// The window surface. Headless renderers have none and render into their own targets.
    surface: Option<wgpu::Surface<'static>>,
    pipelines: WgpuPipelines,
    bind_group_layouts: WgpuBindGroupLayouts,
    atlas_sampler: wgpu::Sampler,
    post_process_sampler: wgpu::Sampler,
    effect_sampler: wgpu::Sampler,
    globals_buffer: wgpu::Buffer,
    /// The gamma parameters, in a buffer of their own: GL drivers that bind two ranges of one
    /// buffer to two uniform blocks (Android emulators') give the second block the wrong range.
    gamma_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    path_globals_bind_group: wgpu::BindGroup,
    instance_buffer: Option<wgpu::Buffer>,
    gradient_stop_buffer: Option<wgpu::Buffer>,
    path_intermediate_texture: Option<wgpu::Texture>,
    path_intermediate_view: Option<wgpu::TextureView>,
    path_msaa_texture: Option<wgpu::Texture>,
    path_msaa_view: Option<wgpu::TextureView>,
    effect_targets: Option<WgpuEffectTargets>,
}

impl WgpuResources {
    fn invalidate_intermediate_textures(&mut self) {
        self.path_intermediate_texture = None;
        self.path_intermediate_view = None;
        self.path_msaa_texture = None;
        self.path_msaa_view = None;
        self.invalidate_effect_targets();
    }

    fn invalidate_effect_targets(&mut self) {
        if let Some(targets) = self.effect_targets.take() {
            targets.destroy();
        }
    }

    #[allow(dead_code)]
    fn ensure_effect_targets(
        &mut self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> &WgpuEffectTargets {
        let key = WgpuEffectTargetKey::new(width, height, format);
        let recreate = self
            .effect_targets
            .as_ref()
            .is_none_or(|targets| !targets.key.matches(width, height, format));

        if recreate {
            self.invalidate_effect_targets();
            self.effect_targets = Some(WgpuEffectTargets::new(&self.device, key));
        }

        self.effect_targets
            .as_ref()
            .expect("effect targets should be initialized")
    }

    fn take_or_create_effect_targets(
        &mut self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> WgpuEffectTargets {
        let key = WgpuEffectTargetKey::new(width, height, format);
        let reuse = self
            .effect_targets
            .as_ref()
            .is_some_and(|targets| targets.key.matches(width, height, format));

        if reuse {
            self.effect_targets
                .take()
                .expect("effect targets should exist when reusable")
        } else {
            self.invalidate_effect_targets();
            WgpuEffectTargets::new(&self.device, key)
        }
    }
}

pub struct WgpuRenderer {
    /// Shared GPU context for device recovery coordination (unused on WASM).
    #[allow(dead_code)]
    pub(crate) context: Option<GpuContext>,
    /// Compositor GPU hint for adapter selection (unused on WASM).
    #[allow(dead_code)]
    pub(crate) compositor_gpu: Option<CompositorGpuHint>,
    #[cfg(not(target_family = "wasm"))]
    pub(crate) backend_policy: wgpu::Backends,
    resources: Option<WgpuResources>,
    surface_config: wgpu::SurfaceConfiguration,
    render_target_format: wgpu::TextureFormat,
    pub(crate) atlas: Arc<WgpuAtlas>,
    path_globals_offset: u64,
    instance_buffer_capacity: u64,
    gradient_stop_buffer_capacity: u64,
    max_buffer_size: u64,
    storage_buffer_alignment: u64,
    uses_webgl_instance_data: bool,
    rendering_params: RenderingParameters,
    pub(crate) is_bgr: bool,
    dual_source_blending: bool,
    adapter_info: wgpu::AdapterInfo,
    transparent_alpha_mode: wgpu::CompositeAlphaMode,
    opaque_alpha_mode: wgpu::CompositeAlphaMode,
    max_texture_size: u32,
    last_error: Arc<Mutex<Option<String>>>,
    failed_frame_count: u32,
    device_lost: std::sync::Arc<std::sync::atomic::AtomicBool>,
    surface_configured: bool,
    needs_redraw: bool,
    #[cfg(feature = "diagnostics")]
    frame_telemetry: RefCell<WgpuFrameTelemetry>,
    #[cfg(target_family = "wasm")]
    html_island_textures: HtmlIslandTextureCache,
    #[cfg(target_family = "wasm")]
    html_island_texture_draw_count: u32,
    #[cfg(target_family = "wasm")]
    diagnostic_max_monochrome_sprite_count: usize,
    #[cfg(target_family = "wasm")]
    diagnostic_max_glyph_mask_count: usize,
    #[cfg(target_family = "wasm")]
    diagnostic_max_image_mask_count: usize,
    #[cfg(target_family = "wasm")]
    diagnostic_max_subpixel_sprite_count: usize,
    #[cfg(target_family = "wasm")]
    diagnostic_max_polychrome_sprite_count: usize,
}

#[cfg(target_family = "wasm")]
pub struct TextRendererDiagnostics {
    pub antialiasing_mode: &'static str,
    pub glyph_sprite_kind: &'static str,
    pub monochrome_sprite_count: usize,
    pub glyph_mask_count: usize,
    pub image_mask_count: usize,
    pub subpixel_sprite_count: usize,
    pub polychrome_sprite_count: usize,
    pub device_pixel_ratio: f32,
    pub surface_format: String,
}

impl WgpuRenderer {
    fn resources(&self) -> &WgpuResources {
        self.resources
            .as_ref()
            .expect("GPU resources not available")
    }

    fn resources_mut(&mut self) -> &mut WgpuResources {
        self.resources
            .as_mut()
            .expect("GPU resources not available")
    }

    #[cfg(feature = "diagnostics")]
    fn begin_frame_telemetry(&self, scene: &Scene, atlas_uploads: (usize, usize)) {
        let sprite_sort_disorder_counts = scene.sprite_sort_disorder_counts();
        let bounds_tree_diagnostics = scene.bounds_tree_diagnostics();
        let global_element_id_path_diagnostics = scene.global_element_id_path_diagnostics();
        let taffy_layout_diagnostics = scene.taffy_layout_diagnostics();
        let taffy_node_request_diagnostics = scene.taffy_node_request_diagnostics();
        let line_layout_cache_diagnostics = scene.line_layout_cache_diagnostics();
        let text_measurement_miss_diagnostics = scene.text_measurement_miss_diagnostics();
        let text_shape_diagnostics = scene.text_shape_diagnostics();
        let style_phase_diagnostics = scene.style_phase_diagnostics();
        let frame_index = self.frame_telemetry.borrow().frame_index.saturating_add(1);
        *self.frame_telemetry.borrow_mut() = WgpuFrameTelemetry {
            schema_version: 1,
            frame_index,
            quad_count: scene.quads.len() as u64,
            shadow_count: scene.shadows.len() as u64,
            visual_effect_count: scene.visual_effects.len() as u64,
            html_island_count: scene.html_islands.len() as u64,
            path_count: scene.paths.len() as u64,
            underline_count: scene.underlines.len() as u64,
            monochrome_sprite_count: scene.monochrome_sprites.len() as u64,
            subpixel_sprite_count: scene.subpixel_sprites.len() as u64,
            polychrome_sprite_count: scene.polychrome_sprites.len() as u64,
            monochrome_sprite_sort_disorder_count: sprite_sort_disorder_counts[0],
            subpixel_sprite_sort_disorder_count: sprite_sort_disorder_counts[1],
            polychrome_sprite_sort_disorder_count: sprite_sort_disorder_counts[2],
            bounds_tree_insertion_count: bounds_tree_diagnostics[0],
            bounds_tree_max_leaf_hit_count: bounds_tree_diagnostics[1],
            bounds_tree_slow_search_count: bounds_tree_diagnostics[2],
            bounds_tree_search_node_visit_count: bounds_tree_diagnostics[3],
            bounds_tree_insertion_child_evaluation_count: bounds_tree_diagnostics[4],
            bounds_tree_retained_node_count: bounds_tree_diagnostics[5],
            global_element_id_creation_count: global_element_id_path_diagnostics[0],
            global_element_id_path_element_count: global_element_id_path_diagnostics[1],
            global_element_id_max_path_length: global_element_id_path_diagnostics[2],
            global_element_id_path_at_most_4_count: global_element_id_path_diagnostics[3],
            global_element_id_path_at_most_8_count: global_element_id_path_diagnostics[4],
            global_element_id_path_at_most_16_count: global_element_id_path_diagnostics[5],
            global_element_id_same_frame_duplicate_count: global_element_id_path_diagnostics[6],
            global_element_id_previous_frame_hit_count: global_element_id_path_diagnostics[7],
            taffy_node_count: taffy_layout_diagnostics[0],
            taffy_measured_leaf_count: taffy_layout_diagnostics[1],
            taffy_leaf_count: taffy_layout_diagnostics[2],
            taffy_container_count: taffy_layout_diagnostics[3],
            taffy_child_edge_count: taffy_layout_diagnostics[4],
            taffy_max_child_count: taffy_layout_diagnostics[5],
            taffy_container_at_most_1_count: taffy_layout_diagnostics[6],
            taffy_container_at_most_2_count: taffy_layout_diagnostics[7],
            taffy_container_at_most_4_count: taffy_layout_diagnostics[8],
            taffy_container_at_most_8_count: taffy_layout_diagnostics[9],
            taffy_flex_container_count: taffy_layout_diagnostics[10],
            taffy_block_container_count: taffy_layout_diagnostics[11],
            taffy_grid_container_count: taffy_layout_diagnostics[12],
            taffy_compute_layout_count: taffy_layout_diagnostics[13],
            taffy_repeated_compute_layout_count: taffy_layout_diagnostics[14],
            taffy_invalidated_descendant_count: taffy_layout_diagnostics[15],
            taffy_layout_bounds_call_count: taffy_layout_diagnostics[16],
            taffy_layout_bounds_cache_hit_count: taffy_layout_diagnostics[17],
            taffy_parent_origin_cache_hit_count: taffy_layout_diagnostics[18],
            taffy_parent_origin_cache_miss_count: taffy_layout_diagnostics[19],
            taffy_measurement_call_count: taffy_layout_diagnostics[20],
            taffy_previous_measurement_call_count: taffy_layout_diagnostics[21],
            taffy_same_input_measurement_count: taffy_layout_diagnostics[22],
            taffy_same_input_same_output_measurement_count: taffy_layout_diagnostics[23],
            taffy_same_input_changed_output_measurement_count: taffy_layout_diagnostics[24],
            taffy_custom_measurement_call_count: taffy_layout_diagnostics[25],
            taffy_text_measurement_call_count: taffy_layout_diagnostics[26],
            taffy_list_measurement_call_count: taffy_layout_diagnostics[27],
            taffy_uniform_list_measurement_call_count: taffy_layout_diagnostics[28],
            taffy_text_local_measurement_reuse_count: taffy_layout_diagnostics[29],
            taffy_text_full_measurement_layout_count: taffy_layout_diagnostics[30],
            taffy_node_request_count: taffy_node_request_diagnostics[0],
            taffy_pooled_node_reuse_count: taffy_node_request_diagnostics[1],
            taffy_new_node_count: taffy_node_request_diagnostics[2],
            taffy_unchanged_style_count: taffy_node_request_diagnostics[3],
            taffy_style_write_count: taffy_node_request_diagnostics[4],
            taffy_unchanged_topology_count: taffy_node_request_diagnostics[5],
            taffy_topology_write_count: taffy_node_request_diagnostics[6],
            taffy_measured_context_refill_count: taffy_node_request_diagnostics[7],
            taffy_measured_context_install_count: taffy_node_request_diagnostics[8],
            taffy_measured_context_dirty_count: taffy_node_request_diagnostics[9],
            taffy_unique_child_insertion_count: taffy_node_request_diagnostics[10],
            text_measurement_missing_cache_count: text_measurement_miss_diagnostics[0],
            text_measurement_wrap_mismatch_count: text_measurement_miss_diagnostics[1],
            text_measurement_truncation_request_count: text_measurement_miss_diagnostics[2],
            text_measurement_previous_truncation_count: text_measurement_miss_diagnostics[3],
            text_shape_call_count: text_shape_diagnostics[0],
            text_shape_line_count: text_shape_diagnostics[1],
            text_shape_decoration_run_count: text_shape_diagnostics[2],
            text_shape_max_decoration_run_count: text_shape_diagnostics[3],
            text_shape_at_most_one_decoration_run_line_count: text_shape_diagnostics[4],
            text_shape_decoration_run_capacity: text_shape_diagnostics[5],
            text_shape_font_run_pool_take_count: text_shape_diagnostics[6],
            text_shape_font_run_pool_hit_count: text_shape_diagnostics[7],
            text_shape_font_run_pool_miss_count: text_shape_diagnostics[8],
            text_shape_font_run_pool_return_count: text_shape_diagnostics[9],
            text_wrapped_layout_request_count: line_layout_cache_diagnostics[0],
            text_wrapped_layout_current_frame_hit_count: line_layout_cache_diagnostics[1],
            text_wrapped_layout_previous_frame_hit_count: line_layout_cache_diagnostics[2],
            text_wrapped_layout_miss_count: line_layout_cache_diagnostics[3],
            text_line_layout_request_count: line_layout_cache_diagnostics[4],
            text_line_layout_current_frame_hit_count: line_layout_cache_diagnostics[5],
            text_line_layout_previous_frame_hit_count: line_layout_cache_diagnostics[6],
            text_line_layout_miss_count: line_layout_cache_diagnostics[7],
            text_layout_cache_key_hash_count: line_layout_cache_diagnostics[8],
            text_layout_cache_key_text_bytes_hashed: line_layout_cache_diagnostics[9],
            text_layout_cache_key_run_items_hashed: line_layout_cache_diagnostics[10],
            text_layout_cache_key_equality_count: line_layout_cache_diagnostics[11],
            style_request_layout_count: style_phase_diagnostics[0],
            style_prepaint_initial_count: style_phase_diagnostics[1],
            style_prepaint_recompute_count: style_phase_diagnostics[2],
            style_paint_count: style_phase_diagnostics[3],
            style_paint_static_eligible_count: style_phase_diagnostics[4],
            style_paint_reuse_eligible_count: style_phase_diagnostics[5],
            style_request_static_eligible_count: style_phase_diagnostics[6],
            style_prepaint_request_candidate_count: style_phase_diagnostics[7],
            style_request_gpux_owned_static_eligible_count: style_phase_diagnostics[8],
            style_request_external_static_eligible_count: style_phase_diagnostics[9],
            style_prepaint_gpux_owned_request_candidate_count: style_phase_diagnostics[10],
            style_prepaint_external_request_candidate_count: style_phase_diagnostics[11],
            style_prepaint_request_reuse_count: style_phase_diagnostics[12],
            style_request_default_base_count: style_phase_diagnostics[13],
            active_screen_effect_count: scene.active_screen_effects.len() as u64,
            atlas_texture_write_count: atlas_uploads.0 as u64,
            atlas_texture_write_bytes: atlas_uploads.1 as u64,
            ..WgpuFrameTelemetry::default()
        };
    }

    #[cfg(feature = "diagnostics")]
    fn update_frame_telemetry(&self, update: impl FnOnce(&mut WgpuFrameTelemetry)) {
        update(&mut self.frame_telemetry.borrow_mut());
    }

    #[cfg(feature = "diagnostics")]
    pub fn frame_telemetry(&self) -> WgpuFrameTelemetry {
        *self.frame_telemetry.borrow()
    }

    /// Creates a new WgpuRenderer from raw window handles.
    ///
    /// The `gpu_context` is a shared reference that coordinates GPU context across
    /// multiple windows. The first window to create a renderer will initialize the
    /// context; subsequent windows will share it.
    ///
    /// # Safety
    /// The caller must ensure that the window handle remains valid for the lifetime
    /// of the returned renderer.
    #[cfg(not(target_family = "wasm"))]
    pub fn new<W>(
        gpu_context: GpuContext,
        window: &W,
        config: WgpuSurfaceConfig,
        compositor_gpu: Option<CompositorGpuHint>,
    ) -> anyhow::Result<Self>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        Self::new_with_backend_policy(
            gpu_context,
            window,
            config,
            compositor_gpu,
            native_backend_policy(),
        )
    }

    /// Creates a new WgpuRenderer with an explicit native backend policy.
    #[cfg(not(target_family = "wasm"))]
    pub fn new_with_backend_policy<W>(
        gpu_context: GpuContext,
        window: &W,
        config: WgpuSurfaceConfig,
        compositor_gpu: Option<CompositorGpuHint>,
        backend_policy: wgpu::Backends,
    ) -> anyhow::Result<Self>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;
        let raw_display_handle = window.display_handle().ok().map(|handle| handle.as_raw());

        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle,
            raw_window_handle: window_handle.as_raw(),
        };

        // Use the existing context's instance if available, otherwise create a new one.
        // The surface must be created with the same instance that will be used for
        // adapter selection, otherwise wgpu will panic.
        let instance = gpu_context
            .borrow()
            .as_ref()
            .map(|ctx| ctx.instance.clone())
            .unwrap_or_else(|| {
                WgpuContext::instance_with_backend_policy(Box::new(window.clone()), backend_policy)
            });

        // Safety: The caller guarantees that the window handle is valid for the
        // lifetime of this renderer. In practice, the RawWindow struct is created
        // from the native window handles and the surface is dropped before the window.
        let surface = unsafe {
            instance
                .create_surface_unsafe(target)
                .map_err(|e| anyhow::anyhow!("Failed to create surface: {e}"))?
        };

        let mut ctx_ref = gpu_context.borrow_mut();
        let context = match ctx_ref.as_mut() {
            Some(context) => {
                context.check_compatible_with_surface(&surface)?;
                context
            }
            None => ctx_ref.insert(WgpuContext::new_with_backend_policy(
                instance,
                &surface,
                compositor_gpu,
                backend_policy,
            )?),
        };

        let atlas = Arc::new(WgpuAtlas::from_context(context));

        Self::new_internal(
            Some(Rc::clone(&gpu_context)),
            context,
            surface,
            config,
            compositor_gpu,
            atlas,
            backend_policy,
            None,
        )
    }

    #[cfg(target_family = "wasm")]
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new_from_surface(
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<Self> {
        let atlas = Arc::new(WgpuAtlas::from_context(context));

        Self::new_internal(
            None,
            context,
            surface,
            config,
            None,
            atlas,
            wgpu::Backends::all(),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_internal(
        gpu_context: Option<GpuContext>,
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
        compositor_gpu: Option<CompositorGpuHint>,
        atlas: Arc<WgpuAtlas>,
        backend_policy: wgpu::Backends,
        shared: Option<&WgpuRenderer>,
    ) -> anyhow::Result<Self> {
        #[cfg(target_family = "wasm")]
        let _ = backend_policy;

        let surface_caps = surface.get_capabilities(&context.adapter);
        let surface_format = choose_surface_format(&surface_caps.formats).ok_or_else(|| {
            anyhow::anyhow!(
                "Surface reports no supported texture formats for adapter {:?}",
                context.adapter.get_info().name
            )
        })?;
        if surface_format.is_srgb() {
            warn!(
                "Surface offers only sRGB formats; encoded paint colors are encoded again and render too light on {:?}",
                surface_format
            );
        }
        let render_target_format = surface_format;

        let pick_alpha_mode =
            |preferences: &[wgpu::CompositeAlphaMode]| -> anyhow::Result<wgpu::CompositeAlphaMode> {
                preferences
                    .iter()
                    .find(|p| surface_caps.alpha_modes.contains(p))
                    .copied()
                    .or_else(|| surface_caps.alpha_modes.first().copied())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Surface reports no supported alpha modes for adapter {:?}",
                            context.adapter.get_info().name
                        )
                    })
            };

        let transparent_alpha_mode = crate::fast::composition::transparent_alpha_mode(
            context,
            pick_alpha_mode(&[
                wgpu::CompositeAlphaMode::PreMultiplied,
                wgpu::CompositeAlphaMode::Inherit,
            ])?,
        );

        let opaque_alpha_mode = pick_alpha_mode(&[
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::Inherit,
        ])?;

        let alpha_mode = if config.transparent {
            transparent_alpha_mode
        } else {
            opaque_alpha_mode
        };

        let device = Arc::clone(&context.device);
        let max_texture_size = device.limits().max_texture_dimension_2d;

        let requested_width = config.size.width.0 as u32;
        let requested_height = config.size.height.0 as u32;
        let clamped_width = requested_width.min(max_texture_size);
        let clamped_height = requested_height.min(max_texture_size);

        if clamped_width != requested_width || clamped_height != requested_height {
            warn!(
                "Requested surface size ({}, {}) exceeds maximum texture dimension {}. \
                 Clamping to ({}, {}). Window content may not fill the entire window.",
                requested_width, requested_height, max_texture_size, clamped_width, clamped_height
            );
        }

        let preferred_surface_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        let surface_usage = if surface_caps.usages.contains(preferred_surface_usage) {
            preferred_surface_usage
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
            format: surface_format,
            width: clamped_width.max(1),
            height: clamped_height.max(1),
            present_mode: choose_present_mode(&surface_caps, config.preferred_present_mode),
            desired_maximum_frame_latency: SURFACE_FRAME_LATENCY,
            alpha_mode,
            view_formats: Vec::new(),
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        // Configure the surface immediately. The adapter selection process already validated
        // that this adapter can successfully configure this surface.
        surface.configure(&context.device, &surface_config);

        // Pipelines compiled for the same target format are shared: they are
        // much slower to create than a surface.
        let shared = shared
            .filter(|shared| shared.render_target_format == render_target_format)
            .and_then(|shared| shared.resources.as_ref())
            .map(|resources| {
                (
                    resources.pipelines.clone(),
                    resources.bind_group_layouts.clone(),
                )
            });

        Ok(Self::from_target(
            gpu_context,
            context,
            Some(surface),
            surface_config,
            render_target_format,
            (transparent_alpha_mode, opaque_alpha_mode),
            compositor_gpu,
            atlas,
            backend_policy,
            None,
            shared,
        ))
    }

    /// Builds the pipelines, buffers and samplers shared by every target: the window surface
    /// when `surface` is present, and a headless renderer's own textures when it is not.
    /// `path_sample_count` overrides the adapter's best path multisample count.
    #[allow(clippy::too_many_arguments)]
    fn from_target(
        gpu_context: Option<GpuContext>,
        context: &WgpuContext,
        surface: Option<wgpu::Surface<'static>>,
        surface_config: wgpu::SurfaceConfiguration,
        render_target_format: wgpu::TextureFormat,
        (transparent_alpha_mode, opaque_alpha_mode): (
            wgpu::CompositeAlphaMode,
            wgpu::CompositeAlphaMode,
        ),
        compositor_gpu: Option<CompositorGpuHint>,
        atlas: Arc<WgpuAtlas>,
        backend_policy: wgpu::Backends,
        path_sample_count: Option<u32>,
        shared: Option<(WgpuPipelines, WgpuBindGroupLayouts)>,
    ) -> Self {
        #[cfg(target_family = "wasm")]
        let _ = backend_policy;
        let device = Arc::clone(&context.device);
        let max_texture_size = device.limits().max_texture_dimension_2d;
        let surface_format = surface_config.format;
        let alpha_mode = surface_config.alpha_mode;

        let queue = Arc::clone(&context.queue);
        let uses_webgl_instance_data = context.uses_webgl_instance_data();
        let dual_source_blending =
            context.supports_dual_source_blending() && !uses_webgl_instance_data;

        let mut rendering_params = RenderingParameters::new(&context.adapter, surface_format);
        if let Some(path_sample_count) = path_sample_count {
            rendering_params.path_sample_count = path_sample_count;
        }
        let (pipelines, bind_group_layouts) = match shared {
            Some(shared) => shared,
            None => {
                let bind_group_layouts =
                    Self::create_bind_group_layouts(&device, uses_webgl_instance_data);
                let pipelines = Self::create_pipelines(
                    &device,
                    &bind_group_layouts,
                    render_target_format,
                    alpha_mode,
                    rendering_params.path_sample_count,
                    dual_source_blending,
                    uses_webgl_instance_data,
                );
                (pipelines, bind_group_layouts)
            }
        };

        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let post_process_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post_process_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let effect_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effect_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let uniform_alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let globals_size = std::mem::size_of::<GlobalParams>() as u64;
        let gamma_size = std::mem::size_of::<GammaParams>() as u64;
        let path_globals_offset = globals_size.next_multiple_of(uniform_alignment);

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals_buffer"),
            size: path_globals_offset + globals_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gamma_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gamma_buffer"),
            size: gamma_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let max_buffer_size = device.limits().max_buffer_size;
        let storage_buffer_alignment = device.limits().min_storage_buffer_offset_alignment as u64;
        let initial_instance_buffer_capacity = 2 * 1024 * 1024;
        let initial_gradient_stop_buffer_capacity = 256 * 1024;
        let instance_buffer = (!uses_webgl_instance_data).then(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance_buffer"),
                size: initial_instance_buffer_capacity,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let gradient_stop_buffer = (!uses_webgl_instance_data).then(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gradient_stop_buffer"),
                size: initial_gradient_stop_buffer_capacity,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals_bind_group"),
            layout: &bind_group_layouts.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: 0,
                        size: Some(NonZeroU64::new(globals_size).unwrap()),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &gamma_buffer,
                        offset: 0,
                        size: Some(NonZeroU64::new(gamma_size).unwrap()),
                    }),
                },
            ],
        });

        let path_globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("path_globals_bind_group"),
            layout: &bind_group_layouts.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: path_globals_offset,
                        size: Some(NonZeroU64::new(globals_size).unwrap()),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &gamma_buffer,
                        offset: 0,
                        size: Some(NonZeroU64::new(gamma_size).unwrap()),
                    }),
                },
            ],
        });

        let adapter_info = context.adapter.get_info();

        let last_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        // wgpu keeps one uncaptured-error callback per device. A windowed renderer owns its
        // callback as before; a surface-free renderer shares a headless device whose context
        // installed one callback for every renderer on it.
        if surface.is_some() {
            let last_error_clone = Arc::clone(&last_error);
            device.on_uncaptured_error(Arc::new(move |error| {
                let mut guard = last_error_clone.lock().unwrap();
                *guard = Some(error.to_string());
            }));
        }

        let resources = WgpuResources {
            device,
            queue,
            surface,
            pipelines,
            bind_group_layouts,
            atlas_sampler,
            post_process_sampler,
            effect_sampler,
            globals_buffer,
            gamma_buffer,
            globals_bind_group,
            path_globals_bind_group,
            instance_buffer,
            gradient_stop_buffer,
            // Defer intermediate texture creation to first draw call via ensure_intermediate_textures().
            // This avoids panics when the device/surface is in an invalid state during initialization.
            path_intermediate_texture: None,
            path_intermediate_view: None,
            path_msaa_texture: None,
            path_msaa_view: None,
            effect_targets: None,
        };

        Self {
            context: gpu_context,
            compositor_gpu,
            #[cfg(not(target_family = "wasm"))]
            backend_policy,
            resources: Some(resources),
            surface_config,
            render_target_format,
            atlas,
            path_globals_offset,
            instance_buffer_capacity: initial_instance_buffer_capacity,
            gradient_stop_buffer_capacity: initial_gradient_stop_buffer_capacity,
            max_buffer_size,
            storage_buffer_alignment,
            uses_webgl_instance_data,
            rendering_params,
            is_bgr: false,
            dual_source_blending,
            adapter_info,
            transparent_alpha_mode,
            opaque_alpha_mode,
            max_texture_size,
            last_error,
            failed_frame_count: 0,
            device_lost: context.device_lost_flag(),
            surface_configured: true,
            needs_redraw: false,
            #[cfg(feature = "diagnostics")]
            frame_telemetry: RefCell::new(WgpuFrameTelemetry::default()),
            #[cfg(target_family = "wasm")]
            html_island_textures: HtmlIslandTextureCache::default(),
            #[cfg(target_family = "wasm")]
            html_island_texture_draw_count: 0,
            #[cfg(target_family = "wasm")]
            diagnostic_max_monochrome_sprite_count: 0,
            #[cfg(target_family = "wasm")]
            diagnostic_max_glyph_mask_count: 0,
            #[cfg(target_family = "wasm")]
            diagnostic_max_image_mask_count: 0,
            #[cfg(target_family = "wasm")]
            diagnostic_max_subpixel_sprite_count: 0,
            #[cfg(target_family = "wasm")]
            diagnostic_max_polychrome_sprite_count: 0,
        }
    }

    fn create_bind_group_layouts(
        device: &wgpu::Device,
        uses_webgl_instance_data: bool,
    ) -> WgpuBindGroupLayouts {
        let globals =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("globals_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<GlobalParams>() as u64
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<GammaParams>() as u64
                            ),
                        },
                        count: None,
                    },
                ],
            });

        let storage_buffer_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: if uses_webgl_instance_data {
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                }
            } else {
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                }
            },
            count: None,
        };

        let instances = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("instances_layout"),
            entries: &[storage_buffer_entry(0), storage_buffer_entry(5)],
        });

        let instances_with_texture =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("instances_with_texture_layout"),
                entries: &[
                    storage_buffer_entry(0),
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    storage_buffer_entry(5),
                ],
            });

        let surfaces = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("surfaces_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<SurfaceParams>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let post_process = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post_process_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let visual_effects = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("visual_effects_layout"),
            entries: &[
                storage_buffer_entry(0),
                storage_buffer_entry(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let mut screen_effect_entries = vec![
            storage_buffer_entry(0),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ];
        if uses_webgl_instance_data {
            // `instance_cursor` can select either uint texture at runtime, so
            // WebGL pipeline validation requires both even though screen effects
            // only read primary records.
            screen_effect_entries.push(storage_buffer_entry(5));
        }
        let screen_effects = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen_effects_layout"),
            entries: &screen_effect_entries,
        });

        WgpuBindGroupLayouts {
            globals,
            instances,
            instances_with_texture,
            surfaces,
            post_process,
            visual_effects,
            screen_effects,
        }
    }

    fn create_pipelines(
        device: &wgpu::Device,
        layouts: &WgpuBindGroupLayouts,
        surface_format: wgpu::TextureFormat,
        _alpha_mode: wgpu::CompositeAlphaMode,
        path_sample_count: u32,
        dual_source_blending: bool,
        uses_webgl_instance_data: bool,
    ) -> WgpuPipelines {
        // Diagnostic guard: verify the device actually has
        // DUAL_SOURCE_BLENDING. We have a crash report (ZED-5G1) where a
        // feature mismatch caused a wgpu-hal abort, but we haven't
        // identified the code path that produces the mismatch. This
        // guard prevents the crash and logs more evidence.
        // Remove this check once:
        // a) We find and fix the root cause, or
        // b) There are no reports of this warning appearing for some time.
        let device_has_feature = device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
        if dual_source_blending && !device_has_feature {
            log::error!(
                "BUG: dual_source_blending flag is true but device does not \
                 have DUAL_SOURCE_BLENDING enabled (device features: {:?}). \
                 Falling back to mono text rendering. Please report this at \
                 https://github.com/zed-industries/zed/issues",
                device.features(),
            );
        }
        let dual_source_blending =
            dual_source_blending && device_has_feature && !uses_webgl_instance_data;

        let base_shader_source = if uses_webgl_instance_data {
            WEBGL_SHADERS
        } else {
            STORAGE_BUFFER_SHADERS
        };
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpui_shaders"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(base_shader_source)),
        });

        let subpixel_shader_module = if dual_source_blending {
            Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gpui_subpixel_shaders"),
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SUBPIXEL_SHADERS)),
            }))
        } else {
            None
        };

        let color_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        };

        let create_pipeline = |name: &str,
                               vs_entry: &str,
                               fs_entry: &str,
                               globals_layout: &wgpu::BindGroupLayout,
                               data_layout: &wgpu::BindGroupLayout,
                               topology: wgpu::PrimitiveTopology,
                               color_targets: &[Option<wgpu::ColorTargetState>],
                               sample_count: u32,
                               module: &wgpu::ShaderModule| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("{name}_layout")),
                bind_group_layouts: &[Some(globals_layout), Some(data_layout)],
                immediate_size: 0,
            });

            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs_entry),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs_entry),
                    targets: color_targets,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview_mask: None,
                cache: None,
            })
        };

        let quads = create_pipeline(
            "quads",
            "vs_quad",
            "fs_quad",
            &layouts.globals,
            &layouts.instances,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let shadows = create_pipeline(
            "shadows",
            "vs_shadow",
            "fs_shadow",
            &layouts.globals,
            &layouts.instances,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let path_rasterization = create_pipeline(
            "path_rasterization",
            "vs_path_rasterization",
            "fs_path_rasterization",
            &layouts.globals,
            &layouts.instances,
            wgpu::PrimitiveTopology::TriangleList,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            path_sample_count,
            &shader_module,
        );

        let paths_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let paths = create_pipeline(
            "paths",
            "vs_path",
            "fs_path",
            &layouts.globals,
            &layouts.instances_with_texture,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(paths_blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            1,
            &shader_module,
        );

        let underlines = create_pipeline(
            "underlines",
            "vs_underline",
            "fs_underline",
            &layouts.globals,
            &layouts.instances,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let mono_sprites = create_pipeline(
            "mono_sprites",
            "vs_mono_sprite",
            "fs_mono_sprite",
            &layouts.globals,
            &layouts.instances_with_texture,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let subpixel_sprites = if let Some(subpixel_module) = &subpixel_shader_module {
            let subpixel_blend = wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Src1,
                    dst_factor: wgpu::BlendFactor::OneMinusSrc1,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            };

            Some(create_pipeline(
                "subpixel_sprites",
                "vs_subpixel_sprite",
                "fs_subpixel_sprite",
                &layouts.globals,
                &layouts.instances_with_texture,
                wgpu::PrimitiveTopology::TriangleStrip,
                &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(subpixel_blend),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                1,
                subpixel_module,
            ))
        } else {
            None
        };

        let poly_sprites = create_pipeline(
            "poly_sprites",
            "vs_poly_sprite",
            "fs_poly_sprite",
            &layouts.globals,
            &layouts.instances_with_texture,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let html_islands = create_pipeline(
            "html_islands",
            "vs_html_island",
            "fs_html_island",
            &layouts.globals,
            &layouts.instances_with_texture,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let surfaces = create_pipeline(
            "surfaces",
            "vs_surface",
            "fs_surface",
            &layouts.globals,
            &layouts.surfaces,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target)],
            1,
            &shader_module,
        );

        let post_process_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post_process_pass_through_layout"),
            bind_group_layouts: &[Some(&layouts.post_process)],
            immediate_size: 0,
        });
        let create_post_process_pipeline = |label: &'static str, fragment_entry: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&post_process_layout),
                vertex: wgpu::VertexState {
                    module: &shader_module,
                    entry_point: Some("vs_post_process"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_module,
                    entry_point: Some(fragment_entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let post_process_pass_through = create_post_process_pipeline(
            "post_process_pass_through",
            "fs_post_process_pass_through",
        );
        let post_process_gaussian_blur_horizontal = create_post_process_pipeline(
            "post_process_gaussian_blur_horizontal",
            "fs_post_process_gaussian_blur_horizontal",
        );
        let post_process_gaussian_blur_vertical = create_post_process_pipeline(
            "post_process_gaussian_blur_vertical",
            "fs_post_process_gaussian_blur_vertical",
        );

        let visual_effects = create_pipeline(
            "visual_effects",
            "vs_visual_effect",
            "fs_visual_effect",
            &layouts.globals,
            &layouts.visual_effects,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            1,
            &shader_module,
        );

        let visual_effect_glows = create_pipeline(
            "visual_effect_glows",
            "vs_visual_effect",
            "fs_visual_effect_glow_add",
            &layouts.globals,
            &layouts.visual_effects,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(visual_effect_glow_blend_state()),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            1,
            &shader_module,
        );

        let screen_effects = create_pipeline(
            "screen_effects",
            "vs_screen_effect",
            "fs_screen_effect",
            &layouts.globals,
            &layouts.screen_effects,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            1,
            &shader_module,
        );

        WgpuPipelines {
            quads,
            shadows,
            path_rasterization,
            paths,
            underlines,
            mono_sprites,
            subpixel_sprites,
            poly_sprites,
            html_islands,
            surfaces,
            post_process_pass_through,
            post_process_gaussian_blur_horizontal,
            post_process_gaussian_blur_vertical,
            visual_effects,
            visual_effect_glows,
            screen_effects,
        }
    }

    fn create_path_intermediate(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("path_intermediate"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    fn create_msaa_if_needed(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        sample_count: u32,
    ) -> Option<(wgpu::Texture, wgpu::TextureView)> {
        if sample_count <= 1 {
            return None;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("path_msaa"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Some((texture, view))
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        let width = size.width.0 as u32;
        let height = size.height.0 as u32;
        let needs_configure = !self.surface_configured
            || width != self.surface_config.width
            || height != self.surface_config.height;

        if needs_configure {
            let clamped_width = width.min(self.max_texture_size);
            let clamped_height = height.min(self.max_texture_size);

            if clamped_width != width || clamped_height != height {
                warn!(
                    "Requested surface size ({}, {}) exceeds maximum texture dimension {}. \
                     Clamping to ({}, {}). Window content may not fill the entire window.",
                    width, height, self.max_texture_size, clamped_width, clamped_height
                );
            }

            self.surface_config.width = clamped_width.max(1);
            self.surface_config.height = clamped_height.max(1);
            let surface_config = self.surface_config.clone();

            {
                let resources = self.resources_mut();

                #[cfg(target_family = "wasm")]
                let resize_poll = wgpu::PollType::Poll;
                #[cfg(not(target_family = "wasm"))]
                let resize_poll = wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                };
                if let Err(e) = resources.device.poll(resize_poll) {
                    warn!("Failed to poll device during resize: {e:?}");
                }

                // Destroy old textures before allocating new ones to avoid GPU memory spikes
                if let Some(ref texture) = resources.path_intermediate_texture {
                    texture.destroy();
                }
                if let Some(ref texture) = resources.path_msaa_texture {
                    texture.destroy();
                }

                if let Some(surface) = &resources.surface {
                    surface.configure(&resources.device, &surface_config);
                }

                // Invalidate intermediate textures - they will be lazily recreated
                // in draw() after we confirm the surface is healthy. This avoids
                // panics when the device/surface is in an invalid state during resize.
                resources.invalidate_intermediate_textures();
            }

            self.surface_configured = true;
        }
    }

    fn ensure_intermediate_textures(&mut self) {
        if self.resources().path_intermediate_texture.is_some() {
            return;
        }

        let format = self.render_target_format;
        let width = self.surface_config.width;
        let height = self.surface_config.height;
        let path_sample_count = self.rendering_params.path_sample_count;
        let resources = self.resources_mut();

        let (t, v) = Self::create_path_intermediate(&resources.device, format, width, height);
        resources.path_intermediate_texture = Some(t);
        resources.path_intermediate_view = Some(v);

        let (path_msaa_texture, path_msaa_view) = Self::create_msaa_if_needed(
            &resources.device,
            format,
            width,
            height,
            path_sample_count,
        )
        .map(|(t, v)| (Some(t), Some(v)))
        .unwrap_or((None, None));
        resources.path_msaa_texture = path_msaa_texture;
        resources.path_msaa_view = path_msaa_view;
    }

    pub fn set_subpixel_layout(&mut self, is_bgr: bool) {
        self.is_bgr = is_bgr;
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        let new_alpha_mode = if transparent {
            self.transparent_alpha_mode
        } else {
            self.opaque_alpha_mode
        };

        if new_alpha_mode != self.surface_config.alpha_mode {
            self.surface_config.alpha_mode = new_alpha_mode;
            let surface_config = self.surface_config.clone();
            let path_sample_count = self.rendering_params.path_sample_count;
            let dual_source_blending = self.dual_source_blending;
            let uses_webgl_instance_data = self.uses_webgl_instance_data;
            let render_target_format = self.render_target_format;
            let resources = self.resources_mut();
            if let Some(surface) = &resources.surface {
                surface.configure(&resources.device, &surface_config);
            }
            resources.pipelines = Self::create_pipelines(
                &resources.device,
                &resources.bind_group_layouts,
                render_target_format,
                surface_config.alpha_mode,
                path_sample_count,
                dual_source_blending,
                uses_webgl_instance_data,
            );
        }
    }

    #[allow(dead_code)]
    pub fn viewport_size(&self) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(self.surface_config.width as i32),
            height: DevicePixels(self.surface_config.height as i32),
        }
    }

    pub fn sprite_atlas(&self) -> &Arc<WgpuAtlas> {
        &self.atlas
    }

    pub fn supports_dual_source_blending(&self) -> bool {
        self.dual_source_blending
    }

    #[cfg(target_family = "wasm")]
    pub fn text_renderer_diagnostics(&self, scene: &Scene) -> TextRendererDiagnostics {
        let uses_subpixel_glyphs = !scene.subpixel_sprites.is_empty();
        TextRendererDiagnostics {
            antialiasing_mode: if uses_subpixel_glyphs {
                "subpixel"
            } else {
                "grayscale"
            },
            glyph_sprite_kind: if uses_subpixel_glyphs {
                "subpixel"
            } else {
                "monochrome"
            },
            monochrome_sprite_count: self.diagnostic_max_monochrome_sprite_count,
            glyph_mask_count: self.diagnostic_max_glyph_mask_count,
            image_mask_count: self.diagnostic_max_image_mask_count,
            subpixel_sprite_count: self.diagnostic_max_subpixel_sprite_count,
            polychrome_sprite_count: self.diagnostic_max_polychrome_sprite_count,
            device_pixel_ratio: scene.scale_factor,
            surface_format: format!("{:?}", self.surface_config.format),
        }
    }

    pub fn gpu_specs(&self) -> GpuSpecs {
        GpuSpecs {
            is_software_emulated: self.adapter_info.device_type == wgpu::DeviceType::Cpu,
            backend: self.adapter_info.backend.to_str().to_string(),
            backend_policy: self.backend_policy_label(),
            device_name: self.adapter_info.name.clone(),
            driver_name: self.adapter_info.driver.clone(),
            driver_info: self.adapter_info.driver_info.clone(),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn backend_policy_label(&self) -> String {
        backend_policy_label(self.backend_policy)
    }

    #[cfg(target_family = "wasm")]
    fn backend_policy_label(&self) -> String {
        backend_policy_label(wgpu::Backends::BROWSER_WEBGPU)
    }

    pub fn effect_capabilities(&self) -> EffectCapabilities {
        wgpu_effect_capabilities()
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    pub fn draw(&mut self, scene: &Scene) -> bool {
        #[cfg(target_family = "wasm")]
        {
            self.diagnostic_max_monochrome_sprite_count = self
                .diagnostic_max_monochrome_sprite_count
                .max(scene.monochrome_sprites.len());
            self.diagnostic_max_glyph_mask_count = self.diagnostic_max_glyph_mask_count.max(
                scene
                    .monochrome_sprites
                    .iter()
                    .filter(|sprite| sprite.kind == MonochromeSpriteKind::Glyph)
                    .count(),
            );
            self.diagnostic_max_image_mask_count = self.diagnostic_max_image_mask_count.max(
                scene
                    .monochrome_sprites
                    .iter()
                    .filter(|sprite| sprite.kind == MonochromeSpriteKind::ImageMask)
                    .count(),
            );
            self.diagnostic_max_subpixel_sprite_count = self
                .diagnostic_max_subpixel_sprite_count
                .max(scene.subpixel_sprites.len());
            self.diagnostic_max_polychrome_sprite_count = self
                .diagnostic_max_polychrome_sprite_count
                .max(scene.polychrome_sprites.len());
        }
        // Bail out early if the surface has been unconfigured (e.g. during
        // Android background/rotation transitions). Attempting to acquire
        // a texture from an unconfigured surface can block indefinitely on
        // some drivers (Adreno).
        if !self.surface_configured {
            return false;
        }

        if !self.prepare_frame() {
            return false;
        }

        #[cfg(target_family = "wasm")]
        self.prune_html_island_textures(scene);

        #[cfg(feature = "diagnostics")]
        {
            let atlas_uploads = self.atlas.before_frame();
            self.begin_frame_telemetry(scene, atlas_uploads);
        }
        #[cfg(not(feature = "diagnostics"))]
        #[cfg(feature = "diagnostics")]
        {
            let atlas_uploads = self.atlas.before_frame();
            self.begin_frame_telemetry(scene, atlas_uploads);
        }
        #[cfg(not(feature = "diagnostics"))]
        self.atlas.before_frame();

        let Some(frame) = self.acquire_surface_frame() else {
            return false;
        };

        self.ensure_intermediate_textures();
        self.write_globals();

        let frame_view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_target_format),
            ..Default::default()
        });

        let encoder = match self.encode_scene_to_view(scene, &frame_view) {
            Ok(encoder) => encoder,
            Err(error) => {
                log::error!("{error}");
                self.resources().queue.present(frame);
                return true;
            }
        };

        self.resources()
            .queue
            .submit(std::iter::once(encoder.finish()));
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.submission_count = telemetry.submission_count.saturating_add(1);
        });
        self.resources().queue.present(frame);
        true
    }

    pub fn render_to_image(&mut self, scene: &Scene) -> anyhow::Result<RgbaImage> {
        if !self.surface_configured {
            anyhow::bail!("Surface capture is unavailable while the surface is unconfigured")
        }

        if !self.prepare_frame() {
            anyhow::bail!("Surface capture is unavailable while the renderer is recovering")
        }

        if !self
            .surface_config
            .usage
            .contains(wgpu::TextureUsages::COPY_SRC)
        {
            // Some surfaces (GLES on Android emulators) cannot be read back; render the same
            // scene into a texture of the surface's size and format instead.
            return self.render_to_offscreen_image(scene);
        }

        self.atlas.before_frame();

        let Some(surface) = self.resources().surface.as_ref() else {
            anyhow::bail!("Surface capture requires a window surface")
        };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                self.reconfigure_surface();
                anyhow::bail!("Surface texture became suboptimal during capture")
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.reconfigure_surface();
                anyhow::bail!("Surface texture was lost during capture")
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                anyhow::bail!("Timed out acquiring a surface texture for capture")
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                anyhow::bail!("Surface capture is unavailable while the window is occluded")
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("Surface texture validation error during capture")
            }
        };

        self.ensure_intermediate_textures();
        self.write_globals();

        let frame_view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_target_format),
            ..Default::default()
        });
        let mut encoder = self.encode_scene_to_view(scene, &frame_view)?;

        let width = self.surface_config.width;
        let height = self.surface_config.height;
        let padded_bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback_size = padded_bytes_per_row as u64 * height as u64;
        let readback = self
            .resources()
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("capture_readback"),
                size: readback_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &frame.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.resources()
            .queue
            .submit(std::iter::once(encoder.finish()));
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.submission_count = telemetry.submission_count.saturating_add(1);
        });
        // Present rather than drop: Vulkan cannot return an acquired swapchain image without
        // presenting it, so every dropped capture would keep one until acquiring times out.
        // The frame holds the current scene, so presenting it changes nothing on screen.
        self.resources().queue.present(frame);

        self.read_buffer_to_image(readback, width, height, padded_bytes_per_row)
    }

    /// Renders `scene` into a texture like the surface (same size and format) and reads it back:
    /// [`Self::render_to_image`] for surfaces without `COPY_SRC`. The window keeps showing its
    /// last presented frame.
    fn render_to_offscreen_image(&mut self, scene: &Scene) -> anyhow::Result<RgbaImage> {
        let width = self.surface_config.width;
        let height = self.surface_config.height;
        let format = self.surface_config.format;
        let view_format = self.render_target_format;
        // Report what the device rejects instead of reading back an empty texture. Browsers
        // cannot wait for the scope, so there errors reach the device's error handler.
        #[cfg(not(target_family = "wasm"))]
        let error_scope = self
            .resources()
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let target = self
            .resources()
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("capture_target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                // TEXTURE_BINDING keeps GL from making the target a renderbuffer.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: if view_format == format {
                    &[]
                } else {
                    std::slice::from_ref(&view_format)
                },
            });
        self.atlas.before_frame();
        self.ensure_intermediate_textures();
        self.write_globals();
        let target_view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(view_format),
            ..Default::default()
        });
        let mut encoder = match self.encode_scene_to_view(scene, &target_view) {
            Ok(encoder) => encoder,
            Err(error) => {
                // Queue writes are staged before encoding; flush them even if the frame fails.
                self.resources().queue.submit(std::iter::empty());
                return Err(error);
            }
        };
        let padded_bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self
            .resources()
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("capture_readback"),
                size: padded_bytes_per_row as u64 * height as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.resources()
            .queue
            .submit(std::iter::once(encoder.finish()));
        #[cfg(not(target_family = "wasm"))]
        if let Some(error) = gpux::block_on(error_scope.pop()) {
            anyhow::bail!("offscreen capture failed: {error}");
        }
        self.read_buffer_to_image(readback, width, height, padded_bytes_per_row)
    }

    #[cfg(target_family = "wasm")]
    pub fn upload_html_island_element_to_texture(
        &mut self,
        id: &str,
        element: &web_sys::HtmlElement,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<HtmlIslandUploadResult> {
        let width = u32::try_from(size.width.0.max(0))
            .map_err(|_| anyhow::anyhow!("HTML island width exceeds u32"))?;
        let height = u32::try_from(size.height.0.max(0))
            .map_err(|_| anyhow::anyhow!("HTML island height exceeds u32"))?;
        anyhow::ensure!(
            width > 0 && height > 0,
            "HTML island upload requires non-zero dimensions"
        );

        let queue = Arc::clone(&self.resources().queue);
        let device = Arc::clone(&self.resources().device);
        let (texture, cache_status) = self.html_island_textures.get_or_create(
            id,
            HtmlIslandTextureDescriptor {
                size,
                format: wgpu::TextureFormat::Rgba8Unorm,
                upload_path: crate::HtmlIslandUploadPath::CopyElementImageToTexture,
            },
            &device,
        );

        let result = upload_html_island_element_to_texture(&queue, element, texture, size)?;
        #[cfg(feature = "diagnostics")]
        let result = {
            let mut result = result;
            result.texture_reallocated = cache_status != HtmlIslandTextureCacheStatus::Hit;
            result
        };
        #[cfg(not(feature = "diagnostics"))]
        let _ = cache_status;
        Ok(result)
    }

    #[cfg(all(target_family = "wasm", feature = "diagnostics"))]
    pub fn html_island_texture_diagnostics(&self) -> (u32, u32, u32) {
        (
            self.html_island_textures.len() as u32,
            self.html_island_textures.pruned_count(),
            self.html_island_texture_draw_count,
        )
    }

    #[cfg(target_family = "wasm")]
    pub fn discard_html_island_texture(&mut self, id: &str) {
        self.html_island_textures.remove(id);
    }

    #[cfg(target_family = "wasm")]
    fn prune_html_island_textures(&mut self, scene: &Scene) {
        let live_ids = scene
            .html_islands
            .iter()
            .map(|island| island.id.to_string())
            .collect::<Vec<_>>();
        self.html_island_textures
            .prune_except(live_ids.iter().map(String::as_str));
    }

    /// A renderer with no surface that draws into caller-provided targets of
    /// `target_format`, sharing the windowed renderer's pipelines, effects and encoding.
    #[cfg(all(
        not(target_family = "wasm"),
        any(test, feature = "bench-support", feature = "test-support")
    ))]
    fn new_surfaceless(
        context: &WgpuContext,
        target_format: wgpu::TextureFormat,
        path_sample_count: u32,
    ) -> Self {
        let atlas = Arc::new(WgpuAtlas::from_context(context));
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: target_format,
            width: 1,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: SURFACE_FRAME_LATENCY,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: Vec::new(),
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        Self::from_target(
            None,
            context,
            None,
            surface_config,
            target_format,
            (
                wgpu::CompositeAlphaMode::Opaque,
                wgpu::CompositeAlphaMode::Opaque,
            ),
            None,
            atlas,
            native_backend_policy(),
            Some(path_sample_count),
            None,
        )
    }

    /// Encodes `scene` into `target_view`, a texture of `size` in the renderer's target
    /// format, and submits it. Size changes rebuild the intermediate textures, as a window
    /// resize does.
    #[cfg(all(
        not(target_family = "wasm"),
        any(test, feature = "bench-support", feature = "test-support")
    ))]
    fn render_offscreen(
        &mut self,
        scene: &Scene,
        target_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<wgpu::SubmissionIndex> {
        let width = size.width.0 as u32;
        let height = size.height.0 as u32;
        if width != self.surface_config.width || height != self.surface_config.height {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.resources_mut().invalidate_intermediate_textures();
        }

        self.atlas.before_frame();
        self.ensure_intermediate_textures();
        self.write_globals();
        let encoder = self.encode_scene_to_view(scene, target_view);
        let encoder = match encoder {
            Ok(encoder) => encoder,
            Err(error) => {
                // Queue writes are staged before encoding; flush them even if the frame fails.
                self.resources().queue.submit(std::iter::empty());
                return Err(error);
            }
        };
        Ok(self
            .resources()
            .queue
            .submit(std::iter::once(encoder.finish())))
    }

    fn prepare_frame(&mut self) -> bool {
        let last_error = self.last_error.lock().unwrap().take();
        if let Some(error) = last_error {
            self.failed_frame_count += 1;
            log::error!(
                "GPU error during frame (failure {} of 10): {error}",
                self.failed_frame_count
            );

            // TBD. Does retrying more actually help?
            if self.failed_frame_count > 10 {
                panic!("Too many consecutive GPU errors. Last error: {error}");
            } else if self.failed_frame_count > 5 {
                if let Some(res) = self.resources.as_mut() {
                    res.invalidate_intermediate_textures();
                }
                self.atlas.clear();
                self.needs_redraw = true;
                self.failed_frame_count = 0;
                return false;
            }
        } else {
            self.failed_frame_count = 0;
        }

        true
    }

    fn reconfigure_surface(&mut self) {
        let surface_config = self.surface_config.clone();
        let resources = self.resources_mut();
        if let Some(surface) = &resources.surface {
            surface.configure(&resources.device, &surface_config);
        }
    }

    fn acquire_surface_frame(&mut self) -> Option<wgpu::SurfaceTexture> {
        let mut reconfigured = false;
        loop {
            let current = self.resources().surface.as_ref()?.get_current_texture();
            match current {
                wgpu::CurrentSurfaceTexture::Success(frame) => return Some(frame),
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) if reconfigured => {
                    self.needs_redraw = true;
                    return Some(frame);
                }
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    drop(frame);
                    self.reconfigure_surface();
                    reconfigured = true;
                }
                wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated
                    if !reconfigured =>
                {
                    self.reconfigure_surface();
                    reconfigured = true;
                }
                wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                    self.needs_redraw = true;
                    return None;
                }
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    self.needs_redraw = true;
                    return None;
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    *self.last_error.lock().unwrap() =
                        Some("Surface texture validation error".to_string());
                    return None;
                }
            }
        }
    }

    fn write_globals(&self) {
        let gamma_params = GammaParams {
            gamma_ratios: self.rendering_params.gamma_ratios,
            grayscale_enhanced_contrast: self.rendering_params.grayscale_enhanced_contrast,
            subpixel_enhanced_contrast: self.rendering_params.subpixel_enhanced_contrast,
            is_bgr: self.is_bgr as u32,
            _pad: 0,
        };

        let globals = GlobalParams {
            viewport_size: [
                self.surface_config.width as f32,
                self.surface_config.height as f32,
            ],
            premultiplied_alpha: if self.surface_config.alpha_mode
                == wgpu::CompositeAlphaMode::PreMultiplied
            {
                1
            } else {
                0
            },
            pad: 0,
        };

        let path_globals = GlobalParams {
            premultiplied_alpha: 0,
            ..globals
        };

        let resources = self.resources();
        resources
            .queue
            .write_buffer(&resources.globals_buffer, 0, bytemuck::bytes_of(&globals));
        resources.queue.write_buffer(
            &resources.globals_buffer,
            self.path_globals_offset,
            bytemuck::bytes_of(&path_globals),
        );
        resources.queue.write_buffer(
            &resources.gamma_buffer,
            0,
            bytemuck::bytes_of(&gamma_params),
        );
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.queue_buffer_write_count =
                telemetry.queue_buffer_write_count.saturating_add(3);
            telemetry.queue_buffer_write_bytes = telemetry
                .queue_buffer_write_bytes
                .saturating_add((size_of::<GlobalParams>() * 2 + size_of::<GammaParams>()) as u64);
        });
    }

    fn encode_scene_to_view(
        &mut self,
        scene: &Scene,
        target_view: &wgpu::TextureView,
    ) -> anyhow::Result<wgpu::CommandEncoder> {
        let width = self.surface_config.width;
        let height = self.surface_config.height;
        let format = self.render_target_format;
        let mut effect_targets = Some(
            self.resources_mut()
                .take_or_create_effect_targets(width, height, format),
        );

        loop {
            let mut instance_offset: u64 = 0;
            let mut gradient_stop_offset: u64 = 0;
            let mut overflow = false;
            let mut current_target = WgpuEffectTargetSlot::SceneColor;
            let mut next_effect_target = WgpuEffectTargetSlot::first_ping_pong();

            let mut encoder =
                self.resources()
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("main_encoder"),
                    });
            let device = Arc::clone(&self.resources().device);
            let captured_snapshot_ids = effect_targets
                .as_mut()
                .expect("effect targets should exist")
                .sync_screen_effect_snapshots(
                    &device,
                    &mut encoder,
                    &scene.active_screen_effect_snapshots,
                );

            {
                let render_target_view = &effect_targets
                    .as_ref()
                    .expect("effect targets should exist")
                    .target(current_target)
                    .view;
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("main_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: render_target_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    ..Default::default()
                });
                #[cfg(feature = "diagnostics")]
                self.update_frame_telemetry(|telemetry| {
                    telemetry.render_pass_count = telemetry.render_pass_count.saturating_add(1);
                });

                for batch in scene.batches() {
                    #[cfg(feature = "diagnostics")]
                    self.update_frame_telemetry(|telemetry| {
                        telemetry.batch_count = telemetry.batch_count.saturating_add(1);
                    });
                    let ok = match batch {
                        PrimitiveBatch::Quads(range) => self.draw_quads(
                            &scene.quads[range],
                            &mut instance_offset,
                            &mut gradient_stop_offset,
                            &mut pass,
                        ),
                        PrimitiveBatch::Shadows(range) => self.draw_shadows(
                            &scene.shadows[range],
                            &mut instance_offset,
                            &mut pass,
                        ),
                        PrimitiveBatch::VisualEffects(range) => {
                            let effects = &scene.visual_effects[range];
                            if effects.is_empty() {
                                true
                            } else {
                                drop(pass);

                                let source_target = current_target;
                                let destination_target = next_effect_target;
                                let targets = effect_targets
                                    .as_ref()
                                    .expect("effect targets should exist");
                                let did_draw = self.draw_visual_effects(
                                    &mut encoder,
                                    effects,
                                    &targets.target(source_target).view,
                                    &targets.target(destination_target).view,
                                    &targets.blur_color.view,
                                    &mut instance_offset,
                                    &mut gradient_stop_offset,
                                );
                                current_target = destination_target;
                                next_effect_target = current_target.next_ping_pong();

                                let targets = effect_targets
                                    .as_ref()
                                    .expect("effect targets should exist");
                                pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("main_pass_after_visual_effect"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &targets.target(current_target).view,
                                        resolve_target: None,
                                        ops: wgpu::Operations {
                                            load: wgpu::LoadOp::Load,
                                            store: wgpu::StoreOp::Store,
                                        },
                                        depth_slice: None,
                                    })],
                                    depth_stencil_attachment: None,
                                    ..Default::default()
                                });
                                #[cfg(feature = "diagnostics")]
                                self.update_frame_telemetry(|telemetry| {
                                    telemetry.render_pass_count =
                                        telemetry.render_pass_count.saturating_add(1);
                                });

                                did_draw
                            }
                        }
                        #[cfg(target_family = "wasm")]
                        PrimitiveBatch::HtmlIslands(range) => self.draw_html_islands(
                            &scene.html_islands[range],
                            &mut instance_offset,
                            &mut pass,
                        ),
                        #[cfg(not(target_family = "wasm"))]
                        PrimitiveBatch::HtmlIslands(_) => true,
                        PrimitiveBatch::Paths(range) => {
                            let paths = &scene.paths[range];
                            if paths.is_empty() {
                                continue;
                            }

                            drop(pass);

                            let did_draw = self.draw_paths_to_intermediate(
                                &mut encoder,
                                paths,
                                &mut instance_offset,
                                &mut gradient_stop_offset,
                            );

                            pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("main_pass_continued"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &effect_targets
                                        .as_ref()
                                        .expect("effect targets should exist")
                                        .target(current_target)
                                        .view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                    depth_slice: None,
                                })],
                                depth_stencil_attachment: None,
                                ..Default::default()
                            });
                            #[cfg(feature = "diagnostics")]
                            self.update_frame_telemetry(|telemetry| {
                                telemetry.render_pass_count =
                                    telemetry.render_pass_count.saturating_add(1);
                            });

                            if did_draw {
                                self.draw_paths_from_intermediate(
                                    paths,
                                    &mut instance_offset,
                                    &mut pass,
                                )
                            } else {
                                false
                            }
                        }
                        PrimitiveBatch::Underlines(range) => self.draw_underlines(
                            &scene.underlines[range],
                            &mut instance_offset,
                            &mut pass,
                        ),
                        PrimitiveBatch::MonochromeSprites { texture_id, range } => self
                            .draw_monochrome_sprites(
                                &scene.monochrome_sprites[range],
                                texture_id,
                                &mut instance_offset,
                                &mut pass,
                            ),
                        PrimitiveBatch::SubpixelSprites { texture_id, range } => self
                            .draw_subpixel_sprites(
                                &scene.subpixel_sprites[range],
                                texture_id,
                                &mut instance_offset,
                                &mut pass,
                            ),
                        PrimitiveBatch::PolychromeSprites { texture_id, range } => self
                            .draw_polychrome_sprites(
                                &scene.polychrome_sprites[range],
                                texture_id,
                                &mut instance_offset,
                                &mut pass,
                            ),
                        PrimitiveBatch::Surfaces(_surfaces) => {
                            // Surfaces are macOS-only for video playback.
                            true
                        }
                    };
                    if !ok {
                        overflow = true;
                        break;
                    }
                }
            }

            if !overflow && !scene.active_screen_effects.is_empty() {
                let source_target = current_target;
                let destination_target = next_effect_target;
                let targets = effect_targets
                    .as_ref()
                    .expect("effect targets should exist");
                let requested_snapshot_id =
                    first_snapshot_transition_id(&scene.active_screen_effects);
                let captured_snapshot = requested_snapshot_id.and_then(|snapshot_id| {
                    targets
                        .captured_snapshot_view(snapshot_id)
                        .map(|view| (snapshot_id, view))
                });
                let (available_snapshot_id, snapshot_view) =
                    if let Some((snapshot_id, snapshot_view)) = captured_snapshot {
                        (Some(snapshot_id), snapshot_view)
                    } else {
                        (None, &targets.target(source_target).view)
                    };
                let did_draw = self.draw_screen_effects(
                    &mut encoder,
                    scene,
                    &scene.active_screen_effects,
                    &targets.target(source_target).view,
                    snapshot_view,
                    available_snapshot_id,
                    &targets.target(destination_target).view,
                    &mut instance_offset,
                );
                if did_draw {
                    current_target = destination_target;
                } else {
                    overflow = true;
                }
            }

            if overflow {
                #[cfg(feature = "diagnostics")]
                self.update_frame_telemetry(|telemetry| {
                    telemetry.buffer_restart_count =
                        telemetry.buffer_restart_count.saturating_add(1);
                });
                effect_targets
                    .as_mut()
                    .expect("effect targets should exist")
                    .mark_snapshots_uncaptured(&captured_snapshot_ids);
                drop(encoder);
                if self.instance_buffer_capacity >= self.max_buffer_size {
                    anyhow::bail!(
                        "instance buffer size grew too large: {}",
                        self.instance_buffer_capacity
                    );
                }

                self.grow_instance_buffer();
                self.grow_gradient_stop_buffer();
                continue;
            }

            effect_targets
                .as_mut()
                .expect("effect targets should exist")
                .remember_presented_scene(&mut encoder, current_target);

            self.draw_post_process_pass_through(
                &mut encoder,
                &effect_targets
                    .as_ref()
                    .expect("effect targets should exist")
                    .target(current_target)
                    .view,
                target_view,
            );

            if let Some(targets) = effect_targets.take() {
                self.resources_mut().effect_targets = Some(targets);
            }

            return Ok(encoder);
        }
    }

    fn read_buffer_to_image(
        &self,
        readback: wgpu::Buffer,
        width: u32,
        height: u32,
        padded_bytes_per_row: u32,
    ) -> anyhow::Result<RgbaImage> {
        let slice = readback.slice(..);
        let (tx, rx) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        self.resources()
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|error| {
                anyhow::anyhow!("Failed to poll GPU for capture readback: {error:?}")
            })?;

        rx.recv()
            .map_err(|_| anyhow::anyhow!("Capture readback channel closed"))?
            .map_err(|error| anyhow::anyhow!("Failed to map capture readback buffer: {error:?}"))?;

        let bytes_per_row = width as usize * 4;
        let padded_bytes_per_row = padded_bytes_per_row as usize;
        let mapped = slice.get_mapped_range().map_err(|error| {
            anyhow::anyhow!("Failed to read capture readback buffer: {error:?}")
        })?;
        let mut pixels = vec![0u8; bytes_per_row * height as usize];

        for y in 0..height as usize {
            let src_offset = y * padded_bytes_per_row;
            let dst_offset = y * bytes_per_row;
            pixels[dst_offset..dst_offset + bytes_per_row]
                .copy_from_slice(&mapped[src_offset..src_offset + bytes_per_row]);
        }

        drop(mapped);
        readback.unmap();

        if matches!(
            self.surface_config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }

        RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("Failed to build an RGBA image from capture pixels"))
    }
    fn draw_quads(
        &self,
        quads: &[Quad],
        instance_offset: &mut u64,
        gradient_stop_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        if quads.is_empty() {
            return true;
        }

        let (packed_quads, packed_stops) = pack_quads(quads);
        let quad_data = unsafe { Self::instance_bytes(&packed_quads) };
        let stop_data = unsafe { Self::instance_bytes(&packed_stops) };
        let Some((quad_offset, quad_size)) =
            self.write_to_instance_buffer(instance_offset, quad_data)
        else {
            return false;
        };
        let Some((stop_offset, stop_size)) =
            self.write_to_gradient_stop_buffer(gradient_stop_offset, stop_data)
        else {
            return false;
        };

        let quad_binding = self.instance_binding(quad_offset, quad_size, quad_data);
        let stop_binding = self.gradient_stop_binding(stop_offset, stop_size, stop_data);
        let resources = self.resources();
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("quad_instances_bind_group"),
                layout: &resources.bind_group_layouts.instances,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: quad_binding.resource(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: stop_binding.resource(),
                    },
                ],
            });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.bind_group_count = telemetry.bind_group_count.saturating_add(1);
        });
        pass.set_pipeline(&resources.pipelines.quads);
        pass.set_bind_group(0, &resources.globals_bind_group, &[]);
        pass.set_bind_group(1, &bind_group, &[]);
        pass.draw(0..4, 0..packed_quads.len() as u32);
        true
    }

    fn draw_shadows(
        &self,
        shadows: &[Shadow],
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let data = unsafe { Self::instance_bytes(shadows) };
        self.draw_instances(
            data,
            shadows.len() as u32,
            &self.resources().pipelines.shadows,
            instance_offset,
            pass,
        )
    }

    fn draw_underlines(
        &self,
        underlines: &[Underline],
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let data = unsafe { Self::instance_bytes(underlines) };
        self.draw_instances(
            data,
            underlines.len() as u32,
            &self.resources().pipelines.underlines,
            instance_offset,
            pass,
        )
    }

    fn draw_monochrome_sprites(
        &self,
        sprites: &[MonochromeSprite],
        texture_id: AtlasTextureId,
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        // The atlas has released this texture; the batch belongs to a stale
        // paint that will be replaced once its view re-renders.
        let Some(tex_info) = self.atlas.get_texture_info(texture_id) else {
            return true;
        };
        let data = unsafe { Self::instance_bytes(sprites) };
        self.draw_instances_with_texture(
            data,
            sprites.len() as u32,
            &tex_info.view,
            &self.resources().pipelines.mono_sprites,
            instance_offset,
            pass,
        )
    }

    fn draw_subpixel_sprites(
        &self,
        sprites: &[SubpixelSprite],
        texture_id: AtlasTextureId,
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        // The atlas has released this texture; the batch belongs to a stale
        // paint that will be replaced once its view re-renders.
        let Some(tex_info) = self.atlas.get_texture_info(texture_id) else {
            return true;
        };
        let data = unsafe { Self::instance_bytes(sprites) };
        let resources = self.resources();
        let pipeline = resources
            .pipelines
            .subpixel_sprites
            .as_ref()
            .unwrap_or(&resources.pipelines.mono_sprites);
        self.draw_instances_with_texture(
            data,
            sprites.len() as u32,
            &tex_info.view,
            pipeline,
            instance_offset,
            pass,
        )
    }

    fn draw_polychrome_sprites(
        &self,
        sprites: &[PolychromeSprite],
        texture_id: AtlasTextureId,
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        // The atlas has released this texture; the batch belongs to a stale
        // paint that will be replaced once its view re-renders.
        let Some(tex_info) = self.atlas.get_texture_info(texture_id) else {
            return true;
        };
        let data = unsafe { Self::instance_bytes(sprites) };
        self.draw_instances_with_texture(
            data,
            sprites.len() as u32,
            &tex_info.view,
            &self.resources().pipelines.poly_sprites,
            instance_offset,
            pass,
        )
    }

    #[cfg(target_family = "wasm")]
    fn draw_html_islands(
        &mut self,
        islands: &[HtmlIslandPrimitive],
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let mut drawn_count = 0u32;
        {
            let resources = self.resources();
            for island in islands {
                if !html_island_has_drawable_area(island.bounds, island.content_mask.bounds) {
                    continue;
                }
                let Some(texture) = self
                    .html_island_textures
                    .texture_view(&island.id.to_string())
                else {
                    continue;
                };
                let packed = PackedHtmlIsland {
                    bounds: island.bounds.into(),
                    content_mask: pack_content_mask(island.content_mask),
                    opacity: island.opacity,
                    _pad: [0.; 3],
                };
                let islands = [packed];
                let data = unsafe { Self::instance_bytes(&islands) };
                if !self.draw_instances_with_texture(
                    data,
                    1,
                    &texture.view,
                    &resources.pipelines.html_islands,
                    instance_offset,
                    pass,
                ) {
                    return false;
                }
                drawn_count = drawn_count.saturating_add(1);
            }
        }
        self.html_island_texture_draw_count = self
            .html_island_texture_draw_count
            .saturating_add(drawn_count);
        true
    }

    fn draw_instances(
        &self,
        data: &[u8],
        instance_count: u32,
        pipeline: &wgpu::RenderPipeline,
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        if instance_count == 0 {
            return true;
        }
        let Some((offset, size)) = self.write_to_instance_buffer(instance_offset, data) else {
            return false;
        };
        let instance_binding = self.instance_binding(offset, size, data);
        let empty_binding = self.empty_gradient_stop_binding();
        let resources = self.resources();
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &resources.bind_group_layouts.instances,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: instance_binding.resource(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: empty_binding.resource(),
                    },
                ],
            });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.bind_group_count = telemetry.bind_group_count.saturating_add(1);
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &resources.globals_bind_group, &[]);
        pass.set_bind_group(1, &bind_group, &[]);
        pass.draw(0..4, 0..instance_count);
        true
    }

    fn draw_instances_with_texture(
        &self,
        data: &[u8],
        instance_count: u32,
        texture_view: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        if instance_count == 0 {
            return true;
        }
        let Some((offset, size)) = self.write_to_instance_buffer(instance_offset, data) else {
            return false;
        };
        let instance_binding = self.instance_binding(offset, size, data);
        let empty_binding = self.empty_gradient_stop_binding();
        let resources = self.resources();
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &resources.bind_group_layouts.instances_with_texture,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: instance_binding.resource(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&resources.atlas_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: empty_binding.resource(),
                    },
                ],
            });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.bind_group_count = telemetry.bind_group_count.saturating_add(1);
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &resources.globals_bind_group, &[]);
        pass.set_bind_group(1, &bind_group, &[]);
        pass.draw(0..4, 0..instance_count);
        true
    }

    fn draw_post_process_pass_through(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
    ) {
        let resources = self.resources();
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post_process_pass_through_bind_group"),
                layout: &resources.bind_group_layouts.post_process,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&resources.post_process_sampler),
                    },
                ],
            });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.bind_group_count = telemetry.bind_group_count.saturating_add(1);
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post_process_pass_through_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.render_pass_count = telemetry.render_pass_count.saturating_add(1);
        });
        pass.set_pipeline(&resources.pipelines.post_process_pass_through);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..4, 0..1);
    }

    fn draw_post_process_filter(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        label: &'static str,
    ) {
        let resources = self.resources();
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &resources.bind_group_layouts.post_process,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&resources.effect_sampler),
                    },
                ],
            });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.bind_group_count = telemetry.bind_group_count.saturating_add(1);
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.render_pass_count = telemetry.render_pass_count.saturating_add(1);
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..4, 0..1);
    }

    fn draw_visual_effects(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        effects: &[VisualEffectPrimitive],
        source_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
        blur_view: &wgpu::TextureView,
        instance_offset: &mut u64,
        effect_offset: &mut u64,
    ) -> bool {
        let packed = pack_visual_effect_primitives(effects);
        if packed.primitives.is_empty() || packed.effects.is_empty() {
            self.draw_post_process_pass_through(encoder, source_view, target_view);
            return true;
        }
        let has_glow = packed
            .effects
            .iter()
            .any(|effect| effect.kind == VISUAL_EFFECT_KIND_GLOW);
        let has_filter_effect = packed
            .effects
            .iter()
            .any(|effect| effect.kind != VISUAL_EFFECT_KIND_GLOW);

        let primitive_data = unsafe { Self::instance_bytes(&packed.primitives) };
        let effect_data = unsafe { Self::instance_bytes(&packed.effects) };
        let Some((primitive_offset, primitive_size)) =
            self.write_to_instance_buffer(instance_offset, primitive_data)
        else {
            return false;
        };
        let Some((effects_offset, effects_size)) =
            self.write_to_gradient_stop_buffer(effect_offset, effect_data)
        else {
            return false;
        };

        let primitive_binding =
            self.instance_binding(primitive_offset, primitive_size, primitive_data);
        let effects_binding = self.gradient_stop_binding(effects_offset, effects_size, effect_data);
        let resources = self.resources();
        if has_filter_effect {
            self.draw_post_process_filter(
                encoder,
                source_view,
                target_view,
                &resources.pipelines.post_process_gaussian_blur_horizontal,
                "post_process_gaussian_blur_horizontal_pass",
            );
            self.draw_post_process_filter(
                encoder,
                target_view,
                blur_view,
                &resources.pipelines.post_process_gaussian_blur_vertical,
                "post_process_gaussian_blur_vertical_pass",
            );
        }
        self.draw_post_process_pass_through(encoder, source_view, target_view);

        let resources = self.resources();
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("visual_effects_bind_group"),
                layout: &resources.bind_group_layouts.visual_effects,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: primitive_binding.resource(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: effects_binding.resource(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&resources.effect_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(blur_view),
                    },
                ],
            });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.bind_group_count = telemetry.bind_group_count.saturating_add(1);
        });

        if has_filter_effect {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visual_effects_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            #[cfg(feature = "diagnostics")]
            self.update_frame_telemetry(|telemetry| {
                telemetry.render_pass_count = telemetry.render_pass_count.saturating_add(1);
            });
            pass.set_pipeline(&resources.pipelines.visual_effects);
            pass.set_bind_group(0, &resources.globals_bind_group, &[]);
            pass.set_bind_group(1, &bind_group, &[]);
            pass.draw(0..4, 0..packed.primitives.len() as u32);
        }

        if has_glow {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visual_effect_glow_add_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            #[cfg(feature = "diagnostics")]
            self.update_frame_telemetry(|telemetry| {
                telemetry.render_pass_count = telemetry.render_pass_count.saturating_add(1);
            });
            pass.set_pipeline(&resources.pipelines.visual_effect_glows);
            pass.set_bind_group(0, &resources.globals_bind_group, &[]);
            pass.set_bind_group(1, &bind_group, &[]);
            pass.draw(0..4, 0..packed.primitives.len() as u32);
        }
        true
    }

    fn draw_screen_effects(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        effects: &[ActiveScreenEffect],
        source_view: &wgpu::TextureView,
        snapshot_view: &wgpu::TextureView,
        available_snapshot_id: Option<ScreenEffectSnapshotId>,
        target_view: &wgpu::TextureView,
        instance_offset: &mut u64,
    ) -> bool {
        let packed =
            pack_screen_effects(scene, effects, self.viewport_size(), available_snapshot_id);
        log::trace!(
            "drawing screen effects active={} packed={} snapshot={:?}",
            effects.len(),
            packed.effects.len(),
            packed.snapshot_id.map(ScreenEffectSnapshotId::as_u64)
        );

        if packed.effects.is_empty() {
            self.draw_post_process_pass_through(encoder, source_view, target_view);
            return true;
        }

        let effect_data = unsafe { Self::instance_bytes(&packed.effects) };
        let Some((effect_offset, effect_size)) =
            self.write_to_instance_buffer(instance_offset, effect_data)
        else {
            return false;
        };

        let effect_binding = self.instance_binding(effect_offset, effect_size, effect_data);
        let resources = self.resources();
        let mut bind_group_entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: effect_binding.resource(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(source_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&resources.effect_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(snapshot_view),
            },
        ];
        if self.uses_webgl_instance_data {
            bind_group_entries.push(wgpu::BindGroupEntry {
                binding: 5,
                resource: effect_binding.resource(),
            });
        }
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("screen_effects_bind_group"),
                layout: &resources.bind_group_layouts.screen_effects,
                entries: &bind_group_entries,
            });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.bind_group_count = telemetry.bind_group_count.saturating_add(1);
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen_effects_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.render_pass_count = telemetry.render_pass_count.saturating_add(1);
        });
        pass.set_pipeline(&resources.pipelines.screen_effects);
        pass.set_bind_group(0, &resources.globals_bind_group, &[]);
        pass.set_bind_group(1, &bind_group, &[]);
        pass.draw(0..4, 0..1);
        true
    }

    unsafe fn instance_bytes<T>(instances: &[T]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                instances.as_ptr() as *const u8,
                std::mem::size_of_val(instances),
            )
        }
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let first_path = &paths[0];
        let sprites: Vec<PathSprite> = if paths.last().map(|p| &p.order) == Some(&first_path.order)
        {
            paths
                .iter()
                .map(|p| PathSprite {
                    bounds: p.clipped_bounds().into(),
                })
                .collect()
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            vec![PathSprite {
                bounds: bounds.into(),
            }]
        };

        let resources = self.resources();
        let Some(path_intermediate_view) = resources.path_intermediate_view.as_ref() else {
            return true;
        };

        let sprite_data = unsafe { Self::instance_bytes(&sprites) };
        self.draw_instances_with_texture(
            sprite_data,
            sprites.len() as u32,
            path_intermediate_view,
            &resources.pipelines.paths,
            instance_offset,
            pass,
        )
    }

    fn draw_paths_to_intermediate(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        paths: &[Path<ScaledPixels>],
        instance_offset: &mut u64,
        gradient_stop_offset: &mut u64,
    ) -> bool {
        let mut vertices = Vec::new();
        let mut gradient_stops = Vec::new();
        for path in paths {
            let background = pack_background(&path.color, &mut gradient_stops);
            vertices.extend(path.vertices.iter().map(|v| PackedPathRasterizationVertex {
                xy_position: pack_point(v.xy_position),
                st_position: pack_point_f32(v.st_position),
                background,
                content_mask: pack_content_mask(v.content_mask),
            }));
        }

        if vertices.is_empty() {
            return true;
        }

        let vertex_data = unsafe { Self::instance_bytes(&vertices) };
        let Some((vertex_offset, vertex_size)) =
            self.write_to_instance_buffer(instance_offset, vertex_data)
        else {
            return false;
        };
        let stop_data = unsafe { Self::instance_bytes(&gradient_stops) };
        let Some((stop_offset, stop_size)) =
            self.write_to_gradient_stop_buffer(gradient_stop_offset, stop_data)
        else {
            return false;
        };

        let vertex_binding = self.instance_binding(vertex_offset, vertex_size, vertex_data);
        let stop_binding = self.gradient_stop_binding(stop_offset, stop_size, stop_data);
        let resources = self.resources();
        let data_bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("path_rasterization_bind_group"),
                layout: &resources.bind_group_layouts.instances,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: vertex_binding.resource(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: stop_binding.resource(),
                    },
                ],
            });
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.bind_group_count = telemetry.bind_group_count.saturating_add(1);
        });

        let Some(path_intermediate_view) = resources.path_intermediate_view.as_ref() else {
            return true;
        };

        let (target_view, resolve_target) = if let Some(ref msaa_view) = resources.path_msaa_view {
            (msaa_view, Some(path_intermediate_view))
        } else {
            (path_intermediate_view, None)
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("path_rasterization_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            #[cfg(feature = "diagnostics")]
            self.update_frame_telemetry(|telemetry| {
                telemetry.render_pass_count = telemetry.render_pass_count.saturating_add(1);
            });

            pass.set_pipeline(&resources.pipelines.path_rasterization);
            pass.set_bind_group(0, &resources.path_globals_bind_group, &[]);
            pass.set_bind_group(1, &data_bind_group, &[]);
            pass.draw(0..vertices.len() as u32, 0..1);
        }

        true
    }

    fn grow_instance_buffer(&mut self) {
        let new_capacity = (self.instance_buffer_capacity * 2).min(self.max_buffer_size);
        log::info!("increased instance buffer size to {}", new_capacity);
        let resources = self.resources_mut();
        resources.instance_buffer = Some(resources.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance_buffer"),
            size: new_capacity,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.instance_buffer_capacity = new_capacity;
    }

    fn grow_gradient_stop_buffer(&mut self) {
        let new_capacity = (self.gradient_stop_buffer_capacity * 2).min(self.max_buffer_size);
        log::info!("increased gradient stop buffer size to {}", new_capacity);
        let resources = self.resources_mut();
        resources.gradient_stop_buffer =
            Some(resources.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gradient_stop_buffer"),
                size: new_capacity,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        self.gradient_stop_buffer_capacity = new_capacity;
    }

    fn write_to_instance_buffer(
        &self,
        instance_offset: &mut u64,
        data: &[u8],
    ) -> Option<(u64, NonZeroU64)> {
        if self.uses_webgl_instance_data {
            let size = (data.len() as u64).max(INSTANCE_TEXTURE_TEXEL_SIZE);
            return Some((0, NonZeroU64::new(size).unwrap()));
        }
        let offset = (*instance_offset).next_multiple_of(self.storage_buffer_alignment);
        let size = (data.len() as u64).max(16);
        if offset + size > self.instance_buffer_capacity {
            return None;
        }
        let resources = self.resources();
        resources.queue.write_buffer(
            resources
                .instance_buffer
                .as_ref()
                .expect("storage instance buffer is available"),
            offset,
            data,
        );
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.instance_bytes_written = telemetry
                .instance_bytes_written
                .saturating_add(data.len() as u64);
            telemetry.queue_buffer_write_count =
                telemetry.queue_buffer_write_count.saturating_add(1);
            telemetry.queue_buffer_write_bytes = telemetry
                .queue_buffer_write_bytes
                .saturating_add(data.len() as u64);
        });
        *instance_offset = offset + size;
        Some((offset, NonZeroU64::new(size).expect("size is at least 16")))
    }

    fn write_to_gradient_stop_buffer(
        &self,
        gradient_stop_offset: &mut u64,
        data: &[u8],
    ) -> Option<(u64, NonZeroU64)> {
        if self.uses_webgl_instance_data {
            let size = (data.len() as u64).max(INSTANCE_TEXTURE_TEXEL_SIZE);
            return Some((0, NonZeroU64::new(size).unwrap()));
        }
        if data.is_empty() {
            return Some((0, NonZeroU64::new(MIN_GRADIENT_STOP_BINDING_SIZE).unwrap()));
        }

        let offset = (*gradient_stop_offset).next_multiple_of(self.storage_buffer_alignment);
        let size = (data.len() as u64).max(MIN_GRADIENT_STOP_BINDING_SIZE);
        if offset + size > self.gradient_stop_buffer_capacity {
            return None;
        }

        let resources = self.resources();
        resources.queue.write_buffer(
            resources
                .gradient_stop_buffer
                .as_ref()
                .expect("storage auxiliary buffer is available"),
            offset,
            data,
        );
        #[cfg(feature = "diagnostics")]
        self.update_frame_telemetry(|telemetry| {
            telemetry.auxiliary_bytes_written = telemetry
                .auxiliary_bytes_written
                .saturating_add(data.len() as u64);
            telemetry.queue_buffer_write_count =
                telemetry.queue_buffer_write_count.saturating_add(1);
            telemetry.queue_buffer_write_bytes = telemetry
                .queue_buffer_write_bytes
                .saturating_add(data.len() as u64);
        });
        *gradient_stop_offset = offset + size;
        Some((
            offset,
            NonZeroU64::new(size).expect("size is at least one packed gradient stop"),
        ))
    }

    fn instance_binding<'a>(
        &'a self,
        offset: u64,
        size: NonZeroU64,
        data: &[u8],
    ) -> WgpuDataBinding<'a> {
        self.data_binding(
            self.resources().instance_buffer.as_ref(),
            offset,
            size,
            data,
            "instance_texture",
        )
    }

    fn gradient_stop_binding<'a>(
        &'a self,
        offset: u64,
        size: NonZeroU64,
        data: &[u8],
    ) -> WgpuDataBinding<'a> {
        self.data_binding(
            self.resources().gradient_stop_buffer.as_ref(),
            offset,
            size,
            data,
            "aux_instance_texture",
        )
    }

    fn empty_gradient_stop_binding(&self) -> WgpuDataBinding<'_> {
        self.gradient_stop_binding(
            0,
            NonZeroU64::new(MIN_GRADIENT_STOP_BINDING_SIZE).unwrap(),
            &[],
        )
    }

    fn data_binding<'a>(
        &'a self,
        buffer: Option<&'a wgpu::Buffer>,
        offset: u64,
        size: NonZeroU64,
        data: &[u8],
        label: &str,
    ) -> WgpuDataBinding<'a> {
        if !self.uses_webgl_instance_data {
            return WgpuDataBinding::Buffer {
                buffer: buffer.expect("storage data buffer is available"),
                offset,
                size,
            };
        }

        let resources = self.resources();
        let texel_count = size.get().div_ceil(INSTANCE_TEXTURE_TEXEL_SIZE);
        let max_dimension = u64::from(resources.device.limits().max_texture_dimension_2d);
        let width = texel_count.min(max_dimension).max(1) as u32;
        let height = texel_count.div_ceil(u64::from(width)).max(1) as u32;
        let texture = resources.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Self::write_instance_texture(&resources.queue, &texture, width, height, data);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        WgpuDataBinding::Texture {
            _texture: texture,
            view,
        }
    }

    fn write_instance_texture(
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
        data: &[u8],
    ) {
        let mut byte_offset = 0usize;
        let mut texel_offset = 0u64;
        let zero_texel = [0u8; INSTANCE_TEXTURE_TEXEL_SIZE as usize];
        let bytes = if data.is_empty() {
            &zero_texel[..]
        } else {
            data
        };
        while byte_offset < bytes.len() {
            let x = (texel_offset % u64::from(width)) as u32;
            let y = (texel_offset / u64::from(width)) as u32;
            if y >= height {
                log::error!("instance texture write exceeded its allocation");
                return;
            }
            let available_texels = u64::from(width - x);
            let remaining_bytes = bytes.len() - byte_offset;
            let complete_texels = remaining_bytes as u64 / INSTANCE_TEXTURE_TEXEL_SIZE;
            let texels = complete_texels.min(available_texels);
            if texels > 0 {
                let byte_count = (texels * INSTANCE_TEXTURE_TEXEL_SIZE) as usize;
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &bytes[byte_offset..byte_offset + byte_count],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(byte_count as u32),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: texels as u32,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                byte_offset += byte_count;
                texel_offset += texels;
            } else {
                let mut final_texel = [0u8; INSTANCE_TEXTURE_TEXEL_SIZE as usize];
                final_texel[..remaining_bytes].copy_from_slice(&bytes[byte_offset..]);
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &final_texel,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(INSTANCE_TEXTURE_TEXEL_SIZE as u32),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                break;
            }
        }
    }

    /// Mark the surface as unconfigured so rendering is skipped until a new
    /// surface is provided via [`replace_surface`](Self::replace_surface).
    ///
    /// This does **not** drop the renderer — the device, queue, atlas, and
    /// pipelines stay alive.  Use this when the native window is destroyed
    /// (e.g. Android `TerminateWindow`) but you intend to re-create the
    /// surface later without losing cached atlas textures.
    pub fn unconfigure_surface(&mut self) {
        self.surface_configured = false;
        // Drop intermediate textures since they reference the old surface size.
        if let Some(res) = self.resources.as_mut() {
            res.invalidate_intermediate_textures();
        }
    }

    /// Replace the wgpu surface with a new one (e.g. after Android destroys
    /// and recreates the native window).  Keeps the device, queue, atlas, and
    /// all pipelines intact so cached `AtlasTextureId`s remain valid.
    ///
    /// The `instance` **must** be the same [`wgpu::Instance`] that was used to
    /// create the adapter and device (i.e. from the [`WgpuContext`]).  Using a
    /// different instance will cause a "Device does not exist" panic because
    /// the wgpu device is bound to its originating instance.
    #[cfg(not(target_family = "wasm"))]
    pub fn replace_surface<W: HasWindowHandle>(
        &mut self,
        window: &W,
        config: WgpuSurfaceConfig,
        instance: &wgpu::Instance,
    ) -> anyhow::Result<()> {
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let surface = create_surface(instance, window_handle.as_raw())?;
        let surface_caps = {
            let context = self
                .context
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("GPU context missing during surface replacement"))?;
            let context = context.borrow();
            let context = context.as_ref().ok_or_else(|| {
                anyhow::anyhow!("GPU context unavailable during surface replacement")
            })?;
            surface.get_capabilities(&context.adapter)
        };

        if !surface_caps.formats.contains(&self.surface_config.format) {
            anyhow::bail!(
                "Replacement surface does not support the renderer surface format {:?}",
                self.surface_config.format
            );
        }

        let width = (config.size.width.0 as u32).max(1);
        let height = (config.size.height.0 as u32).max(1);

        let alpha_mode = if config.transparent {
            self.transparent_alpha_mode
        } else {
            self.opaque_alpha_mode
        };

        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface_config.alpha_mode = if surface_caps.alpha_modes.contains(&alpha_mode) {
            alpha_mode
        } else {
            surface_caps
                .alpha_modes
                .first()
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Replacement surface reports no alpha modes"))?
        };

        self.surface_config.present_mode = choose_present_mode(
            &surface_caps,
            config
                .preferred_present_mode
                .or(Some(self.surface_config.present_mode)),
        );

        let preferred_surface_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        self.surface_config.usage = if surface_caps.usages.contains(preferred_surface_usage) {
            preferred_surface_usage
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };

        {
            let res = self
                .resources
                .as_mut()
                .expect("GPU resources not available");
            surface.configure(&res.device, &self.surface_config);
            res.surface = Some(surface);

            // Invalidate intermediate textures — they'll be recreated lazily.
            res.invalidate_intermediate_textures();
        }

        self.surface_configured = true;

        Ok(())
    }

    pub fn destroy(&mut self) {
        // Release surface-bound GPU resources eagerly so the underlying native
        // window can be destroyed before the renderer itself is dropped.
        self.resources.take();
    }

    /// Returns true if the GPU device was lost and recovery is needed.
    pub fn device_lost(&self) -> bool {
        self.device_lost.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Returns true if a redraw is needed because GPU state was cleared.
    /// Calling this method clears the flag.
    pub fn needs_redraw(&mut self) -> bool {
        std::mem::take(&mut self.needs_redraw)
    }

    /// Recovers from a lost GPU device by recreating the renderer with a new context.
    ///
    /// Call this after detecting `device_lost()` returns true.
    ///
    /// This method coordinates recovery across multiple windows:
    /// - The first window to call this will recreate the shared context
    /// - Subsequent windows will adopt the already-recovered context
    #[cfg(not(target_family = "wasm"))]
    pub fn recover<W>(&mut self, window: &W) -> anyhow::Result<()>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        let gpu_context = self.context.as_ref().expect("recover requires gpu_context");

        // Check if another window already recovered the context
        let needs_new_context = gpu_context
            .borrow()
            .as_ref()
            .is_none_or(|ctx| ctx.device_lost());

        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let surface = if needs_new_context {
            log::warn!("GPU device lost, recreating context...");

            // Drop old resources to release Arc<Device>/Arc<Queue> and GPU resources
            self.resources = None;
            *gpu_context.borrow_mut() = None;

            // Wait briefly for the GPU driver to stabilize, then try to
            // recreate the context without software renderers. If this fails
            // the caller should request another frame and retry — the real GPU
            // may need more time to come back (e.g. after suspend/resume).
            std::thread::sleep(std::time::Duration::from_millis(350));

            let instance = WgpuContext::instance_with_backend_policy(
                Box::new(window.clone()),
                self.backend_policy,
            );
            let surface = create_surface(&instance, window_handle.as_raw())?;
            let new_context = WgpuContext::new_rejecting_software_with_backend_policy(
                instance,
                &surface,
                self.compositor_gpu,
                self.backend_policy,
            )?;
            *gpu_context.borrow_mut() = Some(new_context);
            surface
        } else {
            let ctx_ref = gpu_context.borrow();
            let instance = &ctx_ref.as_ref().unwrap().instance;
            create_surface(instance, window_handle.as_raw())?
        };

        let config = WgpuSurfaceConfig {
            size: gpux::Size {
                width: gpux::DevicePixels(self.surface_config.width as i32),
                height: gpux::DevicePixels(self.surface_config.height as i32),
            },
            transparent: self.surface_config.alpha_mode != wgpu::CompositeAlphaMode::Opaque,
            preferred_present_mode: Some(self.surface_config.present_mode),
        };
        let gpu_context = Rc::clone(gpu_context);
        let ctx_ref = gpu_context.borrow();
        let context = ctx_ref.as_ref().expect("context should exist");

        self.resources = None;
        self.atlas.handle_device_lost(context);

        *self = Self::new_internal(
            Some(gpu_context.clone()),
            context,
            surface,
            config,
            self.compositor_gpu,
            self.atlas.clone(),
            self.backend_policy,
            None,
        )?;

        log::info!("GPU recovery complete");
        Ok(())
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
struct HeadlessRenderTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Renders scenes into offscreen textures and reads them back, with no window or surface.
///
/// gpux port of Zed's Linux headless renderer (#64718, #64842), extended to Windows (DX12,
/// WARP when there is no GPU). It shares the windowed renderer's pipelines, effects, atlas
/// and paint contract: ordinary paint is premultiplied encoded sRGB rendered to a non-sRGB
/// target. Images are opaque (transparent pixels composite over black) and paths always use
/// 4x multisampling, so captures do not depend on the window or the adapter's options.
#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
pub struct WgpuHeadlessRenderer {
    context: WgpuContext,
    renderer: WgpuRenderer,
    target: Option<HeadlessRenderTarget>,
    errors: Arc<crate::DeviceErrorState>,
    observed_error_generation: u64,
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl WgpuHeadlessRenderer {
    /// A renderer on the adapter that `GPUX_WGPU_ADAPTER` selects (`software`, `hardware`,
    /// or a name substring); the best adapter of the native backend policy when it is unset.
    pub fn new() -> anyhow::Result<Self> {
        Self::with_adapter(&crate::HeadlessAdapterSelection::from_env())
    }

    /// A renderer on the adapter `selection` names, ignoring `GPUX_WGPU_ADAPTER`.
    pub fn with_adapter(selection: &crate::HeadlessAdapterSelection) -> anyhow::Result<Self> {
        let (context, target_format) = WgpuContext::new_headless(selection)?;
        let errors = context
            .shared_errors()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("headless context without a device error state"))?;
        // The device may be shared with earlier renderers: observe only errors raised from
        // here on, including any while this renderer builds its pipelines.
        let observed_error_generation = errors.current_generation();
        let renderer = WgpuRenderer::new_surfaceless(
            &context,
            target_format,
            crate::wgpu_context::HEADLESS_PATH_SAMPLE_COUNT,
        );
        Ok(Self {
            context,
            renderer,
            target: None,
            errors,
            observed_error_generation,
        })
    }

    /// The device this renderer draws with, for tests and tools that need raw GPU access.
    pub fn device(&self) -> &wgpu::Device {
        &self.context.device
    }

    /// Fails when the device was lost or raised an error since this renderer last looked.
    fn check_gpu_errors(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.context.device_lost(),
            "GPU device was lost during headless rendering"
        );
        if let Some(error) = self
            .errors
            .observe_error(&mut self.observed_error_generation)
        {
            anyhow::bail!("GPU error during headless rendering: {error}");
        }
        Ok(())
    }

    /// The adapter this renderer draws with: its name, backend, driver and device type.
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.context.adapter.get_info()
    }

    /// GPU specs of the adapter, as windows report them.
    pub fn gpu_specs(&self) -> GpuSpecs {
        self.renderer.gpu_specs()
    }

    /// The shader effects this renderer draws.
    pub fn effect_capabilities(&self) -> EffectCapabilities {
        self.renderer.effect_capabilities()
    }

    /// The format of the render targets, `Bgra8Unorm` or `Rgba8Unorm`.
    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.renderer.render_target_format
    }

    /// The path multisample count, fixed for every adapter.
    pub fn path_sample_count(&self) -> u32 {
        self.renderer.rendering_params.path_sample_count
    }

    /// The largest width or height a render target may have.
    pub fn max_texture_size(&self) -> u32 {
        self.renderer.max_texture_size()
    }

    /// Whether this renderer and `other` draw on the same device.
    pub fn shares_device_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.context.device, &other.context.device)
    }

    fn ensure_target(&mut self, size: Size<DevicePixels>) -> anyhow::Result<()> {
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "invalid headless render target size: {size:?}"
        );
        let max_texture_size = self.max_texture_size();
        anyhow::ensure!(
            size.width.0 as u32 <= max_texture_size && size.height.0 as u32 <= max_texture_size,
            "headless render target size {size:?} exceeds the maximum texture dimension \
             {max_texture_size}"
        );
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        if self.target.as_ref().is_some_and(|target| {
            target.texture.width() == width && target.texture.height() == height
        }) {
            return Ok(());
        }
        let texture = self
            .context
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("headless_render_target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.target_format(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.target = Some(HeadlessRenderTarget { texture, view });
        Ok(())
    }

    fn render(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.check_gpu_errors()?;
        self.ensure_target(size)?;
        let view = self
            .target
            .as_ref()
            .map(|target| target.view.clone())
            .ok_or_else(|| anyhow::anyhow!("headless render target was not created"))?;
        self.renderer.render_offscreen(scene, &view, size)?;
        Ok(())
    }

    /// Copies the render target back to the CPU. The dimensions come from the target itself,
    /// so the copy cannot disagree with what was rendered.
    fn read_image(&mut self) -> anyhow::Result<RgbaImage> {
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("headless render target was not created"))?;
        let (width, height) = (target.texture.width(), target.texture.height());
        let padded_bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback_size = u64::from(padded_bytes_per_row) * u64::from(height);
        let device = &self.context.device;
        anyhow::ensure!(
            readback_size <= device.limits().max_buffer_size,
            "headless readback of {readback_size} bytes exceeds the maximum buffer size {}",
            device.limits().max_buffer_size
        );
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("headless_readback"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("headless_readback_encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.context.queue.submit(std::iter::once(encoder.finish()));
        let mut image =
            self.renderer
                .read_buffer_to_image(readback, width, height, padded_bytes_per_row)?;
        self.check_gpu_errors()?;
        // Premultiplied paint over an opaque black background.
        for pixel in image.pixels_mut() {
            pixel.0[3] = 255;
        }
        Ok(image)
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl gpux::PlatformHeadlessRenderer for WgpuHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<RgbaImage> {
        self.render(scene, size)?;
        self.read_image()
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.render(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn gpux::PlatformAtlas> {
        self.renderer.sprite_atlas().clone()
    }

    fn effect_capabilities(&self) -> EffectCapabilities {
        self.renderer.effect_capabilities()
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        Some(self.renderer.gpu_specs())
    }
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn create_surface(
    instance: &wgpu::Instance,
    raw_window_handle: raw_window_handle::RawWindowHandle,
) -> anyhow::Result<wgpu::Surface<'static>> {
    unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                // Fall back to the display handle already provided via InstanceDescriptor::display.
                raw_display_handle: None,
                raw_window_handle,
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

struct RenderingParameters {
    path_sample_count: u32,
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
}

impl RenderingParameters {
    fn new(adapter: &wgpu::Adapter, surface_format: wgpu::TextureFormat) -> Self {
        use std::env;

        let format_features = adapter.get_texture_format_features(surface_format);
        let path_sample_count = [4, 2, 1]
            .into_iter()
            .find(|&n| format_features.flags.sample_count_supported(n))
            .unwrap_or(1);

        let gamma = env::var("ZED_FONTS_GAMMA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.8_f32)
            .clamp(1.0, 2.2);
        let gamma_ratios = get_gamma_correction_ratios(gamma);

        let grayscale_enhanced_contrast = env::var("ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0_f32)
            .max(0.0);

        let subpixel_enhanced_contrast = env::var("ZED_FONTS_SUBPIXEL_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.5_f32)
            .max(0.0);

        Self {
            path_sample_count,
            gamma_ratios,
            grayscale_enhanced_contrast,
            subpixel_enhanced_contrast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpux::{
        Color, ContentMask, ElementEffect, ScreenEffect, ScreenEffectId, ScreenEffectRegionId,
        ScreenEffectRegionPrimitive, ScreenEffectTarget, TransformationMatrix, backdrop_blur,
        glass, glow, hsla, point, px, screen, size,
    };

    #[test]
    fn storage_buffer_shader_is_valid_wgsl() {
        validate_wgsl(STORAGE_BUFFER_SHADERS, naga::valid::Capabilities::empty());
    }

    #[test]
    fn webgl_shader_is_valid_without_storage_buffers() {
        validate_wgsl(WEBGL_SHADERS, naga::valid::Capabilities::empty());
        assert!(!WEBGL_SHADERS.contains("var<storage"));
    }

    #[test]
    fn subpixel_shader_is_valid_wgsl() {
        validate_wgsl(
            SUBPIXEL_SHADERS,
            naga::valid::Capabilities::DUAL_SOURCE_BLENDING,
        );
    }

    fn validate_wgsl(source: &str, capabilities: naga::valid::Capabilities) {
        let module = naga::front::wgsl::parse_str(source).expect("shader should parse");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
            .validate(&module)
            .expect("shader should validate");
    }

    fn scaled_bounds(width: f32, height: f32) -> Bounds<ScaledPixels> {
        Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(width), ScaledPixels(height)),
        )
    }

    fn prepared_effect(effect: impl Into<ElementEffect>) -> PreparedElementEffect {
        effect.into().prepare()
    }

    fn surface_caps_with_present_modes(
        present_modes: Vec<wgpu::PresentMode>,
    ) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            present_modes,
            ..Default::default()
        }
    }

    #[test]
    fn surface_format_prefers_a_non_srgb_format_so_colors_blend_in_srgb() {
        let formats = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
        ];
        assert_eq!(
            choose_surface_format(&formats),
            Some(wgpu::TextureFormat::Bgra8Unorm)
        );

        let formats = [
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Rgba8Unorm,
        ];
        assert_eq!(
            choose_surface_format(&formats),
            Some(wgpu::TextureFormat::Rgba8Unorm)
        );
    }

    #[test]
    fn choose_present_mode_keeps_mailbox_when_supported() {
        let caps = surface_caps_with_present_modes(vec![
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::Mailbox,
        ]);

        assert_eq!(
            choose_present_mode(&caps, Some(wgpu::PresentMode::Mailbox)),
            wgpu::PresentMode::Mailbox
        );
    }

    #[test]
    fn choose_present_mode_falls_back_from_mailbox_to_immediate_before_fifo() {
        let caps = surface_caps_with_present_modes(vec![
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::Immediate,
        ]);

        assert_eq!(
            choose_present_mode(&caps, Some(wgpu::PresentMode::Mailbox)),
            wgpu::PresentMode::Immediate
        );
    }

    #[test]
    fn choose_present_mode_uses_fifo_without_preference() {
        let caps = surface_caps_with_present_modes(vec![
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Fifo,
        ]);

        assert_eq!(choose_present_mode(&caps, None), wgpu::PresentMode::Fifo);
    }

    #[test]
    fn html_island_drawable_area_requires_non_zero_bounds_and_mask() {
        assert!(html_island_has_drawable_area(
            scaled_bounds(120., 80.),
            scaled_bounds(120., 80.)
        ));
        assert!(!html_island_has_drawable_area(
            scaled_bounds(0., 80.),
            scaled_bounds(120., 80.)
        ));
        assert!(!html_island_has_drawable_area(
            scaled_bounds(120., 0.),
            scaled_bounds(120., 80.)
        ));
        assert!(!html_island_has_drawable_area(
            scaled_bounds(120., 80.),
            scaled_bounds(0., 80.)
        ));
        assert!(!html_island_has_drawable_area(
            scaled_bounds(120., 80.),
            scaled_bounds(120., 0.)
        ));
    }

    #[test]
    fn effect_target_key_tracks_resize_and_format_changes() {
        let key = WgpuEffectTargetKey::new(128, 64, wgpu::TextureFormat::Bgra8Unorm);

        assert!(key.matches(128, 64, wgpu::TextureFormat::Bgra8Unorm));
        assert!(!key.matches(256, 64, wgpu::TextureFormat::Bgra8Unorm));
        assert!(!key.matches(128, 64, wgpu::TextureFormat::Rgba8Unorm));
    }

    #[test]
    fn packed_visual_effect_layout_matches_wgsl_contract() {
        assert_eq!(size_of::<PackedContentMask>(), 32);
        assert_eq!(offset_of!(PackedContentMask, bounds), 0);
        assert_eq!(offset_of!(PackedContentMask, corner_radii), 16);

        assert_eq!(size_of::<PackedVisualEffectPrimitive>(), 104);
        assert_eq!(offset_of!(PackedVisualEffectPrimitive, content_mask), 16);
        assert_eq!(offset_of!(PackedVisualEffectPrimitive, corner_radii), 48);
        assert_eq!(offset_of!(PackedVisualEffectPrimitive, opacity), 64);
        assert_eq!(offset_of!(PackedVisualEffectPrimitive, effect_offset), 68);
        assert_eq!(offset_of!(PackedVisualEffectPrimitive, effect_count), 72);
        assert_eq!(offset_of!(PackedVisualEffectPrimitive, transformation), 80);

        assert_eq!(size_of::<PackedElementEffect>(), 80);
        assert_eq!(offset_of!(PackedElementEffect, params0), 16);
        assert_eq!(offset_of!(PackedElementEffect, params1), 32);
        assert_eq!(offset_of!(PackedElementEffect, color0), 48);
        assert_eq!(offset_of!(PackedElementEffect, color1), 64);
    }

    #[test]
    fn quad_packing_preserves_content_mask_corner_radii() {
        let bounds = scaled_bounds(80., 40.);
        let mask_bounds = Bounds::new(
            point(ScaledPixels(4.), ScaledPixels(6.)),
            size(ScaledPixels(64.), ScaledPixels(32.)),
        );
        let quad = Quad {
            bounds,
            content_mask: ContentMask::rounded(
                mask_bounds,
                Corners {
                    top_left: ScaledPixels(5.),
                    top_right: ScaledPixels(6.),
                    bottom_right: ScaledPixels(7.),
                    bottom_left: ScaledPixels(8.),
                },
            ),
            ..Default::default()
        };

        let (packed, _) = pack_quads(&[quad]);

        assert_eq!(packed[0].content_mask.bounds, PodBounds::from(mask_bounds));
        assert_eq!(packed[0].content_mask.corner_radii, [5., 6., 7., 8.]);
    }

    #[test]
    fn visual_effect_packing_preserves_content_mask_corner_radii() {
        let bounds = scaled_bounds(80., 40.);
        let primitive = VisualEffectPrimitive {
            order: 0,
            bounds,
            content_mask: ContentMask::rounded(bounds, Corners::all(ScaledPixels(11.))),
            corner_radii: Corners::all(ScaledPixels(6.)),
            opacity: 1.0,
            effects: vec![prepared_effect(backdrop_blur(px(8.)))],
            transformation: TransformationMatrix::unit(),
        };

        let packed = pack_visual_effect_primitives(&[primitive]);

        assert_eq!(
            packed.primitives[0].content_mask.corner_radii,
            [11., 11., 11., 11.]
        );
    }

    #[test]
    fn visual_effect_shaders_check_rounded_content_mask_in_normal_and_glow_passes() {
        let shader = include_str!("shaders.wgsl");

        assert_eq!(
            shader
                .matches("content_mask_sdf(input.transformed_position, primitive.content_mask)")
                .count(),
            2
        );
    }

    #[test]
    fn transformed_quad_uses_analytic_coverage() {
        let shader = include_str!("shaders.wgsl");
        let compact_shader = shader.split_whitespace().collect::<String>();

        assert!(shader.contains("fn transformation_is_axis_aligned"));
        assert!(shader.contains("let antialias_margin = select(1.0, 0.0"));
        assert!(compact_shader.contains("unrounded&&axis_aligned){"));
    }

    #[test]
    fn packed_screen_effect_layout_matches_wgsl_contract() {
        assert_eq!(size_of::<PackedScreenEffect>(), 96);
        assert_eq!(offset_of!(PackedScreenEffect, bounds), 16);
        assert_eq!(offset_of!(PackedScreenEffect, tint), 32);
        assert_eq!(offset_of!(PackedScreenEffect, origin), 48);
        assert_eq!(offset_of!(PackedScreenEffect, elapsed_ms), 56);
        assert_eq!(offset_of!(PackedScreenEffect, duration_ms), 60);
        assert_eq!(offset_of!(PackedScreenEffect, radius), 64);
        assert_eq!(offset_of!(PackedScreenEffect, amplitude), 68);
        assert_eq!(offset_of!(PackedScreenEffect, wavelength), 72);
        assert_eq!(offset_of!(PackedScreenEffect, damping), 76);
        assert_eq!(offset_of!(PackedScreenEffect, params), 80);
    }

    #[test]
    fn screen_ripple_packing_scales_window_target_parameters() {
        let mut scene = Scene::default();
        scene.scale_factor = 2.0;
        let effect = ActiveScreenEffect::new(
            ScreenEffectId::new(7),
            ScreenEffect::from(
                screen::ripple(point(px(20.), px(30.)))
                    .duration_ms(800)
                    .radius(px(100.))
                    .amplitude(px(6.))
                    .wavelength(px(32.))
                    .damping(3.0)
                    .chromatic_aberration(0.25)
                    .tint(hsla(0.2, 0.4, 0.6, 0.3)),
            ),
            200,
        );

        let packed = pack_screen_effects(
            &scene,
            &[effect],
            size(DevicePixels(400), DevicePixels(300)),
            None,
        );

        assert_eq!(packed.effects.len(), 1);
        assert_eq!(packed.effects[0].kind, SCREEN_EFFECT_KIND_RIPPLE);
        assert_eq!(
            packed.effects[0].bounds,
            PodBounds {
                origin: [0., 0.],
                size: [400., 300.]
            }
        );
        assert_eq!(packed.effects[0].origin, [40., 60.]);
        assert_eq!(packed.effects[0].elapsed_ms, 200.);
        assert_eq!(packed.effects[0].duration_ms, 800.);
        assert_eq!(packed.effects[0].radius, 200.);
        assert_eq!(packed.effects[0].amplitude, 12.);
        assert_eq!(packed.effects[0].wavelength, 64.);
        assert_eq!(packed.effects[0].damping, 3.);
        assert_eq!(packed.effects[0].params[0], 0.25);
        assert_eq!(
            packed.effects[0].tint,
            pack_paint_color(Color::from(hsla(0.2, 0.4, 0.6, 0.3)).paint_color())
        );
    }

    #[test]
    fn screen_ripple_packing_resolves_region_targets_and_skips_missing_regions() {
        let mut scene = Scene::default();
        scene.scale_factor = 3.0;
        let bounds = Bounds::new(
            point(ScaledPixels(20.), ScaledPixels(30.)),
            size(ScaledPixels(200.), ScaledPixels(100.)),
        );
        scene
            .screen_effect_regions
            .push(ScreenEffectRegionPrimitive {
                order: 0,
                id: ScreenEffectRegionId::new("panel"),
                bounds,
                logical_bounds: Bounds::new(point(px(10.), px(15.)), size(px(100.), px(50.))),
                content_mask: ContentMask::rectangular(bounds),
                corner_radii: Corners::all(ScaledPixels(8.)),
                transformation: TransformationMatrix::unit(),
            });
        let active = ActiveScreenEffect::new(
            ScreenEffectId::new(1),
            ScreenEffect::from(
                screen::ripple(point(px(35.), px(40.))).target(ScreenEffectTarget::region("panel")),
            ),
            120,
        );
        let missing = ActiveScreenEffect::new(
            ScreenEffectId::new(2),
            ScreenEffect::from(
                screen::ripple(point(px(35.), px(40.)))
                    .target(ScreenEffectTarget::region("missing")),
            ),
            120,
        );

        let packed = pack_screen_effects(
            &scene,
            &[active, missing],
            size(DevicePixels(400), DevicePixels(300)),
            None,
        );

        assert_eq!(packed.effects.len(), 1);
        assert_eq!(packed.effects[0].bounds, PodBounds::from(bounds));
        assert_eq!(packed.effects[0].origin, [70., 80.]);
    }

    #[test]
    fn screen_ripple_packing_resolves_explicit_bounds_target() {
        let mut scene = Scene::default();
        scene.scale_factor = 2.0;
        let target_bounds = Bounds::new(point(px(4.), px(5.)), size(px(20.), px(10.)));
        let active = ActiveScreenEffect::new(
            ScreenEffectId::new(3),
            ScreenEffect::from(
                screen::ripple(point(px(7.), px(8.)))
                    .target(ScreenEffectTarget::bounds(target_bounds)),
            ),
            40,
        );

        let packed = pack_screen_effects(
            &scene,
            &[active],
            size(DevicePixels(400), DevicePixels(300)),
            None,
        );

        assert_eq!(
            packed.effects[0].bounds,
            PodBounds {
                origin: [8., 10.],
                size: [40., 20.]
            }
        );
        assert_eq!(packed.effects[0].origin, [14., 16.]);
    }

    #[test]
    fn snapshot_transition_packing_requires_captured_snapshot() {
        let mut scene = Scene::default();
        scene.scale_factor = 2.0;
        let snapshot_id = ScreenEffectSnapshotId::new(9);
        let active = ActiveScreenEffect::new(
            ScreenEffectId::new(4),
            ScreenEffect::from(
                screen::ripple_reveal(snapshot_id, point(px(7.), px(8.)))
                    .target(ScreenEffectTarget::bounds(Bounds::new(
                        point(px(4.), px(5.)),
                        size(px(20.), px(10.)),
                    )))
                    .duration_ms(700)
                    .radius(px(120.))
                    .amplitude(px(5.))
                    .wavelength(px(24.))
                    .damping(2.0)
                    .blur_radius(px(12.)),
            ),
            140,
        );

        let missing = pack_screen_effects(
            &scene,
            std::slice::from_ref(&active),
            size(DevicePixels(400), DevicePixels(300)),
            None,
        );
        assert!(missing.effects.is_empty());
        assert_eq!(missing.snapshot_id, None);

        let packed = pack_screen_effects(
            &scene,
            &[active],
            size(DevicePixels(400), DevicePixels(300)),
            Some(snapshot_id),
        );

        assert_eq!(packed.snapshot_id, Some(snapshot_id));
        assert_eq!(packed.effects.len(), 1);
        assert_eq!(packed.effects[0].kind, SCREEN_EFFECT_KIND_RIPPLE_REVEAL);
        assert_eq!(packed.effects[0].uses_snapshot, 1);
        assert_eq!(
            packed.effects[0].bounds,
            PodBounds {
                origin: [8., 10.],
                size: [40., 20.]
            }
        );
        assert_eq!(packed.effects[0].origin, [14., 16.]);
        assert_eq!(packed.effects[0].elapsed_ms, 140.);
        assert_eq!(packed.effects[0].duration_ms, 700.);
        assert_eq!(packed.effects[0].radius, 240.);
        assert_eq!(packed.effects[0].amplitude, 10.);
        assert_eq!(packed.effects[0].wavelength, 48.);
        assert_eq!(packed.effects[0].damping, 2.);
        assert_eq!(packed.effects[0].params[1], 24.);
    }

    #[test]
    fn visual_effect_packing_preserves_effect_order_and_parameters() {
        let bounds = scaled_bounds(80., 40.);
        let primitive = VisualEffectPrimitive {
            order: 0,
            bounds,
            content_mask: ContentMask::rectangular(bounds),
            corner_radii: Corners::all(ScaledPixels(6.)),
            opacity: 0.75,
            effects: vec![
                prepared_effect(
                    backdrop_blur(px(10.))
                        .saturation(1.2)
                        .brightness(0.8)
                        .tint(hsla(0.2, 0.3, 0.4, 0.5)),
                ),
                prepared_effect(glass().blur(px(18.)).refraction(0.04).distortion(0.03)),
                prepared_effect(
                    glow(hsla(0.6, 0.8, 0.7, 0.45))
                        .radius(px(20.))
                        .spread(px(4.))
                        .intensity(1.5),
                ),
            ],
            transformation: TransformationMatrix::unit(),
        };

        let packed = pack_visual_effect_primitives(&[primitive]);

        assert_eq!(packed.primitives.len(), 1);
        assert_eq!(packed.effects.len(), 3);
        assert_eq!(packed.primitives[0].effect_offset, 0);
        assert_eq!(packed.primitives[0].effect_count, 3);
        assert_eq!(packed.primitives[0].opacity, 0.75);
        assert_eq!(packed.primitives[0].corner_radii, [6., 6., 6., 6.]);

        assert_eq!(packed.effects[0].kind, VISUAL_EFFECT_KIND_BACKDROP_BLUR);
        assert_eq!(packed.effects[0].params0, [10., 1.2, 0.8, 0.]);
        assert_eq!(
            packed.effects[0].color0,
            pack_paint_color(Color::from(hsla(0.2, 0.3, 0.4, 0.5)).paint_color())
        );

        assert_eq!(packed.effects[1].kind, VISUAL_EFFECT_KIND_GLASS);
        assert_eq!(packed.effects[1].params0, [18., 1.18, 1.08, 0.04]);
        assert_eq!(packed.effects[1].params1, [0.03, 0., 0., 0.]);

        assert_eq!(packed.effects[2].kind, VISUAL_EFFECT_KIND_GLOW);
        assert_eq!(packed.effects[2].params0, [20., 4., 1.5, 0.]);
        assert_eq!(
            packed.effects[2].color0,
            pack_paint_color(Color::from(hsla(0.6, 0.8, 0.7, 0.45)).paint_color())
        );
    }

    #[test]
    fn glow_packing_preserves_transparent_color_and_content_mask() {
        let bounds = scaled_bounds(120., 60.);
        let mask = Bounds::new(
            point(ScaledPixels(10.), ScaledPixels(12.)),
            size(ScaledPixels(80.), ScaledPixels(32.)),
        );
        let primitive = VisualEffectPrimitive {
            order: 0,
            bounds,
            content_mask: ContentMask::rectangular(mask),
            corner_radii: Corners::all(ScaledPixels(9.)),
            opacity: 0.5,
            effects: vec![prepared_effect(
                glow(hsla(0.1, 0.2, 0.3, 0.0))
                    .radius(px(32.))
                    .spread(px(6.))
                    .intensity(1.25),
            )],
            transformation: TransformationMatrix::unit(),
        };

        let packed = pack_visual_effect_primitives(&[primitive]);

        assert_eq!(
            packed.primitives[0].content_mask.bounds,
            PodBounds::from(mask)
        );
        assert_eq!(packed.primitives[0].content_mask.corner_radii, [0.; 4]);
        assert_eq!(packed.primitives[0].opacity, 0.5);
        assert_eq!(packed.effects[0].kind, VISUAL_EFFECT_KIND_GLOW);
        assert_eq!(packed.effects[0].params0, [32., 6., 1.25, 0.]);
        assert_eq!(
            packed.effects[0].color0,
            pack_paint_color(Color::from(hsla(0.1, 0.2, 0.3, 0.0)).paint_color())
        );
    }

    #[test]
    fn blur_and_glass_packing_apply_wgpu_quality_radius_caps() {
        let bounds = scaled_bounds(120., 60.);
        let primitive = VisualEffectPrimitive {
            order: 0,
            bounds,
            content_mask: ContentMask::rectangular(bounds),
            corner_radii: Corners::all(ScaledPixels(8.)),
            opacity: 1.0,
            effects: vec![
                prepared_effect(backdrop_blur(px(200.))),
                prepared_effect(glass().blur(px(180.)).high_quality()),
            ],
            transformation: TransformationMatrix::unit(),
        };

        let packed = pack_visual_effect_primitives(&[primitive]);

        assert_eq!(packed.effects[0].kind, VISUAL_EFFECT_KIND_BACKDROP_BLUR);
        assert_eq!(packed.effects[0].params0[0], MAX_WGPU_FAST_BLUR_RADIUS);
        assert_eq!(packed.effects[1].kind, VISUAL_EFFECT_KIND_GLASS);
        assert_eq!(packed.effects[1].params0[0], MAX_WGPU_HIGH_BLUR_RADIUS);
    }

    #[test]
    fn wgpu_effect_capabilities_report_shader_support() {
        let capabilities = wgpu_effect_capabilities();

        assert!(capabilities.element.backdrop_blur);
        assert!(capabilities.element.glass);
        assert!(capabilities.element.glow);
        assert!(capabilities.screen.ripple);
        assert!(capabilities.screen.snapshot_transitions);
    }

    #[test]
    fn glow_blend_state_adds_color_and_preserves_alpha() {
        let blend = visual_effect_glow_blend_state();

        assert_eq!(blend.color.src_factor, wgpu::BlendFactor::One);
        assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::One);
        assert_eq!(blend.color.operation, wgpu::BlendOperation::Add);
        assert_eq!(blend.alpha.src_factor, wgpu::BlendFactor::Zero);
        assert_eq!(blend.alpha.dst_factor, wgpu::BlendFactor::One);
        assert_eq!(blend.alpha.operation, wgpu::BlendOperation::Add);
    }
}
