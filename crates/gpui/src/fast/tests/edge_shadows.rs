//! Shadows whose box has left its clip (`fast/edge_shadows.rs`): which a
//! scene keeps, where it stacks them, and that it orders everything else as
//! it did.

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use crate::{
    Bounds, ContentMask, DrawOrder, Hsla, LayerFrame, LayerKey, Quad, ScaledPixels, Scene, Shadow,
    TileCoord, TransformationMatrix,
    fast::{
        edge_shadows::order_of,
        layers::scene::LayerContent,
        tests::layer_support::{bg, paint_color},
    },
    hsla, point,
    scene::{POSITIONED_DRAW_ORDER_BASE, PaintOperation, Primitive},
    size,
};

type RTree = crate::bounds_tree::BoundsTree<ScaledPixels>;

fn sp(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(width), ScaledPixels(height)),
    }
}

/// The window, and a viewport in it that clips what scrolls.
const WINDOW: (f32, f32, f32, f32) = (0., 0., 400., 300.);
const VIEWPORT: (f32, f32, f32, f32) = (20., 20., 360., 200.);

fn window() -> Bounds<ScaledPixels> {
    sp(WINDOW.0, WINDOW.1, WINDOW.2, WINDOW.3)
}

fn viewport() -> Bounds<ScaledPixels> {
    sp(VIEWPORT.0, VIEWPORT.1, VIEWPORT.2, VIEWPORT.3)
}

fn quad(bounds: Bounds<ScaledPixels>, mask: Bounds<ScaledPixels>) -> Primitive {
    Quad {
        bounds,
        content_mask: ContentMask::rectangular(mask),
        background: bg(hsla(0.6, 0.5, 0.5, 1.)),
        ..Default::default()
    }
    .into()
}

fn shadow(bounds: Bounds<ScaledPixels>, mask: Bounds<ScaledPixels>, blur: f32) -> Primitive {
    Shadow {
        transformation: TransformationMatrix::unit(),
        order: 0,
        blur_radius: ScaledPixels(blur),
        bounds,
        corner_radii: Default::default(),
        content_mask: ContentMask::rectangular(mask),
        color: paint_color(Hsla::black()),
        element_bounds: bounds,
        element_corner_radii: Default::default(),
        inset: 0,
        pad: 0,
    }
    .into()
}

fn scene_of(primitives: &[Primitive]) -> Scene {
    let mut scene = Scene::default();
    for primitive in primitives {
        scene.insert_primitive(primitive.clone());
    }
    scene
}

/// The order the scene gave each of `primitives`, in turn; `None` for one it
/// dropped.
fn orders(scene: &Scene, primitives: &[Primitive]) -> Vec<Option<DrawOrder>> {
    let mut kept = scene.paint_operations.iter().filter_map(|operation| {
        let PaintOperation::Primitive(primitive) = operation else {
            return None;
        };
        Some(primitive)
    });
    let mut next = kept.next();
    let orders = primitives
        .iter()
        .map(|primitive| {
            let same = next.is_some_and(|kept| {
                kept.bounds() == primitive.bounds()
                    && kept.content_mask().bounds == primitive.content_mask().bounds
                    && std::mem::discriminant(kept) == std::mem::discriminant(primitive)
            });
            if !same {
                return None;
            }
            let order = order_of(next.unwrap());
            next = kept.next();
            Some(order)
        })
        .collect();
    assert!(
        next.is_none(),
        "the scene holds a primitive it was not given"
    );
    orders
}

/// The order upstream's rule gives each of `primitives`: one past the
/// highest among the earlier ones whose bounds, clipped to their masks,
/// intersect its own; none for one whose clipped bounds are empty.
fn upstream_orders(primitives: &[Primitive]) -> Vec<Option<DrawOrder>> {
    let mut tree = RTree::default();
    primitives
        .iter()
        .map(|primitive| {
            let clipped = primitive
                .bounds()
                .intersect(&primitive.content_mask().bounds);
            (!clipped.is_empty()).then(|| tree.insert(clipped))
        })
        .collect()
}

