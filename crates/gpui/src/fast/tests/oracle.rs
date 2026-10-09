//! The frame description and the randomized cached-view oracle.
//!
//! The mutation tests prove that [`crate::fast::verify::describe_frame`]
//! notices a change to each kind of field it claims to capture: an oracle
//! comparing descriptions blind to a field would pass whatever a mechanism
//! did to it. The randomized test then drives a window reusing cached views
//! and one drawing every frame from scratch through the same histories.

use super::oracle_support::{PALETTE, run_lockstep, seeds_and_steps};
use crate::{
    App, BoxShadow, Context, CursorStyle, ElementEffect, FastMode, Hitbox, HitboxBehavior,
    HtmlElementContentFit, HtmlElementInputMode, HtmlElementLifetime, HtmlElementWheelBehavior,
    HtmlIslandInputMode, Path, Render, TransformationMatrix, UnderlineStyle, Window, WindowHandle,
    canvas, div, layer_blur, point, prelude::*, px, size,
};

/// Paints a primitive of every family the scene has, with a hitbox, a cursor
/// and a listener, so each can be mutated.
struct EveryFamily;

impl Render for EveryFamily {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            canvas(
                |bounds, window, _| {
                    (
                        window.insert_hitbox(bounds, HitboxBehavior::Normal),
                        window.insert_hitbox(
                            crate::Bounds::new(point(px(5.), px(5.)), size(px(20.), px(20.))),
                            HitboxBehavior::BlockMouse,
                        ),
                    )
                },
                |_, (hitbox, blocker): (Hitbox, Hitbox), window, cx| {
                    let at = |x: f32, y: f32, w: f32, h: f32| {
                        crate::Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
                    };
                    window.paint_drop_shadows(
                        at(10., 10., 40., 40.),
                        crate::Corners::all(px(4.)),
                        &[BoxShadow {
                            color: PALETTE[0].into(),
                            offset: point(px(2.), px(2.)),
                            blur_radius: px(4.),
                            spread_radius: px(0.),
                            inset: false,
                        }],
                    );
                    window.paint_visual_effects(
                        at(60., 10., 40., 40.),
                        crate::Corners::all(px(2.)),
                        &[ElementEffect::from(layer_blur(px(3.)))],
                    );
                    window.paint_quad(crate::fill(at(10., 60., 40., 40.), PALETTE[1]));
                    window.paint_quad(crate::fill(at(20., 70., 40., 40.), PALETTE[2]));
                    window.paint_screen_effect_region(
                        "region",
                        at(110., 10., 40., 40.),
                        crate::Corners::default(),
                    );
                    window.paint_html_island(
                        "island".into(),
                        at(160., 10., 40., 40.),
                        HtmlIslandInputMode::GpuiOwned,
                        1,
                        "<b>island</b>".into(),
                    );
                    window.paint_html_element(
                        "element".into(),
                        at(210., 10., 40., 40.),
                        HtmlElementInputMode::GpuiOwned,
                        HtmlElementLifetime::PruneWhenAbsent,
                        HtmlElementContentFit::Natural,
                        HtmlElementWheelBehavior::Browser,
                        1,
                        "<i>element</i>".into(),
                        "".into(),
                    );
                    let mut path = Path::new(point(px(60.), px(60.)));
                    path.line_to(point(px(100.), px(60.)));
                    path.line_to(point(px(80.), px(100.)));
                    window.paint_path(path, PALETTE[3]);
                    window.paint_underline(
                        point(px(110.), px(80.)),
                        px(40.),
                        &UnderlineStyle {
                            thickness: px(2.),
                            color: Some(PALETTE[0].into()),
                            wavy: false,
                        },
                    );
                    let font_id = window.text_system().resolve_font(&crate::font("Glyphs"));
                    window
                        .paint_glyph(
                            point(px(160.), px(80.)),
                            font_id,
                            crate::GlyphId(1),
                            px(14.),
                            PALETTE[0],
                        )
                        .unwrap();
                    window
                        .paint_emoji(
                            point(px(180.), px(80.)),
                            font_id,
                            crate::GlyphId(2),
                            px(14.),
                        )
                        .unwrap();
                    window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
                    window.set_cursor_style(CursorStyle::Crosshair, &blocker);
                    window.on_mouse_event(|_: &crate::MouseDownEvent, _, _, _| {});
                    let _ = cx;
                },
            )
            .size_full(),
        )
    }
}

