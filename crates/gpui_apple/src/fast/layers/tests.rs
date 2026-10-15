//! Pixel tests of scroll layers: the same content drawn directly and through
//! layer tiles must come out byte for byte the same.
//!
//! Adapted from gpui-fast's `gpui_apple/src/fast/layers/tests.rs`, and the
//! counterpart of `gpui_wgpu`'s `fast::layers::tests`, on Metal. The scenes
//! are drawn by [`MetalHeadlessRenderer`], the renderer of gpux's pixel lane
//! on macOS. A tile never holds a path in gpux, so the content the tiles are
//! rasterized from has none, and a test of its own draws a layer's paths the
//! way the core does, into the frame.
//!
//! ```text
//! cargo test -p gpux-apple --features test-support --lib fast::layers
//! ```

use std::borrow::Cow;
use std::mem::{offset_of, size_of};

use gpux::{
    AtlasKey, AtlasTile, Background, Bounds, Color, ContentMask, Corners, DevicePixels, Edges,
    FontId, GlyphId, ImageId, LAYER_TILE_PREMULTIPLIED, LayerFrame, LayerKey, MonochromeSprite,
    MonochromeSpriteKind, PaintColor, Path, PlatformHeadlessRenderer, PolychromeSprite, Quad,
    RenderGlyphParams, RenderImageParams, Rgba, ScaledPixels, Scene, SceneLayers, Shadow, Size,
    TileCoord, TransformationMatrix, Underline, layer_tile_id, layer_tile_texture_id,
    linear_color_stop, linear_gradient, point, px, rgba, size,
};
use image::RgbaImage;

use crate::fast::layers::TileCache;
use crate::metal_renderer::MetalHeadlessRenderer;

const TILE: u32 = 512;

#[test]
fn a_layer_drawn_from_tiles_equals_its_content_drawn_directly() {
    let mut renderer = renderer();
    let window = device_size(1024, 1024);
    let direct = render_direct(&mut renderer, (0., 0.), None, window);
    let tiles = [coord(0, 0), coord(1, 0), coord(0, 1), coord(1, 1)];
    let layer = layer_frame(LayerKey(1), 1, content(&renderer, (0., 0.)), &tiles);
    let composited = composited_scene(layer, &tiles, (0., 0.), None);
    let assembled = render(&mut renderer, &composited, window);
    assert_eq!(rasterized(&renderer), 4);
    assert_same_pixels(&assembled, &direct);
}

#[test]
fn tiles_of_negative_coordinates_rasterize_like_positive_ones() {
    let mut renderer = renderer();
    let window = device_size(1024, 1024);
    let direct = render_direct(&mut renderer, (0., 0.), None, window);
    // The same content, painted a tile up and left of the content's origin
    // and scrolled back by as much.
    let side = TILE as f32;
    let tiles = [coord(-1, -1), coord(0, -1), coord(-1, 0), coord(0, 0)];
    let layer = layer_frame(LayerKey(1), 1, content(&renderer, (-side, -side)), &tiles);
    let composited = composited_scene(layer, &tiles, (side, side), None);
    let assembled = render(&mut renderer, &composited, window);
    assert_eq!(rasterized(&renderer), 4);
    assert_same_pixels(&assembled, &direct);
}

#[test]
fn a_layer_scrolled_by_an_odd_distance_equals_direct_drawing() {
    let mut renderer = renderer();
    let (expected, composited, window) = scrolled(&renderer, (37., -91.), Paths::Without);
    let expected = render(&mut renderer, &expected, window);
    let actual = render(&mut renderer, &composited, window);
    assert_same_pixels(&actual, &expected);
}