/// `primitives` as they are with the viewport's clip not there: every mask
/// that is the viewport's becomes the window's, grown to hold what scrolls.
fn unclipped(primitives: &[Primitive]) -> Vec<Primitive> {
    let everywhere = sp(-10_000., -10_000., 20_000., 20_000.);
    primitives
        .iter()
        .cloned()
        .map(|mut primitive| {
            let mask = match &mut primitive {
                Primitive::Quad(quad) => &mut quad.content_mask,
                Primitive::Shadow(shadow) => &mut shadow.content_mask,
                _ => unreachable!(),
            };
            if mask.bounds == viewport() {
                mask.bounds = everywhere;
            }
            primitive
        })
        .collect()
}

/// The content a viewport scrolls over, as one background.
fn content_background() -> Primitive {
    quad(sp(20., -500., 360., 2000.), viewport())
}

#[test]
fn a_shadow_is_kept_while_what_it_draws_reaches_into_its_clip() {
    // Its box 5 px below the viewport's bottom edge, at 220.
    let below = sp(40., 225., 320., 60.);
    for (blur, kept) in [(4., true), (2., true), (1.5, false), (0., false)] {
        let primitives = [content_background(), shadow(below, viewport(), blur)];
        let scene = scene_of(&primitives);
        assert_eq!(scene.shadows.len(), kept as usize, "blur {blur}");
        assert_eq!(scene.fast_edge_shadows.len(), kept as usize, "blur {blur}");
        assert_eq!(
            orders(&scene, &primitives)[1].is_some(),
            kept,
            "blur {blur}"
        );
        if kept {
            // As it was handed over: drawn clipped to its own mask.
            let drawn = &scene.shadows[0];
            assert_eq!(drawn.bounds, below);
            assert_eq!(drawn.content_mask.bounds, viewport());
        }
    }
    // Above, left and right of the viewport too.
    for bounds in [
        sp(40., -45., 320., 60.),
        sp(-45., 60., 60., 60.),
        sp(385., 60., 60., 60.),
    ] {
        let scene = scene_of(&[content_background(), shadow(bounds, viewport(), 4.)]);
        assert_eq!(scene.fast_edge_shadows.len(), 1, "{bounds:?}");
    }
    // A shadow whose box shows is none of these.
    let scene = scene_of(&[
        content_background(),
        shadow(sp(40., 215., 320., 60.), viewport(), 4.),
    ]);
    assert_eq!((scene.shadows.len(), scene.fast_edge_shadows.len()), (1, 0));
}

#[test]
fn a_kept_shadow_lies_over_what_its_box_lies_on() {
    // A row whose top 10 px show, and the shadow of a card in it, whose box
    // lies below the viewport; a neighbour above, which the blur reaches.
    let primitives = [
        content_background(),
        quad(sp(40., 150., 320., 50.), viewport()),
        quad(sp(20., 210., 360., 100.), viewport()),
        shadow(sp(40., 225., 320., 60.), viewport(), 8.),
    ];
    let scene = scene_of(&primitives);
    let orders = orders(&scene, &primitives);
    assert_eq!(scene.fast_edge_shadows.len(), 1);
    // Over the background and the row, which lie under its box; not over
    // the neighbour, which is as high as the row.
    assert_eq!(orders, [Some(1), Some(2), Some(2), Some(3)]);
    // As the scene orders them with the clip not there.
    let whole = unclipped(&primitives);
    assert_eq!(orders, self::orders(&scene_of(&whole), &whole));

    // With the row out of view too, the shadow lies on the background
    // alone, level with the neighbour: drawn before it, as shadows are
    // before quads of their order.
    let primitives = [
        content_background(),
        quad(sp(40., 150., 320., 50.), viewport()),
        quad(sp(20., 222., 360., 100.), viewport()),
        shadow(sp(40., 225., 320., 60.), viewport(), 8.),
    ];
    let scene = scene_of(&primitives);
    assert_eq!(
        self::orders(&scene, &primitives),
        [Some(1), Some(2), None, Some(2)]
    );
}

