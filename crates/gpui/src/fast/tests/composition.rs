//! Window composition on the test platform, which records what it is handed:
//! the surface tree and its transactions, unsupported platforms, the path
//! with composition off, the scene split per surface (the top-layer band and
//! the overlay range), balanced clips, surface switches carried by views
//! drawn from last frame, native-surface geometry pushed only on change,
//! the overlay's input region, and native surfaces shown only while their
//! element is drawn.

use super::view_support::VIEWS;
use super::{oracle_support::oracle_app, splice::SPLICE};
use crate::{
    AnyWindowHandle, AppContext as _, Bounds, Context, DevicePixels, Entity, FastMode, IntoElement,
    Render, TestAppContext, Window, WindowHandle, canvas, div,
    fast::composition::{
        CompositionSurfaceId, CompositionSurfaceKind, CompositionTree, TestComposition,
        composed_scene_layers,
    },
    native_surface, point,
    prelude::*,
    px, size,
};

fn recorder(cx: &TestAppContext, window: impl Into<AnyWindowHandle>) -> TestComposition {
    cx.test_window(window.into())
        .0
        .lock()
        .fast_composition
        .clone()
}

#[test]
fn surfaces_have_stable_identity_and_explicit_order() -> anyhow::Result<()> {
    let mut tree = CompositionTree::new();
    let base = tree.insert(CompositionSurfaceKind::Gpui, None)?;
    let webview = tree.insert(CompositionSurfaceKind::Native, None)?;
    let overlay = tree.insert(CompositionSurfaceKind::Gpui, None)?;
    tree.place_below(overlay, webview)?;
    assert_eq!(tree.flattened(), [base, overlay, webview]);
    let video = tree.insert(CompositionSurfaceKind::ExternalGpu, None)?;
    tree.reparent(video, Some(webview))?;
    assert_eq!(tree.parent(video)?, Some(webview));
    assert_eq!(tree.flattened(), [base, overlay, webview, video]);
    // No cycles, no ordering across parents.
    assert!(tree.reparent(webview, Some(video)).is_err());
    assert!(tree.place_above(video, overlay).is_err());
    // Removing a surface keeps its children where it was.
    tree.remove(webview)?;
    assert_eq!(tree.parent(video)?, None);
    assert_eq!(tree.flattened(), [base, overlay, video]);
    Ok(())
}

#[test]
fn scene_ranges_follow_tree_order_and_reject_bad_markers() -> anyhow::Result<()> {
    let mut tree = CompositionTree::new();
    let base = tree.insert(CompositionSurfaceKind::Gpui, None)?;
    let native = tree.insert(CompositionSurfaceKind::Native, None)?;
    let overlay = tree.insert(CompositionSurfaceKind::Gpui, None)?;
    // Ten operations: five empty clip layers.
    let mut scene = crate::Scene::default();
    for _ in 0..5 {
        scene.push_layer(Bounds::default());
        scene.pop_layer();
    }
    let layers = composed_scene_layers(
        &tree,
        &[(base, 0), (overlay, 4), (base, 7)],
        &scene,
        overlay,
    )?;
    let [base_layer, overlay_layer] = layers.as_slice() else {
        panic!("expected base and overlay layers: {layers:?}");
    };
    assert_eq!(base_layer.surface, base);
    assert_eq!(base_layer.ranges, [0..4, 7..10]);
    assert_eq!(overlay_layer.surface, overlay);
    assert_eq!(overlay_layer.ranges, [4..7]);
    assert!(composed_scene_layers(&tree, &[(native, 0)], &scene, overlay).is_err());
    assert!(composed_scene_layers(&tree, &[(base, 2), (base, 1)], &scene, overlay).is_err());
    Ok(())
}

/// A root view that paints a red box, defers a blue one, and may place a
/// native surface and paint part of itself onto the overlay.
struct Content {
    native: Option<crate::WindowCompositionSurface>,
    native_left: f32,
    top_layer: bool,
    onto_overlay: Option<CompositionSurfaceId>,
    clip_overlay: bool,
    child: Option<Entity<Child>>,
    tick: usize,
}

