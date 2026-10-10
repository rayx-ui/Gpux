//! Scroll-layer policy: promotion, decisions, demotion, cooldown, eviction,
//! the compiled scope and the switch. Adapted from gpui-fast's `fast/tests/layers.rs`.

use crate::AppContext as _;
use crate::fast::tests::layer_support::{EmptyView, LAYERS, add_window, set_layers};

/// Scroll layers follow the window's switch where a renderer implements
/// them, release what they keep when it goes off, and need view retention.
#[crate::test]
fn scroll_layers_follow_the_switch_where_compiled(cx: &mut crate::TestAppContext) {
    let window = add_window(cx, |_, _| EmptyView);
    cx.update_window(window.into(), |_, window, cx| {
        let compiled = crate::fast::layers::COMPILED;
        assert_eq!(window.fast_mode().scroll_layers, compiled);
        window.draw(cx).clear(cx);
        assert_eq!(crate::fast::layers::active(window, cx), compiled);

        set_layers(window, false);
        assert!(!window.fast_mode().scroll_layers);
        assert!(window.fast_layers.layers.is_empty());
        assert_eq!(window.fast_owned_state().layer_tiles, 0);
        assert_eq!(
            window.refreshing, compiled,
            "the switch redraws the window from scratch"
        );
        window.draw(cx).clear(cx);
        assert!(!crate::fast::layers::active(window, cx));

        set_layers(window, true);
        assert!(
            !crate::fast::layers::active(window, cx),
            "not while the window is refreshing"
        );
        window.draw(cx).clear(cx);
        assert_eq!(crate::fast::layers::active(window, cx), compiled);

        window.set_fast_mode(crate::FastMode {
            view_retention: false,
            ..LAYERS
        });
        assert!(
            !window.fast_mode().scroll_layers,
            "scroll layers need view retention"
        );
        window.draw(cx).clear(cx);
        assert!(!crate::fast::layers::active(window, cx));
    })
    .unwrap();
}

/// Layers are compiled where a renderer implements their tiles, and a mode
/// asking for them elsewhere is normalized to one without.
#[test]
fn the_compiled_scope_is_the_desktop_renderers() {
    assert_eq!(
        crate::fast::layers::COMPILED,
        cfg!(any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "macos",
            target_os = "windows"
        ))
    );
    assert!(crate::FastMode::ALL.normalized_for(true).scroll_layers);
    assert!(!crate::FastMode::ALL.normalized_for(false).scroll_layers);
    let budget = crate::LAYER_TILE_BUDGET_BYTES;
    assert_eq!(
        budget,
        if cfg!(any(
            target_family = "wasm",
            target_os = "android",
            target_os = "ios"
        )) {
            32 * 1024 * 1024
        } else {
            64 * 1024 * 1024
        }
    );
}

/// A scroll container whose content records the element offset it is
/// prepainted at.
struct Offsets {
    scroll: crate::ScrollHandle,
    seen: std::rc::Rc<std::cell::Cell<crate::Point<crate::Pixels>>>,
}

impl crate::Render for Offsets {
    fn render(
        &mut self,
        _window: &mut crate::Window,
        _cx: &mut crate::Context<Self>,
    ) -> impl crate::IntoElement {
        use crate::{
            InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _,
            Styled as _,
        };
        let seen = self.seen.clone();
        crate::div().size_full().child(
            crate::div()
                .id("s")
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .h(crate::px(100.))
                .child(
                    crate::canvas(
                        move |_, window, _| seen.set(window.element_offset()),
                        |_, _, _, _| {},
                    )
                    .h(crate::px(1000.))
                    .w_full(),
                ),
        )
    }
}