fn open<V: Render>(
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> (crate::TestAppContext, WindowHandle<V>) {
    let mut cx = super::oracle_support::oracle_app(0);
    let window = cx.add_window(build);
    cx.update_window(window.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })
    .unwrap();
    (cx, window)
}

/// Asserts that applying `mutate` to the rendered frame changes its
/// description, then undoes the mutation by redrawing.
fn assert_detected(
    cx: &mut crate::TestAppContext,
    window: WindowHandle<impl Render>,
    what: &str,
    mutate: impl FnOnce(&mut Window, &mut App) -> bool,
) {
    cx.update_window(window.into(), |_, window, cx| {
        let before = window.describe_rendered_frame();
        assert!(
            mutate(window, cx),
            "the frame has nothing to mutate for {what}"
        );
        let after = window.describe_rendered_frame();
        assert_ne!(
            before, after,
            "the frame description missed a change to {what}"
        );
        window.refresh();
        window.draw(cx).clear(cx);
        assert_eq!(
            before,
            window.describe_rendered_frame(),
            "drawing again did not restore the frame after mutating {what}"
        );
    })
    .unwrap();
}

/// Mutates the first primitive of a scene family, if it has one.
macro_rules! mutate_family {
    ($cx:expr, $window:expr, $family:ident, $what:expr, |$p:ident| $body:expr) => {
        assert_detected(
            &mut $cx,
            $window,
            concat!(stringify!($family), " ", $what),
            |window, _| match window.rendered_frame.scene.$family.first_mut() {
                Some($p) => {
                    $body;
                    true
                }
                None => false,
            },
        );
    };
}