impl Content {
    fn new() -> Self {
        Content {
            native: None,
            native_left: 10.,
            top_layer: false,
            onto_overlay: None,
            clip_overlay: false,
            child: None,
            tick: 0,
        }
    }
}

impl Render for Content {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let top_layer = self.top_layer;
        let onto_overlay = self.onto_overlay;
        let clip_overlay = self.clip_overlay;
        div()
            .size_full()
            .relative()
            .child(div().size(px(20.)).bg(crate::red()))
            .child(crate::deferred(
                crate::anchored().child(div().size(px(8.)).bg(crate::blue())),
            ))
            .children(self.native.clone().map(|surface| {
                native_surface(surface)
                    .absolute()
                    .left(px(self.native_left))
                    .top(px(30.))
                    .w(px(50.))
                    .h(px(40.))
            }))
            .children(
                self.child.clone().map(|child| {
                    child.cached(crate::StyleRefinement::default().w(px(30.)).h(px(30.)))
                }),
            )
            .child(format!("tick {}", self.tick))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        if top_layer {
                            window.paint_top_layer(bounds, |window| {
                                window.paint_quad(crate::fill(bounds, crate::green()))
                            });
                        }
                        let paint = |window: &mut Window| {
                            if let Some(surface) = onto_overlay {
                                window
                                    .with_composition_surface(surface, |window| {
                                        window.paint_quad(crate::fill(bounds, crate::yellow()))
                                    })
                                    .unwrap();
                            }
                        };
                        if clip_overlay {
                            window.paint_layer(bounds, paint);
                        } else {
                            paint(window);
                        }
                    },
                )
                .size(px(16.)),
            )
    }
}

/// A child view that paints part of itself onto the overlay, and may place
/// a native surface.
struct Child {
    overlay: CompositionSurfaceId,
    native: Option<crate::WindowCompositionSurface>,
}

impl Render for Child {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let overlay = self.overlay;
        div()
            .size(px(30.))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        window
                            .with_composition_surface(overlay, |window| {
                                window.paint_quad(crate::fill(bounds, crate::white()))
                            })
                            .unwrap();
                    },
                )
                .size(px(10.)),
            )
            .children(
                self.native
                    .clone()
                    .map(|surface| native_surface(surface).size(px(10.))),
            )
    }
}

fn open(mode: FastMode) -> (TestAppContext, WindowHandle<Content>) {
    let mut cx = oracle_app(0);
    let window = cx.add_window(|_, _| Content::new());
    cx.simulate_window_resize(window.into(), size(px(300.), px(200.)));
    cx.set_draw_on_flush(false);
    cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
        .unwrap();
    (cx, window)
}

fn enable(
    cx: &mut TestAppContext,
    window: WindowHandle<Content>,
) -> (CompositionSurfaceId, CompositionSurfaceId) {
    cx.update_window(window.into(), |_, window, _| {
        let composition = window.enable_window_composition().unwrap();
        (composition.base_surface(), composition.overlay_surface())
    })
    .unwrap()
}

/// Draws and presents a frame, returning what each surface presented.
fn present(
    cx: &mut TestAppContext,
    window: WindowHandle<Content>,
) -> Vec<(CompositionSurfaceId, Vec<String>)> {
    cx.update_window(window.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.present();
    })
    .unwrap();
    recorder(cx, window)
        .state()
        .presented
        .last()
        .cloned()
        .unwrap_or_default()
}

fn layer(
    layers: &[(CompositionSurfaceId, Vec<String>)],
    surface: CompositionSurfaceId,
) -> Vec<String> {
    layers
        .iter()
        .find(|(id, _)| *id == surface)
        .map(|(_, lines)| lines.clone())
        .unwrap_or_default()
}

/// How many primitives of `lines` are filled with `color`, as red, green
/// and blue rounded to tenths.
fn colored(lines: &[String], color: (f32, f32, f32)) -> usize {
    let round = |value: f32| (value * 10.).round() as i32;
    lines
        .iter()
        .filter(|line| {
            let Some(start) = line.find("solid: PaintColor { ") else {
                return false;
            };
            let rest = &line[start + "solid: PaintColor { ".len()..];
            let values: Vec<f32> = rest
                .split('}')
                .next()
                .unwrap_or("")
                .split(", ")
                .filter_map(|part| part.split(": ").nth(1)?.trim().parse().ok())
                .collect();
            values.len() == 4
                && values[3] > 0.
                && (round(values[0]), round(values[1]), round(values[2]))
                    == (round(color.0), round(color.1), round(color.2))
        })
        .count()
}