#[test]
fn the_element_casting_a_kept_shadow_stays_over_it() {
    // A card whose top 4 px show at the viewport's bottom edge, under two
    // shadows cast downwards: the box of the first lies below the edge, the
    // second's shows by a pixel.
    let card = sp(40., 216., 320., 60.);
    let primitives = [
        content_background(),
        shadow(sp(43., 226., 314., 57.), viewport(), 15.),
        shadow(sp(44., 219., 312., 53.), viewport(), 6.),
        quad(card, viewport()),
    ];
    let scene = scene_of(&primitives);
    assert_eq!(scene.fast_edge_shadows.len(), 1);
    let orders = orders(&scene, &primitives);
    // The second shadow over the first, the card over both: without the
    // first in the scene's tree both would be level with it.
    assert_eq!(orders, [Some(1), Some(2), Some(3), Some(4)]);
    let whole = unclipped(&primitives);
    assert_eq!(orders, self::orders(&scene_of(&whole), &whole));
    // What lies over the card is over it still.
    let mut more = primitives.to_vec();
    more.push(quad(sp(50., 217., 40., 10.), viewport()));
    assert_eq!(self::orders(&scene_of(&more), &more)[4], Some(5));
}

#[test]
fn a_neighbour_a_kept_shadow_only_reaches_is_not_ordered_by_it() {
    let neighbour = quad(sp(40., 150., 320., 60.), viewport());
    let label = quad(sp(60., 160., 80., 20.), viewport());
    let kept = shadow(sp(40., 222., 320., 60.), viewport(), 16.);
    // The neighbour painted before the shadow, and after it.
    for primitives in [
        vec![
            content_background(),
            neighbour.clone(),
            label.clone(),
            kept.clone(),
        ],
        vec![content_background(), kept, neighbour, label],
    ] {
        let scene = scene_of(&primitives);
        assert_eq!(scene.fast_edge_shadows.len(), 1);
        let orders = orders(&scene, &primitives);
        let upstream = upstream_orders(&primitives);
        for (index, primitive) in primitives.iter().enumerate() {
            match primitive {
                // Level with the neighbour its blur falls on, under it.
                Primitive::Shadow(_) => assert_eq!(orders[index], Some(2)),
                _ => assert_eq!(orders[index], upstream[index], "primitive {index}"),
            }
        }
    }
}

#[test]
fn what_is_painted_outside_a_clip_lies_under_or_over_the_blur_that_shows() {
    let kept = shadow(sp(40., 225., 320., 60.), viewport(), 8.);
    // The window's background and the viewport's own, painted outside the
    // viewport's clip, lie behind it; a footer below the viewport, under
    // the shadow's box in the window, does not.
    let behind = [
        quad(window(), window()),
        quad(sp(0., 230., 400., 70.), window()),
        quad(sp(0., 240., 400., 20.), window()),
        quad(viewport(), window()),
    ];
    let mut primitives = behind.to_vec();
    primitives.push(kept);
    let scene = scene_of(&primitives);
    assert_eq!(
        orders(&scene, &primitives),
        [Some(1), Some(2), Some(3), Some(2), Some(3)]
    );

    // An overlay painted afterwards over the viewport's bottom edge lies
    // over the blur; one over its top edge is ordered as it was.
    let over = quad(sp(100., 200., 200., 40.), window());
    let elsewhere = quad(sp(100., 10., 200., 40.), window());
    primitives.extend([over, elsewhere]);
    let scene = scene_of(&primitives);
    let orders = orders(&scene, &primitives);
    let upstream = upstream_orders(&primitives);
    // Level with the shadow by what it lies on, and raised past it.
    assert_eq!(upstream[5], Some(3));
    assert_eq!(orders[5], Some(4));
    assert_eq!(orders[6], upstream[6]);
    // The overlay would come out level with the shadow over the viewport's
    // background alone.
    let primitives = [
        quad(viewport(), window()),
        shadow(sp(40., 225., 320., 60.), viewport(), 8.),
        quad(sp(100., 200., 200., 15.), window()),
    ];
    let scene = scene_of(&primitives);
    assert_eq!(upstream_orders(&primitives)[2], Some(2));
    assert_eq!(
        self::orders(&scene, &primitives),
        [Some(1), Some(2), Some(3)]
    );
}