#[test]
fn description_detects_every_primitive_field() {
    let (mut cx, window) = open(|_, _| EveryFamily);
    let shifted = |bounds: &mut crate::Bounds<crate::ScaledPixels>| {
        bounds.origin.x += crate::ScaledPixels(1.)
    };
    let rotated = TransformationMatrix::unit().rotate(crate::radians(0.5));

    macro_rules! every_family {
        ($($family:ident),*) => {$(
            mutate_family!(cx, window, $family, "bounds", |p| shifted(&mut p.bounds));
            mutate_family!(cx, window, $family, "content mask", |p| shifted(
                &mut p.content_mask.bounds
            ));
            mutate_family!(cx, window, $family, "content mask radii", |p| p
                .content_mask
                .corner_radii
                .top_left = crate::ScaledPixels(3.));
            // Moving a primitive to the last draw order moves it in the
            // description.
            mutate_family!(cx, window, $family, "draw order", |p| p.order = u32::MAX - 1);
        )*};
    }
    every_family!(
        shadows,
        visual_effects,
        quads,
        html_islands,
        html_elements,
        screen_effect_regions,
        paths,
        underlines,
        polychrome_sprites
    );

    // Glyphs land in the monochrome or the subpixel family depending on the
    // platform's rendering mode; whichever holds them is mutated.
    cx.update_window(window.into(), |_, window, _| {
        let scene = &window.rendered_frame.scene;
        assert!(
            !scene.monochrome_sprites.is_empty() || !scene.subpixel_sprites.is_empty(),
            "the glyph painted no sprite"
        );
    })
    .unwrap();
    assert_detected(&mut cx, window, "glyph sprite bounds", |window, _| {
        let scene = &mut window.rendered_frame.scene;
        if let Some(sprite) = scene.monochrome_sprites.first_mut() {
            shifted(&mut sprite.bounds);
        } else {
            shifted(&mut scene.subpixel_sprites[0].bounds);
        }
        true
    });
    assert_detected(&mut cx, window, "glyph sprite colour", |window, _| {
        let scene = &mut window.rendered_frame.scene;
        let color = crate::color::prepare_color_with_opacity(&PALETTE[3].into(), 1.);
        if let Some(sprite) = scene.monochrome_sprites.first_mut() {
            sprite.color = color;
        } else {
            scene.subpixel_sprites[0].color = color;
        }
        true
    });

    // Transforms.
    mutate_family!(cx, window, quads, "transform", |p| p.transformation =
        rotated);
    mutate_family!(cx, window, shadows, "transform", |p| p.transformation =
        rotated);
    mutate_family!(cx, window, underlines, "transform", |p| p.transformation =
        rotated);
    mutate_family!(cx, window, visual_effects, "transform", |p| p
        .transformation =
        rotated);
    mutate_family!(cx, window, polychrome_sprites, "transform", |p| p
        .transformation =
        rotated);
    mutate_family!(cx, window, screen_effect_regions, "transform", |p| p
        .transformation =
        rotated);

    // Colour and opacity.
    mutate_family!(cx, window, quads, "background", |p| p.background =
        crate::Background::from(PALETTE[4]).prepare());
    mutate_family!(cx, window, shadows, "colour", |p| p.color =
        crate::color::prepare_color_with_opacity(&PALETTE[4].into(), 1.));
    mutate_family!(cx, window, underlines, "colour", |p| p.color =
        crate::color::prepare_color_with_opacity(&PALETTE[4].into(), 1.));
    mutate_family!(cx, window, paths, "colour", |p| p.color =
        crate::Background::from(PALETTE[4]).prepare());
    mutate_family!(cx, window, visual_effects, "opacity", |p| p.opacity = 0.5);
    mutate_family!(cx, window, polychrome_sprites, "opacity", |p| p.opacity =
        0.5);
    mutate_family!(cx, window, html_islands, "opacity", |p| p.opacity = 0.5);
    mutate_family!(cx, window, html_elements, "opacity", |p| p.opacity = 0.5);
}

#[test]
fn description_detects_screen_effect_requests_and_layers() {
    let (mut cx, window) = open(|_, _| EveryFamily);
    // Request a screen effect and a snapshot, so the frame carries both.
    cx.update_window(window.into(), |_, window, cx| {
        window.play_screen_effect(crate::shader_effects::screen::ripple(point(px(5.), px(5.))));
        window.capture_screen_effect_snapshot(crate::ScreenEffectTarget::Window);
        window.draw(cx).clear(cx);
        let scene = &window.rendered_frame.scene;
        assert!(
            !scene.active_screen_effects.is_empty()
                && !scene.active_screen_effect_snapshots.is_empty(),
            "the frame carries no screen-effect request"
        );
    })
    .unwrap();
    assert_detected(&mut cx, window, "a screen-effect request", |window, _| {
        window
            .rendered_frame
            .scene
            .active_screen_effects
            .pop()
            .is_some()
    });
    assert_detected(
        &mut cx,
        window,
        "a screen-effect snapshot request",
        |window, _| {
            window
                .rendered_frame
                .scene
                .active_screen_effect_snapshots
                .pop()
                .is_some()
        },
    );
    assert_detected(&mut cx, window, "a layer", |window, _| {
        let bounds = crate::Bounds::new(
            point(crate::ScaledPixels(0.), crate::ScaledPixels(0.)),
            size(crate::ScaledPixels(5.), crate::ScaledPixels(5.)),
        );
        window.rendered_frame.scene.push_layer(bounds);
        window.rendered_frame.scene.pop_layer();
        true
    });
    assert_detected(&mut cx, window, "the scale factor", |window, _| {
        window.rendered_frame.scene.scale_factor += 1.;
        true
    });
}