const RED: (f32, f32, f32) = (1., 0., 0.);
const BLUE: (f32, f32, f32) = (0., 0., 1.);
const GREEN: (f32, f32, f32) = (0., 0.5, 0.);
const YELLOW: (f32, f32, f32) = (1., 1., 0.);
const WHITE: (f32, f32, f32) = (1., 1., 1.);

/// The controller: enabling is idempotent, a native surface is created
/// below the overlay, the base surface cannot be moved or removed, and a
/// failed surface leaves the tree as it was.
#[test]
fn the_controller_changes_the_tree_transactionally() {
    let (mut cx, window) = open(FastMode::OFF);
    let (base, overlay) = enable(&mut cx, window);
    let again = enable(&mut cx, window);
    assert_eq!(again, (base, overlay));
    cx.update_window(window.into(), |_, window, _| {
        let composition = window.enable_window_composition().unwrap();
        let native = composition.create_native_surface().unwrap();
        assert_eq!(
            composition.children(None).unwrap(),
            [base, native.id(), overlay]
        );
        assert!(composition.place_above(base, overlay).is_err());
        assert!(composition.place_below(native.id(), base).is_err());
        assert!(composition.remove_surface(base).is_err());
        assert!(composition.remove_surface(overlay).is_err());
        let gpui = composition.create_gpui_surface(None).unwrap();
        composition.place_below(gpui.id(), overlay).unwrap();
        assert_eq!(
            composition.children(None).unwrap(),
            [base, native.id(), gpui.id(), overlay]
        );
        composition.remove_surface(gpui.id()).unwrap();
        assert_eq!(
            composition.children(None).unwrap(),
            [base, native.id(), overlay]
        );
    })
    .unwrap();
    recorder(&cx, window).state().fail_next_surface = true;
    cx.update_window(window.into(), |_, window, _| {
        let composition = window.enable_window_composition().unwrap();
        assert!(composition.create_native_surface().is_err());
        assert_eq!(composition.children(None).unwrap().len(), 3);
    })
    .unwrap();
}

/// A platform without composition refuses it, and the window goes on
/// presenting its one scene.
#[test]
fn an_unsupported_platform_refuses_composition() {
    let (mut cx, window) = open(FastMode::OFF);
    recorder(&cx, window).state().unsupported = true;
    let refused = cx
        .update_window(window.into(), |_, window, _| {
            window.enable_window_composition().is_err()
        })
        .unwrap();
    assert!(refused);
    assert!(present(&mut cx, window).is_empty());
    assert!(recorder(&cx, window).state().presented.is_empty());
}

/// With composition off, the frame records no surface switch and is
/// presented whole, as before composition existed.
#[test]
fn the_path_without_composition_is_unchanged() {
    let (mut cx, window) = open(VIEWS);
    for _ in 0..2 {
        present(&mut cx, window);
    }
    cx.update_window(window.into(), |_, window, _| {
        assert!(window.rendered_frame.fast_composition_starts.is_empty());
    })
    .unwrap();
    assert!(recorder(&cx, window).state().presented.is_empty());
}

/// The base surface shows the root view; the overlay shows what is deferred.
#[test]
fn the_overlay_takes_deferred_draws() {
    let (mut cx, window) = open(FastMode::OFF);
    let (base, overlay) = enable(&mut cx, window);
    let layers = present(&mut cx, window);
    assert_eq!(colored(&layer(&layers, base), RED), 1, "{layers:?}");
    assert_eq!(colored(&layer(&layers, base), BLUE), 0, "{layers:?}");
    assert_eq!(colored(&layer(&layers, overlay), BLUE), 1, "{layers:?}");
}

