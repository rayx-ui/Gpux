//! What the renderers' scroll-layer tiles take from the core: the sprites
//! that composite tiles say their textures are premultiplied, a frame drawn
//! from scratch leaves no layer content painted before it, and a shadow is
//! drawn for as long as its blur shows, as a tile shows it.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::layer_renderers::
//! ```

use crate::fast::layers::policy::Decision;
use crate::fast::tests::layer_support::*;
use crate::{
    AnyWindowHandle, Background, Bounds, ContentMask, Corners, LAYER_TILE_PREMULTIPLIED,
    PolychromeSprite, Quad, ScaledPixels, Scene, Shadow, TestAppContext, TransformationMatrix,
    decode_layer_tile, point, rgb, size,
};

fn record_generation(cx: &mut TestAppContext, window: AnyWindowHandle) -> Option<u64> {
    let id = scroller_id(cx, window);
    with_window(cx, window, |window, _| {
        window
            .fast_layers
            .layers
            .get(&id)
            .and_then(|layer| layer.record.as_ref())
            .map(|record| record.generation)
    })
}

fn tile_sprites(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<PolychromeSprite> {
    with_window(cx, window, |window, _| {
        window
            .rendered_frame
            .scene
            .polychrome_sprites
            .iter()
            .filter(|sprite| {
                decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id).is_some()
            })
            .copied()
            .collect()
    })
}

/// A tile's texture holds what was drawn into it, premultiplied; the
/// polychrome shaders multiply an atlas texture's colour by its alpha
/// unless the sprite says otherwise.
#[crate::test]
fn tile_sprites_say_their_textures_are_premultiplied(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let window = page(cx, false).into();
    promote(cx, window);
    assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    let sprites = tile_sprites(cx, window);
    assert!(!sprites.is_empty(), "the frame composites tiles");
    for sprite in sprites {
        assert_eq!(sprite.pad, LAYER_TILE_PREMULTIPLIED);
        assert!(!sprite.grayscale);
        assert_eq!(sprite.opacity, 1.);
    }
}

/// A window is drawn from scratch for what no dependency records: a theme
/// read past the bookkeeping, or a renderer that lost its sprite atlas, whose
/// tiles the content a layer kept would still name. No layer keeps content
/// painted before such a frame, and the content it paints next has a
/// generation the renderer holds no tile of.
#[crate::test]
fn a_frame_drawn_from_scratch_leaves_no_layer_content(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let window = page(cx, false).into();
    promote(cx, window);
    assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    let before = record_generation(cx, window).expect("the layer holds content");

    frame_after(cx, window, |cx| {
        with_window(cx, window, |window, _| window.refresh());
    });
    let stats = with_window(cx, window, |window, _| window.fast_frame_stats());
    assert_eq!(
        (stats.layers_composited, stats.layers_repainted),
        (0, 0),
        "a frame drawn from scratch paints the content into the frame"
    );
    assert_eq!(record_generation(cx, window), None);
    assert!(tile_sprites(cx, window).is_empty());
    let id = scroller_id(cx, window);
    assert!(
        with_window(cx, window, |window, _| window
            .fast_layers
            .layers
            .contains_key(&id)),
        "the container keeps its standing"
    );

    // Scrolled again, it is painted into its layer again, afresh.
    let mut decisions = Vec::new();
    for _ in 0..4 {
        decisions.push(scroll(cx, window, -4.));
        if record_generation(cx, window).is_some() {
            break;
        }
    }
    assert_eq!(
        decisions.last(),
        Some(&Some(Decision::Repaint)),
        "{decisions:?}"
    );
    let after = record_generation(cx, window).expect("the layer holds content again");
    assert!(after > before, "generation {after} after {before}");
    assert_eq!(scroll(cx, window, -4.), Some(Decision::Composite));
}

// --- shadows --- //

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

fn quad(bounds: Bounds<ScaledPixels>, clip: Bounds<ScaledPixels>) -> Quad {
    Quad {
        bounds,
        content_mask: mask(clip),
        background: Background::from(rgb(0x808080)).prepare(),
        ..Default::default()
    }
}

fn shadow(bounds: Bounds<ScaledPixels>, blur: f32, clip: Bounds<ScaledPixels>) -> Shadow {
    Shadow {
        order: 0,
        blur_radius: ScaledPixels(blur),
        bounds,
        corner_radii: Corners::default(),
        content_mask: mask(clip),
        color: crate::Color::from(rgb(0x000000)).paint_color(),
        element_bounds: bounds,
        element_corner_radii: Corners::default(),
        inset: 0,
        pad: 0,
        transformation: TransformationMatrix::unit(),
    }
}

/// A viewport at (0, 100) to (200, 300), with its background painted.
fn viewport_scene() -> (Scene, Bounds<ScaledPixels>) {
    let viewport = sp(0., 100., 200., 200.);
    let mut scene = Scene::default();
    scene.insert_primitive(quad(viewport, viewport));
    (scene, viewport)
}