#[test]
fn description_detects_every_input_field() {
    let (mut cx, window) = open(|_, _| EveryFamily);
    assert_detected(&mut cx, window, "hitbox bounds", |window, _| {
        window.rendered_frame.hitboxes[0].bounds.origin.x += px(1.);
        true
    });
    assert_detected(&mut cx, window, "hitbox mask", |window, _| {
        window.rendered_frame.hitboxes[0]
            .content_mask
            .bounds
            .size
            .width -= px(1.);
        true
    });
    assert_detected(&mut cx, window, "hitbox behaviour", |window, _| {
        window.rendered_frame.hitboxes[0].behavior = HitboxBehavior::BlockMouseExceptScroll;
        true
    });
    assert_detected(&mut cx, window, "hit-test order", |window, _| {
        window.rendered_frame.hitboxes.swap(0, 1);
        true
    });
    assert_detected(&mut cx, window, "a cursor style", |window, _| {
        window.rendered_frame.cursor_styles[0].style = CursorStyle::IBeam;
        true
    });
    assert_detected(&mut cx, window, "the hitbox a cursor names", |window, _| {
        let styles = &mut window.rendered_frame.cursor_styles;
        let hitbox = styles[1].hitbox_id;
        styles[0].hitbox_id = hitbox;
        true
    });
    assert_detected(&mut cx, window, "a mouse listener", |window, _| {
        window.rendered_frame.mouse_listeners.push(None);
        true
    });
    assert_detected(
        &mut cx,
        window,
        "the event a mouse listener listens to",
        |window, _| {
            let events = &mut window.rendered_frame.fast_mouse_listener_events;
            match events.first_mut() {
                Some(event) => {
                    *event = "gpux::MouseUpEvent";
                    true
                }
                None => false,
            }
        },
    );
    assert_detected(&mut cx, window, "an input handler", |window, _| {
        window.rendered_frame.input_handlers.push(None);
        true
    });
    assert_detected(&mut cx, window, "an accessed element state", |window, _| {
        let states = &mut window.rendered_frame.accessed_element_states;
        let state = (
            crate::GlobalElementId::new(&[crate::ElementId::Name("probe".into())]),
            std::any::TypeId::of::<u8>(),
        );
        states.push(state);
        true
    });
    assert_detected(&mut cx, window, "the dispatch tree", |window, _| {
        let nodes = &mut window.rendered_frame.dispatch_tree.nodes;
        nodes[0].context = Some(crate::KeyContext::parse("Probe").unwrap());
        true
    });
    assert_detected(&mut cx, window, "the focus", |window, _| {
        window.rendered_frame.focus = Some(crate::FocusId::default());
        true
    });
    assert_detected(&mut cx, window, "a tooltip request", |window, _| {
        window.rendered_frame.tooltip_requests.push(None);
        true
    });
    assert_detected(&mut cx, window, "a tab stop", |window, _| {
        let history = &mut window.rendered_frame.tab_stops.insertion_history;
        history.push(crate::TabStopOperation::GroupEnd);
        true
    });
    assert_detected(&mut cx, window, "the DevKit node tree", |window, _| {
        window.rendered_frame.devtools_nodes.pop().is_some()
    });
}

/// The randomized cached-view oracle: a window reusing cached views draws
/// what a window drawing from scratch draws, after every step of 16 random
/// histories of 60 steps, and really reuses cached views along the way.
#[test]
fn cached_views_match_frames_drawn_from_scratch() {
    let (seeds, steps) = seeds_and_steps(16, 60);
    let mut replayed = 0;
    for seed in seeds {
        let run = run_lockstep(seed, steps, FastMode::OFF);
        replayed += run.incremental.cached_views_replayed;
    }
    assert!(
        replayed > 0,
        "the incremental window never replayed a cached view, so nothing was compared"
    );
}