#[test]
fn a_frame_that_only_scrolled_draws_the_cached_tiles() {
    let mut renderer = renderer();
    let tiles = [coord(0, 0), coord(1, 0), coord(0, 1), coord(1, 1)];
    let window = device_size(700, 500);
    let viewport = sp(40., 30., 600., 400.);
    let layer = layer_frame(LayerKey(5), 1, content(&renderer, (0., 0.)), &tiles);

    let first = composited_scene(layer.clone(), &tiles, (37., -91.), Some(viewport));
    render(&mut renderer, &first, window);
    assert_eq!(rasterized(&renderer), 4);

    // The same generation, scrolled on: nothing is rasterized, and the frame
    // equals the content drawn where it now is.
    let second = composited_scene(layer, &tiles, (12., -140.), Some(viewport));
    let actual = render(&mut renderer, &second, window);
    assert_eq!(rasterized(&renderer), 4, "the tiles came from the cache");
    let expected = render_direct(&mut renderer, (12., -140.), Some(viewport), window);
    assert_same_pixels(&actual, &expected);
}

/// The core keeps a layer's paths apart from its tiles and draws them into
/// the frame, over the tiles (`fast::layers::paint::lift_paths`). Drawn so,
/// a path comes out as it does without a layer at any translation: gpui-fast
/// rasterizes paths into tiles and is exact at even translations only.
#[test]
fn the_paths_of_a_layer_are_drawn_into_the_frame_over_its_tiles() {
    let mut renderer = renderer();
    let (expected, composited, window) = scrolled(&renderer, (37., -91.), Paths::With);
    let expected = render(&mut renderer, &expected, window);
    let actual = render(&mut renderer, &composited, window);
    assert_same_pixels(&actual, &expected);
}

/// A tile's texture holds premultiplied colour, which the sprite compositing
/// it says ([`LAYER_TILE_PREMULTIPLIED`]): the polychrome shader multiplies
/// an atlas texture's colour by its alpha, and must not do it to a tile's.
/// No layer has a translucent background (the core bakes opaque ones only);
/// this one has, so that the flag shows.
#[test]
fn a_tile_is_composited_as_premultiplied_colour() {
    let mut renderer = renderer();
    let window = device_size(64, 64);
    let translucent = rgba(0xff000080);
    let mut direct = Scene::default();
    direct.insert_primitive(quad(sp(0., 0., 64., 64.), rgba(0xffffffff)));
    direct.insert_primitive(quad(sp(0., 0., 64., 64.), translucent));
    direct.finish();
    let expected = render(&mut renderer, &direct, window);

    let tiles = [coord(0, 0)];
    let layer = LayerFrame {
        background: paint(translucent),
        ..layer_frame(LayerKey(2), 1, Scene::default(), &tiles)
    };
    let mut composited = Scene::default();
    composited.insert_primitive(quad(sp(0., 0., 64., 64.), rgba(0xffffffff)));
    add_tiles(
        &mut composited,
        &layer,
        &tiles,
        (0., 0.),
        sp(0., 0., 64., 64.),
    );
    composited.layers.frames.push(layer);
    composited.finish();
    let actual = render(&mut renderer, &composited, window);
    assert_same_pixels(&actual, &expected);
    let [r, g, b, _] = actual.get_pixel(32, 32).0;
    assert!(
        r == 255 && g.abs_diff(128) <= 1 && b.abs_diff(128) <= 1,
        "half red over white, not {:?}",
        [r, g, b]
    );
}

/// The polychrome sprite record the shader reads: the flag is its second
/// word, which the shader names as the Rust record does (the header it
/// includes is generated from it).
#[test]
fn the_premultiplied_flag_is_the_second_word_of_the_polychrome_sprite_record() {
    assert_eq!(LAYER_TILE_PREMULTIPLIED, 1);
    assert_eq!(offset_of!(PolychromeSprite, order), 0);
    assert_eq!(offset_of!(PolychromeSprite, pad), 4);
    assert_eq!(offset_of!(PolychromeSprite, grayscale), 8);
    assert_eq!(offset_of!(PolychromeSprite, opacity), 12);
    assert_eq!(offset_of!(PolychromeSprite, bounds), 16);
    assert_eq!(size_of::<PolychromeSprite>(), 34 * 4);
    assert!(include_str!("../../shaders.metal").contains("sprite.pad != 0"));
}

