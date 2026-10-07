//! The wgpu headless renderer: scenes render offscreen and read back as RGBA images with no
//! window, on the default adapter and on the host's software adapter (WARP on Windows,
//! lavapipe or llvmpipe on Linux).
//!
//! Run single-threaded (`-- --test-threads=1`): DX12 device creation must not run concurrently.
#![cfg(any(target_os = "linux", target_os = "windows"))]

use gpui_wgpu::{CosmicTextSystem, HeadlessAdapterSelection, WgpuHeadlessRenderer};
use gpux::{
    AtlasKey, Background, BorderStyle, Bounds, Color, ContentMask, Corners, DevicePixels, Edges,
    GlyphId, ImageId, MonochromeSprite, MonochromeSpriteKind, PaintColor, Path,
    PlatformHeadlessRenderer, PlatformTextSystem, PolychromeSprite, Quad, RenderGlyphParams,
    RenderImageParams, ScaledPixels, Scene, Shadow, Size, TransformationMatrix, font, point, px,
    rgb, rgba, size,
};
use image::RgbaImage;
use std::borrow::Cow;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
    size(DevicePixels(width), DevicePixels(height))
}

fn scaled(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(width), ScaledPixels(height)),
    )
}

fn paint(color: impl Into<Color>) -> PaintColor {
    color.into().paint_color()
}

fn everywhere() -> ContentMask<ScaledPixels> {
    ContentMask {
        bounds: scaled(0., 0., 10_000., 10_000.),
        corner_radii: Corners::default(),
    }
}

fn quad(bounds: Bounds<ScaledPixels>, color: u32) -> Quad {
    Quad {
        bounds,
        content_mask: everywhere(),
        background: Background::from(rgb(color)).prepare(),
        border_color: Background::from(rgb(color)).prepare(),
        ..Default::default()
    }
}

/// Asserts a pixel within ±3 per channel, so probes hold across rasterizers.
#[track_caller]
fn assert_pixel(image: &RgbaImage, x: u32, y: u32, expected: [u8; 4]) {
    let actual = image.get_pixel(x, y).0;
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= 3),
        "pixel ({x}, {y}) was {actual:?}, expected {expected:?}"
    );
}

/// Rasterizes `ch` from the bundled IBM Plex Sans into the renderer's atlas and returns a
/// sprite drawing it at `origin` (its baseline).
fn glyph_sprite(
    renderer: &WgpuHeadlessRenderer,
    ch: char,
    origin: (f32, f32),
    color: PaintColor,
) -> MonochromeSprite {
    let text_system = CosmicTextSystem::new_without_system_fonts("IBM Plex Sans");
    text_system
        .add_fonts(
            gpux_fonts::ALL
                .iter()
                .map(|font| Cow::Borrowed(*font))
                .collect(),
        )
        .unwrap();
    let font_id = text_system.font_id(&font("IBM Plex Sans")).unwrap();
    let glyph_id: GlyphId = text_system.glyph_for_char(font_id, ch).unwrap();
    let params = RenderGlyphParams {
        font_id,
        glyph_id,
        font_size: px(48.),
        subpixel_variant: Default::default(),
        scale_factor: 1.,
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    };
    let raster_bounds = text_system.glyph_raster_bounds(&params).unwrap();
    let tile = renderer
        .sprite_atlas()
        .get_or_insert_with(AtlasKey::Glyph(params.clone()), &mut || {
            let (size, bytes) = text_system.rasterize_glyph(&params, raster_bounds)?;
            Ok(Some((size, Cow::Owned(bytes))))
        })
        .unwrap()
        .expect("the glyph has pixels");
    MonochromeSprite {
        order: 0,
        kind: MonochromeSpriteKind::Glyph,
        bounds: Bounds::new(
            point(
                ScaledPixels(origin.0 + raster_bounds.origin.x.0 as f32),
                ScaledPixels(origin.1 + raster_bounds.origin.y.0 as f32),
            ),
            size(
                ScaledPixels(tile.bounds.size.width.0 as f32),
                ScaledPixels(tile.bounds.size.height.0 as f32),
            ),
        ),
        content_mask: everywhere(),
        color,
        tile,
        transformation: TransformationMatrix::unit(),
    }
}

/// An 8×8 opaque green image in the atlas, drawn as a polychrome sprite.
fn image_sprite(renderer: &WgpuHeadlessRenderer, bounds: Bounds<ScaledPixels>) -> PolychromeSprite {
    // Polychrome atlas tiles are BGRA.
    let bgra = [0, 255, 0, 255].repeat(8 * 8);
    let tile = renderer
        .sprite_atlas()
        .get_or_insert_with(
            AtlasKey::Image(RenderImageParams {
                image_id: ImageId(7),
                frame_index: 0,
            }),
            &mut || Ok(Some((device_size(8, 8), Cow::Owned(bgra.clone())))),
        )
        .unwrap()
        .expect("the image has pixels");
    PolychromeSprite {
        order: 0,
        pad: 0,
        grayscale: false,
        opacity: 1.,
        bounds,
        content_mask: everywhere(),
        corner_radii: Corners::default(),
        tile,
        transformation: TransformationMatrix::unit(),
    }
}