/// A blurred shadow fades out over three blur radii beyond its bounds. Its
/// box scrolled just out of the viewport, the blur still shows inside: it is
/// drawn, over the background it shows on, until the blur has left too.
#[test]
fn a_shadow_is_drawn_while_its_blur_reaches_its_mask() {
    let (mut scene, viewport) = viewport_scene();
    // 5 px above the viewport, with a blur that reaches 18.
    scene.insert_primitive(shadow(sp(20., 45., 160., 50.), 6., viewport));
    scene.finish();
    assert_eq!(scene.shadows.len(), 1);
    assert!(
        scene.shadows[0].order > scene.quads[0].order,
        "over the background its blur shows on"
    );
    // Its bounds and mask are as painted: the renderer clips it.
    assert_eq!(scene.shadows[0].bounds, sp(20., 45., 160., 50.));
    assert_eq!(scene.shadows[0].content_mask.bounds, viewport);

    // Out of reach on every side, and beside the viewport within reach of
    // nothing: dropped, as before.
    for bounds in [
        sp(20., 20., 160., 50.),
        sp(20., 330., 160., 50.),
        sp(-200., 150., 150., 50.),
        sp(250., 150., 50., 50.),
    ] {
        let (mut scene, viewport) = viewport_scene();
        scene.insert_primitive(shadow(bounds, 6., viewport));
        assert!(scene.shadows.is_empty(), "{bounds:?} is out of reach");
    }

    // Without a blur a shadow draws nothing beyond its bounds.
    let (mut scene, viewport) = viewport_scene();
    scene.insert_primitive(shadow(sp(20., 45., 160., 50.), 0., viewport));
    assert!(scene.shadows.is_empty());
}

/// An inset shadow fills its element around the hole its bounds are: it
/// shows wherever the element does.
#[test]
fn an_inset_shadow_is_drawn_while_its_element_shows() {
    let (mut scene, viewport) = viewport_scene();
    // The element reaches 30 px into the viewport; the hole stays above it.
    let inset = Shadow {
        inset: 1,
        element_bounds: sp(20., 30., 160., 100.),
        ..shadow(sp(30., 40., 140., 50.), 4., viewport)
    };
    scene.insert_primitive(inset);
    assert_eq!(scene.shadows.len(), 1);

    let (mut scene, viewport) = viewport_scene();
    scene.insert_primitive(Shadow {
        element_bounds: sp(20., 0., 160., 90.),
        ..inset
    });
    let _ = viewport;
    assert!(scene.shadows.is_empty(), "the element left the viewport");
}

/// A shadow kept for its blur is ordered as it was when the last pixel of
/// its bounds showed: under its own element, entering the viewport after
/// it, and without a say in the order of what does not touch that edge.
#[test]
fn a_shadow_kept_for_its_blur_is_ordered_by_the_edge_it_lies_beyond() {
    // An element entering from below: one row of it shows, its shadows,
    // offset down, not yet.
    let (mut scene, viewport) = viewport_scene();
    scene.insert_primitive(shadow(sp(21., 304., 158., 72.), 6., viewport));
    scene.insert_primitive(shadow(sp(22., 303., 156., 70.), 4., viewport));
    scene.insert_primitive(quad(sp(20., 299., 160., 74.), viewport));
    // A neighbour above, wholly in view, a blur's reach away.
    scene.insert_primitive(shadow(sp(21., 204., 158., 72.), 6., viewport));
    scene.insert_primitive(shadow(sp(22., 203., 156., 70.), 4., viewport));
    scene.insert_primitive(quad(sp(20., 199., 160., 74.), viewport));
    let background = scene.quads[0].order;
    let orders: Vec<u32> = scene.shadows.iter().map(|shadow| shadow.order).collect();
    assert_eq!(
        orders,
        [
            background + 1,
            background + 2,
            background + 1,
            background + 2
        ],
        "the kept shadows are ordered like the neighbour's, whose bounds show"
    );
    assert_eq!(scene.quads[1].order, background + 3, "over its own shadows");
    assert_eq!(scene.quads[2].order, background + 3);

    // The same without the element at the edge: the neighbour's orders are
    // what they were.
    let (mut scene, viewport) = viewport_scene();
    scene.insert_primitive(shadow(sp(21., 204., 158., 72.), 6., viewport));
    scene.insert_primitive(shadow(sp(22., 203., 156., 70.), 4., viewport));
    scene.insert_primitive(quad(sp(20., 199., 160., 74.), viewport));
    let orders: Vec<u32> = scene.shadows.iter().map(|shadow| shadow.order).collect();
    assert_eq!(orders, [background + 1, background + 2]);
    assert_eq!(scene.quads[1].order, background + 3);
}

/// Whatever its content mask leaves a part of is culled and ordered by that
/// part, as upstream does it.
#[test]
fn what_shows_within_its_mask_is_ordered_by_what_shows() {
    let (mut scene, viewport) = viewport_scene();
    // Two shadows whose bounds overlap only above the viewport.
    scene.insert_primitive(shadow(sp(20., 60., 100., 60.), 6., viewport));
    scene.insert_primitive(shadow(sp(100., 40., 80., 50.), 6., viewport));
    let background = scene.quads[0].order;
    let orders: Vec<u32> = scene.shadows.iter().map(|shadow| shadow.order).collect();
    // The first shows 20 rows; the second is kept for its blur, at the top
    // edge beside it.
    assert_eq!(orders, [background + 1, background + 2]);

    let (mut scene, viewport) = viewport_scene();
    scene.insert_primitive(quad(sp(20., 60., 100., 30.), viewport));
    assert_eq!(scene.quads.len(), 1, "a quad outside its mask is dropped");
}