#[test]
fn dirty_tiles_are_rasterized_once_they_are_shown() {
    let mut cache = TileCache::default();
    let key = LayerKey(3);
    let all = [coord(0, 0), coord(1, 0), coord(0, 1), coord(1, 1)];
    let mut rasterized = 0;
    let mut frame =
        |cache: &mut TileCache, generation, dirty: &[TileCoord], shown: &[TileCoord]| {
            let layers = scene_layers(key, generation, dirty);
            let planned = cache.begin_frame(&layers, shown.iter().map(|tile| (key, *tile)));
            rasterized += planned.len();
            planned
                .into_iter()
                .map(|(_, tile)| tile)
                .collect::<Vec<_>>()
        };

    let [a, b, c, _] = all;
    // A new layer: only the dirty tiles shown. The others wait until they are.
    let mut shown = vec![a, b];
    shown.sort();
    assert_eq!(frame(&mut cache, 1, &all, &[a, b]), shown);
    // The same generation again: nothing.
    assert_eq!(frame(&mut cache, 1, &all, &[a, b]), vec![]);
    // The next generation dirties a held tile that is not shown: it waits.
    assert_eq!(frame(&mut cache, 2, &[b, c], &[a]), vec![]);
    // Shown again, it is rasterized, and only it.
    assert_eq!(frame(&mut cache, 2, &[b, c], &[a, b]), vec![b]);
    // A dirty tile never shown before is rasterized once it is.
    assert_eq!(frame(&mut cache, 3, &[], &[a, b, c]), vec![c]);
    // A skipped generation may have dirtied any tile: shown tiles again.
    assert_eq!(frame(&mut cache, 5, &[], &[a]), vec![a]);
    // A shown tile the cache never had.
    assert_eq!(frame(&mut cache, 5, &[], &[coord(2, 0)]), vec![coord(2, 0)]);
    assert_eq!(rasterized, 6);
}

#[test]
fn tiles_are_evicted_least_recently_composited_first_within_the_budget() {
    const SIDE: u32 = 16;
    let tile_bytes = (SIDE * SIDE * 4) as u64;
    let key = LayerKey(4);
    let mut cache = TileCache::default();
    let layers = SceneLayers {
        frames: vec![LayerFrame {
            tile_size: SIDE,
            ..layer_frame(key, 1, Scene::default(), &[])
        }],
    };
    let frame = |cache: &mut TileCache, shown: &[TileCoord], budget: u64| {
        cache.begin_frame(&layers, shown.iter().map(|tile| (key, *tile)));
        cache.evict_to_budget(budget);
    };
    let (a, b, c, d, e) = (
        coord(0, 0),
        coord(1, 0),
        coord(2, 0),
        coord(3, 0),
        coord(4, 0),
    );

    frame(&mut cache, &[a, b, c], 3 * tile_bytes);
    frame(&mut cache, &[b, c, d], 3 * tile_bytes);
    assert!(
        !cache.holds(key, a),
        "the least recently composited tile goes"
    );
    assert!(cache.holds(key, b) && cache.holds(key, c) && cache.holds(key, d));

    frame(&mut cache, &[c], 3 * tile_bytes);
    frame(&mut cache, &[d, e], tile_bytes);
    assert!(!cache.holds(key, b) && !cache.holds(key, c));
    assert!(
        cache.holds(key, d) && cache.holds(key, e),
        "tiles composited this frame stay, over budget or not"
    );
}