#[test]
fn a_kept_shadow_in_a_layer_takes_its_band() {
    // In a positioned layer: over what it lies on there, whatever normal
    // content holds.
    let mut scene = Scene::default();
    scene.insert_primitive(content_background());
    scene.insert_primitive(quad(sp(20., 200., 360., 200.), viewport()));
    scene.push_positioned_layer(viewport());
    scene.insert_primitive(quad(sp(20., 100., 360., 400.), viewport()));
    scene.insert_primitive(shadow(sp(40., 225., 320., 60.), viewport(), 8.));
    scene.insert_primitive(quad(sp(40., 218., 320., 60.), viewport()));
    scene.pop_layer();
    assert_eq!(scene.fast_edge_shadows.len(), 1);
    assert_eq!(scene.shadows[0].order, POSITIONED_DRAW_ORDER_BASE + 2);
    assert_eq!(scene.quads[3].order, POSITIONED_DRAW_ORDER_BASE + 3);

    // Normal content painted afterwards over the shadow's box stays normal
    // content, and a shadow of its own is ordered among it, not over the
    // positioned layer.
    scene.insert_primitive(quad(sp(40., 210., 320., 60.), viewport()));
    scene.insert_primitive(shadow(sp(40., 228., 320., 60.), viewport(), 8.));
    assert_eq!(scene.fast_edge_shadows.len(), 2);
    assert_eq!(scene.quads[4].order, 3);
    assert_eq!(scene.shadows[1].order, 4);

    // In a layer of one order, at that order, as all it holds.
    let mut scene = Scene::default();
    scene.insert_primitive(content_background());
    scene.push_layer(viewport());
    scene.insert_primitive(quad(sp(20., 100., 360., 400.), viewport()));
    scene.insert_primitive(shadow(sp(40., 225., 320., 60.), viewport(), 8.));
    scene.insert_primitive(quad(sp(40., 218., 320., 60.), viewport()));
    scene.pop_layer();
    assert_eq!(scene.fast_edge_shadows.len(), 1);
    let layer = scene.quads[1].order;
    assert_eq!(layer, 2);
    assert_eq!(scene.shadows[0].order, layer);
    assert_eq!(scene.quads[2].order, layer);
}

#[test]
fn an_inset_shadow_is_kept_where_its_element_shows() {
    // An element whose top 6 px show; its inset shadow's inner box, 10 px
    // in, lies below the viewport.
    let element = sp(40., 214., 320., 60.);
    let mut inset = Shadow {
        transformation: TransformationMatrix::unit(),
        order: 0,
        blur_radius: ScaledPixels(4.),
        bounds: sp(50., 224., 300., 40.),
        corner_radii: Default::default(),
        content_mask: ContentMask::rectangular(viewport()),
        color: paint_color(Hsla::black()),
        element_bounds: element,
        element_corner_radii: Default::default(),
        inset: 1,
        pad: 0,
    };
    let primitives = [
        content_background(),
        quad(element, viewport()),
        inset.into(),
    ];
    let scene = scene_of(&primitives);
    assert_eq!(scene.fast_edge_shadows.len(), 1);
    // Over its element, which lies under its box.
    assert_eq!(orders(&scene, &primitives), [Some(1), Some(2), Some(3)]);
    // One whose element is out of view draws nothing.
    inset.element_bounds = sp(40., 222., 320., 60.);
    inset.bounds = sp(50., 232., 300., 40.);
    let scene = scene_of(&[content_background(), inset.into()]);
    assert_eq!(scene.shadows.len(), 0);
}

