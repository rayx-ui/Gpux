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

pub(super) fn sp(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(width), ScaledPixels(height)),
    }
}

/// The window, and a viewport in it that clips what scrolls.
const WINDOW: (f32, f32, f32, f32) = (0., 0., 400., 300.);
const VIEWPORT: (f32, f32, f32, f32) = (20., 20., 360., 200.);

pub(super) fn window() -> Bounds<ScaledPixels> {
    sp(WINDOW.0, WINDOW.1, WINDOW.2, WINDOW.3)
}

pub(super) fn viewport() -> Bounds<ScaledPixels> {
    sp(VIEWPORT.0, VIEWPORT.1, VIEWPORT.2, VIEWPORT.3)
}

pub(super) fn quad(bounds: Bounds<ScaledPixels>, mask: Bounds<ScaledPixels>) -> Primitive {
    Quad {
        bounds,
        content_mask: ContentMask::rectangular(mask),
        background: bg(hsla(0.6, 0.5, 0.5, 1.)),
        ..Default::default()
    }
    .into()
}

pub(super) fn shadow(
    bounds: Bounds<ScaledPixels>,
    mask: Bounds<ScaledPixels>,
    blur: f32,
) -> Primitive {
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
pub(super) fn orders(scene: &Scene, primitives: &[Primitive]) -> Vec<Option<DrawOrder>> {
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

    // With the row out of view too, the shadow still lies over it, and so
    // over the neighbour: the row the scene dropped keeps its place in the
    // order.
    let primitives = [
        content_background(),
        quad(sp(40., 150., 320., 50.), viewport()),
        quad(sp(20., 222., 360., 100.), viewport()),
        shadow(sp(40., 225., 320., 60.), viewport(), 8.),
    ];
    let scene = scene_of(&primitives);
    assert_eq!(
        self::orders(&scene, &primitives),
        [Some(1), Some(2), None, Some(3)]
    );
    // Rows straight on the background behind the viewport, painted outside
    // its clip: the dropped row lies over that background as the rows in
    // view do.
    let primitives = [
        quad(window(), window()),
        quad(viewport(), window()),
        quad(sp(20., 140., 360., 70.), viewport()),
        quad(sp(20., 222., 360., 100.), viewport()),
        shadow(sp(40., 225., 320., 60.), viewport(), 8.),
    ];
    let scene = scene_of(&primitives);
    assert_eq!(
        self::orders(&scene, &primitives),
        [Some(1), Some(2), Some(3), None, Some(4)]
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
fn what_is_painted_behind_a_clip_lies_under_the_shadows_kept_in_it() {
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

    // What is painted afterwards beside the shadow's box, over its blur or
    // elsewhere, is ordered as it was: a shadow orders what lies over its
    // box, kept or not, and a blur and a neighbour are drawn by their draw
    // orders.
    let over = quad(sp(100., 200., 200., 40.), window());
    let elsewhere = quad(sp(100., 10., 200., 40.), window());
    primitives.extend([over, elsewhere]);
    let scene = scene_of(&primitives);
    let orders = orders(&scene, &primitives);
    let upstream = upstream_orders(&primitives);
    assert_eq!(orders[5..], upstream[5..]);
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

    // A layer painted after a kept shadow, holding what lies over the
    // shadow's box (a line of text cut by the viewport's edge), is raised to
    // the shadow's order, and all it holds from there on with it.
    let mut scene = Scene::default();
    scene.insert_primitive(content_background());
    scene.insert_primitive(quad(sp(20., 210., 360., 200.), viewport()));
    scene.insert_primitive(shadow(sp(40., 225., 320., 60.), viewport(), 8.));
    assert_eq!(scene.shadows[0].order, 3);
    scene.push_layer(sp(20., 100., 360., 20.));
    scene.insert_primitive(quad(sp(300., 100., 30., 20.), viewport()));
    scene.insert_primitive(quad(sp(60., 100., 30., 200.), viewport()));
    scene.insert_primitive(quad(sp(100., 100., 30., 20.), viewport()));
    scene.pop_layer();
    let in_layer: Vec<_> = scene.quads[2..].iter().map(|quad| quad.order).collect();
    assert_eq!(in_layer, [2, 3, 3]);
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

/// A scene drops no primitive upstream's rule keeps and lowers none, and
/// any two whose clipped bounds intersect stay in paint order. A scene none
/// of whose primitives a mask cuts or drops is upstream's, order for order.
#[test]
fn nothing_is_dropped_lowered_or_put_out_of_order() {
    let mut rng = StdRng::seed_from_u64(0x5eed_ed6e);
    let (mut with, mut raised, mut as_before) = (0, 0, 0);
    for run in 0..600 {
        // A third of the scenes have shadows without blur, which none is
        // kept for.
        let blur = if run % 3 == 0 { 0.001 } else { 24. };
        let primitives = random_scene(&mut rng, blur);
        let scene = scene_of(&primitives);
        let orders = orders(&scene, &primitives);
        let upstream = upstream_orders(&primitives);
        with += (scene.fast_edge_shadows.len() > 0) as usize;
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
    assert!(with >= 200, "{with} scenes kept a shadow");
    assert!(raised >= 50, "{raised} primitives were raised");

    // Quads and shadows all inside the window, in its mask.
    for run in 0..200 {
        let primitives: Vec<Primitive> = (0..rng.random_range(5..80))
            .map(|_| {
                let bounds = sp(
                    rng.random_range(30.0..200.0f32).round(),
                    rng.random_range(30.0..150.0f32).round(),
                    rng.random_range(4.0..170.0f32).round(),
                    rng.random_range(4.0..120.0f32).round(),
                );
                if rng.random_range(0..3) == 0 {
                    shadow(bounds, window(), rng.random_range(0.0..24.0))
                } else {
                    quad(bounds, window())
                }
            })
            .collect();
        let scene = scene_of(&primitives);
        assert_eq!(
            orders(&scene, &primitives),
            upstream_orders(&primitives),
            "run {run}"
        );
        as_before += primitives.len();
    }
    assert!(
        as_before >= 5000,
        "{as_before} primitives were ordered as before"
    );
}

/// Content scattered in and around the viewport, all of it in the
/// viewport's clip.
fn random_content(rng: &mut StdRng) -> Vec<Primitive> {
    let count = rng.random_range(5..80);
    let mut primitives = vec![content_background()];
    for _ in 0..count {
        let bounds = sp(
            rng.random_range(-60.0..400.0f32).round(),
            rng.random_range(-300.0..520.0f32).round(),
            rng.random_range(4.0..220.0f32).round(),
            rng.random_range(4.0..140.0f32).round(),
        );
        primitives.push(if rng.random_range(0..3) == 0 {
            shadow(bounds, viewport(), rng.random_range(0.0..24.0))
        } else {
            quad(bounds, viewport())
        });
    }
    primitives
}

/// A clip changes no order: every primitive a scene keeps of content in one
/// clip has the order it has with the clip not there, wherever the clip
/// lies over the content. That is what lets a layer's tiles, painted once,
/// stand for the content at every scroll offset.
#[test]
fn content_is_ordered_as_with_its_clip_not_there() {
    let mut rng = StdRng::seed_from_u64(0x00c1_1bed);
    let (mut kept, mut shadows, mut raised) = (0, 0, 0);
    for run in 0..400 {
        let primitives = random_content(&mut rng);
        let whole = unclipped(&primitives);
        let expected = orders(&scene_of(&whole), &whole);
        assert!(expected.iter().all(Option::is_some));
        let scene = scene_of(&primitives);
        let orders = orders(&scene, &primitives);
        let upstream = upstream_orders(&primitives);
        for (index, order) in orders.iter().enumerate() {
            let Some(order) = order else {
                continue;
            };
            assert_eq!(
                Some(*order),
                expected[index],
                "run {run}, primitive {index} of {}",
                primitives.len()
            );
            kept += 1;
            raised += (upstream[index].is_some_and(|before| before < *order)) as usize;
        }
        shadows += scene.fast_edge_shadows.len();
    }
    assert!(kept >= 5000, "{kept} primitives were kept");
    assert!(shadows >= 300, "{shadows} shadows were kept for their blur");
    assert!(raised >= 300, "{raised} primitives were raised");
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

/// Rows with backgrounds of their own, each holding a card under a shadow
/// that reaches over the row above, in the clip `mask`.
fn shadowed_rows(mask: Bounds<ScaledPixels>) -> Vec<Vec<Primitive>> {
    (0..8)
        .map(|row| {
            let top = 74. * row as f32;
            vec![
                quad(sp(0., top, 256., 74.), mask),
                shadow(sp(12., top + 10., 232., 54.), mask, 16.),
                quad(sp(12., top + 10., 232., 54.), mask),
            ]
        })
        .collect()
}

/// A tile draws its primitives at the orders the content gave them as a
/// whole. The shadow of the first row a tile does not hold reaches into it:
/// among the primitives over that tile alone, nothing lies under the
/// shadow's box, which would put it level with the row above and under it.
#[test]
fn a_tile_draws_its_primitives_at_the_orders_of_the_content() {
    let painted = sp(0., -512., 256., 2048.);
    let rows = shadowed_rows(painted);
    // The fourth row begins at 222, 34 px above the first tile's bottom
    // edge; moved down by 40, it begins below it.
    let moved: Vec<Primitive> = rows
        .iter()
        .flatten()
        .map(|primitive| {
            crate::fast::layers::scene::translate_primitive(
                primitive,
                point(ScaledPixels(0.), ScaledPixels(40.)),
            )
        })
        .collect();
    let content = scene_of(&moved);
    let in_content = orders(&content, &moved);
    let frame = |content: LayerContent| LayerFrame {
        key: LayerKey(1),
        generation: 1,
        background: paint_color(Hsla::white()),
        tile_size: 256,
        content,
        dirty_tiles: Vec::new(),
    };
    let tile = frame(LayerContent::from(content)).tile_scene(TileCoord { x: 0, y: 0 });
    // The shadow of the fourth row's card, whose box begins at 272.
    let reaching = tile
        .shadows
        .iter()
        .find(|shadow| shadow.bounds.origin.y == ScaledPixels(272.))
        .expect("the tile draws the blur reaching into it");
    assert_eq!(Some(reaching.order), in_content[10]);
    // Over the third row's background, which it falls on.
    let above = tile
        .quads
        .iter()
        .find(|quad| quad.bounds == sp(0., 188., 256., 74.))
        .unwrap();
    assert!(reaching.order > above.order);
    for primitive in tile.shadows.iter().map(|shadow| Primitive::Shadow(*shadow)) {
        let index = moved
            .iter()
            .position(|other| other.bounds() == primitive.bounds())
            .unwrap();
        assert_eq!(Some(order_of(&primitive)), in_content[index]);
    }

    // A list's content is a scene a row: the rows are ordered as painted
    // one after another.
    let parts = rows.iter().map(|row| {
        crate::fast::layers::scene::LayerPart::new(None, std::rc::Rc::new(scene_of(row)))
    });
    let whole: Vec<Primitive> = rows.iter().flatten().cloned().collect();
    let in_whole = orders(&scene_of(&whole), &whole);
    let listed = frame(LayerContent::from_parts(parts));
    for coordinate in [TileCoord { x: 0, y: 0 }, TileCoord { x: 0, y: 1 }] {
        let tile = listed.tile_scene(coordinate);
        let origin = listed.tile_bounds(coordinate).origin;
        assert!(!tile.shadows.is_empty());
        for shadow in &tile.shadows {
            let index = whole
                .iter()
                .position(|other| {
                    matches!(other, Primitive::Shadow(_))
                        && other.bounds().origin.y == shadow.bounds.origin.y + origin.y
                })
                .unwrap();
            assert_eq!(Some(shadow.order), in_whole[index], "{coordinate:?}");
        }
    }
}

/// A table of clipped cells whose content runs past each: every cell's
/// mask has entries of its own, a short list each, and what was painted
/// behind a mask's edge is worked out for the first of them only.
#[test]
fn a_thousand_clips_cost_a_thousand_short_lists() {
    let mut scene = Scene::default();
    scene.insert_primitive(quad(window(), window()));
    for cell in 0..2000 {
        let (left, top) = ((cell % 40) as f32 * 10., (cell / 40) as f32 * 6.);
        let mask = sp(left, top, 9., 5.);
        // One primitive the cell cuts, and two it drops.
        scene.insert_primitive(quad(sp(left, top, 14., 5.), mask));
        scene.insert_primitive(quad(sp(left + 12., top, 6., 5.), mask));
        scene.insert_primitive(quad(sp(left + 20., top, 6., 5.), mask));
    }
    assert_eq!(scene.quads.len(), 2001);
    // Every cut quad lies over the window's background and nothing else.
    assert!(scene.quads[1..].iter().all(|quad| quad.order == 2));
}

/// What is painted, a clip pushed or popped, or a primitive in the clips
/// open then.
#[derive(Clone)]
pub(super) enum Item {
    Push(Bounds<ScaledPixels>),
    Pop,
    /// The content of a scrolling container begins, and ends.
    BeginScroll,
    EndScroll,
    Quad(Bounds<ScaledPixels>),
    Shadow(Bounds<ScaledPixels>, f32),
    /// A border of `width` round an element of the bounds, as `Style::paint` paints one with
    /// corners of `radius`: four quads of the bounds, each in one side's strip.
    Border(Bounds<ScaledPixels>, f32, f32),
}

/// Paints `items` as a window does, one large enough to hold them all:
/// each clip is told to the scene, and each primitive's mask is the clips
/// open around it, intersected. Returns the scene and the primitives as
/// they were handed to it.
pub(super) fn paint(items: &[Item]) -> (Scene, Vec<Primitive>) {
    paint_with(items, true)
}

/// The strips `Style::paint` paints a border's quads in (top, right, bottom, left) round an
/// element of `bounds`, for a border of `width` and corners of `radius`.
pub(super) fn border_strips(
    bounds: Bounds<ScaledPixels>,
    width: f32,
    radius: f32,
) -> [Bounds<ScaledPixels>; 4] {
    let at = |x: f32, y: f32| point(ScaledPixels(x), ScaledPixels(y));
    let (left, top, right, bottom) = (
        bounds.left().0,
        bounds.top().0,
        bounds.right().0,
        bounds.bottom().0,
    );
    let reach = width.max(radius);
    let top_strip = Bounds::from_corners(at(left, top), at(right, top + reach));
    let bottom_strip = Bounds::from_corners(at(left, bottom - reach), at(right, bottom));
    let left_strip = Bounds::from_corners(
        at(left, top_strip.bottom().0),
        at(bottom_strip.left().0 + width, bottom_strip.top().0),
    );
    let right_strip = Bounds::from_corners(
        at(top_strip.right().0 - width, top_strip.bottom().0),
        at(right, bottom_strip.top().0),
    );
    [top_strip, right_strip, bottom_strip, left_strip]
}

/// [`paint`], a border's four quads each under a clip of its own told to the scene as they
/// were before (`one_clip` false), or under the one clip a border is told as (`one_clip`).
pub(super) fn paint_with(items: &[Item], one_clip: bool) -> (Scene, Vec<Primitive>) {
    use crate::fast::edge_shadows::{ClipOp, clip_op};
    let mut scene = Scene::default();
    let mut masks = vec![sp(-10_000., -10_000., 20_000., 20_000.)];
    let mut primitives = Vec::new();
    for item in items {
        let mask = *masks.last().unwrap();
        let primitive = match item {
            Item::Push(clip) => {
                masks.push(mask.intersect(clip));
                clip_op(&mut scene, ClipOp::Push(*clip));
                continue;
            }
            Item::Pop => {
                masks.pop();
                clip_op(&mut scene, ClipOp::Pop);
                continue;
            }
            Item::BeginScroll => {
                clip_op(&mut scene, ClipOp::BeginScroll);
                continue;
            }
            Item::EndScroll => {
                clip_op(&mut scene, ClipOp::EndScroll);
                continue;
            }
            Item::Border(bounds, width, radius) => {
                let strips = border_strips(*bounds, *width, *radius);
                // As `fast::border_clips::paint` does: an element thinner than its border has
                // strips that reach past it, and each is a clip.
                let one_clip = one_clip
                    && strips.iter().all(|strip| {
                        strip.left() >= bounds.left()
                            && strip.top() >= bounds.top()
                            && strip.right() <= bounds.right()
                            && strip.bottom() <= bounds.bottom()
                    });
                if one_clip {
                    clip_op(
                        &mut scene,
                        ClipOp::Border {
                            bounds: *bounds,
                            strips,
                        },
                    );
                }
                for strip in strips {
                    if !one_clip {
                        clip_op(&mut scene, ClipOp::Push(strip));
                    }
                    let primitive = quad(*bounds, mask.intersect(&strip));
                    scene.insert_primitive(primitive.clone());
                    primitives.push(primitive);
                    if !one_clip {
                        clip_op(&mut scene, ClipOp::Pop);
                    }
                }
                if one_clip {
                    clip_op(&mut scene, ClipOp::Pop);
                }
                continue;
            }
            Item::Quad(bounds) => quad(*bounds, mask),
            Item::Shadow(bounds, blur) => shadow(*bounds, mask, *blur),
        };
        scene.insert_primitive(primitive.clone());
        primitives.push(primitive);
    }
    (scene, primitives)
}

/// The order of each primitive of `items`, painted, in turn.
pub(super) fn painted_orders(items: &[Item]) -> (Vec<Option<DrawOrder>>, usize) {
    let (scene, primitives) = paint(items);
    (orders(&scene, &primitives), scene.fast_edge_shadows.len())
}

/// `items` with the first clip pushed, the viewport's, not there: grown to
/// hold everything.
fn without_the_viewport(items: &[Item]) -> Vec<Item> {
    let mut items = items.to_vec();
    let first = items
        .iter_mut()
        .find(|item| matches!(item, Item::Push(_)))
        .unwrap();
    *first = Item::Push(sp(-10_000., -10_000., 20_000., 20_000.));
    items
}

/// A card that clips its content, cut by the viewport's bottom edge, holds
/// a child taller than itself. With the viewport not there the child
/// reaches as far as the card, not as far as its own bounds: it does not lie
/// under the shadow of the card below, whose box begins past the card.
#[test]
fn a_clip_inside_a_viewport_ends_where_it_ends() {
    let card = sp(40., 170., 320., 60.);
    let next = sp(40., 242., 320., 60.);
    let items = [
        Item::Quad(window()),
        Item::Push(viewport()),
        Item::Quad(sp(20., -500., 360., 2000.)),
        // The card, its shadow before it, and the child it clips.
        Item::Shadow(card, 14.),
        Item::Quad(card),
        Item::Push(card),
        Item::Quad(sp(48., 178., 304., 200.)),
        Item::Quad(sp(60., 190., 100., 180.)),
        Item::Pop,
        // The card below, out of view; its shadow reaches in.
        Item::Shadow(next, 14.),
        Item::Quad(next),
        Item::Pop,
    ];
    let (orders, kept) = painted_orders(&items);
    assert_eq!(kept, 1);
    // The window, the content, the card's shadow, the card, its children;
    // then the next card's shadow, over the content alone.
    assert_eq!(
        orders,
        [
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            Some(6),
            Some(3),
            None
        ]
    );
    let (whole, _) = painted_orders(&without_the_viewport(&items));
    for (order, whole) in orders.iter().zip(&whole) {
        assert!(order.is_none() || order == whole);
    }

    // A child that does reach under the next card's shadow, the card not
    // clipping it, lies under it.
    let mut unclipped = items.to_vec();
    unclipped.remove(8);
    unclipped.remove(5);
    let (orders, _) = painted_orders(&unclipped);
    assert_eq!(orders[6], Some(7));
}

/// A row's border is drawn in a mask of its own, the border's edge. A row
/// that reaches up over the row before lies over that row's border too,
/// in view or dropped: clips are told one by one, so what a border covers
/// with the viewport not there is known.
#[test]
fn what_is_painted_in_a_mask_of_its_own_is_among_its_clips_entries() {
    // Rows 70 px tall, each beginning 8 px above the end of the row before,
    // bordered below, each holding a shadowed card.
    let row = |index: usize| {
        let top = 62. * index as f32 - 40.;
        let border = sp(20., top + 68., 360., 2.);
        vec![
            Item::Quad(sp(20., top, 360., 70.)),
            Item::Push(border),
            Item::Quad(sp(20., top, 360., 70.)),
            Item::Pop,
            Item::Shadow(sp(40., top + 14., 320., 40.), 16.),
            Item::Quad(sp(40., top + 14., 320., 40.)),
        ]
    };
    let mut items = vec![Item::Quad(window()), Item::Push(viewport())];
    items.extend((0..7).flat_map(row));
    items.push(Item::Pop);
    let (orders, kept) = painted_orders(&items);
    let (whole, _) = painted_orders(&without_the_viewport(&items));
    assert!(kept >= 1, "a shadow below the viewport reaches into it");
    assert!(orders.iter().any(Option::is_none), "rows are out of view");
    for (index, (order, whole)) in orders.iter().zip(&whole).enumerate() {
        assert!(
            order.is_none() || order == whole,
            "primitive {index}: {order:?} against {whole:?}"
        );
    }
    // Each row lies over the border of the row before: the orders climb
    // down the rows.
    let backgrounds: Vec<_> = (0..7).map(|row| whole[1 + row * 4].unwrap()).collect();
    assert!(backgrounds.windows(2).all(|pair| pair[0] < pair[1]));
}

/// What was painted before a clip was pushed lies behind what the clip
/// holds, where it is: a row the clip drops, with nothing of the clip's
/// under it, lies over what is behind the stretch of the clip's edge it
/// would come into view at, as the rows in view lie over what is behind
/// them.
#[test]
fn a_dropped_primitive_lies_over_what_is_behind_its_stretch_of_the_edge() {
    let items = |left: f32| {
        vec![
            Item::Quad(window()),
            // A nest of backgrounds behind the viewport's right half.
            Item::Quad(sp(200., 20., 180., 200.)),
            Item::Quad(sp(206., 26., 168., 188.)),
            Item::Quad(sp(212., 32., 156., 176.)),
            Item::Push(viewport()),
            // A row in view and the next, out of view, with its card's
            // shadow reaching in.
            Item::Quad(sp(left, 150., 150., 60.)),
            Item::Quad(sp(left, 226., 150., 60.)),
            Item::Shadow(sp(left + 10., 230., 130., 50.), 12.),
            Item::Pop,
        ]
    };
    // On the left, over the window alone. On the right the row in view
    // lies over the whole nest, and the one below, when it comes into view
    // at the viewport's bottom edge, over the outermost of it, which alone
    // reaches there.
    let (left, kept) = painted_orders(&items(30.));
    assert_eq!(kept, 1);
    assert_eq!(left[4..], [Some(2), None, Some(3)]);
    let (right, _) = painted_orders(&items(220.));
    assert_eq!(right[4..], [Some(5), None, Some(4)]);
}

/// Content in a viewport, with cards that clip what they hold, scattered
/// in and around it. With `scrolling`, the viewport and some of the cards
/// are scrolling containers, as the window tells of them.
fn random_content_in_clips(rng: &mut StdRng, scrolling: bool) -> Vec<Item> {
    let mut items = vec![Item::Push(viewport())];
    if scrolling {
        items.push(Item::BeginScroll);
    }
    items.push(Item::Quad(sp(20., -500., 360., 2000.)));
    let scattered = |rng: &mut StdRng, around: Bounds<ScaledPixels>| {
        let bounds = sp(
            (around.left().0 + rng.random_range(-40.0..around.size.width.0)).round(),
            (around.top().0 + rng.random_range(-40.0..around.size.height.0)).round(),
            rng.random_range(4.0..160.0f32).round(),
            rng.random_range(4.0..160.0f32).round(),
        );
        if rng.random_range(0..3) == 0 {
            Item::Shadow(bounds, rng.random_range(0.0..24.0))
        } else {
            Item::Quad(bounds)
        }
    };
    let everywhere = sp(-20., -200., 440., 640.);
    for _ in 0..rng.random_range(4..30) {
        if rng.random_range(0..4) == 0 {
            // A card: its background, and what it clips.
            let card = sp(
                rng.random_range(-20.0..340.0f32).round(),
                rng.random_range(-200.0..400.0f32).round(),
                rng.random_range(40.0..200.0f32).round(),
                rng.random_range(30.0..160.0f32).round(),
            );
            let scrolls = scrolling && rng.random_range(0..2) == 0;
            items.push(Item::Quad(card));
            items.push(Item::Push(card));
            if scrolls {
                items.push(Item::BeginScroll);
            }
            for _ in 0..rng.random_range(1..6) {
                items.push(scattered(rng, card));
            }
            if scrolls {
                items.push(Item::EndScroll);
            }
            items.push(Item::Pop);
        } else if rng.random_range(0..12) == 0 {
            // Out of the viewport's reach: it leaves nothing.
            items.push(Item::Quad(sp(40., 9000., 80., 30.)));
        } else {
            items.push(scattered(rng, everywhere));
        }
    }
    if scrolling {
        items.push(Item::EndScroll);
    }
    items.push(Item::Pop);
    items
}

/// Content whose cards clip what they hold has, primitive for primitive,
/// the orders it has with its viewport not there: the cards' clips stay.
#[test]
fn content_with_clips_of_its_own_is_ordered_as_with_its_viewport_not_there() {
    let mut rng = StdRng::seed_from_u64(0x0c11_bbed);
    let (mut kept, mut shadows, mut dropped) = (0, 0, 0);
    for run in 0..1000 {
        // Half of them with scrolling containers told, the viewport and
        // cards in it: what follows a card that scrolls lies over its
        // blurs, in view or not.
        let items = random_content_in_clips(&mut rng, run % 2 == 1);
        let (orders, edge) = painted_orders(&items);
        let (whole, _) = painted_orders(&without_the_viewport(&items));
        for (index, (order, whole)) in orders.iter().zip(&whole).enumerate() {
            let Some(order) = order else {
                dropped += 1;
                continue;
            };
            assert_eq!(
                Some(*order),
                *whole,
                "run {run}, primitive {index} of {}",
                orders.len()
            );
            kept += 1;
        }
        shadows += edge;
    }
    assert!(kept >= 6000, "{kept} primitives were kept");
    assert!(dropped >= 3000, "{dropped} primitives were dropped");
    assert!(shadows >= 200, "{shadows} shadows were kept for their blur");
}

/// Clips are paint operations: a scene replayed from another's operations
/// knows the clips its primitives were painted in, and orders them alike.
#[test]
fn replaying_a_scene_keeps_its_clips() {
    let mut rng = StdRng::seed_from_u64(0x0c11_95ed);
    for run in 0..400 {
        let items = random_content_in_clips(&mut rng, run % 2 == 1);
        let (scene, primitives) = paint(&items);
        let mut replayed = Scene::default();
        replayed.replay(0..scene.paint_operations.len(), &scene);
        assert_eq!(
            orders(&replayed, &primitives),
            orders(&scene, &primitives),
            "run {run}"
        );
        assert_eq!(
            replayed.paint_operations.len(),
            scene.paint_operations.len(),
            "run {run}"
        );
    }
}

/// A row with a border all round, as an element paints one: its
/// background, then the border as four quads, each in a clip of its own
/// along one edge; and a shadowed card.
fn bordered_row(top: f32) -> Vec<Item> {
    let row = sp(20., top, 360., 60.);
    let mut items = vec![Item::Quad(row)];
    for edge in [
        sp(20., top, 360., 1.),
        sp(20., top + 59., 360., 1.),
        sp(20., top, 1., 60.),
        sp(379., top, 1., 60.),
    ] {
        items.extend([Item::Push(edge), Item::Quad(row), Item::Pop]);
    }
    items.extend([
        Item::Shadow(sp(40., top + 10., 320., 40.), 14.),
        Item::Quad(sp(40., top + 10., 320., 40.)),
    ]);
    items
}

/// What is painted above a viewport changes nothing below it: however many
/// bordered rows, each with clips of its own, are out of view above, the
/// shadow kept at the bottom edge is ordered as with them all in view.
#[test]
fn how_much_lies_above_a_viewport_changes_no_order() {
    for above in [0, 1, 15, 16, 17, 64, 65, 300] {
        // The last row is the first out of view below; its card's shadow
        // reaches in.
        let rows = above + 4;
        let first = 221. - 60. * (rows - 1) as f32;
        let mut items = vec![Item::Quad(window()), Item::Push(viewport())];
        for row in 0..rows {
            items.extend(bordered_row(first + 60. * row as f32));
        }
        items.push(Item::Pop);
        let (orders, kept) = painted_orders(&items);
        let (whole, _) = painted_orders(&without_the_viewport(&items));
        assert_eq!(kept, 1, "{above} rows above");
        for (index, (order, whole)) in orders.iter().zip(&whole).enumerate() {
            assert!(
                order.is_none() || order == whole,
                "{above} rows above, primitive {index} of {}: {order:?} against {whole:?}",
                orders.len()
            );
        }
        // The kept shadow: over the window, its row and nothing else.
        let shadow = orders.len() - 2;
        assert_eq!(orders[shadow], Some(3), "{above} rows above");
    }
}

/// A scene replayed as two ranges that share open clips, as the scenes of
/// a window's surfaces are put together, orders what it holds as the whole
/// scene does: a clip pushed again where it was goes on with its entries.
#[test]
fn ranges_replayed_one_after_another_share_their_clips_entries() {
    let mut rng = StdRng::seed_from_u64(0x5e9_a4a7e);
    let mut split_in_a_clip = 0;
    for run in 0..600 {
        let items = random_content_in_clips(&mut rng, run % 2 == 1);
        let (scene, primitives) = paint(&items);
        let count = scene.paint_operations.len();
        let middle = rng.random_range(1..count);
        let mut replayed = Scene::default();
        replayed.replay_balanced(0..middle, &scene).unwrap();
        split_in_a_clip += (replayed.paint_operations.len() > middle) as usize;
        replayed.replay_balanced(middle..count, &scene).unwrap();
        assert_eq!(
            orders(&replayed, &primitives),
            orders(&scene, &primitives),
            "run {run}, split at {middle} of {count}"
        );
    }
    assert!(
        split_in_a_clip >= 400,
        "{split_in_a_clip} splits fell inside a clip"
    );
}

/// What is painted after a scrolling container lies over all of it, the
/// blurs of its shadows too, as it lies over the container's layer when it
/// has one. Inside the container a neighbour beside a shadow is ordered as
/// it always was.
#[test]
fn what_is_painted_after_a_scrolling_container_lies_over_its_blurs() {
    let card = sp(40., 150., 320., 50.);
    let items = |scrolling: bool| {
        let mut items = vec![Item::Quad(window()), Item::Push(viewport())];
        if scrolling {
            items.push(Item::BeginScroll);
        }
        items.extend([
            Item::Quad(sp(20., -500., 360., 2000.)),
            Item::Quad(sp(20., 140., 360., 70.)),
            Item::Shadow(card, 12.),
            Item::Quad(card),
            // A neighbour under the blur, beside the shadow's box.
            Item::Quad(sp(40., 205., 50., 10.)),
        ]);
        if scrolling {
            items.push(Item::EndScroll);
        }
        items.extend([
            Item::Pop,
            // A bar over the viewport's bottom edge, under the blur too.
            Item::Quad(sp(100., 212., 200., 30.)),
        ]);
        items
    };
    // By their boxes alone the bar is level with the row and under the
    // card's shadow.
    let (orders, _) = painted_orders(&items(false));
    assert_eq!(
        orders,
        [
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            Some(4),
            Some(3)
        ]
    );
    // After the container, it lies over the shadow; the neighbour inside
    // stays where it was.
    let (orders, _) = painted_orders(&items(true));
    assert_eq!(
        orders,
        [
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            Some(4),
            Some(5)
        ]
    );
}

/// Two containers laid one over the other clip alike. What is painted
/// between them lies behind the second, though not behind the first: a clip
/// pushed where one was, after something else was painted, is another clip.
#[test]
fn a_clip_like_an_earlier_one_has_its_own_entries() {
    let items = [
        Item::Quad(window()),
        Item::Push(viewport()),
        Item::Quad(sp(20., 225., 360., 60.)),
        Item::Pop,
        // Laid over the first container before the second is painted.
        Item::Quad(viewport()),
        Item::Quad(sp(30., 30., 340., 190.)),
        Item::Quad(sp(40., 40., 320., 180.)),
        Item::Push(viewport()),
        Item::Shadow(sp(60., 226., 280., 50.), 10.),
        Item::Pop,
    ];
    let (orders, kept) = painted_orders(&items);
    assert_eq!(kept, 1);
    // Over the three quads laid between, not over the first container's
    // dropped row.
    assert_eq!(orders, [Some(1), None, Some(2), Some(3), Some(4), Some(5)]);

    // With nothing painted between, it is the first clip going on: the
    // shadow lies over what that clip dropped under its box.
    let items = [
        Item::Quad(window()),
        Item::Push(viewport()),
        Item::Quad(sp(20., 225., 360., 60.)),
        Item::Pop,
        Item::Push(viewport()),
        Item::Shadow(sp(60., 226., 280., 50.), 10.),
        Item::Pop,
    ];
    let (orders, _) = painted_orders(&items);
    assert_eq!(orders, [Some(1), None, Some(3)]);
}

/// What is painted after a scrolling card lies over the card's blurs where
/// the two meet outside the viewport as well as inside it: what lies over
/// that, and shows, is stacked as with the viewport not there.
#[test]
fn what_follows_a_scrolling_container_lies_over_its_blurs_out_of_view_too() {
    // A scrolling card across the viewport's bottom edge, and a shadow in it
    // whose blur spreads down the card, below the viewport.
    let card = sp(40., 150., 320., 200.);
    let items = [
        Item::Quad(window()),
        Item::Push(viewport()),
        Item::BeginScroll,
        Item::Quad(sp(20., -500., 360., 2000.)),
        Item::Quad(card),
        Item::Push(card),
        Item::BeginScroll,
        Item::Quad(sp(60., 160., 280., 30.)),
        Item::Quad(sp(60., 190., 280., 6.)),
        Item::Shadow(sp(60., 192., 100., 40.), 12.),
        Item::EndScroll,
        Item::Pop,
        // After the card: one over the blur below the shadow's box, out of
        // view; and one beside the blur, in view, over the first.
        Item::Quad(sp(150., 236., 200., 30.)),
        Item::Quad(sp(300., 200., 40., 60.)),
        Item::EndScroll,
        Item::Pop,
    ];
    let (orders, _) = painted_orders(&items);
    let (whole, _) = painted_orders(&without_the_viewport(&items));
    assert_eq!(
        whole,
        [1, 2, 3, 4, 4, 5, 6, 7].map(Some),
        "with the viewport not there"
    );
    // The one out of view is dropped; the one over it is where it would be.
    assert_eq!(orders[6], None);
    assert_eq!(orders[7], whole[7]);
}

/// Working out what lies behind a clip does not pass over all that was
/// painted before it: a table of clipped cells over one background, each
/// dropping a line that runs past it, costs a few steps a cell however many
/// cells came before, and a table painted whole, most of it out of view,
/// asks once for each column.
#[test]
fn what_lies_behind_a_clip_is_found_without_a_pass_over_the_scene() {
    use crate::fast::edge_shadows::{ClipOp, clip_op};
    let table = |cells: usize, told: bool, down: f32| {
        let mut scene = Scene::default();
        scene.insert_primitive(quad(window(), window()));
        for cell in 0..cells {
            let (left, top) = ((cell % 100) as f32 * 30., down + (cell / 100) as f32 * 20.);
            let mask = sp(left, top, 28., 18.);
            if told {
                clip_op(&mut scene, ClipOp::Push(mask));
            }
            // What the cell shows, and a line past its right edge.
            scene.insert_primitive(quad(sp(left + 1., top, 20., 10.), mask));
            scene.insert_primitive(quad(sp(left + 29., top, 6., 2.), mask));
            if told {
                clip_op(&mut scene, ClipOp::Pop);
            }
        }
        assert_eq!(scene.quads.len(), cells + 1);
        scene.fast_edge_shadows.behind_work
    };
    for told in [true, false] {
        let (few, many) = (table(1000, told, 0.), table(16_000, told, 0.));
        assert!(few >= 1000, "each cell asks what lies behind it: {few}");
        // Not a pass a cell, which is 128 million bounds: what lies near.
        assert!(
            many <= 16_000 * 16 && many <= few * 32,
            "{many} bounds looked at for 16 000 cells, {few} for 1000 (told: {told})"
        );
        // Nor further down than a grid of the tree's cells spans, where a
        // list's rows stand in their content.
        let far = table(16_000, told, 300_000.);
        assert!(
            far <= 16_000 * 64,
            "{far} bounds looked at for 16 000 cells far down (told: {told})"
        );
    }

    // A table of 3000 rows of 8 clipped cells in a viewport showing ten of
    // them, each cell cutting what it holds: the cells out of the viewport's
    // reach leave nothing, and ask nothing.
    let (mut scene, clip) = (Scene::default(), viewport());
    scene.insert_primitive(quad(window(), window()));
    clip_op(&mut scene, ClipOp::Push(clip));
    clip_op(&mut scene, ClipOp::BeginScroll);
    for row in 0..3000 {
        let top = 20. * row as f32 - 30_000.;
        for column in 0..8 {
            let cell = sp(20. + 45. * column as f32, top, 44., 19.);
            let mask = cell.intersect(&clip);
            scene.insert_primitive(quad(cell, clip));
            clip_op(&mut scene, ClipOp::Push(cell));
            scene.insert_primitive(quad(sp(cell.left().0 + 4., top + 4., 60., 10.), mask));
            clip_op(&mut scene, ClipOp::Pop);
        }
    }
    clip_op(&mut scene, ClipOp::EndScroll);
    clip_op(&mut scene, ClipOp::Pop);
    let looked = scene.fast_edge_shadows.behind_work;
    assert!(looked <= 64, "{looked} bounds looked at");
}

/// A primitive dropped out of reach of its clip leaves no paint operation,
/// and changes nothing a scene holds: a scene replayed from the operations,
/// which does not meet it, orders what follows as the scene that did.
#[test]
fn what_leaves_no_operation_changes_no_order() {
    let items = |far: bool| {
        let mut items = vec![
            Item::Push(window()),
            Item::Quad(window()),
            Item::Push(viewport()),
            Item::BeginScroll,
            // Two rows below the viewport, one over the other.
            Item::Quad(sp(20., 225., 360., 60.)),
            Item::Quad(sp(20., 230., 360., 40.)),
            Item::EndScroll,
            Item::Pop,
        ];
        if far {
            items.push(Item::Quad(sp(40., 9000., 80., 30.)));
        }
        // The viewport again, with nothing painted since: a shadow over
        // both rows.
        items.extend([
            Item::Push(viewport()),
            Item::BeginScroll,
            Item::Shadow(sp(60., 236., 280., 30.), 10.),
            Item::EndScroll,
            Item::Pop,
            Item::Pop,
        ]);
        items
    };
    let (scene, primitives) = paint(&items(true));
    let (orders_with, kept) = (orders(&scene, &primitives), scene.fast_edge_shadows.len());
    assert_eq!(kept, 1);
    assert_eq!(orders_with, [Some(1), None, None, None, Some(4)]);
    let mut replayed = Scene::default();
    replayed.replay(0..scene.paint_operations.len(), &scene);
    assert_eq!(orders(&replayed, &primitives), orders_with);
    // Nor does it matter that it was painted at all.
    let (mut without, _) = painted_orders(&items(false));
    without.insert(3, None);
    assert_eq!(orders_with, without);
}

/// What a tree held when so many bounds had been inserted is found as a
/// pass over them all finds it, for bounds the tree ordered and bounds it
/// was given an ordering for, without looking at more than lie near.
#[test]
fn what_a_tree_held_is_found_as_a_pass_over_it_finds_it() {
    use crate::fast::bounds_tree::BoundsTree;
    let mut rng = StdRng::seed_from_u64(0x0be_41bd);
    // Near the origin, left of and above it, far from it, and spread wider
    // than a grid of the tree's cells spans.
    for (count, spread, from) in [
        (40, 300.0f32, 0.0f32),
        (700, 900., -5000.),
        (9000, 6000., 0.),
        (9000, 400., 0.),
        (3000, 3000., 3_000_000.),
        (3000, 200_000., -100_000.),
        (9000, 6000., 700_000.),
    ] {
        let mut tree = BoundsTree::<ScaledPixels>::default();
        let mut inserted = Vec::new();
        let (mut asked, mut passed_over) = (0, 0);
        for index in 0..count {
            let bounds = sp(
                from + rng.random_range(0.0..spread).round(),
                from + rng.random_range(0.0..spread).round(),
                rng.random_range(2.0..90.0f32).round(),
                rng.random_range(2.0..90.0f32).round(),
            );
            let ordering = if rng.random_range(0..7) == 0 {
                let ordering = rng.random_range(1..40);
                tree.insert_at(bounds, ordering);
                ordering
            } else {
                tree.insert(bounds)
            };
            inserted.push((bounds, ordering));
            if index % 9 != 0 {
                continue;
            }
            // Now and then, about any of what was inserted so far.
            let first = rng.random_range(0..=inserted.len());
            let query = sp(
                from + rng.random_range(-20.0..spread).round(),
                from + rng.random_range(-20.0..spread).round(),
                rng.random_range(1.0..200.0f32).round(),
                rng.random_range(1.0..30.0f32).round(),
            );
            let expected = inserted[..first]
                .iter()
                .filter(|(bounds, _)| bounds.intersects(&query))
                .map(|(_, ordering)| *ordering)
                .max()
                .unwrap_or(0);
            let (found, looked) = tree.max_behind(first, &query);
            assert_eq!(
                found, expected,
                "{count} bounds over {spread} from {from}, the first {first}"
            );
            asked += first;
            passed_over += looked;
        }
        if spread == 6000. {
            assert!(
                passed_over * 50 < asked,
                "{passed_over} looked at where a pass looks at {asked}"
            );
        }
    }
}

/// Bounds too large for any window are held too, in a grid no larger than
/// any other: the cells grow with what they have to span. Bounds with an
/// edge that is no finite number have no place in a grid, and are found by
/// looking at every bounds.
#[test]
fn what_a_tree_held_is_found_among_bounds_of_any_size() {
    use crate::fast::bounds_tree::BoundsTree;
    let mut rng = StdRng::seed_from_u64(0xb16_b0d5);
    for huge in [1e7f32, 1e17, 1e30, f32::MAX / 4.] {
        let mut tree = BoundsTree::<ScaledPixels>::default();
        let mut inserted = Vec::new();
        for index in 0..400 {
            let bounds = match index % 50 {
                7 => sp(-huge, -huge, 2. * huge, 2. * huge),
                23 => sp(
                    rng.random_range(0.0..500.0f32).round(),
                    -huge,
                    40.,
                    2. * huge,
                ),
                // An edge that is no number of pixels at all.
                41 if huge == 1e17 => sp(100., 300., f32::INFINITY, 20.),
                42 if huge == 1e17 => sp(300., 100., 20., f32::INFINITY),
                43 if huge == 1e17 => sp(-f32::MAX, 400., f32::INFINITY, f32::INFINITY),
                44 if huge == 1e17 => sp(f32::NEG_INFINITY, 500., f32::INFINITY, 30.),
                _ => sp(
                    rng.random_range(0.0..900.0f32).round(),
                    rng.random_range(0.0..900.0f32).round(),
                    rng.random_range(2.0..90.0f32).round(),
                    rng.random_range(2.0..90.0f32).round(),
                ),
            };
            inserted.push((bounds, tree.insert(bounds)));
            let first = rng.random_range(0..=inserted.len());
            let query = sp(
                rng.random_range(-20.0..900.0f32).round(),
                rng.random_range(-20.0..900.0f32).round(),
                rng.random_range(1.0..200.0f32).round(),
                rng.random_range(1.0..30.0f32).round(),
            );
            let expected = inserted[..first]
                .iter()
                .filter(|(bounds, _)| bounds.intersects(&query))
                .map(|(_, ordering)| *ordering)
                .max()
                .unwrap_or(0);
            assert_eq!(
                tree.max_behind(first, &query).0,
                expected,
                "bounds of {huge}, the first {first}"
            );
        }
    }
}

/// The scene's tree replays the orderings of the last fill while the same
/// bounds come in the same order. Bounds it was given an ordering for
/// (`insert_at`) are part of that, and an ordering given last time is not
/// taken for one the tree would give now, nor the other way round.
#[test]
fn the_tree_replays_orderings_it_was_given() {
    use crate::fast::bounds_tree::BoundsTree;
    let mut rng = StdRng::seed_from_u64(0x0a7_91e5);
    let mut tree = BoundsTree::<ScaledPixels>::default();
    let mut fill: Vec<(Bounds<ScaledPixels>, Option<u32>)> = (0..60)
        .map(|_| {
            let bounds = sp(
                rng.random_range(0.0..300.0f32).round(),
                rng.random_range(0.0..300.0f32).round(),
                rng.random_range(4.0..120.0f32).round(),
                rng.random_range(4.0..120.0f32).round(),
            );
            let given = (rng.random_range(0..5) == 0).then(|| rng.random_range(1..9));
            (bounds, given)
        })
        .collect();
    let (mut replayed, mut flipped) = (0, 0);
    for frame in 0..400 {
        // Now and then a position is given its ordering where the tree
        // gave one before, or the other way round, or its ordering changes.
        if frame % 3 == 1 {
            let position = rng.random_range(0..fill.len());
            fill[position].1 = match fill[position].1 {
                Some(_) if rng.random_range(0..2) == 0 => None,
                _ => Some(rng.random_range(1..9)),
            };
            flipped += 1;
        }
        let mut fresh = BoundsTree::<ScaledPixels>::default();
        tree.clear();
        for (position, (bounds, given)) in fill.iter().enumerate() {
            let (ordering, expected) = match given {
                Some(ordering) => {
                    tree.insert_at(*bounds, *ordering);
                    fresh.insert_at(*bounds, *ordering);
                    (*ordering, *ordering)
                }
                None => (tree.insert(*bounds), fresh.insert(*bounds)),
            };
            assert_eq!(ordering, expected, "frame {frame}, position {position}");
        }
        replayed += tree.replayed;
    }
    assert!(flipped >= 100);
    assert!(replayed >= 2000, "{replayed} orderings were replayed");
}

/// `items`, less the clip they begin and end with, painted in `region` as a
/// layer's content is: a clip the content's operations do not tell of
/// (`layers::paint::repaint`).
fn content_in(region: Bounds<ScaledPixels>, items: &[Item]) -> Scene {
    use crate::fast::edge_shadows::{ClipOp, clip, clip_op};
    let mut content = Scene::default();
    clip(&mut content, ClipOp::Push(region));
    let mut masks = vec![region];
    for item in &items[1..items.len() - 1] {
        let mask = *masks.last().unwrap();
        match item {
            Item::Push(bounds) => {
                masks.push(mask.intersect(bounds));
                clip_op(&mut content, ClipOp::Push(*bounds));
            }
            Item::Pop => {
                masks.pop();
                clip_op(&mut content, ClipOp::Pop);
            }
            Item::Quad(bounds) => content.insert_primitive(quad(*bounds, mask)),
            Item::Shadow(bounds, blur) => content.insert_primitive(shadow(*bounds, mask, *blur)),
            Item::BeginScroll | Item::EndScroll | Item::Border(..) => unreachable!(),
        }
    }
    clip(&mut content, ClipOp::Pop);
    content
}

/// A layer's content is painted in its painted region and replayed into a
/// frame clipped to the viewport. What the replay leaves is ordered as the
/// content painted in that viewport is: the replay tells the scene the
/// viewport, which the content's operations know nothing of.
#[test]
fn content_replayed_in_a_viewport_is_ordered_as_content_painted_in_it() {
    let mut rng = StdRng::seed_from_u64(0x1a7e_4ed);
    let region = sp(-10_000., -10_000., 20_000., 20_000.);
    let (mut kept, mut at_a_card) = (0, 0);
    for run in 0..300 {
        let items = random_content_in_clips(&mut rng, false);
        let content = content_in(region, &items);
        let mut replayed = Scene::default();
        crate::fast::layers::paint::replay_content(
            &mut replayed,
            &content.paint_operations,
            point(ScaledPixels(0.), ScaledPixels(0.)),
            viewport(),
            Bounds::default(),
        );
        let (painted, primitives) = paint(&items);
        let expected = orders(&painted, &primitives);
        assert_eq!(orders(&replayed, &primitives), expected, "run {run}");
        kept += expected.iter().flatten().count();
        at_a_card += primitives
            .iter()
            .zip(&expected)
            .filter(|(primitive, order)| {
                let mask = primitive.content_mask().bounds;
                order.is_some()
                    && mask != viewport()
                    && primitive.bounds().intersect(&mask) != primitive.bounds()
            })
            .count();
    }
    assert!(kept >= 2000, "{kept} primitives were kept");
    assert!(at_a_card >= 100, "{at_a_card} were cut by a card");
}

/// A layer's content is kept moved to where its tiles stand. Every
/// primitive keeps the order it was painted at, in the region it was
/// painted in, cards across the region's edge and all: nothing is ordered
/// again by a scene that was not told of the region.
#[test]
fn content_moved_keeps_the_orders_it_was_painted_at() {
    let mut rng = StdRng::seed_from_u64(0x70_c0de);
    let delta = point(ScaledPixels(-37.), ScaledPixels(412.));
    let mut kept = 0;
    for run in 0..200 {
        let items = random_content_in_clips(&mut rng, false);
        // The viewport for a region, so that the region cuts what it holds.
        let content = content_in(viewport(), &items);
        let moved = crate::fast::layers::paint::translated_scene(&content, delta);
        assert_eq!(
            moved.paint_operations.len(),
            content.paint_operations.len(),
            "run {run}"
        );
        for (moved, painted) in moved.paint_operations.iter().zip(&content.paint_operations) {
            match (moved, painted) {
                (PaintOperation::Primitive(moved), PaintOperation::Primitive(painted)) => {
                    assert_eq!(order_of(moved), order_of(painted), "run {run}");
                    assert_eq!(moved.bounds().origin, painted.bounds().origin + delta);
                    kept += 1;
                }
                (PaintOperation::FastDropped(moved), PaintOperation::FastDropped(painted)) => {
                    assert_eq!(*moved, painted.moved(delta));
                }
                (PaintOperation::FastClip(moved), PaintOperation::FastClip(painted)) => {
                    assert_eq!(*moved, painted.moved(delta));
                }
                _ => panic!("run {run}: an operation of another kind"),
            }
        }
    }
    assert!(kept >= 1500, "{kept} primitives were kept");
}

/// A row of a list as its layer keeps it: a scene of its own, painted in
/// the list's region, which no clip tells of.
fn row_part(items: &[Item]) -> crate::fast::layers::scene::LayerPart {
    use crate::fast::edge_shadows::{ClipOp, clip_op};
    let region = sp(-4_000_000., -4_000_000., 8_000_000., 8_000_000.);
    let mut scene = Scene::default();
    let mut masks = vec![region];
    for item in items {
        let mask = *masks.last().unwrap();
        match item {
            Item::Push(bounds) => {
                masks.push(mask.intersect(bounds));
                clip_op(&mut scene, ClipOp::Push(*bounds));
            }
            Item::Pop => {
                masks.pop();
                clip_op(&mut scene, ClipOp::Pop);
            }
            Item::BeginScroll => clip_op(&mut scene, ClipOp::BeginScroll),
            Item::EndScroll => clip_op(&mut scene, ClipOp::EndScroll),
            Item::Quad(bounds) => scene.insert_primitive(quad(*bounds, mask)),
            Item::Shadow(bounds, blur) => scene.insert_primitive(shadow(*bounds, mask, *blur)),
            Item::Border(..) => unreachable!(),
        }
    }
    crate::fast::layers::scene::LayerPart::new(None, std::rc::Rc::new(scene))
}

/// The order of each paint operation of `parts` painted one after another
/// into one scene, part by part: what a list's content is ordered as.
fn painted_together(parts: &[crate::fast::layers::scene::LayerPart]) -> Vec<Vec<DrawOrder>> {
    let mut whole = Scene::default();
    parts
        .iter()
        .map(|part| {
            let before = whole.paint_operations.len();
            whole.replay(0..part.scene.paint_operations.len(), &part.scene);
            whole.paint_operations[before..]
                .iter()
                .map(|operation| match operation {
                    PaintOperation::Primitive(primitive) => order_of(primitive),
                    _ => 0,
                })
                .collect()
        })
        .collect()
}

/// A list's rows are ordered as painted one after another, but only the
/// rows that meet are painted together to find it out: every row, whether
/// it reaches over its neighbours or lies clear of them, cards that clip
/// and scroll and shadows whose blurs fall on the rows around, has the
/// orders it has when the whole content is painted in one scene.
#[test]
fn rows_ordered_with_the_rows_they_meet_are_ordered_as_the_whole_content_is() {
    use crate::fast::layers::scene::{LayerContent, ORDERED_OPERATIONS};
    let mut rng = StdRng::seed_from_u64(0x0707_11e5);
    let (mut alone, mut together) = (0, 0);
    for run in 0..300 {
        // Rows 80 px apart from a place far down the list; most keep to
        // themselves, some reach up or down, and now and then one is laid
        // back over rows painted long before it.
        let down = rng.random_range(0..40_000) as f32 * 80.;
        let count = rng.random_range(2..14);
        let parts: Vec<_> = (0..count)
            .map(|index| {
                let top = match rng.random_range(0..9) {
                    0 => down + rng.random_range(0..count) as f32 * 80.,
                    _ => down + index as f32 * 80.,
                };
                let reach = match rng.random_range(0..3) {
                    0 => rng.random_range(1.0..60.0f32).round(),
                    _ => 0.,
                };
                let row = sp(20., top - reach, 360., 76. + reach);
                let mut items = vec![Item::Quad(row)];
                for _ in 0..rng.random_range(1..5) {
                    let card = sp(
                        rng.random_range(30.0..250.0f32).round(),
                        top + rng.random_range(4.0..40.0f32).round(),
                        rng.random_range(30.0..120.0f32).round(),
                        rng.random_range(10.0..30.0f32).round(),
                    );
                    match rng.random_range(0..4) {
                        // A shadowed card: its blur falls on the rows
                        // around, which orders nothing.
                        0 => items.extend([Item::Shadow(card, 14.), Item::Quad(card)]),
                        // A card that clips what it holds, scrolling or not.
                        1 => {
                            let scrolls = rng.random_range(0..2) == 0;
                            items.extend([Item::Quad(card), Item::Push(card)]);
                            items.extend(scrolls.then_some(Item::BeginScroll));
                            items.push(Item::Shadow(
                                sp(card.left().0 + 4., card.top().0 + 4., 20., 40.),
                                4.,
                            ));
                            items.push(Item::Quad(sp(
                                card.left().0 - 10.,
                                card.top().0 + 2.,
                                60.,
                                8.,
                            )));
                            items.extend(scrolls.then_some(Item::EndScroll));
                            items.extend([Item::Pop, Item::Quad(card)]);
                        }
                        _ => items.push(Item::Quad(card)),
                    }
                }
                row_part(&items)
            })
            .collect();
        let expected = painted_together(&parts);
        let before = ORDERED_OPERATIONS.get();
        let content = LayerContent::from_parts(parts.clone());
        let orders = content.orders_in_full().unwrap();
        let ordered = ORDERED_OPERATIONS.get() - before;
        for (index, (orders, expected)) in orders.iter().zip(&expected).enumerate() {
            assert_eq!(
                &orders[..],
                &expected[..],
                "run {run}, row {index} of {count}"
            );
        }
        // Every row was ordered once, with the rows it meets.
        let operations: usize = parts
            .iter()
            .map(|part| part.scene.paint_operations.len())
            .sum();
        assert_eq!(ordered, operations, "run {run}");
        let reaching =
            expected
                .iter()
                .zip(&parts)
                .filter(|(orders, part)| {
                    let mut on_its_own = Scene::default();
                    on_its_own.replay(0..part.scene.paint_operations.len(), &part.scene);
                    on_its_own.paint_operations.iter().zip(orders.iter()).any(
                        |(operation, order)| match operation {
                            PaintOperation::Primitive(primitive) => order_of(primitive) != *order,
                            _ => false,
                        },
                    )
                })
                .count();
        together += reaching;
        alone += count - reaching;
    }
    assert!(alone >= 1000, "{alone} rows were ordered as they are alone");
    assert!(together >= 100, "{together} rows were ordered over others");
}

/// A row scrolled into a list costs its own ordering: the rows that stay
/// keep the orders they had, however many they are and however far down the
/// list they stand. A row that reaches over the row before is ordered with
/// that one, and no other.
#[test]
fn a_row_scrolled_in_costs_its_own_ordering() {
    use crate::fast::layers::scene::{COMPARED_EXTENTS, LayerContent, ORDERED_OPERATIONS};
    let rows = |reaching: bool| {
        (0..201)
            .map(|index| {
                let top = 2_400_000. + index as f32 * 80.;
                // Every other row's background reaches 8 px up over the row
                // before, when rows reach.
                let up = if reaching && index % 2 == 1 { 8. } else { 0. };
                let card = sp(40., top + 10., 300., 50.);
                let mut items = vec![
                    Item::Quad(sp(20., top - up, 360., 80. + up)),
                    // Its blur falls on the rows above and below.
                    Item::Shadow(card, 16.),
                    Item::Quad(card),
                ];
                items
                    .extend((0..20).map(|glyph| {
                        Item::Quad(sp(50. + glyph as f32 * 12., top + 30., 10., 14.))
                    }));
                row_part(&items)
            })
            .collect::<Vec<_>>()
    };
    for reaching in [false, true] {
        let rows = rows(reaching);
        let each = rows[0].scene.paint_operations.len();
        assert_eq!(each, 23);
        let first = LayerContent::from_parts(rows[..200].iter().cloned());
        let before = ORDERED_OPERATIONS.get();
        let kept: Vec<_> = first.orders().unwrap().to_vec();
        assert_eq!(ORDERED_OPERATIONS.get() - before, 200 * each);

        // The first row leaves and one comes at the end.
        let next = LayerContent::from_parts(rows[1..].iter().cloned());
        let before = ORDERED_OPERATIONS.get();
        let orders = next.orders().unwrap();
        let in_full = next.orders_in_full().unwrap();
        let ordered = ORDERED_OPERATIONS.get() - before;
        match reaching {
            // The row that left lay under the second, which is ordered
            // again, on its own now; and the row that came.
            true => assert_eq!(ordered, 2 * each),
            false => assert_eq!(ordered, each),
        }
        // The rows that stayed, and were not ordered again, hold the very
        // orders they held.
        let unchanged = kept[1..]
            .iter()
            .zip(orders.iter())
            .filter(|((before, was), (after, is))| {
                was == is
                    && match (before, after) {
                        (Some(before), Some(after)) => std::rc::Rc::ptr_eq(before, after),
                        (before, after) => before.is_none() && after.is_none(),
                    }
            })
            .count();
        assert!(unchanged >= 198, "{unchanged} rows kept their orders");
        assert_eq!(painted_together(&rows[1..]).len(), orders.len());
        for (orders, expected) in in_full.iter().zip(painted_together(&rows[1..])) {
            assert_eq!(&orders[..], &expected[..]);
        }

        // The same content asked again costs nothing, and finding which
        // rows meet compares each with the few beside it, not with them
        // all.
        let before = ORDERED_OPERATIONS.get();
        let compared = COMPARED_EXTENTS.get();
        let again = LayerContent::from_parts(rows[1..].iter().cloned());
        again.orders().unwrap();
        assert_eq!(ORDERED_OPERATIONS.get(), before);
        let compared = COMPARED_EXTENTS.get() - compared;
        assert!(compared <= 3 * 200, "{compared} comparisons for 200 rows");
    }

    // Two thousand rows, down a list and side by side.
    for sideways in [false, true] {
        let rows: Vec<_> = (0..2000)
            .map(|index| {
                let along = 1_000_000. + index as f32 * 8.;
                row_part(&[Item::Quad(match sideways {
                    false => sp(20., along, 360., 8.),
                    true => sp(along, 20., 8., 360.),
                })])
            })
            .collect();
        let compared = COMPARED_EXTENTS.get();
        LayerContent::from_parts(rows).orders().unwrap();
        let compared = COMPARED_EXTENTS.get() - compared;
        assert!(
            compared <= 2 * 2000,
            "{compared} comparisons for 2000 rows (side by side: {sideways})"
        );
    }
}

/// A long list of rows of forty quads: a `uniform_list`, or a `list` when
/// it has a state for one.
struct LongList(Option<crate::ListState>);

fn long_list_row(row: usize) -> crate::Div {
    use crate::{ParentElement as _, Styled as _, div, px, rgb};
    div()
        .w(px(900.))
        .h(px(24.))
        .flex()
        .bg(if row.is_multiple_of(2) {
            rgb(0xf4f1ea)
        } else {
            rgb(0xe9e4d8)
        })
        .children((0..40).map(|_| div().w(px(8.)).h(px(12.)).ml(px(2.)).bg(rgb(0x224466))))
}

impl crate::Render for LongList {
    fn render(
        &mut self,
        _: &mut crate::Window,
        cx: &mut crate::Context<Self>,
    ) -> impl crate::IntoElement {
        use crate::{IntoElement as _, ParentElement as _, Styled as _, div, px, rgb};
        let list = match &self.0 {
            Some(state) => crate::list(state.clone(), |row, _, _| {
                long_list_row(row).into_any_element()
            })
            .w(px(900.))
            .h(px(700.))
            .bg(rgb(0xffffff))
            .into_any_element(),
            None => crate::uniform_list(
                "list",
                20_000,
                cx.processor(|_, range: std::ops::Range<usize>, _, _| {
                    range.map(long_list_row).collect::<Vec<_>>()
                }),
            )
            .w(px(900.))
            .h(px(700.))
            .bg(rgb(0xffffff))
            .into_any_element(),
        };
        div().size_full().bg(rgb(0xffffff)).child(list)
    }
}

/// A list scrolled a row a frame from its layer's tiles orders the row that
/// comes into the layer and no other, at its top and far down it: ordering
/// the layer's whole content for every row, in the content's coordinates,
/// made such a frame thirty times as dear far down a `uniform_list` as it
/// had been, and ten times as dear as painting the list without a layer. A
/// `list` paints the rows it shows every frame, in one scene, and they keep
/// the orders that scene gave them.
#[crate::test]
fn a_list_scrolled_a_row_a_frame_orders_the_rows_that_come(cx: &mut crate::TestAppContext) {
    use crate::fast::layers::scene::{COMPARED_EXTENTS, ORDERED_OPERATIONS};
    use crate::{AppContext as _, ScrollDelta, ScrollWheelEvent, TouchPhase, px};
    for uniform in [true, false] {
        let window = crate::fast::tests::layer_support::add_window(cx, move |_, _| {
            LongList(
                (!uniform)
                    .then(|| crate::ListState::new(20_000, crate::ListAlignment::Top, px(0.))),
            )
        });
        let any: crate::AnyWindowHandle = window.into();
        cx.test_window(any).simulate_scale_factor_change(2.);
        let draw = |cx: &mut crate::TestAppContext| {
            cx.update_window(any, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        };
        let wheel = |cx: &mut crate::TestAppContext, dy: f32| {
            let frame = cx
                .update_window(any, |_, window, _| window.fast_layers.frame)
                .unwrap();
            cx.update_window(any, |_, window, cx| {
                window.dispatch_event(
                    crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: crate::point(px(20.), px(20.)),
                        delta: ScrollDelta::Pixels(crate::point(px(0.), px(dy))),
                        modifiers: Default::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            })
            .unwrap();
            let drawn = cx
                .update_window(any, |_, window, _| window.fast_layers.frame)
                .unwrap();
            if drawn == frame {
                draw(cx);
            }
        };
        draw(cx);
        draw(cx);
        // A row: its background and forty quads.
        let row = 41;
        for far in [0., 100_000.] {
            if far > 0. {
                wheel(cx, -far);
            }
            for _ in 0..6 {
                wheel(cx, -24.);
            }
            let (mut most, mut composited, mut rows) = (0, 0, 0);
            let mut compared = 0;
            for step in 0..60 {
                let before = ORDERED_OPERATIONS.get();
                let compared_before = COMPARED_EXTENTS.get();
                // Down, and back up.
                wheel(cx, if step < 40 { -24. } else { 24. });
                most = most.max(ORDERED_OPERATIONS.get() - before);
                compared = compared.max(COMPARED_EXTENTS.get() - compared_before);
                // The orders the layer's content holds, kept or found again,
                // are those of its rows painted one after another.
                rows += cx
                    .update_window(any, |_, window, _| {
                        let Some(record) = (window.fast_layers.layers.values().next())
                            .and_then(|layer| layer.record.as_ref())
                        else {
                            return 0;
                        };
                        let content = &record.content;
                        let expected = painted_together(content.parts());
                        let Some(orders) = content.orders_in_full() else {
                            return 0;
                        };
                        for (orders, expected) in orders.iter().zip(&expected) {
                            assert_eq!(&orders[..], &expected[..], "{far} px down, step {step}");
                        }
                        orders.len()
                    })
                    .unwrap();

                composited += cx
                    .update_window(any, |_, window, _| {
                        !window.rendered_frame.scene.layers.frames.is_empty()
                    })
                    .unwrap() as usize;
            }
            // A `list` paints its rows again every other frame or so.
            assert!(
                composited >= if uniform { 50 } else { 20 },
                "{composited} of 60 frames were drawn from the list's layer (uniform: {uniform})"
            );
            assert!(rows >= 400, "{rows} rows were checked (uniform: {uniform})");
            // Which rows meet is found without comparing each with all.
            assert!(
                compared <= 4 * 200,
                "{compared} comparisons of rows in a frame (uniform: {uniform})"
            );
            assert!(
                most <= 3 * row,
                "{most} operations ordered in a frame that scrolled one row in, {far} px down \
                 (uniform: {uniform})"
            );
        }
    }
}

/// The rows a list paints in one frame are painted one after another in one
/// scene, which orders them as the content wants them: rows of one such
/// pass keep the orders they were painted at, and are not painted again to
/// find them out. A row that met something painted in the scene that became
/// no part of the content is ordered again, with the rows it meets.
#[test]
fn rows_painted_in_one_scene_keep_the_orders_it_gave_them() {
    use crate::fast::edge_shadows::{ClipOp, clip_op};
    use crate::fast::layers::scene::{LayerContent, LayerPart, ORDERED_OPERATIONS, PaintPass};
    let mut rng = StdRng::seed_from_u64(0x9a55_0e5);
    let region = sp(-4_000_000., -4_000_000., 8_000_000., 8_000_000.);
    let (mut kept, mut again) = (0, 0);
    for run in 0..300 {
        // The rows are painted where the window shows them, and moved to
        // where they stand in the content, as `lists::end_paint_rows` moves
        // them.
        let to_content = point(
            ScaledPixels(0.),
            ScaledPixels(rng.random_range(0..40_000) as f32 * 80.),
        );
        let count = rng.random_range(3..12);
        // Now and then a row is painted that the content does not keep:
        // one that became no part, or a part the content does not hold.
        let left_out = (rng.random_range(0..3) == 0).then(|| rng.random_range(0..count));
        let as_a_part = rng.random_range(0..2) == 0;
        let mut scene = Scene::default();
        let mut ranges = Vec::new();
        for index in 0..count {
            let top = 40. + index as f32 * 80.;
            let reach = match rng.random_range(0..3) {
                0 => rng.random_range(1.0..60.0f32).round(),
                _ => 0.,
            };
            let start = scene.paint_operations.len();
            scene.insert_primitive(quad(sp(20., top - reach, 360., 76. + reach), region));
            for _ in 0..rng.random_range(1..5) {
                let card = sp(
                    rng.random_range(30.0..250.0f32).round(),
                    top + rng.random_range(4.0..40.0f32).round(),
                    rng.random_range(30.0..120.0f32).round(),
                    rng.random_range(10.0..30.0f32).round(),
                );
                scene.insert_primitive(shadow(card, region, 14.));
                scene.insert_primitive(quad(card, region));
                if rng.random_range(0..2) == 0 {
                    // What the card clips, and cuts.
                    clip_op(&mut scene, ClipOp::Push(card));
                    let mask = card.intersect(&region);
                    scene.insert_primitive(quad(
                        sp(card.left().0 - 10., card.top().0 + 2., 60., 8.),
                        mask,
                    ));
                    clip_op(&mut scene, ClipOp::Pop);
                }
            }
            ranges.push(start..scene.paint_operations.len());
        }
        let pass = std::rc::Rc::new(PaintPass::default());
        let copy = |range: &std::ops::Range<usize>| -> Vec<PaintOperation> {
            scene.paint_operations[range.clone()]
                .iter()
                .map(|operation| match operation {
                    PaintOperation::Primitive(primitive) => PaintOperation::Primitive(
                        crate::fast::layers::scene::translate_primitive(primitive, to_content),
                    ),
                    PaintOperation::FastDropped(dropped) => {
                        PaintOperation::FastDropped(dropped.moved(to_content))
                    }
                    PaintOperation::FastClip(clip) => {
                        PaintOperation::FastClip(clip.moved(to_content))
                    }
                    _ => unreachable!(),
                })
                .collect()
        };
        let mut parts = Vec::new();
        for (index, range) in ranges.iter().enumerate() {
            if left_out == Some(index) && !as_a_part {
                // Noted as it was painted, with how far the rest moved.
                pass.leave_out(&scene.paint_operations[range.clone()], to_content);
                continue;
            }
            let mut row = Scene::default();
            row.paint_operations = copy(range);
            let part = LayerPart::painted(None, std::rc::Rc::new(row), &pass);
            if left_out != Some(index) {
                parts.push(part);
            }
        }
        let expected = painted_together(&parts);
        let before = ORDERED_OPERATIONS.get();
        let content = LayerContent::from_parts(parts.clone());
        let orders = content.orders_in_full().unwrap();
        let ordered = ORDERED_OPERATIONS.get() - before;
        for (index, (orders, expected)) in orders.iter().zip(&expected).enumerate() {
            assert_eq!(&orders[..], &expected[..], "run {run}, row {index}");
        }
        match left_out {
            None => assert_eq!(ordered, 0, "run {run}: every row keeps its painted orders"),
            Some(_) => again += (ordered > 0) as usize,
        }
        kept += (ordered == 0) as usize;
    }
    assert!(kept >= 200, "{kept} contents kept their painted orders");
    assert!(again >= 20, "{again} contents had rows ordered again");
}

/// What the clips told leave of a primitive that lies wholly outside one of
/// them is empty. Where edges fall between device pixels the mask such a
/// primitive carries is the empty box of the logical clips, not grown to the
/// grid, and differs from the clips told by a few millionths of a pixel: it
/// is still the clips told, not a clip the window did not tell of, and the
/// shadow it drops leaves its edge at the clips, so what is painted after it
/// is raised over the blur.
#[test]
fn an_empty_mask_off_the_grid_is_the_clips_told() {
    use crate::fast::edge_shadows::{ClipOp, clip_op};
    let outer = sp(20., 20., 360., 200.);
    let card = sp(40., 400., 320., 100.);
    let run = |off: f32| {
        let mut scene = Scene::default();
        scene.insert_primitive(quad(window(), window()));
        clip_op(&mut scene, ClipOp::Push(outer));
        clip_op(&mut scene, ClipOp::BeginScroll);
        scene.insert_primitive(quad(sp(20., -500., 360., 2000.), outer));
        clip_op(&mut scene, ClipOp::Push(card));
        // The card is out of view: the mask it paints in is empty, and
        // off by `off` from what the two clips told leave.
        let told = outer.intersect(&card);
        assert!(told.is_empty());
        let mask = Bounds {
            origin: point(told.origin.x + ScaledPixels(off), told.origin.y),
            size: size(told.size.width - ScaledPixels(2. * off), told.size.height),
        };
        scene.insert_primitive(shadow(sp(60., 380., 280., 40.), mask, 12.));
        clip_op(&mut scene, ClipOp::Pop);
        clip_op(&mut scene, ClipOp::EndScroll);
        clip_op(&mut scene, ClipOp::Pop);
        scene.insert_primitive(quad(sp(100., 200., 200., 60.), window()));
        (
            scene.fast_edge_shadows.untold_count(),
            scene.quads.last().unwrap().order,
        )
    };
    let (exact, exact_order) = run(0.);
    let (off, off_order) = run(0.00003);
    assert_eq!(exact, off, "clips the window did not tell of");
    assert_eq!(exact_order, off_order, "the order of what is painted after");
}