/// Where the content of a container scrolled to -10.37 px is prepainted, at
/// a scale factor of 1.25, in a window with scroll layers on or off and
/// their snapping forced or not.
fn content_offset(cx: &mut crate::TestAppContext, layers: bool, forced: bool) -> crate::Pixels {
    let seen = std::rc::Rc::new(std::cell::Cell::new(crate::Point::default()));
    let window = add_window(cx, {
        let seen = seen.clone();
        |_, _| Offsets {
            scroll: crate::ScrollHandle::new(),
            seen,
        }
    });
    cx.test_window(window.into())
        .simulate_scale_factor_change(1.25);
    cx.update_window(window.into(), |_, window, cx| {
        set_layers(window, layers);
        window.force_layer_snapping(forced);
        window.draw(cx).clear(cx);
    })
    .unwrap();
    window
        .update(cx, |view, _, cx| {
            crate::fast::tests::layer_support::set_offset(
                &view.scroll,
                crate::point(crate::px(0.), crate::px(-10.37)),
            );
            cx.notify();
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    seen.get().y
}

/// A path with vertices off the grid paths are snapped to, as `window`
/// paints it.
fn painted_path(window: &crate::Window) -> (crate::Path<crate::ScaledPixels>, bool) {
    let origin = crate::point(crate::px(10.1234), crate::px(20.5678));
    let mut path = crate::Path::new(origin);
    path.line_to(origin + crate::point(crate::px(12.3456), crate::px(0.)));
    path.line_to(origin + crate::point(crate::px(0.), crate::px(7.891)));
    let path = path.scale(1.);
    let painted = crate::fast::layers::paint::snapped_path(window, path.clone());
    let positions = |path: &crate::Path<crate::ScaledPixels>| {
        path.vertices
            .iter()
            .map(|vertex| vertex.xy_position)
            .collect::<Vec<_>>()
    };
    let unchanged = positions(&painted) == positions(&path);
    (painted, unchanged)
}

/// The switch also turns on what tiles need: scroll offsets snapped to
/// device pixels, glyph origins quantized so that they shift by whole pixels,
/// and path vertices snapped to a grid. With it off, output is today's.
#[crate::test]
fn the_switch_gates_snapping(cx: &mut crate::TestAppContext) {
    assert_eq!(content_offset(cx, false, false), crate::px(-10.37));
    if crate::fast::layers::COMPILED {
        // -10.37 px is -12.9625 device px; the content goes to -13.
        assert_eq!(content_offset(cx, true, false) * 1.25, crate::px(-13.));
        // A reference window snaps without layers.
        assert_eq!(content_offset(cx, false, true) * 1.25, crate::px(-13.));
    }

    let snap = crate::fast::layers::paint::snap_offset;
    let fractional = crate::point(crate::px(0.37), crate::px(-10.37));
    assert_eq!(snap(fractional, 1.25, false), fractional);
    let snapped = snap(fractional, 1.25, true);
    assert_eq!(
        (snapped.x * 1.25, snapped.y * 1.25),
        (crate::px(0.), crate::px(-13.))
    );
    let whole = crate::point(crate::px(-8.), crate::px(-12.8));
    assert_eq!(snap(whole, 1.25, true), whole, "whole device pixels stay");

    // Glyph origins: upstream's rounding mirrors around zero, the layers'
    // does not.
    let origin = crate::point(crate::ScaledPixels(-10.4), crate::ScaledPixels(-3.4));
    let (on, off) = (
        crate::fast::glyphs::glyph_pixel_origin(true, origin),
        crate::fast::glyphs::glyph_pixel_origin(false, origin),
    );
    assert_eq!(on, crate::fast::glyphs::quantize_origin(origin));
    assert_ne!(on, off);
    // An emoji's ties go to the pixel below, not toward zero.
    let tie = crate::point(crate::ScaledPixels(-10.5), crate::ScaledPixels(-3.5));
    assert_eq!(
        crate::fast::glyphs::emoji_origin(true, tie),
        crate::fast::glyphs::quantize_emoji_origin(tie)
    );
    assert_ne!(
        crate::fast::glyphs::emoji_origin(true, tie),
        crate::fast::glyphs::emoji_origin(false, tie)
    );

    // Paths.
    let window = add_window(cx, |_, _| EmptyView);
    cx.update_window(window.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let (_, unchanged) = painted_path(window);
        assert_eq!(unchanged, !crate::fast::layers::COMPILED);
        set_layers(window, false);
        let (_, unchanged) = painted_path(window);
        assert!(unchanged, "with the switch off a path is painted as it is");
        window.force_layer_snapping(true);
        let (_, unchanged) = painted_path(window);
        assert_eq!(unchanged, !crate::fast::layers::COMPILED);
    })
    .unwrap();
}

mod policies {
    use crate::fast::layers::policy::Decision;
    use crate::fast::tests::layer_support::{
        LayerPage, decision, frame_after, page, promote, scroll, scroll_and, scroller_id,
    };
    use crate::fast::tests::layer_support::{draw, with_window};
    use crate::{
        AnyElement, AnyWindowHandle, GlobalElementId, InteractiveElement as _, IntoElement,
        ParentElement as _, Path, Styled as _, TestAppContext, WindowHandle, anchored, canvas,
        deferred, div, point, px, size,
    };
    use std::rc::Rc;

    /// Puts `extra` at the top of the page's scroll container.
    fn with_extra(
        cx: &mut TestAppContext,
        handle: WindowHandle<LayerPage>,
        extra: impl Fn() -> AnyElement + 'static,
    ) {
        handle
            .update(cx, |page, _, cx| {
                page.extra = Some(Rc::new(extra));
                cx.notify();
            })
            .unwrap();
        draw(cx, handle.into());
    }

    fn has_layer(cx: &mut TestAppContext, window: AnyWindowHandle, id: &GlobalElementId) -> bool {
        with_window(cx, window, |window, _| {
            window.fast_layers.layers.contains_key(id)
        })
    }

    fn has_record(cx: &mut TestAppContext, window: AnyWindowHandle) -> bool {
        let id = scroller_id(cx, window);
        with_window(cx, window, |window, _| {
            window
                .fast_layers
                .layers
                .get(&id)
                .is_some_and(|layer| layer.record.is_some())
        })
    }

    fn layers_demoted(cx: &mut TestAppContext, window: AnyWindowHandle) -> u64 {
        with_window(cx, window, |window, _| window.fast_layers.demotions)
    }

    #[crate::test]
    fn a_container_is_promoted_after_two_scrolled_frames(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert!(!has_record(cx, window));
        // A frame drawn in between for something else breaks the streak.
        frame_after(cx, window, |cx| {
            handle.update(cx, |_, _, cx| cx.notify()).unwrap();
        });
        assert_eq!(decision(cx, window), Some(Decision::Bypass));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert!(!has_record(cx, window));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
        assert!(has_record(cx, window));
    }

    #[crate::test]
    fn deferred_draws_inside_make_it_ineligible(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        with_extra(cx, handle, || {
            div()
                .h(px(10.))
                .child(deferred(
                    anchored().child(div().w(px(50.)).h(px(50.)).bg(crate::red())),
                ))
                .into_any_element()
        });
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert!(!has_record(cx, window));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
    }

    /// Content that keeps holding what a layer cannot composite is painted
    /// into a layer to find out less and less often.
    #[crate::test]
    fn content_that_stays_ineligible_is_tried_less_and_less_often(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        with_extra(cx, handle, || {
            div()
                .h(px(10.))
                .child(deferred(
                    anchored().child(div().w(px(50.)).h(px(50.)).bg(crate::red())),
                ))
                .into_any_element()
        });
        let mut tried = Vec::new();
        for frame in 0..300 {
            let dy = if (frame / 20) % 2 == 0 { -5. } else { 5. };
            if scroll(cx, window, dy) == Some(Decision::Repaint) {
                tried.push(frame);
            }
        }
        let gaps: Vec<usize> = tried.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(tried.len() >= 4, "it is tried again: {tried:?}");
        assert!(
            gaps.windows(2).all(|pair| pair[1] > pair[0]),
            "each wait is longer than the last: {tried:?}"
        );
        assert!(tried.len() <= 6, "{tried:?}");
    }

    #[crate::test]
    fn anchored_elements_inside_make_it_ineligible(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        // Positioned against the window's edges when prepainted, which a
        // composited layer would move with the content.
        with_extra(cx, handle, || {
            div()
                .h(px(10.))
                .child(anchored().child(div().w(px(50.)).h(px(50.)).bg(crate::red())))
                .into_any_element()
        });
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        // On today's path while it holds one, but not demoted (spec §6.5).
        assert_eq!(layers_demoted(cx, window), 0);
        assert!(!has_record(cx, window));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
    }

    #[crate::test]
    fn an_animation_frame_requested_by_the_owner_makes_it_ineligible(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        frame_after(cx, window, |cx| {
            handle
                .update(cx, |page, _, cx| {
                    page.animate = true;
                    cx.notify();
                })
                .unwrap();
        });
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert!(!has_record(cx, window));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
    }

    #[crate::test]
    fn a_focused_input_inside_makes_it_ineligible(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        let focus = with_window(cx, window, |_, cx| cx.focus_handle());
        let input_focus = focus.clone();
        with_extra(cx, handle, move || {
            div()
                .h(px(10.))
                .track_focus(&input_focus)
                .child(crate::fast::tests::layer_support::text_input(
                    input_focus.clone(),
                    "inside",
                ))
                .into_any_element()
        });
        with_window(cx, window, |window, cx| window.focus(&focus, cx));
        draw(cx, window);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert!(!has_record(cx, window));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
    }

    #[crate::test]
    fn a_layer_whose_input_loses_focus_is_composited_again_soon(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        // A focused input keeps the container on today's path only while
        // it is focused: it is not demoted, which would keep the container
        // waiting for 60 stable frames (spec §6.5).
        let handle = page(cx, false);
        let window = handle.into();
        let focus = with_window(cx, window, |_, cx| cx.focus_handle());
        let input_focus = focus.clone();
        with_extra(cx, handle, move || {
            div()
                .h(px(10.))
                .track_focus(&input_focus)
                .child(crate::fast::tests::layer_support::text_input(
                    input_focus.clone(),
                    "inside",
                ))
                .into_any_element()
        });
        with_window(cx, window, |window, cx| window.focus(&focus, cx));
        draw(cx, window);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        frame_after(cx, window, |cx| {
            with_window(cx, window, |window, cx| window.blur(cx));
        });
        let mut decisions = Vec::new();
        // Down, where there is room to scroll: at the top the wheel would
        // move nothing, which is no scroll.
        for _ in 0..12 {
            decisions.push(scroll(cx, window, -10.).unwrap());
        }
        assert!(
            decisions.contains(&Decision::Composite),
            "composited again within 12 frames: {decisions:?}"
        );
        assert_eq!(layers_demoted(cx, window), 0);
    }

    /// Content painting a path, drawn over by `cover` when set.
    fn with_path(cx: &mut TestAppContext, handle: WindowHandle<LayerPage>, cover: bool) {
        with_extra(cx, handle, move || {
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    // Where it shows once scrolled by the 40 px promotion
                    // takes; out of view it is culled.
                    let origin = bounds.origin + point(px(0.), px(60.));
                    let mut path = Path::new(origin);
                    path.line_to(origin + point(px(10.), px(0.)));
                    path.line_to(origin + point(px(10.), px(10.)));
                    path.line_to(origin);
                    window.paint_path(path, crate::red());
                    if cover {
                        window.paint_quad(crate::fill(
                            crate::Bounds::new(origin, size(px(5.), px(5.))),
                            crate::blue().opacity(0.5),
                        ));
                    }
                },
            )
            .w(px(10.))
            .h(px(100.))
            .into_any_element()
        });
    }

    #[crate::test]
    fn content_with_paths_nothing_draws_over_composites(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        with_path(cx, handle, false);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        assert_eq!(layers_demoted(cx, window), 0);
    }

    #[crate::test]
    fn content_drawing_over_a_path_is_demoted(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        with_path(cx, handle, true);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert_eq!(layers_demoted(cx, window), 1);
        assert!(!has_record(cx, window));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
    }

    #[crate::test]
    fn churning_content_is_demoted_and_repromoted_after_60_stable_frames(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, true);
        let window = handle.into();
        let rows = handle
            .read_with(cx, |page, _| page.rows.clone().unwrap())
            .unwrap();
        let change = |tint: u32| {
            let rows = rows.clone();
            move |cx: &mut crate::App| {
                rows.update(cx, |rows, cx| {
                    rows.tint = tint;
                    cx.notify();
                })
            }
        };
        promote(cx, window);
        // Frequency and work guards both apply; expensive overscan may
        // demote the layer before the eighth changed frame.
        for tint in 1..=8 {
            if scroll_and(cx, window, -10., change(0x100000 + tint)) == Some(Decision::Bypass) {
                break;
            }
        }
        assert_eq!(layers_demoted(cx, window), 1);
        assert!(!has_record(cx, window));
        assert_eq!(scroll(cx, window, -10.), Some(Decision::Bypass));
        assert_eq!(scroll(cx, window, -10.), Some(Decision::Bypass));
        // A change while it is demoted starts the wait again.
        assert_eq!(
            scroll_and(cx, window, -10., change(0x300000)),
            Some(Decision::Bypass)
        );
        for _ in 0..59 {
            draw(cx, window);
        }
        assert_eq!(scroll(cx, window, -10.), Some(Decision::Bypass));
        assert_eq!(scroll(cx, window, -10.), Some(Decision::Repaint));
        assert_eq!(scroll(cx, window, -10.), Some(Decision::Composite));
    }

    #[crate::test]
    fn content_changing_every_other_frame_is_demoted(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, true);
        let window = handle.into();
        let rows = handle
            .read_with(cx, |page, _| page.rows.clone().unwrap())
            .unwrap();
        promote(cx, window);
        // A 60 Hz feed in a 120 Hz window must not keep repainting the
        // layer's overscan indefinitely at the eight-of-sixteen boundary.
        for tint in 1..=8 {
            if scroll(cx, window, -10.) == Some(Decision::Bypass) {
                break;
            }
            if scroll_and(cx, window, -10., |cx| {
                rows.update(cx, |rows, cx| {
                    rows.tint = 0x100000 + tint;
                    cx.notify();
                });
            }) == Some(Decision::Bypass)
            {
                break;
            }
        }
        assert_eq!(layers_demoted(cx, window), 1);
        assert!(!has_record(cx, window));
    }

    #[crate::test]
    fn overscan_repaints_below_the_frequency_threshold_are_demoted(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, true);
        let window = handle.into();
        let rows = handle
            .read_with(cx, |page, _| page.rows.clone().unwrap())
            .unwrap();
        promote(cx, window);
        for frame in 0..64 {
            scroll_and(cx, window, -5., |cx| {
                if frame % 4 == 0 {
                    rows.update(cx, |rows, cx| {
                        rows.tint += 1;
                        cx.notify();
                    });
                }
            });
        }
        assert_eq!(decision(cx, window), Some(Decision::Bypass));
        assert_eq!(layers_demoted(cx, window), 1);
        assert!(!has_record(cx, window));
    }

    #[crate::test]
    fn repeatedly_unprofitable_layers_extend_their_cooldown(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, true);
        let window = handle.into();
        let rows = handle
            .read_with(cx, |page, _| page.rows.clone().unwrap())
            .unwrap();
        promote(cx, window);
        for cooldown in [60, 120, 240, 480, 960, 1920] {
            for _ in 0..8 {
                if scroll_and(cx, window, -1., |cx| {
                    rows.update(cx, |rows, cx| {
                        rows.tint += 1;
                        cx.notify();
                    });
                }) == Some(Decision::Bypass)
                {
                    break;
                }
            }
            let id = scroller_id(cx, window);
            let remaining = with_window(cx, window, |window, _| {
                window.fast_layers.layers[&id].policy.demoted_until.unwrap()
                    - (window.fast_layers.frame - 1)
            });
            assert_eq!(remaining, cooldown);
            assert!(!has_record(cx, window));
            for _ in 0..cooldown - 1 {
                draw(cx, window);
            }
            assert!(has_layer(cx, window, &id), "cooldown retains backoff");
            // A retained ancestor may skip the first resumed prepaint.
            // Give the wheel enough frames to promote the container again.
            for _ in 0..4 {
                scroll(cx, window, -1.);
                if has_record(cx, window) {
                    break;
                }
            }
            assert!(has_record(cx, window), "stable content is promoted again");
            assert_eq!(scroll(cx, window, -1.), Some(Decision::Composite));
        }
        assert_eq!(layers_demoted(cx, window), 6);
        // The last recovery followed 1920 quiet frames, so a new period
        // of expensive refreshes starts with the initial cooldown again.
        for _ in 0..8 {
            if scroll_and(cx, window, -1., |cx| {
                rows.update(cx, |rows, cx| {
                    rows.tint += 1;
                    cx.notify();
                });
            }) == Some(Decision::Bypass)
            {
                break;
            }
        }
        let id = scroller_id(cx, window);
        assert_eq!(
            with_window(cx, window, |window, _| {
                window.fast_layers.layers[&id].policy.demoted_until.unwrap()
                    - (window.fast_layers.frame - 1)
            }),
            60
        );
        assert_eq!(layers_demoted(cx, window), 7);
    }

    #[crate::test]
    fn a_layer_not_composited_for_120_frames_is_dropped(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = page(cx, false).into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        let id = scroller_id(cx, window);
        for _ in 0..119 {
            draw(cx, window);
        }
        assert!(has_layer(cx, window, &id));
        draw(cx, window);
        assert!(!has_layer(cx, window, &id));
    }

    #[crate::test]
    fn resize_drops_layers(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = page(cx, false).into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        let id = scroller_id(cx, window);
        cx.simulate_window_resize(window, size(px(800.), px(500.)));
        draw(cx, window);
        assert!(!has_layer(cx, window, &id));

        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        cx.simulate_window_scale_factor_change(window, 1.5);
        draw(cx, window);
        assert!(!has_layer(cx, window, &id));
    }
}