/// A scroll leaves tiles behind for good: a tile no frame composites is
/// released after a few frames, its texture kept for the tiles the scroll
/// brings, so a long scroll holds the tiles it shows and not every tile it
/// passed until the budget runs out.
#[test]
fn tiles_no_frame_composites_are_released_after_a_few_frames() {
    const SIDE: u32 = 16;
    let key = LayerKey(5);
    let mut cache = TileCache::default();
    let layers = SceneLayers {
        frames: vec![LayerFrame {
            tile_size: SIDE,
            ..layer_frame(key, 1, Scene::default(), &[])
        }],
    };
    // A scroll showing two tiles a frame, moving a tile a frame.
    for step in 0..64 {
        let shown = [coord(0, step), coord(0, step + 1)];
        cache.begin_frame(&layers, shown.iter().map(|tile| (key, *tile)));
    }
    let held = (0..66)
        .filter(|row| cache.holds(key, coord(0, *row)))
        .count();
    assert!(
        held <= 2 + super::tile_cache::TILE_IDLE_FRAMES as usize,
        "{held} tiles held"
    );
    assert!(cache.holds(key, coord(0, 63)) && cache.holds(key, coord(0, 64)));
    assert!(!cache.holds(key, coord(0, 0)), "a tile left behind goes");

    // Turning back within the idle frames finds the tiles left behind.
    cache.begin_frame(&layers, [(key, coord(0, 61))].into_iter());
    assert!(cache.holds(key, coord(0, 61)));
    assert!(
        cache.holds(key, coord(0, 64)),
        "a tile shown a frame ago stays"
    );
}

/// A renderer over its budget evicts while it draws, and what it evicted is
/// rasterized again when a frame shows it.
#[test]
fn an_evicted_tile_is_rasterized_again_when_it_is_shown() {
    let mut renderer = renderer();
    let window = device_size(1024, 1024);
    let tiles = [coord(0, 0), coord(1, 0), coord(0, 1), coord(1, 1)];
    let layer = layer_frame(LayerKey(1), 1, content(&renderer, (0., 0.)), &tiles);
    renderer.renderer.fast_layers.budget_bytes = 2 * (TILE * TILE * 4) as u64;

    let top = [coord(0, 0), coord(1, 0)];
    let bottom = [coord(0, 1), coord(1, 1)];
    render(
        &mut renderer,
        &composited_scene(layer.clone(), &top, (0., 0.), None),
        window,
    );
    render(
        &mut renderer,
        &composited_scene(layer.clone(), &bottom, (0., 0.), None),
        window,
    );
    assert_eq!(rasterized(&renderer), 4);
    let cache = &renderer.renderer.fast_layers;
    assert!(!cache.holds(LayerKey(1), coord(0, 0)) && cache.holds(LayerKey(1), coord(0, 1)));

    let actual = render(
        &mut renderer,
        &composited_scene(layer, &tiles, (0., 0.), None),
        window,
    );
    assert_eq!(rasterized(&renderer), 6);
    let direct = render_direct(&mut renderer, (0., 0.), None, window);
    assert_same_pixels(&actual, &direct);
}

#[test]
fn a_resize_releases_every_tile() {
    let mut renderer = renderer();
    let tiles = [coord(0, 0), coord(1, 0), coord(0, 1), coord(1, 1)];
    let layer = layer_frame(LayerKey(1), 1, content(&renderer, (0., 0.)), &tiles);
    let composited = composited_scene(layer, &tiles, (0., 0.), None);
    render(&mut renderer, &composited, device_size(1024, 1024));
    assert!(
        renderer
            .renderer
            .fast_layers
            .holds(LayerKey(1), coord(0, 0))
    );

    // What a window's resize does: `MetalRenderer::update_drawable_size`.
    renderer
        .renderer
        .update_drawable_size(device_size(1000, 1000));
    assert!(renderer.renderer.fast_layers.is_empty());

    // The next frame rasterizes the tiles it composites again.
    let window = device_size(1000, 1000);
    let actual = render(&mut renderer, &composited, window);
    assert_eq!(rasterized(&renderer), 8);
    let direct = render_direct(&mut renderer, (0., 0.), None, window);
    assert_same_pixels(&actual, &direct);

    // A frame of another size, as an offscreen capture draws, starts over too.
    render(&mut renderer, &composited, device_size(900, 900));
    assert_eq!(rasterized(&renderer), 12);
}