/// What the root view paints in the top-layer band goes to the overlay.
#[test]
fn the_overlay_takes_the_top_layer_band() {
    let (mut cx, window) = open(FastMode::OFF);
    let (base, overlay) = enable(&mut cx, window);
    window
        .update(&mut cx, |content, _, cx| {
            content.top_layer = true;
            cx.notify();
        })
        .unwrap();
    let layers = present(&mut cx, window);
    assert_eq!(colored(&layer(&layers, base), GREEN), 0, "{layers:?}");
    assert_eq!(colored(&layer(&layers, overlay), GREEN), 1, "{layers:?}");
}

/// A switch to the overlay inside a clip layer replays the layer around
/// both stretches, balanced.
#[test]
fn clips_replay_balanced_across_surfaces() {
    let (mut cx, window) = open(FastMode::OFF);
    let (base, overlay) = enable(&mut cx, window);
    window
        .update(&mut cx, move |content, _, cx| {
            content.onto_overlay = Some(overlay);
            content.clip_overlay = true;
            cx.notify();
        })
        .unwrap();
    let layers = present(&mut cx, window);
    assert_eq!(colored(&layer(&layers, overlay), YELLOW), 1, "{layers:?}");
    assert_eq!(colored(&layer(&layers, base), YELLOW), 0, "{layers:?}");
    assert_eq!(colored(&layer(&layers, base), RED), 1, "{layers:?}");
}

/// A view drawn from last frame carries the switches it made, so what it
/// painted onto the overlay stays there: replayed as a cached view with fast
/// mode off, reused with view retention, copied around a splice.
#[test]
fn views_drawn_from_last_frame_keep_their_surface_switches() {
    for mode in [FastMode::OFF, VIEWS, SPLICE] {
        let (mut cx, window) = open(mode);
        let (base, overlay) = enable(&mut cx, window);
        window
            .update(&mut cx, move |content, _, cx| {
                content.child = Some(cx.new(|_| Child {
                    overlay,
                    native: None,
                }));
                cx.notify();
            })
            .unwrap();
        present(&mut cx, window);
        for step in 0..3 {
            window
                .update(&mut cx, |content, _, cx| {
                    content.tick += 1;
                    cx.notify();
                })
                .unwrap();
            let layers = present(&mut cx, window);
            assert_eq!(
                colored(&layer(&layers, overlay), WHITE),
                1,
                "{mode:?} step {step}: {layers:?}"
            );
            assert_eq!(
                colored(&layer(&layers, base), WHITE),
                0,
                "{mode:?} step {step}"
            );
        }
        let stats = cx
            .update_window(window.into(), |_, window, _| window.fast_frame_stats())
            .unwrap();
        assert!(
            stats.cached_views_replayed + stats.views_reused + stats.views_spliced > 0,
            "{mode:?}: the child was drawn from last frame: {stats:?}"
        );
    }
}

/// A native surface placed by an element gets its bounds in device pixels
/// once, again only when they change, and the platform is handed the order
/// again only then.
#[test]
fn native_surface_bounds_are_pushed_only_on_change() {
    let (mut cx, window) = open(VIEWS);
    enable(&mut cx, window);
    let surface = cx
        .update_window(window.into(), |_, window, _| {
            window
                .enable_window_composition()
                .unwrap()
                .create_native_surface()
                .unwrap()
        })
        .unwrap();
    window
        .update(&mut cx, move |content, _, cx| {
            content.native = Some(surface);
            cx.notify();
        })
        .unwrap();
    present(&mut cx, window);
    let native = recorder(&cx, window).state().native_surfaces[0].clone();
    assert_eq!(native.bounds_writes.get(), 1);
    assert_eq!(
        native.bounds.get(),
        Bounds::new(
            point(DevicePixels(20), DevicePixels(60)),
            size(DevicePixels(100), DevicePixels(80))
        )
    );
    let orders = recorder(&cx, window).state().orders.len();
    for _ in 0..3 {
        window
            .update(&mut cx, |content, _, cx| {
                content.tick += 1;
                cx.notify();
            })
            .unwrap();
        present(&mut cx, window);
    }
    assert_eq!(native.bounds_writes.get(), 1);
    assert_eq!(recorder(&cx, window).state().orders.len(), orders);
    window
        .update(&mut cx, |content, _, cx| {
            content.native_left = 40.;
            cx.notify();
        })
        .unwrap();
    present(&mut cx, window);
    assert_eq!(native.bounds_writes.get(), 2);
    assert_eq!(native.bounds.get().origin.x, DevicePixels(80));
    assert_eq!(recorder(&cx, window).state().orders.len(), orders + 1);
}

