//! Screenshot-capable test contexts: `TestAppContext::build_with_renderer` with the bundled
//! text system and the platform's headless renderer, `VisualTestContext::capture_screenshot`
//! at scale factors 1 and 2, and effect capabilities and GPU specs forwarded from the renderer.
//!
//! Run single-threaded (`-- --test-threads=1`): DX12 device creation must not run concurrently.
#![cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]

use gpux::{
    Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext, TestDispatcher,
    TestWindowOptions, VisualTestContext, Window, div, effect_frame, layer_blur, px, rgb, size,
};
use image::RgbaImage;

fn screenshot_context() -> TestAppContext {
    TestAppContext::build_with_renderer(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
        gpui_platform::current_headless_renderer,
    )
}

/// A white 120×80 window with a red 40×20 block at (10, 10) and black text at (10, 50).
struct Card;

impl Render for Card {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(10.))
                    .w(px(40.))
                    .h(px(20.))
                    .bg(rgb(0xff0000)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(44.))
                    .text_size(px(24.))
                    .text_color(rgb(0x000000))
                    .child("HH"),
            )
    }
}

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

fn open_card(cx: &mut TestAppContext, scale_factor: f32) -> &mut VisualTestContext {
    let (_, cx) = cx.add_window_view_with_options(
        TestWindowOptions::new(size(px(120.), px(80.))).scale_factor(scale_factor),
        |_, _| Card,
    );
    cx
}

#[test]
fn captures_the_window_at_scale_factors_one_and_two() {
    for scale_factor in [1., 2.] {
        let mut cx = screenshot_context();
        let window = open_card(&mut cx, scale_factor);
        let image = window.capture_screenshot().unwrap();
        let s = scale_factor as u32;
        assert_eq!(image.dimensions(), (120 * s, 80 * s));

        assert_pixel(&image, 30 * s, 20 * s, [255, 0, 0, 255]);
        assert_pixel(&image, 100 * s, 20 * s, [255, 255, 255, 255]);
        assert_pixel(&image, 5 * s, 5 * s, [255, 255, 255, 255]);
        // The block's edges land on whole logical pixels at both scales.
        assert_pixel(&image, 10 * s, 10 * s, [255, 0, 0, 255]);
        assert_pixel(&image, 10 * s - 1, 10 * s, [255, 255, 255, 255]);

        // The text "HH" rasterizes with the bundled font: its stems are dark.
        let darkest = (44 * s..74 * s)
            .flat_map(|y| (10 * s..50 * s).map(move |x| (x, y)))
            .map(|(x, y)| image.get_pixel(x, y).0[0])
            .min()
            .unwrap();
        assert!(
            darkest < 60,
            "the text rendered (darkest red channel {darkest})"
        );
        cx.quit();
    }
}

#[test]
fn screenshots_are_deterministic_and_opaque() {
    let capture = || {
        let mut cx = screenshot_context();
        let image = open_card(&mut cx, 2.).capture_screenshot().unwrap();
        cx.quit();
        image
    };
    let first = capture();
    assert!(first.pixels().all(|pixel| pixel.0[3] == 255), "opaque");
    assert_eq!(first, capture(), "the same window renders the same pixels");
}

#[test]
fn windows_report_the_renderers_capabilities_and_gpu() {
    let mut cx = screenshot_context();
    let renderer = gpui_platform::current_headless_renderer()
        .unwrap()
        .expect("this OS has a headless renderer");
    let window = open_card(&mut cx, 1.);
    let (capabilities, specs) =
        window.update(|window, _| (window.effect_capabilities(), window.gpu_specs()));
    assert_eq!(capabilities, renderer.effect_capabilities());
    let specs = specs.expect("the window reports its renderer's GPU");
    assert!(!specs.device_name.is_empty());
    assert_eq!(Some(specs), renderer.gpu_specs());
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    assert!(
        capabilities.element.backdrop_blur && capabilities.element.glow,
        "the wgpu renderer draws element effects: {capabilities:?}"
    );
    cx.quit();
}

#[gpux::test]
fn contexts_without_a_renderer_report_no_capabilities_and_cannot_capture(cx: &mut TestAppContext) {
    let window = open_card(cx, 1.);
    let (capabilities, specs) =
        window.update(|window, _| (window.effect_capabilities(), window.gpu_specs()));
    assert_eq!(capabilities, Default::default());
    assert_eq!(specs, None);
    let error = window.capture_screenshot().unwrap_err();
    assert!(
        error.to_string().contains("no HeadlessRenderer"),
        "the error names the missing renderer: {error}"
    );
}

/// A red block inside an `effect_frame`, optionally with a layer blur.
struct Blurred {
    blur: bool,
}

impl Render for Blurred {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let block = div().w(px(40.)).h(px(40.)).bg(rgb(0xff0000));
        let frame = if self.blur {
            effect_frame().effect(layer_blur(px(6.))).child(block)
        } else {
            effect_frame().child(block)
        };
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .child(div().absolute().left(px(40.)).top(px(20.)).child(frame))
    }
}