/// The frame that was to rasterize tiles was not drawn as it was encoded:
/// the cache counted them as held when the frame began, so it starts over.
#[test]
fn a_failed_frame_releases_the_tiles_it_planned() {
    let mut renderer = renderer();
    let tiles = [coord(0, 0)];
    let layer = layer_frame(LayerKey(1), 1, content(&renderer, (0., 0.)), &tiles);
    let composited = composited_scene(layer, &tiles, (0., 0.), None);
    let window = device_size(512, 512);
    render(&mut renderer, &composited, window);
    assert_eq!(rasterized(&renderer), 1);
    assert!(
        renderer
            .renderer
            .fast_layers
            .holds(LayerKey(1), coord(0, 0))
    );

    // What the renderer does when a frame's instances do not fit its buffer,
    // before it encodes the frame again with a larger one.
    crate::fast::layers::raster::frame_failed(&mut renderer.renderer);
    assert!(renderer.renderer.fast_layers.is_empty());

    let actual = render(&mut renderer, &composited, window);
    assert_eq!(rasterized(&renderer), 2);
    let direct = render_direct(&mut renderer, (0., 0.), None, window);
    assert_same_pixels(&actual, &direct);
}

// --- scenes --- //

#[derive(Clone, Copy, PartialEq)]
enum Paths {
    With,
    Without,
}

/// A window with a viewport over scrolled content and a scrollbar above it:
/// the scene that draws the content where the scroll put it, the scene that
/// composites it from tiles painted at the content's origin, and the
/// window's size.
fn scrolled(
    renderer: &MetalHeadlessRenderer,
    translation: (f32, f32),
    paths: Paths,
) -> (Scene, Scene, Size<DevicePixels>) {
    let window = device_size(700, 500);
    let viewport = sp(40., 30., 600., 400.);
    let scrollbar = quad(sp(620., 40., 12., 120.), rgba(0x888888ff));

    let mut direct = Scene::default();
    add_panel(&mut direct, window);
    add_content(&mut direct, renderer, translation, viewport);
    if paths == Paths::With {
        direct.insert_primitive(path(translation, viewport));
    }
    direct.insert_primitive(scrollbar.clone());
    direct.finish();

    let tiles = [coord(0, 0), coord(1, 0), coord(0, 1), coord(1, 1)];
    let layer = layer_frame(LayerKey(5), 1, content(renderer, (0., 0.)), &tiles);
    let mut composited = Scene::default();
    add_panel(&mut composited, window);
    add_tiles(&mut composited, &layer, &tiles, translation, viewport);
    if paths == Paths::With {
        // After the tile quads, as `fast::layers::paint::insert_paths`
        // inserts them.
        composited.insert_primitive(path(translation, viewport));
    }
    composited.insert_primitive(scrollbar);
    composited.layers.frames.push(layer);
    composited.finish();
    (direct, composited, window)
}

/// The window's opaque panel, in the layers' baked background colour.
fn add_panel(scene: &mut Scene, window: Size<DevicePixels>) {
    scene.insert_primitive(quad(
        sp(0., 0., window.width.0 as f32, window.height.0 as f32),
        background(),
    ));
}

/// A scene that draws [`content`] moved by `translation` over the panel,
/// clipped to `viewport` (the whole 1024×1024 window when `None`).
fn direct_scene(
    renderer: &MetalHeadlessRenderer,
    translation: (f32, f32),
    viewport: Option<Bounds<ScaledPixels>>,
) -> Scene {
    let mut scene = Scene::default();
    add_panel(&mut scene, device_size(1024, 1024));
    let viewport = viewport.unwrap_or(sp(0., 0., 1024., 1024.));
    add_content(&mut scene, renderer, translation, viewport);
    scene.finish();
    scene
}

/// A scene that composites `tiles` of `layer`, scrolled by `translation`
/// since they were painted, over the panel, clipped to `viewport` (the
/// whole 1024×1024 window when `None`).
fn composited_scene(
    layer: LayerFrame,
    tiles: &[TileCoord],
    translation: (f32, f32),
    viewport: Option<Bounds<ScaledPixels>>,
) -> Scene {
    let mut scene = Scene::default();
    add_panel(&mut scene, device_size(1024, 1024));
    let viewport = viewport.unwrap_or(sp(0., 0., 1024., 1024.));
    add_tiles(&mut scene, &layer, tiles, translation, viewport);
    scene.layers.frames.push(layer);
    scene.finish();
    scene
}

