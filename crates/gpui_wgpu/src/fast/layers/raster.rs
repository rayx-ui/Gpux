//! Rasterizing a scroll layer tile: the layer's content over the tile,
//! translated into the tile's space, drawn into the tile's texture cleared
//! to the layer's baked background, with the frame's pipelines and a
//! viewport the size of the tile.
//!
//! Adapted from gpui-fast's `gpui_wgpu/src/fast/layers/raster.rs`.
//!
//! All position-dependent shading is relative to each primitive's own bounds
//! (the gradient dither included), and the tile is a whole number of device
//! pixels from the content's origin, so a tile holds exactly the pixels the
//! content would have drawn over the same background in the window.

use gpux::{PaintColor, PrimitiveBatch, Scene, TileCoord};

use crate::{
    WgpuRenderer,
    fast::layers::{composite, tile_cache::TileTarget},
};

/// A tile to rasterize this frame, with the part of its layer's content
/// that covers it.
pub(crate) struct TileRaster {
    /// The index of the tile's layer in the frame's `SceneLayers::frames`.
    pub(crate) layer: usize,
    pub(crate) tile: TileCoord,
    pub(crate) scene: Scene,
}

/// Starts the frame that draws `scene`: returns the tiles to rasterize
/// before it composites them, with their textures ready.
pub(crate) fn plan(renderer: &mut WgpuRenderer, scene: &Scene) -> Vec<TileRaster> {
    if scene.layers.frames.is_empty() && renderer.fast_layers.cache.is_idle() {
        return Vec::new();
    }
    let mut cache = std::mem::take(&mut renderer.fast_layers.cache);
    let planned = cache.begin_frame(&scene.layers, composite::composited_tiles(scene));
    let resources = renderer.resources();
    cache.prepare(
        &TileTarget {
            device: &resources.device,
            queue: &resources.queue,
            format: renderer.render_target_format,
            premultiplied_alpha: renderer.surface_config.alpha_mode
                == wgpu::CompositeAlphaMode::PreMultiplied,
            globals_layout: &resources.bind_group_layouts.globals,
            gamma_buffer: &resources.gamma_buffer,
        },
        &scene.layers,
        &planned,
    );
    renderer.fast_layers.cache = cache;
    planned
        .iter()
        .map(|&(layer, tile)| TileRaster {
            layer,
            tile,
            scene: scene.layers.frames[layer].tile_scene(tile),
        })
        .collect()
}

/// The clear colour a tile over `background` starts from: the bytes an
/// opaque quad of that colour writes to the frame's non-sRGB texture.
pub(crate) fn clear_color(background: PaintColor) -> wgpu::Color {
    wgpu::Color {
        r: background.r as f64,
        g: background.g as f64,
        b: background.b as f64,
        a: background.a as f64,
    }
}

/// Records the passes that rasterize `tiles` into their textures, before the
/// frame's main pass draws them. Returns `false` when the instance data did
/// not fit, as the renderer's batch functions do: the frame is recorded
/// again with larger buffers.
pub(crate) fn rasterize(
    renderer: &WgpuRenderer,
    encoder: &mut wgpu::CommandEncoder,
    scene: &Scene,
    tiles: &[TileRaster],
    instance_offset: &mut u64,
    gradient_stop_offset: &mut u64,
) -> bool {
    for tile in tiles {
        let layer = &scene.layers.frames[tile.layer];
        let cache = &renderer.fast_layers.cache;
        let Some(view) = cache
            .texture(layer.key, tile.tile)
            .filter(|_| cache.globals(layer.tile_size).is_some())
        else {
            debug_assert!(false, "a planned tile was not prepared");
            continue;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("layer_tile_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color(layer.background)),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        #[cfg(feature = "diagnostics")]
        renderer.update_frame_telemetry(|telemetry| {
            telemetry.render_pass_count = telemetry.render_pass_count.saturating_add(1);
        });
        renderer.fast_layers.rasterizing.set(Some(layer.tile_size));
        let drawn = draw(
            renderer,
            &tile.scene,
            instance_offset,
            gradient_stop_offset,
            &mut pass,
        );
        renderer.fast_layers.rasterizing.set(None);
        if !drawn {
            return false;
        }
        #[cfg(test)]
        renderer
            .fast_layers
            .rasterized
            .set(renderer.fast_layers.rasterized.get() + 1);
        #[cfg(feature = "diagnostics")]
        renderer
            .fast_layers
            .rasterized_total
            .set(renderer.fast_layers.rasterized_total.get() + 1);
    }
    true
}

/// Draws a tile's `scene` into `pass`. A layer's content holds what a tile
/// can: no path (the core draws a layer's paths into the frame), no effect,
/// island or surface (their container is never a layer), no other layer's
/// tile (a scroll container inside a layer is painted into it).
fn draw(
    renderer: &WgpuRenderer,
    scene: &Scene,
    instance_offset: &mut u64,
    gradient_stop_offset: &mut u64,
    pass: &mut wgpu::RenderPass<'_>,
) -> bool {
    for batch in scene.batches() {
        #[cfg(feature = "diagnostics")]
        renderer.update_frame_telemetry(|telemetry| {
            telemetry.batch_count = telemetry.batch_count.saturating_add(1);
        });
        let ok = match batch {
            PrimitiveBatch::Quads(range) => renderer.draw_quads(
                &scene.quads[range],
                instance_offset,
                gradient_stop_offset,
                pass,
            ),
            PrimitiveBatch::Shadows(range) => {
                renderer.draw_shadows(&scene.shadows[range], instance_offset, pass)
            }
            PrimitiveBatch::Underlines(range) => {
                renderer.draw_underlines(&scene.underlines[range], instance_offset, pass)
            }
            PrimitiveBatch::MonochromeSprites { texture_id, range } => renderer
                .draw_monochrome_sprites(
                    &scene.monochrome_sprites[range],
                    texture_id,
                    instance_offset,
                    pass,
                ),
            PrimitiveBatch::SubpixelSprites { texture_id, range } => renderer
                .draw_subpixel_sprites(
                    &scene.subpixel_sprites[range],
                    texture_id,
                    instance_offset,
                    pass,
                ),
            PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                if composite::is_layer_texture(texture_id) {
                    debug_assert!(false, "a layer tile inside a layer tile");
                    true
                } else {
                    renderer.draw_polychrome_sprites(
                        &scene.polychrome_sprites[range],
                        texture_id,
                        instance_offset,
                        pass,
                    )
                }
            }
            PrimitiveBatch::Paths(_)
            | PrimitiveBatch::VisualEffects(_)
            | PrimitiveBatch::HtmlIslands(_)
            | PrimitiveBatch::Surfaces(_) => {
                debug_assert!(false, "a layer tile holds what no tile can");
                true
            }
        };
        if !ok {
            return false;
        }
    }
    true
}

/// The frame that was to rasterize the planned tiles was not drawn: the
/// cache counts them as held, so it starts over.
pub(crate) fn frame_failed(renderer: &mut WgpuRenderer) {
    super::release(renderer);
}