/// A scene of quads and shadows scattered in and around the viewport, in
/// the viewport's clip, a card's inside it, and the window's.
fn random_scene(rng: &mut StdRng, blur: f32) -> Vec<Primitive> {
    let masks = [viewport(), sp(60., 120., 200., 110.), window()];
    let count = rng.random_range(5..60);
    let mut primitives = vec![quad(window(), window()), content_background()];
    for _ in 0..count {
        let mask = masks[rng.random_range(0..masks.len())];
        let bounds = sp(
            rng.random_range(-60.0..400.0f32).round(),
            rng.random_range(-80.0..320.0f32).round(),
            rng.random_range(4.0..220.0f32).round(),
            rng.random_range(4.0..90.0f32).round(),
        );
        primitives.push(if rng.random_range(0..3) == 0 {
            shadow(bounds, mask, rng.random_range(0.0..blur))
        } else {
            quad(bounds, mask)
        });
    }
    primitives
}

/// A scene that keeps no shadow for its blur is the scene upstream's rule
/// builds; one that keeps some drops nothing upstream keeps, lowers no
/// order, and leaves any two primitives whose clipped bounds intersect in
/// paint order.
#[test]
fn everything_else_is_ordered_as_before() {
    let mut rng = StdRng::seed_from_u64(0x5eed_ed6e);
    let (mut without, mut with, mut raised) = (0, 0, 0);
    for run in 0..600 {
        // A third of the scenes have shadows without blur, which none is
        // kept for.
        let blur = if run % 3 == 0 { 0.001 } else { 24. };
        let primitives = random_scene(&mut rng, blur);
        let scene = scene_of(&primitives);
        let orders = orders(&scene, &primitives);
        let upstream = upstream_orders(&primitives);
        if scene.fast_edge_shadows.len() == 0 {
            without += 1;
            assert_eq!(orders, upstream, "run {run}");
            continue;
        }
        with += 1;
        let clipped = |index: usize| {
            let primitive = &primitives[index];
            primitive
                .bounds()
                .intersect(&primitive.content_mask().bounds)
        };
        for (index, (order, before)) in orders.iter().zip(&upstream).enumerate() {
            let Some(before) = before else {
                // Kept for its blur, or dropped as before.
                assert!(
                    order.is_none() || matches!(primitives[index], Primitive::Shadow(_)),
                    "run {run}, primitive {index}"
                );
                continue;
            };
            let order = order.expect("a primitive kept before is kept");
            assert!(order >= *before, "run {run}, primitive {index}");
            raised += (order > *before) as usize;
            for earlier in 0..index {
                if upstream[earlier].is_some() && clipped(earlier).intersects(&clipped(index)) {
                    assert!(
                        orders[earlier].unwrap() < order,
                        "run {run}: primitives {earlier} and {index}"
                    );
                }
            }
        }
    }
    assert!(without >= 150, "{without} scenes kept no shadow");
    assert!(with >= 200, "{with} scenes kept a shadow");
    assert!(
        raised >= 50,
        "{raised} primitives were raised over a shadow"
    );
}

/// A kept shadow is a paint operation: a scene replayed from another's
/// operations, as a view drawn from the last frame is, holds the same
/// primitives at the same orders.
#[test]
fn replaying_a_scene_keeps_its_shadows_and_their_orders() {
    let mut rng = StdRng::seed_from_u64(0x0ed6_e5ad);
    let mut kept = 0;
    for run in 0..200 {
        let primitives = random_scene(&mut rng, 24.);
        let scene = scene_of(&primitives);
        let mut replayed = Scene::default();
        replayed.replay(0..scene.paint_operations.len(), &scene);
        assert_eq!(
            orders(&replayed, &primitives),
            orders(&scene, &primitives),
            "run {run}"
        );
        assert_eq!(
            replayed.fast_edge_shadows.len(),
            scene.fast_edge_shadows.len()
        );
        kept += scene.fast_edge_shadows.len();
        // In two parts, the second after other content.
        let middle = scene.paint_operations.len() / 2;
        let mut parts = Scene::default();
        parts.replay(0..middle, &scene);
        parts.replay(middle..scene.paint_operations.len(), &scene);
        assert_eq!(
            orders(&parts, &primitives),
            orders(&scene, &primitives),
            "run {run}"
        );
    }
    assert!(kept >= 100, "{kept} shadows were kept");
}