/// A 256×128 scene with one primitive family per region, over a white backdrop.
///
/// | x       | y 0..64                        | y 64..128                      |
/// |---------|--------------------------------|--------------------------------|
/// | 0..64   | red and blue solid quads       | rounded border (radius 12)     |
/// | 64..128 | dashed green border            | blurred black shadow           |
/// | 128..192| filled blue triangle (path)    | glyph "H" in black             |
/// | 192..256| green image sprite             | red clipped to its mask, red at 50% opacity |
fn reference_scene(renderer: &WgpuHeadlessRenderer) -> Scene {
    let mut scene = Scene::default();
    scene.insert_primitive(quad(scaled(0., 0., 256., 128.), 0xffffff));

    scene.insert_primitive(quad(scaled(4., 4., 28., 56.), 0xff0000));
    scene.insert_primitive(quad(scaled(32., 4., 28., 56.), 0x0000ff));

    scene.insert_primitive(Quad {
        corner_radii: Corners::all(ScaledPixels(12.)),
        border_widths: Edges::all(ScaledPixels(3.)),
        border_color: Background::from(rgb(0x00ff00)).prepare(),
        ..quad(scaled(4., 68., 56., 56.), 0xff0000)
    });

    scene.insert_primitive(Quad {
        border_style: BorderStyle::Dashed,
        border_widths: Edges::all(ScaledPixels(4.)),
        border_color: Background::from(rgb(0x00ff00)).prepare(),
        ..quad(scaled(68., 4., 56., 56.), 0xffffff)
    });

    let shadow_bounds = scaled(80., 80., 32., 32.);
    scene.insert_primitive(Shadow {
        order: 0,
        blur_radius: ScaledPixels(6.),
        bounds: shadow_bounds,
        corner_radii: Corners::default(),
        content_mask: everywhere(),
        color: paint(rgb(0x000000)),
        element_bounds: shadow_bounds,
        element_corner_radii: Corners::default(),
        inset: 0,
        pad: 0,
        transformation: TransformationMatrix::unit(),
    });

    let mut triangle = Path::new(point(px(136.), px(56.)));
    triangle.line_to(point(px(184.), px(56.)));
    triangle.line_to(point(px(160.), px(8.)));
    triangle.line_to(point(px(136.), px(56.)));
    let mut triangle = triangle.scale(1.).with_content_mask(everywhere());
    triangle.color = Background::from(rgb(0x0000ff)).prepare();
    scene.insert_primitive(triangle);

    scene.insert_primitive(glyph_sprite(
        renderer,
        'H',
        (144., 116.),
        paint(rgb(0x000000)),
    ));

    scene.insert_primitive(image_sprite(renderer, scaled(200., 8., 48., 48.)));

    // Clipping: a red quad wider than its content mask only paints inside the mask.
    scene.insert_primitive(Quad {
        content_mask: ContentMask {
            bounds: scaled(196., 68., 24., 24.),
            corner_radii: Corners::default(),
        },
        ..quad(scaled(196., 68., 56., 24.), 0xff0000)
    });
    // Opacity: red at 50% alpha over the white backdrop.
    scene.insert_primitive(Quad {
        background: Background::from(rgba(0xff000080)).prepare(),
        ..quad(scaled(196., 100., 56., 24.), 0xff0000)
    });

    scene.finish();
    scene
}