#[test]
fn effect_frames_render_when_the_renderer_supports_them() {
    let capture = |blur: bool| {
        let mut cx = screenshot_context();
        let (_, window) = cx.add_window_view_with_options(
            TestWindowOptions::new(size(px(120.), px(80.))),
            move |_, _| Blurred { blur },
        );
        let supported =
            window.update(|window, _| window.effect_capabilities().element.backdrop_blur);
        let image = window.capture_screenshot().unwrap();
        cx.quit();
        (supported, image)
    };
    let (supported, blurred) = capture(true);
    let (_, sharp) = capture(false);

    // Without the effect the block's left edge (x = 40) is sharp: white outside, red inside.
    assert_pixel(&sharp, 39, 40, [255, 255, 255, 255]);
    assert_pixel(&sharp, 40, 40, [255, 0, 0, 255]);
    assert_pixel(&sharp, 60, 40, [255, 0, 0, 255]);
    let outside = blurred.get_pixel(39, 40).0;
    let inside = blurred.get_pixel(40, 40).0;
    if supported {
        // The layer blur runs: it tints the backdrop past the block's edge and lightens the
        // block just inside it. The test pins that the effect renders, not its strength.
        assert!(
            outside[1] < 252 && inside[1] > 10,
            "the blur changes both sides of the edge: outside {outside:?}, inside {inside:?}"
        );
        assert_ne!(blurred, sharp);
    } else {
        // Unsupported effects take their documented fallback: the child renders unblurred.
        assert_eq!(blurred, sharp);
    }
}

/// A renderer without shader effects, modelled on the platform renderer: it keeps the
/// trait's default capabilities (none) and, like a backend without effect pipelines, skips the
/// effect batches of every scene it draws.
struct WithoutEffects {
    inner: Box<dyn gpux::PlatformHeadlessRenderer>,
    /// The most effect primitives GPUI emitted in one scene.
    effects_emitted: std::rc::Rc<std::cell::Cell<usize>>,
}

impl WithoutEffects {
    fn without_effects(scene: &gpux::Scene) -> gpux::Scene {
        let mut stripped = gpux::Scene::default();
        stripped.scale_factor = scene.scale_factor;
        stripped.shadows = scene.shadows.clone();
        stripped.quads = scene.quads.clone();
        stripped.paths = scene.paths.clone();
        stripped.underlines = scene.underlines.clone();
        stripped.monochrome_sprites = scene.monochrome_sprites.clone();
        stripped.subpixel_sprites = scene.subpixel_sprites.clone();
        stripped.polychrome_sprites = scene.polychrome_sprites.clone();
        stripped
    }
}

impl gpux::PlatformHeadlessRenderer for WithoutEffects {
    fn render_scene_to_image(
        &mut self,
        scene: &gpux::Scene,
        size: gpux::Size<gpux::DevicePixels>,
    ) -> anyhow::Result<RgbaImage> {
        self.effects_emitted
            .set(self.effects_emitted.get().max(scene.visual_effects.len()));
        self.inner
            .render_scene_to_image(&Self::without_effects(scene), size)
    }

    fn render_scene(
        &mut self,
        scene: &gpux::Scene,
        size: gpux::Size<gpux::DevicePixels>,
    ) -> anyhow::Result<()> {
        self.inner.render_scene(&Self::without_effects(scene), size)
    }

    fn sprite_atlas(&self) -> std::sync::Arc<dyn gpux::PlatformAtlas> {
        self.inner.sprite_atlas()
    }
}

#[test]
fn effect_frames_take_their_fallback_when_the_renderer_lacks_effects() {
    let capture = |blur: bool| {
        let effects_emitted = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut cx = TestAppContext::build_with_renderer(
            TestDispatcher::new(0),
            None,
            gpui_platform::bundled_text_system(),
            {
                let effects_emitted = effects_emitted.clone();
                move || {
                    Ok(gpui_platform::current_headless_renderer()?.map(|inner| {
                        Box::new(WithoutEffects {
                            inner,
                            effects_emitted: effects_emitted.clone(),
                        }) as Box<dyn gpux::PlatformHeadlessRenderer>
                    }))
                }
            },
        );
        let (_, window) = cx.add_window_view_with_options(
            TestWindowOptions::new(size(px(120.), px(80.))),
            move |_, _| Blurred { blur },
        );
        let capabilities = window.update(|window, _| window.effect_capabilities());
        let image = window.capture_screenshot().unwrap();
        cx.quit();
        (capabilities, image, effects_emitted.get())
    };
    let (capabilities, blurred, emitted_with_blur) = capture(true);
    let (_, sharp, emitted_without_blur) = capture(false);
    assert_eq!(capabilities, Default::default(), "no effect is claimed");
    // GPUI still emits the effect for the renderer to draw or skip.
    assert_eq!((emitted_with_blur, emitted_without_blur), (1, 0));
    // A renderer without the blur skips it: the frame's child renders exactly as without the
    // effect, with the same layout.
    assert_eq!(blurred, sharp);
    assert_pixel(&blurred, 39, 40, [255, 255, 255, 255]);
    assert_pixel(&blurred, 40, 40, [255, 0, 0, 255]);
}