/// The primitives of `scene` in the order they are drawn: by draw order,
/// shadows before quads, then as painted.
fn drawing_order(scene: &Scene) -> Vec<(Bounds<ScaledPixels>, bool)> {
    let mut drawn: Vec<(DrawOrder, bool, usize, Bounds<ScaledPixels>)> = scene
        .paint_operations
        .iter()
        .enumerate()
        .filter_map(|(index, operation)| match operation {
            PaintOperation::Primitive(primitive) => Some((
                order_of(primitive),
                !matches!(primitive, Primitive::Shadow(_)),
                index,
                primitive.bounds(),
            )),
            _ => None,
        })
        .collect();
    drawn.sort_by_key(|(order, quad, index, _)| (*order, *quad, *index));
    drawn
        .into_iter()
        .map(|(_, quad, _, bounds)| (bounds, quad))
        .collect()
}

/// A tile of a scroll layer draws a shadow whose box lies outside it, over
/// what it lies on, as the content was painted. A viewport that ends where
/// the tile does draws the same primitives in the same order.
#[test]
fn a_viewport_stacks_a_kept_shadow_as_a_tile_does() {
    let tile = sp(0., 0., 256., 256.);
    let painted = sp(0., -512., 256., 2048.);
    // Cards 70 px apart under two shadows each, in rows with backgrounds,
    // down the content: the tile's bottom edge cuts through the fourth.
    let content = |mask: Bounds<ScaledPixels>| {
        let mut primitives = vec![quad(sp(0., -512., 256., 2048.), mask)];
        for card in 0..8 {
            let top = 6. + 70. * card as f32;
            primitives.extend([
                quad(sp(0., top - 3., 256., 70.), mask),
                shadow(sp(15., top + 13., 226., 58.), mask, 15.),
                shadow(sp(16., top + 8., 224., 56.), mask, 6.),
                quad(sp(12., top, 232., 64.), mask),
                quad(sp(24., top + 12., 80., 14.), mask),
            ]);
        }
        primitives
    };
    for scroll in 0..70 {
        let moved = |primitives: Vec<Primitive>| -> Vec<Primitive> {
            let delta = point(ScaledPixels(0.), ScaledPixels(-(scroll as f32)));
            primitives
                .iter()
                .map(|primitive| {
                    let mut primitive =
                        crate::fast::layers::scene::translate_primitive(primitive, delta);
                    // The clip stays where it is.
                    match &mut primitive {
                        Primitive::Quad(quad) => quad.content_mask.bounds.origin.y -= delta.y,
                        Primitive::Shadow(shadow) => shadow.content_mask.bounds.origin.y -= delta.y,
                        _ => unreachable!(),
                    }
                    primitive
                })
                .collect()
        };
        let frame = LayerFrame {
            key: LayerKey(1),
            generation: 1,
            background: paint_color(Hsla::white()),
            tile_size: 256,
            content: LayerContent::from(scene_of(&moved(content(painted)))),
            dirty_tiles: Vec::new(),
        };
        let from_tile = drawing_order(&frame.tile_scene(TileCoord { x: 0, y: 0 }));
        let direct = scene_of(&moved(content(tile)));
        assert_eq!(
            drawing_order(&direct),
            from_tile,
            "scrolled by {scroll}: {} shadows kept",
            direct.fast_edge_shadows.len()
        );
    }
}