fn assert_reference_scene(renderer: &mut WgpuHeadlessRenderer) {
    let scene = reference_scene(renderer);
    let image = renderer
        .render_scene_to_image(&scene, device_size(256, 128))
        .unwrap();
    assert_eq!(image.dimensions(), (256, 128));

    // Solid quads.
    assert_pixel(&image, 16, 32, RED);
    assert_pixel(&image, 48, 32, BLUE);
    assert_pixel(&image, 2, 2, WHITE);

    // Rounded border: the corner lies outside the rounding, the edge is the border color and
    // the inside is the fill.
    assert_pixel(&image, 5, 69, WHITE);
    assert_pixel(&image, 5, 96, GREEN);
    assert_pixel(&image, 32, 96, RED);

    // Dashed border: the top edge alternates dashes and gaps.
    let top_edge = (70..122)
        .map(|x| image.get_pixel(x, 5).0)
        .collect::<Vec<_>>();
    let dashes = top_edge
        .iter()
        .filter(|pixel| pixel[1] > 200 && pixel[0] < 60);
    let gaps = top_edge
        .iter()
        .filter(|pixel| pixel[0] > 200 && pixel[2] > 200);
    assert!(
        dashes.count() >= 8 && gaps.count() >= 8,
        "a dashed edge has both dashes and gaps: {top_edge:?}"
    );
    assert_pixel(&image, 96, 32, WHITE);

    // Blurred shadow: dark at its center, fading into the white backdrop.
    let center = image.get_pixel(96, 96).0;
    let edge = image.get_pixel(80, 96).0;
    let outside = image.get_pixel(70, 96).0;
    assert!(
        center[0] < 40 && edge[0] > center[0] && outside[0] > edge[0],
        "the shadow fades outward: center {center:?}, edge {edge:?}, outside {outside:?}"
    );

    // Path: inside the triangle is blue, beside its slanted edge is white.
    assert_pixel(&image, 160, 44, BLUE);
    assert_pixel(&image, 140, 16, WHITE);

    // Glyph: the "H" stems are dark, the counter between them stays light.
    let stem_column = (144..160)
        .map(|x| image.get_pixel(x, 100).0[0])
        .min()
        .unwrap();
    assert!(stem_column < 60, "the glyph's left stem is dark");
    assert_pixel(&image, 150, 66, WHITE);

    // Polychrome sprite.
    assert_pixel(&image, 224, 32, GREEN);

    // Clipping and opacity.
    assert_pixel(&image, 206, 80, RED);
    assert_pixel(&image, 240, 80, WHITE);
    let translucent = image.get_pixel(224, 112).0;
    assert!(
        translucent[0] > 250
            && (124..=132).contains(&translucent[1])
            && (124..=132).contains(&translucent[2]),
        "50% red over white composites in encoded sRGB: {translucent:?}"
    );
}

#[test]
fn renders_reference_primitives_on_the_default_adapter() {
    let mut renderer = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Default)
        .expect("a default headless adapter");
    assert_reference_scene(&mut renderer);
}

#[test]
fn renders_reference_primitives_on_the_software_adapter() {
    let mut renderer = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software)
        .expect("a software adapter: WARP on Windows, lavapipe or llvmpipe on Linux");
    let info = renderer.adapter_info();
    assert_eq!(info.device_type, gpui_wgpu::wgpu::DeviceType::Cpu);
    assert!(renderer.gpu_specs().is_software_emulated);
    assert_reference_scene(&mut renderer);
}

#[test]
fn adapter_identity_names_the_backend() {
    let renderer = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software)
        .expect("a software adapter");
    let info = renderer.adapter_info();
    assert!(!info.name.is_empty());
    #[cfg(target_os = "windows")]
    {
        assert_eq!(info.backend, gpui_wgpu::wgpu::Backend::Dx12);
        assert!(
            info.name.contains("Microsoft Basic Render Driver"),
            "{}",
            info.name
        );
    }
    #[cfg(target_os = "linux")]
    assert!(matches!(
        info.backend,
        gpui_wgpu::wgpu::Backend::Vulkan | gpui_wgpu::wgpu::Backend::Gl
    ));

    let specs = renderer.gpu_specs();
    assert_eq!(specs.device_name, info.name);
    assert_eq!(specs.driver_name, info.driver);
    assert_eq!(specs.backend, info.backend.to_str());

    // A name substring selects the same adapter.
    let by_name = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Name(
        info.name.to_uppercase(),
    ))
    .unwrap();
    assert_eq!(by_name.adapter_info().name, info.name);
}

#[test]
fn renderers_share_one_device_per_adapter_selection() {
    let first = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    let second = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    assert!(first.shares_device_with(&second));
    drop(first);
    let mut third =
        WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    assert!(third.shares_device_with(&second));
    // A renderer joining the shared device still renders.
    let mut scene = Scene::default();
    scene.insert_primitive(quad(scaled(0., 0., 4., 4.), 0x00ff00));
    scene.finish();
    let image = third
        .render_scene_to_image(&scene, device_size(4, 4))
        .unwrap();
    assert_pixel(&image, 1, 1, GREEN);
}

