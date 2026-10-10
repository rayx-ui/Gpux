//! What an overlay surface draws: text in grayscale, whatever the window's
//! subpixel capability, since it blends over native content; and backdrop
//! effects that sample only the GPUI content of the overlay, never native
//! pixels, which the overlay's renderer cannot see.

use super::oracle_support::oracle_app;
use crate::{
    AnyWindowHandle, Context, IntoElement, Render, SharedString, TestAppContext, TextRenderingMode,
    Window, WindowHandle, backdrop_blur, canvas, div, effect_frame,
    fast::composition::CompositionSurfaceId, prelude::*, px, size,
};

struct Texts;

impl Render for Texts {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child("base text")
            .child(crate::deferred(
                crate::anchored().child(
                    div().child("overlay text").child(
                        effect_frame()
                            .effect(backdrop_blur(px(4.)))
                            .child(div().size(px(20.))),
                    ),
                ),
            ))
            .child(
                canvas(
                    |_, _, _| {},
                    |bounds, _, window, cx| {
                        window.paint_top_layer(bounds, |window| {
                            let text: SharedString = "top layer text".into();
                            let style = window.text_style();
                            let run = style.to_run(text.len());
                            let line = window.text_system().shape_line(text, px(12.), &[run], None);
                            line.paint(
                                bounds.origin,
                                px(16.),
                                crate::TextAlign::Left,
                                None,
                                window,
                                cx,
                            )
                            .ok();
                        });
                    },
                )
                .w(px(120.))
                .h(px(16.)),
            )
    }
}

fn open() -> (TestAppContext, WindowHandle<Texts>) {
    let mut cx = oracle_app(0);
    cx.update(|cx| cx.set_text_rendering_mode(TextRenderingMode::Subpixel));
    let window = cx.add_window(|_, _| Texts);
    cx.simulate_window_resize(window.into(), size(px(300.), px(200.)));
    cx.set_draw_on_flush(false);
    let handle: AnyWindowHandle = window.into();
    cx.test_window(handle).0.lock().subpixel_rendering_supported = true;
    (cx, window)
}

fn present(
    cx: &mut TestAppContext,
    window: WindowHandle<Texts>,
) -> Vec<(CompositionSurfaceId, Vec<String>)> {
    cx.update_window(window.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.present();
    })
    .unwrap();
    let handle: AnyWindowHandle = window.into();
    let composition = cx.test_window(handle).0.lock().fast_composition.clone();
    composition
        .state()
        .presented
        .last()
        .cloned()
        .unwrap_or_default()
}

fn count(lines: &[String], kind: &str) -> usize {
    lines.iter().filter(|line| line.starts_with(kind)).count()
}

fn surfaces(
    cx: &mut TestAppContext,
    window: WindowHandle<Texts>,
) -> (CompositionSurfaceId, CompositionSurfaceId) {
    cx.update_window(window.into(), |_, window, _| {
        let composition = window.enable_window_composition().unwrap();
        (composition.base_surface(), composition.overlay_surface())
    })
    .unwrap()
}

/// Under a subpixel-capable window, the base surface's text is subpixel and
/// the overlay's, deferred or in the top-layer band, grayscale.
#[test]
fn overlay_text_is_grayscale_under_a_subpixel_window() {
    let (mut cx, window) = open();
    let (base, overlay) = surfaces(&mut cx, window);
    let layers = present(&mut cx, window);
    let layer = |surface| {
        layers
            .iter()
            .find(|(id, _)| *id == surface)
            .map(|(_, lines)| lines.clone())
            .unwrap_or_default()
    };
    let (base, overlay) = (layer(base), layer(overlay));
    assert!(count(&base, "SubpixelSprite") > 0, "{base:?}");
    assert_eq!(count(&base, "MonochromeSprite"), 0, "{base:?}");
    assert_eq!(count(&overlay, "SubpixelSprite"), 0, "{overlay:?}");
    // "overlay text" and "top layer text", a glyph each but for spaces.
    assert!(count(&overlay, "MonochromeSprite") >= 20, "{overlay:?}");
}

/// Without composition, the same window paints all its text subpixel: the
/// grayscale policy is the overlay's alone.
#[test]
fn text_stays_subpixel_without_composition() {
    let (mut cx, window) = open();
    let sprites = cx
        .update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let scene = &window.rendered_frame.scene;
            (scene.subpixel_sprites.len(), scene.monochrome_sprites.len())
        })
        .unwrap();
    assert!(sprites.0 > 0, "{sprites:?}");
    assert_eq!(sprites.1, 0, "{sprites:?}");
}

/// A backdrop effect deferred onto the overlay is drawn by the overlay's
/// renderer from the overlay's scene: it samples what the overlay painted
/// before it, never the base surface or native content beneath.
#[test]
fn backdrop_effects_on_the_overlay_sample_only_its_gpui_content() {
    let (mut cx, window) = open();
    let (base, overlay) = surfaces(&mut cx, window);
    let layers = present(&mut cx, window);
    let effects = |surface| {
        layers
            .iter()
            .find(|(id, _)| *id == surface)
            .map(|(_, lines)| count(lines, "VisualEffectPrimitive"))
            .unwrap_or(0)
    };
    assert_eq!(effects(overlay), 1, "{layers:?}");
    assert_eq!(effects(base), 0, "{layers:?}");
}