/// Inserts the quads that composite `tiles` of `layer`, as
/// `fast::layers::paint::insert_tile_quads` does.
fn add_tiles(
    scene: &mut Scene,
    layer: &LayerFrame,
    tiles: &[TileCoord],
    translation: (f32, f32),
    viewport: Bounds<ScaledPixels>,
) {
    scene.push_layer(viewport);
    for &tile in tiles {
        let bounds = layer.tile_bounds(tile);
        let side = layer.tile_size as f32;
        scene.insert_primitive(PolychromeSprite {
            order: 0,
            pad: LAYER_TILE_PREMULTIPLIED,
            grayscale: false,
            opacity: 1.,
            bounds: sp(
                bounds.origin.x.0 + translation.0,
                bounds.origin.y.0 + translation.1,
                side,
                side,
            ),
            content_mask: mask(viewport),
            corner_radii: Corners::default(),
            tile: AtlasTile {
                texture_id: layer_tile_texture_id(layer.key),
                tile_id: layer_tile_id(tile),
                padding: 0,
                bounds: Bounds {
                    origin: point(DevicePixels(0), DevicePixels(0)),
                    size: device_size(layer.tile_size as i32, layer.tile_size as i32),
                },
            },
            transformation: TransformationMatrix::unit(),
        });
    }
    scene.pop_layer();
}

/// A layer's content: every primitive kind a tile holds, spread over the
/// four tiles of (0, 0)..(1024, 1024) and across their edges, moved by
/// `offset`.
fn content(renderer: &MetalHeadlessRenderer, offset: (f32, f32)) -> Scene {
    let mut scene = Scene::default();
    add_content(&mut scene, renderer, offset, everywhere().bounds);
    scene.finish();
    scene
}

/// Inserts [`content`]'s primitives into `scene`, clipped to `clip`.
fn add_content(
    scene: &mut Scene,
    renderer: &MetalHeadlessRenderer,
    offset: (f32, f32),
    clip: Bounds<ScaledPixels>,
) {
    let (ox, oy) = offset;
    let at = |x: f32, y: f32, w: f32, h: f32| sp(x + ox, y + oy, w, h);
    let clipped = |bounds: Bounds<ScaledPixels>| mask(bounds.intersect(&clip));

    scene.insert_primitive(Quad {
        content_mask: mask(clip),
        ..quad(at(100., 100., 200., 150.), rgba(0xcc3322ff))
    });
    // A rounded, bordered quad whose left edge crosses pixel centres.
    scene.insert_primitive(Quad {
        bounds: at(450.5, 60., 150., 120.),
        content_mask: clipped(at(0., 0., 1024., 1024.)),
        border_color: Background::from(rgba(0x222222ff)).prepare(),
        corner_radii: Corners::all(ScaledPixels(12.)),
        border_widths: Edges::all(ScaledPixels(3.)),
        ..quad(at(0., 0., 0., 0.), rgba(0xeeeeeeff))
    });
    // The shadow's tail crosses the tile edge its bounds stop short of.
    scene.insert_primitive(Shadow {
        order: 0,
        blur_radius: ScaledPixels(8.),
        bounds: at(380., 420., 120., 70.),
        corner_radii: Corners::all(ScaledPixels(6.)),
        content_mask: mask(clip),
        color: paint(rgba(0x00000080)),
        element_bounds: at(380., 420., 120., 70.),
        element_corner_radii: Corners::all(ScaledPixels(6.)),
        inset: 0,
        pad: 0,
        transformation: TransformationMatrix::unit(),
    });
    // A dithered gradient across the corner the four tiles share and beyond.
    scene.insert_primitive(Quad {
        bounds: at(440., 560., 460., 240.),
        content_mask: mask(clip),
        background: linear_gradient(
            45.,
            [
                linear_color_stop(rgba(0x331111ff), 0.),
                linear_color_stop(rgba(0x111133ff), 1.),
            ],
        )
        .prepare(),
        corner_radii: Corners::all(ScaledPixels(20.)),
        ..quad(at(0., 0., 0., 0.), rgba(0x000000ff))
    });
    scene.insert_primitive(Underline {
        order: 0,
        pad: 0,
        bounds: at(50., 505., 900., 8.),
        content_mask: mask(clip),
        color: paint(rgba(0xffcc00ff)),
        thickness: ScaledPixels(2.),
        wavy: 1,
        transformation: TransformationMatrix::unit(),
    });
    // A clipped quad whose mask crosses a tile edge.
    scene.insert_primitive(Quad {
        content_mask: clipped(at(490., 250., 300., 100.)),
        ..quad(at(700., 200., 200., 200.), rgba(0x44aa44ff))
    });

    scene.insert_primitive(MonochromeSprite {
        order: 0,
        kind: MonochromeSpriteKind::Glyph,
        bounds: at(505., 700., 16., 16.),
        content_mask: mask(clip),
        color: paint(rgba(0xffffffff)),
        tile: glyph_tile(renderer, 1, false),
        transformation: TransformationMatrix::unit(),
    });
    scene.insert_primitive(PolychromeSprite {
        order: 0,
        pad: 0,
        grayscale: false,
        opacity: 1.,
        bounds: at(300., 500., 20., 20.),
        content_mask: mask(clip),
        corner_radii: Corners::all(ScaledPixels(4.)),
        tile: image_tile(renderer),
        transformation: TransformationMatrix::unit(),
    });
}