/// The overlay takes pointer input only where it drew: the deferred box and
/// the top-layer band, not the rest of the window, and no shadow.
#[test]
fn the_overlay_takes_input_only_where_it_draws() {
    let (mut cx, window) = open(FastMode::OFF);
    let (_, overlay) = enable(&mut cx, window);
    window
        .update(&mut cx, |content, _, cx| {
            content.top_layer = true;
            cx.notify();
        })
        .unwrap();
    present(&mut cx, window);
    let scale = cx
        .update_window(window.into(), |_, window, _| window.scale_factor())
        .unwrap();
    let regions = recorder(&cx, window)
        .state()
        .input_regions
        .last()
        .cloned()
        .unwrap();
    let region = regions
        .iter()
        .find(|(surface, _)| *surface == overlay)
        .map(|(_, region)| region.clone())
        .unwrap();
    assert!(!region.is_empty(), "{regions:?}");
    // The 8px deferred box and the 16px top-layer canvas, in device pixels.
    let area: f32 = region
        .iter()
        .map(|bounds| bounds.size.width.0 * bounds.size.height.0)
        .sum();
    let expected = (8. * scale).powi(2) + (16. * scale).powi(2);
    assert!(area <= expected + 1., "{area} > {expected}: {region:?}");
    let corner = point(
        crate::ScaledPixels(290. * scale),
        crate::ScaledPixels(190. * scale),
    );
    assert!(
        region.iter().all(|bounds| !bounds.contains(&corner)),
        "{region:?}"
    );
}

/// A native surface placed in a view drawn from last frame stays shown, is
/// hidden in the first frame that places it nowhere, and is shown again
/// when placed again: replayed as a cached view with fast mode off, reused
/// with view retention, copied around a splice.
#[test]
fn native_surfaces_are_shown_while_their_element_is_drawn() {
    for mode in [FastMode::OFF, VIEWS, SPLICE] {
        let (mut cx, window) = open(mode);
        let (_, overlay) = enable(&mut cx, window);
        let surface = cx
            .update_window(window.into(), |_, window, _| {
                window
                    .enable_window_composition()
                    .unwrap()
                    .create_native_surface()
                    .unwrap()
            })
            .unwrap();
        let placed = surface.clone();
        window
            .update(&mut cx, move |content, _, cx| {
                content.child = Some(cx.new(|_| Child {
                    overlay,
                    native: Some(placed),
                }));
                cx.notify();
            })
            .unwrap();
        present(&mut cx, window);
        let native = recorder(&cx, window).state().native_surfaces[0].clone();
        for step in 0..3 {
            window
                .update(&mut cx, |content, _, cx| {
                    content.tick += 1;
                    cx.notify();
                })
                .unwrap();
            present(&mut cx, window);
            assert!(
                native.visibility_changes.borrow().is_empty(),
                "{mode:?} step {step}: {:?}",
                native.visibility_changes.borrow()
            );
        }
        let stats = cx
            .update_window(window.into(), |_, window, _| window.fast_frame_stats())
            .unwrap();
        assert!(
            stats.cached_views_replayed + stats.views_reused + stats.views_spliced > 0,
            "{mode:?}: the child was drawn from last frame: {stats:?}"
        );

        window
            .update(&mut cx, |content, _, cx| {
                content.child = None;
                cx.notify();
            })
            .unwrap();
        present(&mut cx, window);
        assert_eq!(*native.visibility_changes.borrow(), [false], "{mode:?}");
        present(&mut cx, window);
        assert_eq!(*native.visibility_changes.borrow(), [false], "{mode:?}");

        window
            .update(&mut cx, move |content, _, cx| {
                content.child = Some(cx.new(|_| Child {
                    overlay,
                    native: Some(surface),
                }));
                cx.notify();
            })
            .unwrap();
        present(&mut cx, window);
        assert_eq!(
            *native.visibility_changes.borrow(),
            [false, true],
            "{mode:?}"
        );
    }
}