#[test]
fn captures_each_requested_size_and_strips_row_padding() {
    let mut renderer =
        WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    let mut scene = Scene::default();
    scene.insert_primitive(quad(scaled(2., 2., 4., 3.), 0xff0000));
    scene.finish();

    // 13 px rows are 52 bytes, so readback strips the 256-byte copy-row padding.
    let image = renderer
        .render_scene_to_image(&scene, device_size(13, 7))
        .unwrap();
    assert_eq!(image.dimensions(), (13, 7));
    assert_pixel(&image, 0, 0, BLACK);
    assert_pixel(&image, 3, 3, RED);
    assert_pixel(&image, 12, 6, BLACK);

    let image = renderer
        .render_scene_to_image(&scene, device_size(17, 9))
        .unwrap();
    assert_eq!(image.dimensions(), (17, 9));
    assert_pixel(&image, 3, 3, RED);
    assert_pixel(&image, 16, 8, BLACK);

    // Opaque output: an empty scene is black, never transparent.
    let image = renderer
        .render_scene_to_image(&Scene::default(), device_size(13, 7))
        .unwrap();
    assert!(image.pixels().all(|pixel| pixel.0 == BLACK));
}

#[test]
fn rejects_invalid_and_oversize_targets_without_breaking() {
    let mut renderer =
        WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    let too_large = renderer.max_texture_size() as i32 + 1;
    for size in [
        device_size(0, 8),
        device_size(8, 0),
        device_size(-1, 8),
        device_size(too_large, 8),
    ] {
        assert!(
            renderer.render_scene(&Scene::default(), size).is_err(),
            "{size:?} should be rejected"
        );
        assert!(
            renderer
                .render_scene_to_image(&Scene::default(), size)
                .is_err(),
            "{size:?} should be rejected"
        );
    }
    let image = renderer
        .render_scene_to_image(&Scene::default(), device_size(4, 4))
        .unwrap();
    assert_eq!(image.dimensions(), (4, 4));
}

#[test]
fn targets_are_non_srgb_with_fixed_path_sampling() {
    let renderer = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    let format = renderer.target_format();
    assert!(
        matches!(
            format,
            gpui_wgpu::wgpu::TextureFormat::Bgra8Unorm | gpui_wgpu::wgpu::TextureFormat::Rgba8Unorm
        ),
        "{format:?}"
    );
    assert!(!format.is_srgb());
    assert_eq!(renderer.path_sample_count(), 4);
}

#[test]
fn adapter_selection_parses_the_override_and_reports_missing_adapters() {
    assert_eq!(
        HeadlessAdapterSelection::parse(""),
        HeadlessAdapterSelection::Default
    );
    assert_eq!(
        HeadlessAdapterSelection::parse(" Software "),
        HeadlessAdapterSelection::Software
    );
    assert_eq!(
        HeadlessAdapterSelection::parse("HARDWARE"),
        HeadlessAdapterSelection::Hardware
    );
    assert_eq!(
        HeadlessAdapterSelection::parse("llvmpipe"),
        HeadlessAdapterSelection::Name("llvmpipe".into())
    );
    assert_eq!(HeadlessAdapterSelection::ENV_VAR, "GPUX_WGPU_ADAPTER");

    let error = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Name(
        "no-such-adapter-name".into(),
    ))
    .err()
    .expect("no adapter has that name");
    let message = format!("{error:#}");
    assert!(message.contains("Available adapters"), "{message}");
    assert!(message.contains("GPUX_WGPU_ADAPTER"), "{message}");
}

/// Raises a validation error on `renderer`'s device: a buffer may not be mappable for both
/// reading and writing without a native-only feature.
fn raise_validation_error(renderer: &WgpuHeadlessRenderer) {
    let _ = renderer
        .device()
        .create_buffer(&gpui_wgpu::wgpu::BufferDescriptor {
            label: Some("invalid_for_test"),
            size: 4,
            usage: gpui_wgpu::wgpu::BufferUsages::MAP_READ
                | gpui_wgpu::wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        });
}

#[test]
fn every_renderer_on_a_shared_device_observes_its_gpu_errors() {
    let render = |renderer: &mut WgpuHeadlessRenderer| {
        renderer.render_scene_to_image(&Scene::default(), device_size(4, 4))
    };
    let mut first =
        WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    let later = WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    assert!(first.shares_device_with(&later));
    // Dropping the newest renderer must not stop the earlier one from seeing errors.
    drop(later);

    raise_validation_error(&first);
    let error = render(&mut first).expect_err("the error fails the next render");
    assert!(
        format!("{error:#}").contains("GPU error during headless rendering"),
        "{error:#}"
    );
    // Each error is reported once; the renderer stays usable.
    render(&mut first).unwrap();

    // A renderer joining after an error does not report it, while existing ones do.
    raise_validation_error(&first);
    let mut joined =
        WgpuHeadlessRenderer::with_adapter(&HeadlessAdapterSelection::Software).unwrap();
    render(&mut joined).unwrap();
    assert!(render(&mut first).is_err());

    // An error raised after both exist reaches both.
    raise_validation_error(&joined);
    assert!(render(&mut first).is_err());
    assert!(render(&mut joined).is_err());
}