/// A path of lines and a curve across a tile edge, moved by `offset`. It is
/// the last thing its scene draws where it lies: the core lifts a layer's
/// paths out of its tiles only when nothing is drawn over them.
fn path((ox, oy): (f32, f32), clip: Bounds<ScaledPixels>) -> Path<ScaledPixels> {
    let mut path = Path::new(point(px(480. + ox), px(300. + oy)));
    path.line_to(point(px(560.5 + ox), px(330. + oy)));
    path.curve_to(
        point(px(500. + ox), px(560. + oy)),
        point(px(600. + ox), px(450. + oy)),
    );
    path.line_to(point(px(480. + ox), px(300. + oy)));
    let mut path = path.scale(1.).with_content_mask(mask(clip));
    path.color = Background::from(rgba(0x8800ffcc)).prepare();
    path
}

/// A 16×16 glyph of a made-up font, uploaded to the renderer's atlas: a
/// monochrome one, or a subpixel one.
fn glyph_tile(renderer: &MetalHeadlessRenderer, glyph: u32, subpixel: bool) -> AtlasTile {
    let key = AtlasKey::Glyph(RenderGlyphParams {
        font_id: FontId(9_999),
        glyph_id: GlyphId(glyph),
        font_size: px(12.),
        subpixel_variant: point(0, 0),
        scale_factor: 1.,
        is_emoji: false,
        subpixel_rendering: subpixel,
        dilation: 0,
    });
    let bytes_per_pixel = if subpixel { 4 } else { 1 };
    let bytes: Vec<u8> = (0..16 * 16 * bytes_per_pixel)
        .map(|i| ((i * 37) % 256) as u8)
        .collect();
    renderer
        .sprite_atlas()
        .get_or_insert_with(key, &mut || {
            Ok(Some((device_size(16, 16), Cow::Owned(bytes.clone()))))
        })
        .expect("glyph uploaded")
        .expect("glyph tile")
}

/// A 20×20 image, translucent in places, uploaded to the renderer's atlas.
fn image_tile(renderer: &MetalHeadlessRenderer) -> AtlasTile {
    let key = AtlasKey::Image(RenderImageParams {
        image_id: ImageId(9_999),
        frame_index: 0,
    });
    let bytes: Vec<u8> = (0..20 * 20)
        .flat_map(|i: u32| {
            [
                (i * 7 % 256) as u8,
                (i * 13 % 256) as u8,
                (i * 3 % 256) as u8,
                if i.is_multiple_of(5) { 128 } else { 255 },
            ]
        })
        .collect();
    renderer
        .sprite_atlas()
        .get_or_insert_with(key, &mut || {
            Ok(Some((device_size(20, 20), Cow::Owned(bytes.clone()))))
        })
        .expect("image uploaded")
        .expect("image tile")
}

// --- layer helpers --- //

fn coord(x: i32, y: i32) -> TileCoord {
    TileCoord { x, y }
}

/// The opaque colour under the layers' content, baked into their tiles.
fn background() -> Rgba {
    rgba(0x336699ff)
}

fn layer_frame(key: LayerKey, generation: u64, content: Scene, dirty: &[TileCoord]) -> LayerFrame {
    LayerFrame {
        key,
        generation,
        background: paint(background()),
        tile_size: TILE,
        content: content.into(),
        dirty_tiles: dirty.to_vec(),
    }
}

fn scene_layers(key: LayerKey, generation: u64, dirty: &[TileCoord]) -> SceneLayers {
    SceneLayers {
        frames: vec![layer_frame(key, generation, Scene::default(), dirty)],
    }
}

// --- helpers --- //

fn renderer() -> MetalHeadlessRenderer {
    MetalHeadlessRenderer::new()
}

fn render(
    renderer: &mut MetalHeadlessRenderer,
    scene: &Scene,
    size: Size<DevicePixels>,
) -> RgbaImage {
    renderer
        .render_scene_to_image(scene, size)
        .expect("the scene rendered")
}

/// Renders [`direct_scene`].
fn render_direct(
    renderer: &mut MetalHeadlessRenderer,
    translation: (f32, f32),
    viewport: Option<Bounds<ScaledPixels>>,
    size: Size<DevicePixels>,
) -> RgbaImage {
    let scene = direct_scene(renderer, translation, viewport);
    render(renderer, &scene, size)
}

/// How many tiles `renderer` has rasterized.
fn rasterized(renderer: &MetalHeadlessRenderer) -> usize {
    renderer.renderer.fast_layers.rasterized
}

fn sp(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(w), ScaledPixels(h)),
    }
}

fn mask(bounds: Bounds<ScaledPixels>) -> ContentMask<ScaledPixels> {
    ContentMask {
        bounds,
        corner_radii: Corners::default(),
    }
}

/// A content mask that clips nothing the tests draw.
fn everywhere() -> ContentMask<ScaledPixels> {
    mask(sp(-10_000., -10_000., 20_000., 20_000.))
}

fn paint(color: impl Into<Color>) -> PaintColor {
    color.into().paint_color()
}

fn quad(bounds: Bounds<ScaledPixels>, color: Rgba) -> Quad {
    Quad {
        bounds,
        content_mask: everywhere(),
        background: Background::from(color).prepare(),
        ..Default::default()
    }
}

fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
    size(DevicePixels(width), DevicePixels(height))
}

#[track_caller]
fn assert_same_pixels(actual: &RgbaImage, expected: &RgbaImage) {
    assert_eq!(actual.dimensions(), expected.dimensions());
    let differing: Vec<(u32, u32)> = actual
        .enumerate_pixels()
        .filter(|(x, y, pixel)| *pixel != expected.get_pixel(*x, *y))
        .map(|(x, y, _)| (x, y))
        .collect();
    let Some(&(x, y)) = differing.first() else {
        return;
    };
    let (min_x, max_x) = differing
        .iter()
        .fold((u32::MAX, 0), |(lo, hi), (x, _)| (lo.min(*x), hi.max(*x)));
    let (min_y, max_y) = differing
        .iter()
        .fold((u32::MAX, 0), |(lo, hi), (_, y)| (lo.min(*y), hi.max(*y)));
    panic!(
        "{} pixels differ, within x {min_x}..={max_x}, y {min_y}..={max_y}; first at {:?}: \
         {:?} instead of {:?}",
        differing.len(),
        (x, y),
        actual.get_pixel(x, y),
        expected.get_pixel(x, y),
    );
}
