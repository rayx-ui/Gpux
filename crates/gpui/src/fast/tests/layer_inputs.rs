//! Scroll-layer inputs: tile hashing, background baking, glyph reach, path
//! lifting and the bypass reasons. Adapted from gpui-fast's `fast/tests/layers.rs`.

use crate::{
    AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds, ContentMask, DevicePixels, Hsla,
    LayerFrame, LayerKey, MonochromeSprite, Path, Pixels, Point, PolychromeSprite, Quad,
    ScaledPixels, Scene, Shadow, SubpixelSprite, TileCoord, TileId, TransformationMatrix,
    Underline, decode_layer_tile, fast::layers::scene::translate_primitive, layer_tile_id,
    layer_tile_texture_id, point, px, scene::Primitive, size,
};

#[test]
fn layer_tile_ids_round_trip_and_never_collide_with_the_atlas() {
    for layer in [0, 1, 77, 0x00FF_FFFF] {
        for tile in [(0, 0), (-1, 3), (2047, -2048), (-2048, 2047)] {
            let key = LayerKey(layer);
            let coord = TileCoord {
                x: tile.0,
                y: tile.1,
            };
            let texture = layer_tile_texture_id(key);
            assert_eq!(texture.kind, AtlasTextureKind::Polychrome);
            assert!(texture.index >= crate::LAYER_TILE_TEXTURE_BASE);
            assert_eq!(
                decode_layer_tile(texture, layer_tile_id(coord)),
                Some((key, coord))
            );
        }
    }
    let atlas = AtlasTextureId {
        index: 3,
        kind: AtlasTextureKind::Polychrome,
    };
    assert_eq!(
        decode_layer_tile(atlas, layer_tile_id(TileCoord { x: 0, y: 0 })),
        None
    );
    let mono = AtlasTextureId {
        index: crate::LAYER_TILE_TEXTURE_BASE,
        kind: AtlasTextureKind::Monochrome,
    };
    assert_eq!(
        decode_layer_tile(mono, layer_tile_id(TileCoord { x: 0, y: 0 })),
        None
    );
}

fn sp(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(w), ScaledPixels(h)),
    }
}

fn wide_mask() -> ContentMask<ScaledPixels> {
    ContentMask::rectangular(sp(-10_000., -10_000., 20_000., 20_000.))
}

fn quad(bounds: Bounds<ScaledPixels>) -> Quad {
    Quad {
        bounds,
        content_mask: wide_mask(),
        background: crate::fast::tests::layer_support::bg(Hsla::red()),
        ..Default::default()
    }
}

fn layer(content: Scene) -> LayerFrame {
    LayerFrame {
        key: LayerKey(1),
        generation: 1,
        background: crate::fast::tests::layer_support::paint_color(crate::rgba(0xffffffff)),
        tile_size: 512,
        content: content.into(),
        dirty_tiles: Vec::new(),
    }
}

#[test]
fn a_tile_scene_holds_the_primitives_over_the_tile_in_its_own_space() {
    let mut content = Scene::default();
    content.insert_primitive(quad(sp(10., 10., 20., 20.))); // tile (0,0) only
    content.insert_primitive(quad(sp(500., 100., 40., 10.))); // tiles (0,0) and (1,0)
    content.insert_primitive(quad(sp(600., 700., 10., 10.))); // tile (1,1) only
    content.insert_primitive(quad(sp(-30., -30., 10., 10.))); // tile (-1,-1) only
    content.finish();
    let layer = layer(content);

    assert_eq!(
        layer.tile_bounds(TileCoord { x: -1, y: 2 }),
        sp(-512., 1024., 512., 512.)
    );

    let t10 = layer.tile_scene(TileCoord { x: 1, y: 0 });
    assert_eq!(t10.quads.len(), 1);
    assert_eq!(t10.quads[0].bounds, sp(500. - 512., 100., 40., 10.));
    assert_eq!(
        t10.quads[0].content_mask.bounds.origin,
        point(ScaledPixels(-10_000. - 512.), ScaledPixels(-10_000.))
    );

    let t00 = layer.tile_scene(TileCoord { x: 0, y: 0 });
    assert_eq!(t00.quads.len(), 2);
    // The two do not overlap, so they may share a draw order; the first
    // painted still sorts first.
    assert!(
        t00.quads[0].order <= t00.quads[1].order,
        "drawing order kept"
    );
    assert_eq!(t00.quads[0].bounds, sp(10., 10., 20., 20.));
    assert_eq!(t00.quads[1].bounds, sp(500., 100., 40., 10.));

    let t11 = layer.tile_scene(TileCoord { x: 1, y: 1 });
    assert_eq!(t11.quads.len(), 1);
    assert_eq!(t11.quads[0].bounds, sp(600. - 512., 700. - 512., 10., 10.));

    let tneg = layer.tile_scene(TileCoord { x: -1, y: -1 });
    assert_eq!(tneg.quads.len(), 1);
    assert_eq!(tneg.quads[0].bounds, sp(-30. + 512., -30. + 512., 10., 10.));

    assert!(layer.tile_scene(TileCoord { x: 3, y: 3 }).quads.is_empty());
}

#[test]
fn a_tile_scene_leaves_out_primitives_masked_off_the_tile() {
    let mut content = Scene::default();
    // Spans tiles (0,0) and (1,0), but its mask keeps it inside tile (0,0).
    content.insert_primitive(Quad {
        content_mask: ContentMask::rectangular(sp(0., 0., 512., 512.)),
        ..quad(sp(500., 100., 40., 10.))
    });
    content.finish();
    let layer = layer(content);
    assert_eq!(layer.tile_scene(TileCoord { x: 0, y: 0 }).quads.len(), 1);
    assert!(layer.tile_scene(TileCoord { x: 1, y: 0 }).quads.is_empty());
}

#[test]
fn a_tile_scene_keeps_layers_sharing_one_draw_order() {
    let mut content = Scene::default();
    content.push_layer(sp(0., 0., 1024., 100.));
    content.insert_primitive(quad(sp(10., 10., 10., 10.)));
    content.insert_primitive(quad(sp(600., 10., 10., 10.)));
    content.pop_layer();
    content.insert_primitive(quad(sp(20., 20., 10., 10.)));
    content.finish();
    let layer = layer(content);

    let t00 = layer.tile_scene(TileCoord { x: 0, y: 0 });
    assert_eq!(t00.quads.len(), 2);
    assert!(t00.quads[0].order < t00.quads[1].order);
    let t10 = layer.tile_scene(TileCoord { x: 1, y: 0 });
    assert_eq!(t10.quads.len(), 1);
    assert_eq!(t10.quads[0].bounds, sp(600. - 512., 10., 10., 10.));
}

/// Content handed to the renderer in parts, as a virtual list's rows are,
/// draws over each tile what it draws handed over as one scene: a part
/// reaching no pixel of a tile is left out of the tile's scene.
#[test]
fn content_in_parts_draws_each_tile_as_one_scene_does() {
    use crate::fast::layers::{
        scene::{LayerContent, LayerPart},
        tiles::part_tile_hashes,
    };
    use crate::scene::PaintOperation;
    use std::rc::Rc;

    let color = |row: usize| crate::hsla(row as f32 / 7., 0.5, 0.5, 1.);
    // Six rows, 200 px tall and 700 px wide: a background each; in every
    // other row a layer around two overlapping quads; in row 3 a shadow
    // blurred into the rows beside it.
    let rows: Vec<Vec<PaintOperation>> = (0..6)
        .map(|row| {
            let y = row as f32 * 200.;
            let mut operations = vec![PaintOperation::Primitive(
                Quad {
                    background: crate::fast::tests::layer_support::bg(color(row)),
                    ..quad(sp(0., y, 700., 200.))
                }
                .into(),
            )];
            if row % 2 == 0 {
                operations.push(PaintOperation::StartLayer(sp(0., y, 700., 200.)));
                operations.push(PaintOperation::Primitive(
                    quad(sp(480., y + 50., 60., 60.)).into(),
                ));
                operations.push(PaintOperation::Primitive(
                    Quad {
                        background: crate::fast::tests::layer_support::bg(color(row + 1)),
                        ..quad(sp(500., y + 70., 60., 60.))
                    }
                    .into(),
                ));
                operations.push(PaintOperation::EndLayer);
            }
            if row == 3 {
                operations.push(PaintOperation::Primitive(
                    Shadow {
                        transformation: TransformationMatrix::unit(),
                        order: 0,
                        blur_radius: ScaledPixels(20.),
                        bounds: sp(100., y + 150., 300., 40.),
                        corner_radii: Default::default(),
                        content_mask: wide_mask(),
                        color: crate::fast::tests::layer_support::paint_color(Hsla::black()),
                        element_bounds: sp(100., y + 150., 300., 40.),
                        element_corner_radii: Default::default(),
                        inset: 0,
                        pad: 0,
                    }
                    .into(),
                ));
            }
            operations
        })
        .collect();

    let mut whole = Scene::default();
    for operation in rows.iter().flatten() {
        match operation {
            PaintOperation::Primitive(primitive) => whole.insert_primitive(primitive.clone()),
            PaintOperation::FastDropped(dropped) => {
                crate::fast::edge_shadows::dropped(&mut whole, *dropped)
            }
            PaintOperation::FastClip(clip) => crate::fast::edge_shadows::clip_op(&mut whole, *clip),
            PaintOperation::StartLayer(bounds) => whole.push_layer(*bounds),
            PaintOperation::StartPositionedLayer(bounds) => whole.push_positioned_layer(*bounds),
            PaintOperation::StartTopLayer(bounds) => whole.push_top_layer(*bounds),
            PaintOperation::EndLayer => whole.pop_layer(),
        }
    }
    whole.finish();
    let parts = rows.iter().map(|operations| {
        let (_, reach) = part_tile_hashes(operations, 512);
        let mut scene = Scene::default();
        scene.paint_operations = operations
            .iter()
            .map(|operation| match operation {
                PaintOperation::Primitive(primitive) => {
                    PaintOperation::Primitive(primitive.clone())
                }
                PaintOperation::StartLayer(bounds) => PaintOperation::StartLayer(*bounds),
                PaintOperation::StartPositionedLayer(bounds) => {
                    PaintOperation::StartPositionedLayer(*bounds)
                }
                PaintOperation::StartTopLayer(bounds) => PaintOperation::StartTopLayer(*bounds),
                PaintOperation::EndLayer => PaintOperation::EndLayer,
                PaintOperation::FastDropped(dropped) => PaintOperation::FastDropped(*dropped),
                PaintOperation::FastClip(clip) => PaintOperation::FastClip(*clip),
            })
            .collect();
        LayerPart::new(Some(reach.unwrap_or_default()), Rc::new(scene))
    });
    let whole = layer(whole);
    let in_parts = LayerFrame {
        content: LayerContent::from_parts(parts),
        ..whole.clone()
    };
    for y in -1..4 {
        for x in -1..3 {
            let tile = TileCoord { x, y };
            let drawn =
                |frame: &LayerFrame| crate::fast::layers::verify::drawn(&frame.tile_scene(tile));
            assert_eq!(drawn(&in_parts), drawn(&whole), "tile {tile:?}");
        }
    }
}

fn atlas_tile() -> AtlasTile {
    AtlasTile {
        texture_id: AtlasTextureId {
            index: 0,
            kind: AtlasTextureKind::Monochrome,
        },
        tile_id: TileId(9),
        padding: 0,
        bounds: Bounds {
            origin: point(DevicePixels(0), DevicePixels(0)),
            size: size(DevicePixels(4), DevicePixels(4)),
        },
    }
}

#[test]
fn translation_moves_every_position_a_primitive_carries() {
    let delta = point(ScaledPixels(7.), ScaledPixels(-3.));
    let mask = ContentMask::rectangular(sp(0., 0., 100., 100.));
    let moved_mask = sp(7., -3., 100., 100.);
    let translate = |primitive: Primitive| translate_primitive(&primitive, delta);

    let shadow = Shadow {
        transformation: TransformationMatrix::unit(),
        order: 0,
        blur_radius: ScaledPixels(2.),
        bounds: sp(1., 2., 3., 4.),
        corner_radii: Default::default(),
        content_mask: mask,
        color: crate::fast::tests::layer_support::paint_color(Hsla::red()),
        element_bounds: sp(5., 6., 7., 8.),
        element_corner_radii: Default::default(),
        inset: 0,
        pad: 0,
    };
    let Primitive::Shadow(moved) = translate(shadow.into()) else {
        unreachable!()
    };
    assert_eq!(moved.bounds, sp(8., -1., 3., 4.));
    assert_eq!(moved.element_bounds, sp(12., 3., 7., 8.));
    assert_eq!(moved.content_mask.bounds, moved_mask);
    assert_eq!(moved.blur_radius, ScaledPixels(2.));

    let Primitive::Quad(moved) = translate(
        Quad {
            content_mask: mask,
            ..quad(sp(1., 2., 3., 4.))
        }
        .into(),
    ) else {
        unreachable!()
    };
    assert_eq!(moved.bounds, sp(8., -1., 3., 4.));
    assert_eq!(moved.content_mask.bounds, moved_mask);

    let mut path = Path::new(point(px(10.), px(20.)));
    path.push_triangle(
        (
            point(px(10.), px(20.)),
            point(px(30.), px(20.)),
            point(px(10.), px(40.)),
        ),
        (point(0., 1.), point(0., 1.), point(0., 1.)),
    );
    let mut path = path.scale(1.);
    path.content_mask = mask;
    for vertex in &mut path.vertices {
        vertex.content_mask = mask;
    }
    let bounds = path.bounds;
    let Primitive::Path(moved) = translate(path.into()) else {
        unreachable!()
    };
    assert_eq!(moved.bounds.origin, bounds.origin + delta);
    assert_eq!(moved.bounds.size, bounds.size);
    assert_eq!(moved.content_mask.bounds, moved_mask);
    let positions: Vec<_> = moved.vertices.iter().map(|v| v.xy_position).collect();
    assert_eq!(
        positions,
        vec![
            point(ScaledPixels(17.), ScaledPixels(17.)),
            point(ScaledPixels(37.), ScaledPixels(17.)),
            point(ScaledPixels(17.), ScaledPixels(37.)),
        ]
    );
    assert!(
        moved
            .vertices
            .iter()
            .all(|v| v.content_mask.bounds == moved_mask)
    );
    assert!(
        moved
            .vertices
            .iter()
            .all(|v| v.st_position == point(0., 1.))
    );

    let underline = Underline {
        transformation: TransformationMatrix::unit(),
        order: 0,
        pad: 0,
        bounds: sp(1., 2., 3., 4.),
        content_mask: mask,
        color: crate::fast::tests::layer_support::paint_color(Hsla::red()),
        thickness: ScaledPixels(1.),
        wavy: true.into(),
    };
    let Primitive::Underline(moved) = translate(underline.into()) else {
        unreachable!()
    };
    assert_eq!(moved.bounds, sp(8., -1., 3., 4.));
    assert_eq!(moved.content_mask.bounds, moved_mask);

    // A transformation applies to window positions, `R·p + t`, so moving the
    // sprite by `delta` moves `t` to `t + (I − R)·delta`.
    let rotation = TransformationMatrix {
        rotation_scale: [[0., -1.], [1., 0.]],
        translation: [5., 6.],
    };
    let expected = TransformationMatrix {
        rotation_scale: [[0., -1.], [1., 0.]],
        // (I − R)·(7, −3) = (7, −3) − (3, 7) = (4, −10)
        translation: [5. + 4., 6. - 10.],
    };
    let mono = MonochromeSprite {
        kind: crate::scene::MonochromeSpriteKind::Glyph,
        order: 0,
        bounds: sp(1., 2., 3., 4.),
        content_mask: mask,
        color: crate::fast::tests::layer_support::paint_color(Hsla::red()),
        tile: atlas_tile(),
        transformation: rotation,
    };
    let Primitive::MonochromeSprite(moved) = translate(mono.into()) else {
        unreachable!()
    };
    assert_eq!(moved.bounds, sp(8., -1., 3., 4.));
    assert_eq!(moved.content_mask.bounds, moved_mask);
    assert_eq!(moved.transformation, expected);
    assert_eq!(moved.tile, atlas_tile());
    let window_position = |m: &TransformationMatrix, p: Point<Pixels>| m.apply(p);
    assert_eq!(
        window_position(&moved.transformation, point(px(8.), px(-1.))),
        window_position(&rotation, point(px(1.), px(2.))) + point(px(7.), px(-3.)),
    );

    let unit = MonochromeSprite {
        transformation: TransformationMatrix::unit(),
        ..mono
    };
    let Primitive::MonochromeSprite(moved) = translate(unit.into()) else {
        unreachable!()
    };
    assert_eq!(moved.transformation, TransformationMatrix::unit());

    let subpixel = SubpixelSprite {
        order: 0,
        pad: 0,
        bounds: sp(1., 2., 3., 4.),
        content_mask: mask,
        color: crate::fast::tests::layer_support::paint_color(Hsla::red()),
        tile: atlas_tile(),
        transformation: rotation,
    };
    let Primitive::SubpixelSprite(moved) = translate(subpixel.into()) else {
        unreachable!()
    };
    assert_eq!(moved.bounds, sp(8., -1., 3., 4.));
    assert_eq!(moved.content_mask.bounds, moved_mask);
    assert_eq!(moved.transformation, expected);

    let poly = PolychromeSprite {
        transformation: TransformationMatrix::unit(),
        order: 0,
        pad: 0,
        grayscale: false,
        opacity: 1.,
        bounds: sp(1., 2., 3., 4.),
        content_mask: mask,
        corner_radii: Default::default(),
        tile: atlas_tile(),
    };
    let Primitive::PolychromeSprite(moved) = translate(poly.into()) else {
        unreachable!()
    };
    assert_eq!(moved.bounds, sp(8., -1., 3., 4.));
    assert_eq!(moved.content_mask.bounds, moved_mask);
    assert_eq!(moved.tile, atlas_tile());
}

#[test]
fn a_tile_scene_keeps_overlapping_primitives_in_drawing_order() {
    let mut content = Scene::default();
    content.insert_primitive(quad(sp(10., 10., 20., 20.)));
    content.insert_primitive(quad(sp(20., 20., 20., 20.)));
    content.finish();
    let layer = layer(content);
    let t00 = layer.tile_scene(TileCoord { x: 0, y: 0 });
    assert_eq!(t00.quads.len(), 2);
    assert!(t00.quads[0].order < t00.quads[1].order);
    assert_eq!(t00.quads[1].bounds, sp(20., 20., 20., 20.));
}

/// Upstream's glyph quantization, as `Window::paint_glyph` had it.
fn old_quantize(x: f32, y: f32) -> (f32, f32, u8) {
    use crate::{SUBPIXEL_VARIANTS_X as VX, SUBPIXEL_VARIANTS_Y as VY};
    let qx = crate::util::round_half_toward_zero(x * VX as f32) / VX as f32;
    let qy = crate::util::round_half_toward_zero(y * VY as f32) / VY as f32;
    (qx.trunc(), qy.trunc(), (qx.fract() * VX as f32) as u8)
}

#[test]
fn glyph_quantization_is_unchanged_on_screen() {
    for i in 0..20_000 {
        let x = i as f32 * 0.0137;
        let y = i as f32 * 0.0291;
        let (origin, variant) =
            crate::fast::glyphs::quantize_origin(point(ScaledPixels(x), ScaledPixels(y)));
        let (ox, oy, v) = old_quantize(x, y);
        assert_eq!(
            (origin.x.0, origin.y.0, variant.x, variant.y),
            (ox, oy, v, 0),
            "at ({x}, {y})"
        );
        let emoji =
            crate::fast::glyphs::quantize_emoji_origin(point(ScaledPixels(x), ScaledPixels(y)));
        assert_eq!(
            (emoji.x.0, emoji.y.0),
            (
                crate::util::round_half_toward_zero(x),
                crate::util::round_half_toward_zero(y)
            ),
            "emoji at ({x}, {y})"
        );
    }
}

#[test]
fn glyph_quantization_moves_with_whole_pixel_shifts() {
    for i in 0..5_000 {
        let x = -300. + i as f32 * 0.0731;
        let y = -300. + i as f32 * 0.0519;
        let (a, va) = crate::fast::glyphs::quantize_origin(point(ScaledPixels(x), ScaledPixels(y)));
        let (b, vb) = crate::fast::glyphs::quantize_origin(point(
            ScaledPixels(x + 1024.),
            ScaledPixels(y + 1024.),
        ));
        assert_eq!(va, vb, "variant at ({x}, {y})");
        assert_eq!(
            (b.x.0 - a.x.0, b.y.0 - a.y.0),
            (1024., 1024.),
            "origin at ({x}, {y})"
        );
        let ea =
            crate::fast::glyphs::quantize_emoji_origin(point(ScaledPixels(x), ScaledPixels(y)));
        let eb = crate::fast::glyphs::quantize_emoji_origin(point(
            ScaledPixels(x + 1024.),
            ScaledPixels(y + 1024.),
        ));
        assert_eq!(
            (eb.x.0 - ea.x.0, eb.y.0 - ea.y.0),
            (1024., 1024.),
            "emoji at ({x}, {y})"
        );
    }
}

/// The paint stream (M3): painting a scroll container's content into a
/// layer, tile diffing, snapping, background baking and compositing.
mod paint {
    use crate::AppContext as _;
    use crate::fast::layers::tiles::{dirty_tiles, tile_hashes};
    use crate::{Bounds, ContentMask, Hsla, Quad, ScaledPixels, Scene, TileCoord, point, size};

    fn sp(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
        Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(y)),
            size: size(ScaledPixels(w), ScaledPixels(h)),
        }
    }

    fn quad(bounds: Bounds<ScaledPixels>, color: Hsla) -> Quad {
        Quad {
            bounds,
            content_mask: ContentMask::rectangular(sp(-10_000., -10_000., 20_000., 20_000.)),
            background: crate::fast::tests::layer_support::bg(color),
            ..Default::default()
        }
    }

    fn scene(quads: &[Quad]) -> Scene {
        let mut scene = Scene::default();
        for quad in quads {
            scene.insert_primitive(quad.clone());
        }
        scene.finish();
        scene
    }

    fn region() -> Bounds<ScaledPixels> {
        sp(0., 0., 1024., 1024.)
    }

    fn dirty(old: &Scene, new: &Scene) -> Vec<TileCoord> {
        dirty_tiles(
            &tile_hashes(old, 512, region()),
            &tile_hashes(new, 512, region()),
        )
    }

    fn tiles(coords: &[(i32, i32)]) -> Vec<TileCoord> {
        coords.iter().map(|&(x, y)| TileCoord { x, y }).collect()
    }

    #[test]
    fn every_tile_of_the_region_gets_a_hash_even_an_empty_one() {
        let hashes = tile_hashes(
            &scene(&[quad(sp(10., 10., 5., 5.), Hsla::red())]),
            512,
            region(),
        );
        let mut keys: Vec<_> = hashes.keys().copied().collect();
        keys.sort();
        assert_eq!(keys, tiles(&[(0, 0), (0, 1), (1, 0), (1, 1)]));
        assert_eq!(
            hashes[&TileCoord { x: 1, y: 0 }],
            hashes[&TileCoord { x: 1, y: 1 }]
        );
        assert_ne!(
            hashes[&TileCoord { x: 0, y: 0 }],
            hashes[&TileCoord { x: 1, y: 1 }]
        );
    }

    #[test]
    fn identical_scenes_dirty_no_tile() {
        let quads = [
            quad(sp(10., 10., 20., 20.), Hsla::red()),
            quad(sp(500., 600., 40., 10.), Hsla::blue()),
        ];
        assert_eq!(dirty(&scene(&quads), &scene(&quads)), Vec::new());
    }

    #[test]
    fn a_colour_change_dirties_the_tiles_the_quad_covers() {
        let before = [
            quad(sp(10., 10., 20., 20.), Hsla::red()),
            quad(sp(500., 600., 40., 10.), Hsla::blue()),
        ];
        let mut after = before.clone();
        after[1].background = crate::fast::tests::layer_support::bg(Hsla::green());
        assert_eq!(
            dirty(&scene(&before), &scene(&after)),
            tiles(&[(0, 1), (1, 1)])
        );
    }

    #[test]
    fn a_move_within_one_tile_dirties_that_tile() {
        let before = [
            quad(sp(10., 10., 20., 20.), Hsla::red()),
            quad(sp(600., 600., 10., 10.), Hsla::blue()),
        ];
        let mut after = before.clone();
        after[1].bounds = sp(640., 610., 10., 10.);
        assert_eq!(dirty(&scene(&before), &scene(&after)), tiles(&[(1, 1)]));
    }

    #[test]
    fn an_added_primitive_spanning_two_tiles_dirties_both() {
        let before = [quad(sp(10., 10., 20., 20.), Hsla::red())];
        let after = [
            quad(sp(10., 10., 20., 20.), Hsla::red()),
            quad(sp(100., 500., 10., 40.), Hsla::blue()),
        ];
        assert_eq!(
            dirty(&scene(&before), &scene(&after)),
            tiles(&[(0, 0), (0, 1)])
        );
    }

    #[test]
    fn tiles_new_to_the_region_are_dirty() {
        let quads = [quad(sp(10., 10., 20., 20.), Hsla::red())];
        let old = tile_hashes(&scene(&quads), 512, sp(0., 0., 512., 512.));
        let new = tile_hashes(&scene(&quads), 512, sp(0., 0., 512., 1024.));
        assert_eq!(dirty_tiles(&old, &new), tiles(&[(0, 1)]));
    }

    #[test]
    fn a_primitive_moved_by_whole_tiles_hashes_like_its_old_tile() {
        // Its mask moves with it, so it draws the same pixels in its tile.
        let at = |bounds: Bounds<ScaledPixels>| Quad {
            content_mask: ContentMask::rectangular(bounds),
            ..quad(bounds, Hsla::red())
        };
        let a = tile_hashes(&scene(&[at(sp(10., 10., 20., 20.))]), 512, region());
        let b = tile_hashes(&scene(&[at(sp(522., 522., 20., 20.))]), 512, region());
        assert_eq!(a[&TileCoord { x: 0, y: 0 }], b[&TileCoord { x: 1, y: 1 }]);
    }

    /// gpux primitives carry a transformation, which is part of what a tile
    /// shows: a change of it dirties the tiles the primitive covers, and one
    /// moved by whole tiles with the primitive leaves its tile as it was.
    #[test]
    fn a_transformation_is_part_of_what_a_tile_holds() {
        // A quarter turn about the quad's centre, in numbers `f32` holds
        // exactly: a transformation is `R·p + t`, about `c` with
        // `t = c - R·c`.
        let quarter_turn = crate::TransformationMatrix {
            rotation_scale: [[0., -1.], [1., 0.]],
            translation: [40., 0.],
        };
        let bounds = sp(10., 10., 20., 20.);
        let at = |transformation: crate::TransformationMatrix| Quad {
            content_mask: ContentMask::rectangular(bounds),
            transformation,
            ..quad(bounds, Hsla::red())
        };
        let elsewhere = quad(sp(600., 600., 20., 20.), Hsla::blue());
        let plain = scene(&[at(crate::TransformationMatrix::unit()), elsewhere.clone()]);
        let turned = scene(&[at(quarter_turn), elsewhere]);
        assert_eq!(dirty(&plain, &turned), tiles(&[(0, 0)]));
        assert_eq!(dirty(&turned, &turned), tiles(&[]));

        // Moved by a whole tile, the transformation moves with the quad and
        // the tile it lands in holds what its old one held.
        let moved = crate::fast::layers::scene::translate_primitive(
            &crate::scene::Primitive::Quad(at(quarter_turn)),
            point(ScaledPixels(512.), ScaledPixels(512.)),
        );
        let crate::scene::Primitive::Quad(moved) = moved else {
            unreachable!()
        };
        assert_ne!(moved.transformation, quarter_turn);
        let a = tile_hashes(&scene(&[at(quarter_turn)]), 512, region());
        let b = tile_hashes(&scene(&[moved]), 512, region());
        assert_eq!(a[&TileCoord { x: 0, y: 0 }], b[&TileCoord { x: 1, y: 1 }]);
    }

    #[test]
    fn a_mask_change_outside_a_tile_leaves_the_tile_clean() {
        // The painted region, every primitive's mask while a layer paints,
        // moves with the scroll offset: only the tiles it crosses change.
        let masked = |mask: Bounds<ScaledPixels>| {
            [
                Quad {
                    content_mask: ContentMask::rectangular(mask),
                    ..quad(sp(10., 10., 20., 900.), Hsla::red())
                },
                Quad {
                    content_mask: ContentMask::rectangular(mask),
                    ..quad(sp(600., 10., 20., 900.), Hsla::blue())
                },
            ]
        };
        let before = masked(sp(0., -100., 1024., 1000.));
        let after = masked(sp(0., -90., 1024., 1000.));
        assert_eq!(
            dirty(&scene(&before), &scene(&after)),
            tiles(&[(0, 1), (1, 1)])
        );
    }

    fn row_color(row: usize) -> Hsla {
        crate::hsla(row as f32 / 64., 0.5, 0.5, 1.)
    }

    /// A panel, white at first, holding a 100 px scroll container of `rows` rows of
    /// 20 px, each its own colour.
    struct Rows {
        rows: usize,
        panel: Hsla,
        scroll: crate::ScrollHandle,
        height: crate::Pixels,
    }

    impl crate::Render for Rows {
        fn render(
            &mut self,
            _window: &mut crate::Window,
            _cx: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            use crate::{
                InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _,
                Styled as _,
            };
            crate::div().size_full().bg(self.panel).child(
                crate::div()
                    .id("s")
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .h(self.height)
                    .children(
                        (0..self.rows).map(|i| crate::div().h(crate::px(20.)).bg(row_color(i))),
                    ),
            )
        }
    }

    fn rows_window(cx: &mut crate::TestAppContext, rows: usize) -> crate::WindowHandle<Rows> {
        rows_window_of_height(cx, rows, crate::px(100.))
    }

    fn rows_window_of_height(
        cx: &mut crate::TestAppContext,
        rows: usize,
        height: crate::Pixels,
    ) -> crate::WindowHandle<Rows> {
        let window = crate::fast::tests::layer_support::add_window(cx, |_, _| Rows {
            rows,
            panel: crate::white(),
            scroll: crate::ScrollHandle::new(),
            height,
        });
        // The first frame redraws everything, which never uses layers.
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        window
    }

    /// Draws `window` with its view rebuilt and every scroll container made
    /// to decide `decision`.
    fn draw_deciding(
        cx: &mut crate::TestAppContext,
        window: crate::WindowHandle<Rows>,
        decision: crate::fast::layers::policy::Decision,
    ) {
        window
            .update(cx, |_, window, cx| {
                window.fast_layers.forced_decision = Some(decision);
                cx.notify();
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    }

    fn with_record<R>(
        cx: &mut crate::TestAppContext,
        window: crate::WindowHandle<Rows>,
        f: impl FnOnce(&crate::fast::layers::record::LayerRecord, &crate::Window) -> R,
    ) -> R {
        cx.update_window(window.into(), |_, window, _| {
            assert_eq!(window.fast_layers.layers.len(), 1, "one layer");
            let layer = window.fast_layers.layers.values().next().unwrap();
            f(
                layer.record.as_ref().expect("the layer was painted"),
                window,
            )
        })
        .unwrap()
    }

    fn row_quads(scene: &Scene, rows: usize) -> Vec<(usize, Bounds<ScaledPixels>)> {
        scene
            .quads
            .iter()
            .filter_map(|quad| {
                (0..rows)
                    .find(|&row| {
                        quad.background == crate::fast::tests::layer_support::bg(row_color(row))
                    })
                    .map(|row| (row, quad.bounds))
            })
            .collect()
    }

    #[crate::test]
    fn a_repainted_layer_holds_the_content_in_content_space(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = rows_window(cx, 40);
        draw_deciding(cx, window, crate::fast::layers::policy::Decision::Repaint);

        let main_rows = cx
            .update_window(window.into(), |_, window, _| {
                row_quads(&window.rendered_frame.scene, 40)
            })
            .unwrap();
        assert_eq!(main_rows, Vec::new(), "the rows are painted into the layer");

        let scale = cx
            .update_window(window.into(), |_, window, _| window.scale_factor())
            .unwrap();
        with_record(cx, window, |record, _| {
            // The viewport, 100 px, and two viewports of overscan below it;
            // nothing above, at the top.
            assert_eq!(record.viewport.size.height, crate::px(100.));
            assert_eq!(record.painted_region.origin.y, crate::px(0.));
            assert_eq!(record.painted_region.size.height, crate::px(300.));
            let rows = row_quads(record.content.scene().unwrap(), 40);
            assert_eq!(
                rows.iter().map(|(row, _)| *row).collect::<Vec<_>>(),
                (0..15).collect::<Vec<_>>()
            );
            for (row, bounds) in rows {
                assert_eq!(bounds.origin.y, ScaledPixels(row as f32 * 20. * scale));
                assert_eq!(bounds.size.height, ScaledPixels(20. * scale));
            }
            assert_eq!(record.generation, 1);
            let mut tiles: Vec<_> = record.tile_hashes.keys().copied().collect();
            tiles.sort();
            // 300 px at the test window's scale of 2 is 600 device px: two
            // tiles high.
            assert!(tiles.iter().all(|tile| tile.y <= 1), "{tiles:?}");
            assert!(tiles.iter().any(|tile| tile.y == 1), "{tiles:?}");
            assert_eq!(
                record.dirty_tiles, tiles,
                "a first paint dirties every tile"
            );
        });
    }

    #[crate::test]
    fn scrolled_content_is_stored_where_it_was_before_the_scroll(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = rows_window(cx, 40);
        window
            .read_with(cx, |view, _| {
                crate::fast::tests::layer_support::set_offset(
                    &view.scroll,
                    crate::point(crate::px(0.), crate::px(-300.)),
                )
            })
            .unwrap();
        draw_deciding(cx, window, crate::fast::layers::policy::Decision::Repaint);
        let scale = cx
            .update_window(window.into(), |_, window, _| window.scale_factor())
            .unwrap();
        with_record(cx, window, |record, _| {
            // Overscan of two viewports above and below the one at 300..400.
            assert_eq!(record.painted_region.origin.y, crate::px(-200.));
            assert_eq!(record.painted_region.size.height, crate::px(500.));
            let rows = row_quads(record.content.scene().unwrap(), 40);
            assert_eq!(
                rows.iter().map(|(row, _)| *row).collect::<Vec<_>>(),
                (5..30).collect::<Vec<_>>()
            );
            for (row, bounds) in rows {
                assert_eq!(bounds.origin.y, ScaledPixels(row as f32 * 20. * scale));
            }
        });
    }

    /// A renderer keeps a layer's tiles by key and generation. A record
    /// dropped (content that could not be composited, a demotion, a
    /// background that could not be baked) and painted again must not come
    /// back at a generation the renderer holds tiles of, or those stale
    /// tiles would be shown.
    #[crate::test]
    fn a_layer_painted_again_after_its_record_was_dropped_gets_a_new_generation(
        cx: &mut crate::TestAppContext,
    ) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        use crate::fast::layers::policy::Decision;
        let window = rows_window(cx, 40);
        draw_deciding(cx, window, Decision::Repaint);
        let first = with_record(cx, window, |record, _| record.generation);
        cx.update_window(window.into(), |_, window, _| {
            for layer in window.fast_layers.layers.values_mut() {
                layer.record = None;
            }
        })
        .unwrap();
        draw_deciding(cx, window, Decision::Repaint);
        let (second, dirty, tiles) = with_record(cx, window, |record, _| {
            (
                record.generation,
                record.dirty_tiles.len(),
                record.tile_hashes.len(),
            )
        });
        assert!(second > first, "generation {second} after {first}");
        assert_eq!(dirty, tiles, "every tile is dirty");
        let framed = cx
            .update_window(window.into(), |_, window, _| {
                window
                    .rendered_frame
                    .scene
                    .layers
                    .frames
                    .iter()
                    .map(|frame| frame.generation)
                    .collect::<Vec<_>>()
            })
            .unwrap();
        assert_eq!(
            framed,
            vec![second],
            "the frame hands the renderer the new generation"
        );
    }

    #[crate::test]
    fn a_repaint_at_a_shifted_offset_dirties_only_the_edge_tiles(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        use crate::fast::layers::policy::Decision;
        // Enough viewport for the painted region to span many tiles.
        let window = rows_window_of_height(cx, 400, crate::px(1000.));
        let scroll_to = |cx: &mut crate::TestAppContext, y: f32| {
            window
                .read_with(cx, |view, _| {
                    crate::fast::tests::layer_support::set_offset(
                        &view.scroll,
                        crate::point(crate::px(0.), crate::px(y)),
                    )
                })
                .unwrap();
            draw_deciding(cx, window, Decision::Repaint);
        };
        scroll_to(cx, -3000.);
        scroll_to(cx, -3010.);
        with_record(cx, window, |record, _| {
            let rows: Vec<i32> = record.tile_hashes.keys().map(|tile| tile.y).collect();
            let (top, bottom) = (*rows.iter().min().unwrap(), *rows.iter().max().unwrap());
            assert!(bottom - top >= 4, "the region spans many tile rows");
            assert!(!record.dirty_tiles.is_empty(), "the region's edges moved");
            for tile in &record.dirty_tiles {
                assert!(
                    tile.y == top || tile.y == bottom,
                    "tile {tile:?} inside the region ({top}..={bottom}) is dirty"
                );
            }
        });
        // The same content at the same offset dirties nothing.
        scroll_to(cx, -3010.);
        with_record(cx, window, |record, _| {
            assert_eq!(record.dirty_tiles, Vec::new());
        });
    }

    #[crate::test]
    fn overscan_is_clamped_to_content(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        // 60 px of rows in the 100 px viewport: nothing to scroll to.
        let window = rows_window(cx, 3);
        draw_deciding(cx, window, crate::fast::layers::policy::Decision::Repaint);
        with_record(cx, window, |record, window| {
            assert_eq!(
                record.painted_region, record.viewport,
                "no overscan past the content"
            );
            assert_eq!(row_quads(record.content.scene().unwrap(), 3).len(), 3);
            let region = record.painted_region.scale(window.scale_factor());
            for tile in record.tile_hashes.keys() {
                let top = tile.y as f32 * 512.;
                assert!(
                    top < region.bottom_right().y.0,
                    "tile {tile:?} below the content"
                );
            }
        });
    }

    fn bake(quads: &[Quad], window_opaque: bool) -> Option<crate::PaintColor> {
        crate::fast::layers::background::bake(
            &scene(quads),
            sp(100., 100., 200., 300.),
            window_opaque,
        )
    }

    #[test]
    fn an_opaque_panel_under_the_viewport_is_baked() {
        let panel = quad(sp(0., 0., 1000., 1000.), Hsla::blue());
        assert_eq!(
            bake(std::slice::from_ref(&panel), true),
            Some(crate::fast::tests::layer_support::paint_color(Hsla::blue()))
        );
        // Something painted later away from the viewport changes nothing.
        let elsewhere = quad(sp(500., 500., 10., 10.), Hsla::red());
        assert_eq!(
            bake(&[panel.clone(), elsewhere], true),
            Some(crate::fast::tests::layer_support::paint_color(Hsla::blue()))
        );
        // The topmost panel is the one baked.
        let above = quad(sp(50., 50., 400., 400.), Hsla::green());
        assert_eq!(
            bake(&[panel, above], true),
            Some(crate::fast::tests::layer_support::paint_color(Hsla::green()))
        );
    }

    #[test]
    fn only_a_solid_opaque_panel_covering_the_viewport_is_baked() {
        let panel = quad(sp(0., 0., 1000., 1000.), Hsla::blue());
        assert_eq!(
            bake(std::slice::from_ref(&panel), false),
            None,
            "transparent window"
        );

        let gradient = Quad {
            background: crate::fast::tests::layer_support::bg(crate::linear_gradient(
                0.,
                [
                    crate::linear_color_stop(Hsla::red(), 0.),
                    crate::linear_color_stop(Hsla::blue(), 1.),
                ],
            )),
            ..panel.clone()
        };
        assert_eq!(bake(&[gradient], true), None, "gradient");

        let translucent = quad(sp(0., 0., 1000., 1000.), Hsla::blue().opacity(0.5));
        assert_eq!(bake(&[translucent], true), None, "translucent");

        let partly_inside = quad(sp(250., 250., 100., 100.), Hsla::red());
        assert_eq!(
            bake(&[panel.clone(), partly_inside], true),
            None,
            "partly covered"
        );

        let short = quad(sp(0., 0., 1000., 350.), Hsla::blue());
        assert_eq!(bake(&[short], true), None, "does not cover the viewport");

        let clipped = Quad {
            content_mask: ContentMask::rectangular(sp(0., 0., 1000., 200.)),
            ..panel.clone()
        };
        assert_eq!(
            bake(&[clipped], true),
            None,
            "clipped short of the viewport"
        );

        let rounded_inside = Quad {
            bounds: sp(90., 90., 500., 500.),
            corner_radii: crate::Corners::all(ScaledPixels(20.)),
            ..panel.clone()
        };
        assert_eq!(bake(&[rounded_inside], true), None, "corner inside");
        let rounded_outside = Quad {
            bounds: sp(0., 0., 1000., 1000.),
            corner_radii: crate::Corners::all(ScaledPixels(20.)),
            ..panel.clone()
        };
        assert_eq!(
            bake(&[rounded_outside], true),
            Some(crate::fast::tests::layer_support::paint_color(Hsla::blue())),
            "corners away from the viewport"
        );

        let bordered = Quad {
            bounds: sp(95., 0., 1000., 1000.),
            border_widths: crate::Edges::all(ScaledPixels(10.)),
            border_color: crate::fast::tests::layer_support::bg(Hsla::red()),
            ..panel
        };
        assert_eq!(
            bake(std::slice::from_ref(&bordered), true),
            None,
            "border inside"
        );
        let transparent_border = Quad {
            border_color: crate::fast::tests::layer_support::bg(Hsla::transparent_black()),
            ..bordered
        };
        assert_eq!(
            bake(&[transparent_border], true),
            Some(crate::fast::tests::layer_support::paint_color(Hsla::blue())),
            "an invisible border"
        );
    }

    /// Draws `window` over a panel of colour `panel`, every scroll container
    /// made to decide `decision`.
    fn draw_over(
        cx: &mut crate::TestAppContext,
        window: crate::WindowHandle<Rows>,
        panel: Hsla,
        decision: crate::fast::layers::policy::Decision,
    ) {
        window
            .update(cx, |view, window, cx| {
                view.panel = panel;
                window.fast_layers.forced_decision = Some(decision);
                cx.notify();
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    }

    #[crate::test]
    fn a_layer_bakes_the_panel_under_it(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = rows_window(cx, 40);
        draw_deciding(cx, window, crate::fast::layers::policy::Decision::Repaint);
        with_record(cx, window, |record, _| {
            assert_eq!(
                record.background,
                crate::fast::tests::layer_support::paint_color(crate::white())
            );
        });
    }

    #[crate::test]
    fn background_change_repaints_layer(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        use crate::fast::layers::policy::Decision;
        let window = rows_window(cx, 40);
        draw_deciding(cx, window, Decision::Repaint);
        let (generation, tiles) = with_record(cx, window, |record, _| {
            let mut tiles: Vec<_> = record.tile_hashes.keys().copied().collect();
            tiles.sort();
            (record.generation, tiles)
        });

        // Painted again with nothing changed but the panel: every tile is
        // cleared with another colour, so every tile is dirty.
        draw_over(cx, window, Hsla::blue(), Decision::Repaint);
        let generation = with_record(cx, window, |record, _| {
            assert_eq!(
                record.background,
                crate::fast::tests::layer_support::paint_color(Hsla::blue())
            );
            assert_eq!(record.generation, generation + 1);
            assert_eq!(record.dirty_tiles, tiles);
            record.generation
        });

        // Composited over another panel: the content stands, the tiles are
        // cleared with the new colour.
        draw_over(cx, window, Hsla::green(), Decision::Composite);
        with_record(cx, window, |record, _| {
            assert_eq!(
                record.background,
                crate::fast::tests::layer_support::paint_color(Hsla::green())
            );
            assert_eq!(record.generation, generation + 1);
            assert_eq!(record.dirty_tiles, tiles);
        });

        // Nothing changed: the same tiles, nothing dirty.
        draw_deciding(cx, window, Decision::Repaint);
        with_record(cx, window, |record, _| {
            assert_eq!(
                record.background,
                crate::fast::tests::layer_support::paint_color(Hsla::green())
            );
            assert_eq!(record.dirty_tiles, Vec::new());
        });
    }

    #[crate::test]
    fn no_layer_is_painted_over_a_background_it_cannot_bake(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = rows_window(cx, 40);
        draw_over(
            cx,
            window,
            Hsla::blue().opacity(0.5),
            crate::fast::layers::policy::Decision::Repaint,
        );
        cx.update_window(window.into(), |_, window, _| {
            assert_eq!(
                row_quads(&window.rendered_frame.scene, 40).len(),
                5,
                "the rows are painted as without layers"
            );
            assert!(
                window
                    .fast_layers
                    .layers
                    .values()
                    .all(|layer| layer.record.is_none())
            );
        })
        .unwrap();
    }

    /// The layer tiles `scene` composites, with the layer each belongs to.
    fn tile_quads(scene: &Scene) -> Vec<(crate::LayerKey, TileCoord, crate::PolychromeSprite)> {
        scene
            .polychrome_sprites
            .iter()
            .filter_map(|sprite| {
                crate::decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id)
                    .map(|(key, coord)| (key, coord, *sprite))
            })
            .collect()
    }

    #[crate::test]
    fn tile_quads_cover_the_viewport_at_the_current_offset(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        use crate::fast::layers::policy::Decision;
        let window = rows_window(cx, 40);
        draw_deciding(cx, window, Decision::Repaint);
        window
            .read_with(cx, |view, _| {
                crate::fast::tests::layer_support::set_offset(
                    &view.scroll,
                    crate::point(crate::px(0.), crate::px(-130.)),
                )
            })
            .unwrap();
        draw_deciding(cx, window, Decision::Composite);

        cx.update_window(window.into(), |_, window, _| {
            let scale = window.scale_factor();
            let layer = window.fast_layers.layers.values().next().expect("a layer");
            let record = layer.record.as_ref().expect("painted");
            let scene = &window.rendered_frame.scene;
            assert_eq!(row_quads(scene, 40), Vec::new(), "the rows are not drawn");

            // The viewport, snapped as every content mask is.
            let viewport = record.viewport;
            let floor = |v: crate::Pixels| crate::util::floor_to_device_pixel(v.0, scale);
            let ceil = |v: crate::Pixels| crate::util::ceil_to_device_pixel(v.0, scale);
            let mask = Bounds::from_corners(
                point(
                    ScaledPixels(floor(viewport.left())),
                    ScaledPixels(floor(viewport.top())),
                ),
                point(
                    ScaledPixels(ceil(viewport.right())),
                    ScaledPixels(ceil(viewport.bottom())),
                ),
            );
            let translation = point(ScaledPixels(0.), ScaledPixels(-130. * scale));

            assert_eq!(scene.layers.frames.len(), 1);
            let frame = &scene.layers.frames[0];
            assert_eq!(frame.key, layer.key);
            assert!(frame.content.ptr_eq(&record.content));
            assert_eq!(frame.generation, record.generation);
            assert_eq!(frame.background, record.background);
            assert_eq!(frame.tile_size, 512);
            assert_eq!(frame.dirty_tiles, record.dirty_tiles);

            let quads = tile_quads(scene);
            let mut expected = Vec::new();
            let content_viewport = Bounds {
                origin: mask.origin - translation,
                size: mask.size,
            };
            for y in -4..4 {
                for x in -4..8 {
                    let tile = TileCoord { x, y };
                    if frame.tile_bounds(tile).intersects(&content_viewport) {
                        expected.push(tile);
                    }
                }
            }
            let mut coords: Vec<_> = quads.iter().map(|(_, coord, _)| *coord).collect();
            coords.sort();
            assert!(!expected.is_empty());
            assert_eq!(coords, expected);
            let order = quads[0].2.order;
            for (key, coord, sprite) in &quads {
                assert_eq!(*key, layer.key);
                let tile = frame.tile_bounds(*coord);
                assert_eq!(
                    sprite.bounds,
                    Bounds {
                        origin: tile.origin + translation,
                        size: tile.size,
                    }
                );
                assert_eq!(sprite.content_mask.bounds, mask);
                assert_eq!(sprite.order, order, "the tiles share one draw order");
                assert_eq!(sprite.opacity, 1.);
                assert_eq!(sprite.tile.bounds.size.width.0, 512);
                assert_eq!(sprite.tile.bounds.size.height.0, 512);
                assert_eq!(sprite.tile.bounds.origin, crate::Point::default());
            }
        })
        .unwrap();
    }

    #[crate::test]
    fn a_repainted_layer_is_composited_where_it_was_painted(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = rows_window(cx, 40);
        cx.update_window(window.into(), |_, window, _| window.reset_fast_stats())
            .unwrap();
        draw_deciding(cx, window, crate::fast::layers::policy::Decision::Repaint);
        cx.update_window(window.into(), |_, window, _| {
            let scene = &window.rendered_frame.scene;
            let quads = tile_quads(scene);
            assert!(!quads.is_empty());
            for (_, coord, sprite) in &quads {
                let tile = scene.layers.frames[0].tile_bounds(*coord);
                assert_eq!(sprite.bounds, tile, "no translation at offset 0");
            }
            let stats = window.fast_stats();
            // Painted into its layer and shown from its tiles: counted once.
            assert_eq!(stats.layers_repainted, 1);
            assert_eq!(stats.layers_composited, 0);
            assert!(stats.tiles_composited > 0);
            assert_eq!(
                stats.tiles_rasterized,
                scene.layers.frames[0].dirty_tiles.len() as u64
            );
        })
        .unwrap();
    }

    #[crate::test]
    fn a_reused_view_keeps_compositing_its_layer(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = rows_window(cx, 40);
        draw_deciding(cx, window, crate::fast::layers::policy::Decision::Repaint);
        let before = cx
            .update_window(window.into(), |_, window, _| {
                tile_quads(&window.rendered_frame.scene).len()
            })
            .unwrap();
        // Nothing changed: the view is reused, its scene replayed.
        cx.update_window(window.into(), |_, window, cx| {
            window.reset_fast_stats();
            window.draw(cx).clear(cx)
        })
        .unwrap();
        cx.update_window(window.into(), |_, window, _| {
            let scene = &window.rendered_frame.scene;
            assert_eq!(tile_quads(scene).len(), before);
            assert_eq!(scene.layers.frames.len(), 1, "the layer frame comes along");
            let stats = window.fast_stats();
            assert_eq!(stats.layers_repainted, 0, "the view was reused");
            assert!(stats.views_reused > 0, "the view was reused");
        })
        .unwrap();
    }

    #[crate::test]
    fn a_composited_layer_over_a_background_it_cannot_bake_is_drawn_into_the_frame(
        cx: &mut crate::TestAppContext,
    ) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        use crate::fast::layers::policy::Decision;
        let window = rows_window(cx, 40);
        let direct = cx
            .update_window(window.into(), |_, window, _| {
                row_quads(&window.rendered_frame.scene, 40)
            })
            .unwrap();
        draw_deciding(cx, window, Decision::Repaint);
        draw_over(cx, window, Hsla::blue().opacity(0.5), Decision::Composite);
        cx.update_window(window.into(), |_, window, _| {
            let scene = &window.rendered_frame.scene;
            assert!(tile_quads(scene).is_empty());
            assert!(scene.layers.frames.is_empty());
            assert_eq!(row_quads(scene, 40), direct, "the rows as without a layer");
            let layer = window.fast_layers.layers.values().next().expect("a layer");
            assert!(layer.record.is_none(), "painted afresh next time");
        })
        .unwrap();
    }

    #[crate::test]
    fn a_layer_painted_while_scrolled_is_drawn_where_it_shows_over_a_background_it_cannot_bake(
        cx: &mut crate::TestAppContext,
    ) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        use crate::fast::layers::policy::Decision;
        let window = rows_window(cx, 40);
        let scroll_to = |cx: &mut crate::TestAppContext, y: f32| {
            window
                .read_with(cx, |view, _| {
                    crate::fast::tests::layer_support::set_offset(
                        &view.scroll,
                        point(crate::px(0.), crate::px(y)),
                    );
                })
                .unwrap();
        };
        scroll_to(cx, -60.);
        draw_deciding(cx, window, Decision::Bypass);
        let direct = cx
            .update_window(window.into(), |_, window, _| {
                row_quads(&window.rendered_frame.scene, 40)
            })
            .unwrap();

        scroll_to(cx, -40.);
        draw_deciding(cx, window, Decision::Repaint);
        scroll_to(cx, -60.);
        draw_over(cx, window, Hsla::blue().opacity(0.5), Decision::Composite);
        cx.update_window(window.into(), |_, window, _| {
            let scene = &window.rendered_frame.scene;
            assert!(tile_quads(scene).is_empty());
            let drawn: Vec<_> = row_quads(scene, 40)
                .into_iter()
                .filter(|(row, _)| direct.iter().any(|(direct, _)| direct == row))
                .collect();
            assert_eq!(drawn, direct, "the rows where they show at the new offset");
        })
        .unwrap();
    }

    /// A scroll container whose content paints a path under a row.
    struct PathRows;

    impl crate::Render for PathRows {
        fn render(
            &mut self,
            _window: &mut crate::Window,
            _cx: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            use crate::{
                InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _,
                Styled as _,
            };
            crate::div().size_full().bg(crate::white()).child(
                crate::div()
                    .id("s")
                    .overflow_y_scroll()
                    .h(crate::px(100.))
                    .child(
                        crate::canvas(
                            |_, _, _| {},
                            |bounds, _, window, _| {
                                let origin = bounds.origin;
                                let mut path = crate::Path::new(origin);
                                path.line_to(origin + crate::point(crate::px(30.), crate::px(0.)));
                                path.line_to(origin + crate::point(crate::px(0.), crate::px(30.)));
                                window.paint_path(path, crate::black());
                            },
                        )
                        .h(crate::px(40.))
                        .w_full(),
                    )
                    .children((0..20).map(|i| crate::div().h(crate::px(20.)).bg(row_color(i)))),
            )
        }
    }

    /// Paths are never composited from tiles (spec §5.6), but a path nothing
    /// in the content draws over is drawn into the frame over the layer's
    /// tiles, where it lands on the same pixels as drawn without a layer.
    #[crate::test]
    fn paths_nothing_draws_over_are_drawn_over_the_tiles(cx: &mut crate::TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = crate::fast::tests::layer_support::add_window(cx, |_, _| PathRows);
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (direct_paths, direct) = cx
            .update_window(window.into(), |_, window, _| {
                let scene = &window.rendered_frame.scene;
                (scene.paths.len(), crate::fast::layers::verify::drawn(scene))
            })
            .unwrap();
        assert_eq!(direct_paths, 1);

        window
            .update(cx, |_, window, cx| {
                window.fast_layers.forced_decision =
                    Some(crate::fast::layers::policy::Decision::Repaint);
                window.reset_fast_stats();
                cx.notify();
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        cx.update_window(window.into(), |_, window, _| {
            let stats = window.fast_stats();
            // Painted into its layer and shown from its tiles: counted once.
            assert_eq!(stats.layers_repainted, 1);
            assert_eq!(stats.layers_composited, 0);
            assert!(stats.tiles_composited > 0);
            let layer = window.fast_layers.layers.values().next().expect("a layer");
            let record = layer.record.as_ref().expect("painted");
            assert!(!record.has_paths);
            assert!(
                record.content.scene().unwrap().paths.is_empty(),
                "the tiles hold no path"
            );
            assert_eq!(record.paths.len(), 1, "the layer keeps the path apart");
            let scene = &window.rendered_frame.scene;
            assert!(!tile_quads(scene).is_empty(), "the tiles are composited");
            assert_eq!(scene.layers.frames.len(), 1);
            assert_eq!(scene.paths.len(), 1, "the path is in the frame");
            assert!(
                scene.paths[0].order > tile_quads(scene)[0].2.order,
                "the path is drawn over the tiles"
            );
            assert!(row_quads(scene, 20).is_empty(), "the rows are in the tiles");
            assert_eq!(
                crate::fast::layers::verify::drawn(scene),
                direct,
                "the frame draws what it drew without a layer"
            );
        })
        .unwrap();
    }
}

mod footprints {
    use super::{atlas_tile, layer, sp, wide_mask};
    use crate::fast::layers::tiles::{dirty_tiles, tile_hashes};
    use crate::{
        Hsla, MonochromeSprite, Radians, Scene, Shadow, TileCoord, TransformationMatrix, point,
    };

    fn shadow(color: Hsla) -> Shadow {
        // Its blur reaches x = 500 + 3 * 10 = 530, inside tile (1, 0).
        Shadow {
            transformation: TransformationMatrix::unit(),
            order: 0,
            blur_radius: crate::ScaledPixels(10.),
            bounds: sp(400., 100., 100., 100.),
            corner_radii: Default::default(),
            content_mask: wide_mask(),
            color: crate::fast::tests::layer_support::paint_color(color),
            element_bounds: sp(400., 100., 100., 100.),
            element_corner_radii: Default::default(),
            inset: 0,
            pad: 0,
        }
    }

    fn rotated_sprite(color: Hsla) -> MonochromeSprite {
        // A 70 px square ending 2 px short of tile (1, 0), turned an eighth
        // around its centre (475, 135): its corners reach x = 524.5.
        MonochromeSprite {
            kind: crate::scene::MonochromeSpriteKind::Glyph,
            order: 0,
            bounds: sp(440., 100., 70., 70.),
            content_mask: wide_mask(),
            color: crate::fast::tests::layer_support::paint_color(color),
            tile: atlas_tile(),
            transformation: TransformationMatrix::unit()
                .translate(point(crate::ScaledPixels(475.), crate::ScaledPixels(135.)))
                .rotate(Radians(std::f32::consts::FRAC_PI_4))
                .translate(point(
                    crate::ScaledPixels(-475.),
                    crate::ScaledPixels(-135.),
                )),
        }
    }

    fn scene_of(primitive: impl Into<crate::scene::Primitive>) -> Scene {
        let mut scene = Scene::default();
        scene.insert_primitive(primitive);
        scene.finish();
        scene
    }

    fn dirty(old: &Scene, new: &Scene) -> Vec<TileCoord> {
        let region = sp(0., 0., 1024., 512.);
        dirty_tiles(
            &tile_hashes(old, 512, region),
            &tile_hashes(new, 512, region),
        )
    }

    const BOTH: [TileCoord; 2] = [TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 0 }];

    #[test]
    fn a_shadow_change_dirties_every_tile_its_blur_reaches() {
        assert_eq!(
            dirty(
                &scene_of(shadow(Hsla::red())),
                &scene_of(shadow(Hsla::blue()))
            ),
            BOTH
        );
    }

    #[test]
    fn a_tile_scene_holds_a_shadow_whose_blur_reaches_the_tile() {
        let frame = layer(scene_of(shadow(Hsla::red())));
        assert_eq!(frame.tile_scene(TileCoord { x: 1, y: 0 }).shadows.len(), 1);
        assert!(
            frame
                .tile_scene(TileCoord { x: 1, y: 1 })
                .shadows
                .is_empty()
        );
    }

    #[test]
    fn a_shadow_masked_off_a_tile_leaves_it_alone() {
        let masked = |color| Shadow {
            content_mask: crate::ContentMask::rectangular(sp(0., 0., 512., 512.)),
            ..shadow(color)
        };
        assert_eq!(
            dirty(
                &scene_of(masked(Hsla::red())),
                &scene_of(masked(Hsla::blue()))
            ),
            [TileCoord { x: 0, y: 0 }]
        );
    }

    #[test]
    fn a_rotated_sprite_change_dirties_every_tile_it_turns_into() {
        assert_eq!(
            dirty(
                &scene_of(rotated_sprite(Hsla::red())),
                &scene_of(rotated_sprite(Hsla::blue()))
            ),
            BOTH
        );
        let frame = layer(scene_of(rotated_sprite(Hsla::red())));
        assert_eq!(
            frame
                .tile_scene(TileCoord { x: 1, y: 0 })
                .monochrome_sprites
                .len(),
            1
        );
    }
}

mod integration {

    use crate::{
        AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
        TestAppContext, Window, div, px,
    };

    /// One line of 8 px text on a 40 px line, whose top is `top`.
    struct TallLine {
        top: f32,
    }

    impl Render for TallLine {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .absolute()
                    .top(px(self.top))
                    .text_size(px(8.))
                    .line_height(px(40.))
                    .child("A"),
            )
        }
    }

    fn glyphs_drawn(top: f32) -> usize {
        let mut cx = crate::fast::tests::oracle_support::oracle_app(0);
        let cx = &mut cx;
        let window = crate::fast::tests::layer_support::add_window(cx, |_, _| TallLine { top });
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.rendered_frame.scene.monochrome_sprites.len()
                + window.rendered_frame.scene.subpixel_sprites.len()
        })
        .unwrap()
    }

    /// Content whose path nothing draws over composites its tiles with the
    /// path drawn over them, and draws what it draws without a layer at every
    /// offset, odd numbers of device pixels included.
    #[crate::test]
    fn a_path_drawn_over_the_tiles_matches_drawing_without_layers(cx: &mut TestAppContext) {
        path_over_tiles_matches_drawing_without_layers(cx, 1.);
    }

    #[crate::test]
    fn a_path_drawn_over_the_tiles_matches_drawing_without_layers_at_a_fractional_scale(
        cx: &mut TestAppContext,
    ) {
        path_over_tiles_matches_drawing_without_layers(cx, 1.25);
        path_over_tiles_matches_drawing_without_layers(cx, 1.6);
    }

    fn path_over_tiles_matches_drawing_without_layers(cx: &mut TestAppContext, scale_factor: f32) {
        use crate::fast::tests::layer_support::{LayerPage, new_page};
        use crate::{Path, canvas, point, size};
        let composited = matches_drawing_without_layers(cx, scale_factor, |cx| LayerPage {
            extra: Some(std::rc::Rc::new(|| {
                canvas(
                    |_, _, _| {},
                    |bounds, _, window, _| {
                        // Two paths, the second over the first, as a progress
                        // circle draws its track and its arc.
                        for (inset, color) in [(0., crate::red()), (3., crate::green())] {
                            let origin = bounds.origin + point(px(20. + inset), px(70.));
                            let mut path = Path::new(origin);
                            path.line_to(origin + point(px(12.), px(0.)));
                            path.curve_to(
                                origin + point(px(0.), px(12.)),
                                origin + point(px(14.), px(14.)),
                            );
                            path.line_to(origin);
                            window.paint_path(path, color);
                        }
                        window.paint_quad(crate::fill(
                            crate::Bounds::new(bounds.origin, size(px(10.), px(10.))),
                            crate::blue(),
                        ));
                    },
                )
                .w(px(100.))
                .h(px(100.))
                .into_any_element()
            })),
            ..new_page(false, cx)
        });
        assert!(composited >= 6, "composited {composited} frames");
    }

    /// A scrollbar beside the container, which reads the offset where it is
    /// prepainted and painted, moves with the offset while the content is
    /// composited, as it does without layers.
    #[crate::test]
    fn a_scrollbar_beside_a_composited_layer_matches_drawing_without_layers(
        cx: &mut TestAppContext,
    ) {
        use crate::fast::tests::layer_support::{LayerPage, new_page};
        for scale_factor in [1., 1.6] {
            let composited = matches_drawing_without_layers(cx, scale_factor, |cx| LayerPage {
                scrollbar: true,
                ..new_page(false, cx)
            });
            assert!(composited >= 6, "composited {composited} frames");
        }
    }

    /// Scrolls a window with layers and one without, each holding the page
    /// `page` builds, at `scale_factor`, and checks after every scroll that
    /// they draw the same. Returns how many frames composited a layer.
    fn matches_drawing_without_layers<V: crate::Render>(
        cx: &mut TestAppContext,
        scale_factor: f32,
        page: impl Fn(&mut crate::App) -> V,
    ) -> u64 {
        stats_matching_drawing_without_layers(cx, scale_factor, page)
            .map_or(u64::MAX, |stats| stats.layers_composited)
    }

    /// [`matches_drawing_without_layers`], returning what the window with
    /// layers counted, where layers are compiled.
    pub(super) fn stats_matching_drawing_without_layers<V: crate::Render>(
        cx: &mut TestAppContext,
        scale_factor: f32,
        page: impl Fn(&mut crate::App) -> V,
    ) -> Option<crate::FastStats> {
        use crate::point;
        if !crate::fast::layers::COMPILED {
            return None;
        }
        let mut open = |cx: &mut TestAppContext, layers: bool| {
            let window = crate::fast::tests::layer_support::add_window(cx, |_, cx| page(cx));
            cx.simulate_window_scale_factor_change(window.into(), scale_factor);
            cx.update_window(window.into(), |_, window, cx| {
                // The window without layers draws every frame from scratch,
                // and snaps what one with them snaps.
                if layers {
                    crate::fast::tests::layer_support::set_layers(window, true);
                } else {
                    crate::fast::tests::layer_support::make_reference(window);
                }
                window.draw(cx).clear(cx);
            })
            .unwrap();
            window
        };
        let layered = open(cx, true);
        let plain = open(cx, false);
        let drawn = |cx: &mut TestAppContext, window: crate::WindowHandle<V>| {
            cx.update_window(window.into(), |_, window, _| window.painted_primitives())
                .unwrap()
        };
        for (step, dy) in [-20., -20., -7., -3., -11., -1., 5., -13., -2.]
            .into_iter()
            .enumerate()
        {
            for window in [layered, plain] {
                cx.update_window(window.into(), |_, window, cx| {
                    window.dispatch_event(
                        crate::PlatformInput::ScrollWheel(crate::ScrollWheelEvent {
                            position: point(px(20.), px(20.)),
                            delta: crate::ScrollDelta::Pixels(point(px(0.), px(dy))),
                            modifiers: Default::default(),
                            touch_phase: crate::TouchPhase::Moved,
                        }),
                        cx,
                    );
                    window.draw(cx).clear(cx);
                })
                .unwrap();
            }
            let (actual, expected) = (drawn(cx, layered), drawn(cx, plain));
            if actual != expected {
                let only = |a: &[String], b: &[String]| -> Vec<String> {
                    a.iter().filter(|line| !b.contains(line)).cloned().collect()
                };
                panic!(
                    "scale {scale_factor}, step {step}, scrolled by {dy}: only with layers \
                     {:#?}, only without {:#?}",
                    only(&actual, &expected),
                    only(&expected, &actual)
                );
            }
        }
        Some(
            cx.update_window(layered.into(), |_, window, _| window.fast_stats())
                .unwrap(),
        )
    }

    /// Rows in a view of their own, counting how often it renders.
    struct CountedRows {
        renders: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Render for CountedRows {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use crate::rgb;
            self.renders.set(self.renders.get() + 1);
            div().children(
                (0..40).map(|ix| div().h(px(20.)).bg(rgb(0x100000 + ix * 0x10)).child("row")),
            )
        }
    }

    /// A scroll container whose content is a child view (pattern A).
    struct ChildViewPage {
        rows: crate::Entity<CountedRows>,
    }

    impl Render for ChildViewPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use crate::{InteractiveElement as _, StatefulInteractiveElement as _, rgb};
            div().size_full().bg(rgb(0xffffff)).child(
                div()
                    .id("scroller")
                    .overflow_y_scroll()
                    .w(px(200.))
                    .h(px(100.))
                    .child(self.rows.clone()),
            )
        }
    }

    fn child_view_page(
        cx: &mut crate::App,
    ) -> (ChildViewPage, std::rc::Rc<std::cell::Cell<usize>>) {
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        let rows = cx.new(|_| CountedRows {
            renders: renders.clone(),
        });
        (ChildViewPage { rows }, renders)
    }

    /// A frame that composites a layer whose content is a child view lays
    /// the view out as it was, without rendering it (spec §6.3, pattern A):
    /// only the view holding the container is rendered.
    #[crate::test]
    fn a_composited_child_view_is_not_rendered(cx: &mut TestAppContext) {
        use crate::fast::layers::policy::Decision;
        use crate::fast::tests::layer_support::{promote, scroll};
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle =
            crate::fast::tests::layer_support::add_window(cx, |_, cx| child_view_page(cx).0);
        let renders = handle
            .read_with(cx, |page, cx| page.rows.read(cx).renders.clone())
            .unwrap();
        let window: crate::AnyWindowHandle = handle.into();
        for _ in 0..2 {
            cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        }
        promote(cx, window);
        assert_eq!(scroll(cx, window, -10.), Some(Decision::Composite));
        let before = renders.get();
        for step in 0..4 {
            assert_eq!(
                scroll(cx, window, -10.),
                Some(Decision::Composite),
                "step {step}"
            );
        }
        assert_eq!(renders.get(), before, "the child view was not rendered");
    }

    /// A child view laid out from what its layer kept draws as without
    /// layers, on frames that composite it and on frames that paint it
    /// afresh.
    #[crate::test]
    fn a_composited_child_view_matches_drawing_without_layers(cx: &mut TestAppContext) {
        for scale_factor in [1., 1.6] {
            let composited =
                matches_drawing_without_layers(cx, scale_factor, |cx| child_view_page(cx).0);
            assert!(composited >= 6, "composited {composited} frames");
        }
    }

    /// A scroll container around a smaller one, its content no taller than
    /// itself, as GPUI Kit's gallery holds a story's own scroll area.
    struct NestedScrollers {
        outer: crate::ScrollHandle,
        inner: crate::ScrollHandle,
    }

    impl Render for NestedScrollers {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use crate::{InteractiveElement as _, StatefulInteractiveElement as _, rgb};
            div().size_full().bg(rgb(0xffffff)).child(
                div()
                    .id("outer")
                    .overflow_y_scroll()
                    .track_scroll(&self.outer)
                    .w(px(200.))
                    .h(px(100.))
                    .child(
                        div()
                            .id("scroller")
                            .overflow_y_scroll()
                            .track_scroll(&self.inner)
                            .w(px(200.))
                            .h(px(100.))
                            .children(
                                (0..40).map(|ix| div().h(px(20.)).bg(rgb(0x100000 + ix * 0x10))),
                            ),
                    ),
            )
        }
    }

    /// A wheel over the inner container moves the outer one's offset too,
    /// which it cannot scroll and puts back when it is prepainted: the outer
    /// container did not scroll, and the inner one gets the layer.
    #[crate::test]
    fn a_container_that_cannot_scroll_is_not_promoted_by_the_wheel(cx: &mut TestAppContext) {
        use crate::fast::layers::policy::Decision;
        use crate::fast::tests::layer_support::{decision, promote, scroll};
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = crate::fast::tests::layer_support::add_window(cx, |_, _| NestedScrollers {
            outer: crate::ScrollHandle::new(),
            inner: crate::ScrollHandle::new(),
        });
        let window: crate::AnyWindowHandle = handle.into();
        for _ in 0..2 {
            cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        }
        promote(cx, window);
        for step in 0..4 {
            assert_eq!(
                scroll(cx, window, -10.),
                Some(Decision::Composite),
                "step {step}"
            );
        }
        let outer_painted = cx
            .update_window(window, |_, window, _| {
                window
                    .fast_layers
                    .layers
                    .iter()
                    .any(|(id, layer)| id.last() == Some(&"outer".into()) && layer.record.is_some())
            })
            .unwrap();
        assert!(!outer_painted, "the outer container has no layer");
        assert_eq!(decision(cx, window), Some(Decision::Composite));
    }

    /// A line's glyphs never reach into an empty content mask, however near
    /// it they lie: a box straddling the mask's edge does not overlap it.
    #[test]
    fn no_glyph_reaches_into_an_empty_mask() {
        use crate::{Bounds, point, size};
        let glyph = Bounds::new(point(px(10.), px(10.)), size(px(8.), px(10.)));
        let empty = Bounds::new(point(px(0.), px(15.)), size(px(100.), px(0.)));
        let thin = Bounds::new(point(px(0.), px(15.)), size(px(100.), px(1.)));
        assert!(!crate::fast::glyphs::may_reach(glyph, px(8.), &empty));
        assert!(crate::fast::glyphs::may_reach(glyph, px(8.), &thin));
    }

    /// A glyph sits on its line's baseline, below the top of a tall line: it
    /// is drawn wherever it reaches into the content mask, even when the top
    /// of its line, where upstream looks for it, is outside the mask. A
    /// scroll layer paints the line in its overscan and shows the glyph, so
    /// drawing from scratch must show it too.
    #[test]
    fn a_glyph_low_on_a_tall_line_is_drawn_where_it_reaches_into_the_mask() {
        assert_eq!(glyphs_drawn(10.), 1, "a line inside the window");
        assert_eq!(glyphs_drawn(-100.), 0, "a line far above the window");
        // The glyph lies between 15 and 24 px below the line's top, so a line
        // 20 px above the window shows its bottom 4 px.
        assert_eq!(glyphs_drawn(-20.), 1, "a glyph reaching into the window");
    }
}

/// What gpux paints that no tile can hold: a container whose content holds
/// any of it is drawn into the frame as without a layer, counted as
/// ineligible, and never composited.
mod uncompositable {
    use super::integration::stats_matching_drawing_without_layers;
    use crate::fast::stats::LayerBypassReason;
    use crate::fast::tests::layer_support::{LayerPage, new_page};
    use crate::{
        AnyElement, Bounds, Corners, IntoElement, ParentElement as _, Pixels, Styled as _,
        TestAppContext, Window, canvas, div, px, rgb, size,
    };
    use std::rc::Rc;

    /// An element whose paint runs `paint` over a 100 px square.
    fn painting(paint: fn(Bounds<Pixels>, &mut Window)) -> AnyElement {
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| paint(bounds, window),
        )
        .w(px(100.))
        .h(px(100.))
        .into_any_element()
    }

    /// Scrolls a page whose content holds `extra` with and without layers,
    /// checking both draw the same, that no frame composited the layer, and
    /// that the container was counted as ineligible.
    fn is_never_composited(cx: &mut TestAppContext, extra: fn() -> AnyElement) {
        for scale_factor in [1., 1.25] {
            let Some(stats) =
                stats_matching_drawing_without_layers(cx, scale_factor, |cx| LayerPage {
                    extra: Some(Rc::new(extra)),
                    ..new_page(false, cx)
                })
            else {
                return;
            };
            assert_eq!(stats.layers_composited, 0, "{stats:?}");
            assert_eq!(stats.tiles_composited, 0, "{stats:?}");
            assert!(
                stats.layer_bypasses[LayerBypassReason::Ineligible as usize] > 0,
                "{stats:?}"
            );
        }
    }

    #[crate::test]
    fn a_visual_effect_in_the_content(cx: &mut TestAppContext) {
        is_never_composited(cx, || {
            painting(|bounds, window| {
                window.paint_visual_effects(
                    bounds,
                    Corners::default(),
                    &[crate::ElementEffect::Glow(crate::GlowEffect::new(
                        crate::red(),
                    ))],
                );
            })
        });
    }

    #[crate::test]
    fn a_screen_effect_region_in_the_content(cx: &mut TestAppContext) {
        is_never_composited(cx, || {
            painting(|bounds, window| {
                window.paint_screen_effect_region("region", bounds, Corners::default());
            })
        });
    }

    #[crate::test]
    fn an_html_island_in_the_content(cx: &mut TestAppContext) {
        is_never_composited(cx, || {
            painting(|bounds, window| {
                window.paint_html_island(
                    "island".into(),
                    bounds,
                    crate::HtmlIslandInputMode::GpuiOwned,
                    0,
                    "<p>island</p>".into(),
                );
            })
        });
    }

    #[crate::test]
    fn fixed_content(cx: &mut TestAppContext) {
        is_never_composited(cx, || {
            div()
                .h(px(40.))
                .child(
                    div()
                        .fixed()
                        .top(px(30.))
                        .left(px(40.))
                        .w(px(20.))
                        .h(px(10.))
                        .bg(rgb(0xff00ff)),
                )
                .into_any_element()
        });
    }

    #[crate::test]
    fn sticky_content(cx: &mut TestAppContext) {
        is_never_composited(cx, || {
            div()
                .sticky()
                .top_0()
                .w(px(60.))
                .h(px(10.))
                .bg(rgb(0x00ffff))
                .into_any_element()
        });
    }

    #[crate::test]
    fn top_layer_content(cx: &mut TestAppContext) {
        is_never_composited(cx, || {
            painting(|bounds, window| {
                window.paint_top_layer(bounds, |window| {
                    window.paint_quad(crate::fill(
                        Bounds::new(bounds.origin, size(px(10.), px(10.))),
                        crate::green(),
                    ));
                });
            })
        });
    }
}

/// Content of a composed window that no tile can hold: what is painted
/// onto another surface of the composition, and native surfaces.
mod composed {
    use crate::fast::stats::LayerBypassReason;
    use crate::fast::tests::layer_support::{
        LayerPage, add_window, draw, new_page, set_layers, with_window,
    };
    use crate::{
        AnyElement, AnyWindowHandle, IntoElement, ParentElement as _, Styled as _, TestAppContext,
        Window, WindowHandle, canvas, div, native_surface, point, px,
    };
    use std::rc::Rc;

    /// A page in a composed window, with layers or as their reference,
    /// whose content holds what `extra` builds for the window.
    fn open(
        cx: &mut TestAppContext,
        layers: bool,
        extra: fn(&mut Window) -> Rc<dyn Fn() -> AnyElement>,
    ) -> WindowHandle<LayerPage> {
        let handle = add_window(cx, |_, cx| new_page(false, cx));
        let window: AnyWindowHandle = handle.into();
        let extra = with_window(cx, window, |window, _| {
            if layers {
                set_layers(window, true);
            } else {
                crate::fast::tests::layer_support::make_reference(window);
            }
            window.enable_window_composition().unwrap();
            extra(window)
        });
        handle
            .update(cx, |page, _, cx| {
                page.extra = Some(extra);
                cx.notify();
            })
            .unwrap();
        draw(cx, window);
        draw(cx, window);
        handle
    }

    /// What `window` last drew: its primitives in drawing order and, with
    /// `surfaces`, the surface each stretch of its scene goes to.
    fn drawn(
        cx: &mut TestAppContext,
        window: AnyWindowHandle,
        surfaces: bool,
    ) -> (Vec<String>, Vec<(crate::CompositionSurfaceId, usize)>) {
        with_window(cx, window, |window, _| {
            let starts = &window.rendered_frame.fast_composition_starts;
            (
                window.painted_primitives(),
                if surfaces {
                    starts.switches().to_vec()
                } else {
                    Vec::new()
                },
            )
        })
    }

    /// Scrolls a composed page with and without layers, checking both draw
    /// the same and that the layer is not composited once it is known to
    /// hold what it cannot carry: from the first frame when its scene tells
    /// (`from_the_first`), where the surfaces drawn onto are compared too,
    /// and from the frame after the one that painted it otherwise.
    fn is_never_composited(
        cx: &mut TestAppContext,
        from_the_first: bool,
        extra: fn(&mut Window) -> Rc<dyn Fn() -> AnyElement>,
    ) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let layered = open(cx, true, extra);
        let plain = open(cx, false, extra);
        assert_eq!(
            drawn(cx, layered.into(), from_the_first),
            drawn(cx, plain.into(), from_the_first),
            "at rest"
        );
        for (step, dy) in [-20., -20., -7., -3., -11., 5.].into_iter().enumerate() {
            for window in [layered, plain] {
                with_window(cx, window.into(), |window, cx| {
                    window.dispatch_event(
                        crate::PlatformInput::ScrollWheel(crate::ScrollWheelEvent {
                            position: point(px(20.), px(60.)),
                            delta: crate::ScrollDelta::Pixels(point(px(0.), px(dy))),
                            modifiers: Default::default(),
                            touch_phase: crate::TouchPhase::Moved,
                        }),
                        cx,
                    );
                    window.draw(cx).clear(cx);
                });
            }
            assert_eq!(
                drawn(cx, layered.into(), from_the_first),
                drawn(cx, plain.into(), from_the_first),
                "step {step}"
            );
        }
        let stats = with_window(cx, layered.into(), |window, _| window.fast_stats());
        assert_eq!(stats.layers_composited, 0, "{stats:?}");
        // The frame that painted the layer showed its tiles, unless its
        // scene told then that it holds what no tile can.
        assert_eq!(
            stats.layers_repainted,
            if from_the_first { 0 } else { 1 },
            "{stats:?}"
        );
        assert!(
            stats.layer_bypasses[LayerBypassReason::Ineligible as usize] > 0,
            "{stats:?}"
        );
    }

    /// Content painted onto the overlay surface keeps going to it: a tile
    /// is one surface's.
    #[crate::test]
    fn content_painted_onto_another_surface(cx: &mut TestAppContext) {
        is_never_composited(cx, true, |window| {
            let overlay = window
                .enable_window_composition()
                .unwrap()
                .overlay_surface();
            Rc::new(move || {
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        window.paint_quad(crate::fill(bounds, crate::red()));
                        window
                            .with_composition_surface(overlay, |window| {
                                window.paint_quad(crate::fill(bounds, crate::yellow()))
                            })
                            .unwrap();
                        window.paint_quad(crate::fill(bounds, crate::blue()));
                    },
                )
                .w(px(60.))
                .h(px(30.))
                .into_any_element()
            })
        });
    }

    /// A native surface in the content is placed by the platform where the
    /// content is prepainted, which a composited frame does not do: the
    /// frame that painted the layer placed it, and the frames after it go
    /// back to today's path.
    #[crate::test]
    fn a_native_surface_in_the_content(cx: &mut TestAppContext) {
        is_never_composited(cx, false, |window| {
            let surface = window
                .enable_window_composition()
                .unwrap()
                .create_native_surface()
                .unwrap();
            Rc::new(move || {
                div()
                    .h(px(40.))
                    .child(native_surface(surface.clone()).w(px(50.)).h(px(30.)))
                    .into_any_element()
            })
        });
    }
}

/// Every reason a scroll container is drawn on today's path is counted for
/// the frame it is.
mod bypasses {
    use crate::fast::layers::policy::Decision;
    use crate::fast::stats::LayerBypassReason;
    use crate::fast::tests::layer_support::{
        LayerPage, add_window, decision, draw, frame_after, page, promote, scroll, scroll_and,
        scroll_at, viewport, with_window,
    };
    use crate::{
        AnyWindowHandle, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
        ScrollHandle, StatefulInteractiveElement as _, Styled as _, TestAppContext, Window,
        WindowHandle, deferred, div, point, px, rgb,
    };
    use std::rc::Rc;

    /// How many scroll containers the last frame drew on today's path for
    /// `reason`.
    fn counted(cx: &mut TestAppContext, window: AnyWindowHandle, reason: LayerBypassReason) -> u64 {
        with_window(cx, window, |window, _| {
            window.fast_frame_stats().layer_bypasses[reason as usize]
        })
    }

    /// The reasons the last frame counted, and how many containers for each.
    fn reasons(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<(LayerBypassReason, u64)> {
        LayerBypassReason::ALL
            .into_iter()
            .map(|reason| (reason, counted(cx, window, reason)))
            .filter(|(_, count)| *count > 0)
            .collect()
    }

    fn with_extra(
        cx: &mut TestAppContext,
        handle: WindowHandle<LayerPage>,
        extra: impl Fn() -> crate::AnyElement + 'static,
    ) {
        handle
            .update(cx, |page, _, cx| {
                page.extra = Some(Rc::new(extra));
                cx.notify();
            })
            .unwrap();
        draw(cx, handle.into());
    }

    #[crate::test]
    fn a_container_not_scrolled_long_enough_is_not_promoted(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = page(cx, false).into();
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert_eq!(reasons(cx, window), [(LayerBypassReason::NotPromoted, 1)]);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
        assert_eq!(reasons(cx, window), []);
    }

    #[crate::test]
    fn a_frame_drawn_from_scratch_is_inactive(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = page(cx, false).into();
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
        frame_after(cx, window, |cx| {
            with_window(cx, window, |window, _| window.refresh())
        });
        assert_eq!(reasons(cx, window), [(LayerBypassReason::Inactive, 1)]);
    }

    #[crate::test]
    fn content_that_keeps_changing_is_demoted(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, true);
        let window = handle.into();
        let rows = handle
            .read_with(cx, |page, _| page.rows.clone().unwrap())
            .unwrap();
        promote(cx, window);
        let mut demoted = false;
        for step in 0..20 {
            scroll_and(cx, window, -5., |cx| {
                rows.update(cx, |rows, cx| {
                    rows.tint = 0x200000 + step * 0x100;
                    cx.notify();
                })
            });
            if counted(cx, window, LayerBypassReason::Demoted) == 1 {
                demoted = true;
                break;
            }
        }
        assert!(demoted, "the container was never demoted");
        assert_eq!(scroll(cx, window, -5.), Some(Decision::Bypass));
        assert_eq!(reasons(cx, window), [(LayerBypassReason::Demoted, 1)]);
    }

    #[crate::test]
    fn content_a_layer_cannot_carry_is_ineligible(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        with_extra(cx, handle, || {
            div()
                .h(px(10.))
                .child(deferred(div().w(px(50.)).h(px(50.)).bg(crate::red())))
                .into_any_element()
        });
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
        assert_eq!(reasons(cx, window), [(LayerBypassReason::Ineligible, 1)]);
    }

    /// A children-prepainted listener in the content hands bounds to code
    /// outside it as the content is prepainted, which a composited frame
    /// skips: the container stays on today's path.
    #[crate::test]
    fn a_children_prepainted_listener_in_the_content_is_ineligible(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        with_extra(cx, handle, || {
            div()
                .h(px(10.))
                .on_children_prepainted(|_, _, _| {})
                .child(div().h(px(5.)))
                .into_any_element()
        });
        promote(cx, window);
        for _ in 0..3 {
            assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
            assert_eq!(reasons(cx, window), [(LayerBypassReason::Ineligible, 1)]);
        }
    }

    /// A container clipped by a rounded mask: tiles are composited clipped
    /// to a rectangle.
    struct Rounded {
        handle: ScrollHandle,
    }

    impl Render for Rounded {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0xffffff)).child(
                div()
                    .w(viewport().size.width)
                    .h(viewport().size.height)
                    .rounded(px(12.))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("scroller")
                            .overflow_y_scroll()
                            .track_scroll(&self.handle)
                            .size_full()
                            .children(
                                (0..40)
                                    .map(|index| div().h(px(20.)).bg(rgb(0x100000 + index * 0x10))),
                            ),
                    ),
            )
        }
    }

    #[crate::test]
    fn a_container_under_a_rounded_clip_is_ineligible(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window: AnyWindowHandle = add_window(cx, |_, _| Rounded {
            handle: ScrollHandle::new(),
        })
        .into();
        draw(cx, window);
        draw(cx, window);
        for _ in 0..4 {
            frame_after(cx, window, |cx| {
                with_window(cx, window, |window, cx| {
                    window.dispatch_event(
                        crate::PlatformInput::ScrollWheel(crate::ScrollWheelEvent {
                            position: point(px(20.), px(20.)),
                            delta: crate::ScrollDelta::Pixels(point(px(0.), px(-20.))),
                            modifiers: Default::default(),
                            touch_phase: crate::TouchPhase::Moved,
                        }),
                        cx,
                    );
                })
            });
            assert_eq!(reasons(cx, window), [(LayerBypassReason::Ineligible, 1)]);
        }
        with_window(cx, window, |window, _| {
            assert!(window.fast_layers.layers.is_empty());
        });
    }

    /// A window edge between device pixels clips glyphs where it lies and
    /// quads at the pixels covering it, which one clip of a layer's tiles
    /// cannot stand in for.
    #[crate::test]
    fn a_container_clipped_between_device_pixels_is_ineligible(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = page(cx, false).into();
        cx.simulate_window_resize(window, crate::size(px(300.), px(80.4)));
        draw(cx, window);
        for _ in 0..4 {
            // Before the policy is asked: it decides nothing.
            assert_eq!(scroll(cx, window, -20.), None);
            assert_eq!(reasons(cx, window), [(LayerBypassReason::Ineligible, 1)]);
        }
        // On device pixels again, the container is promoted as any other.
        cx.simulate_window_resize(window, crate::size(px(300.), px(80.)));
        draw(cx, window);
        promote(cx, window);
        assert_eq!(scroll(cx, window, -20.), Some(Decision::Composite));
    }

    #[crate::test]
    fn a_container_inside_a_layer_is_nested(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = page(cx, false);
        let window = handle.into();
        promote(cx, window);
        // The inner container is painted into the outer one's layer.
        with_extra(cx, handle, || {
            div()
                .id("inner")
                .overflow_x_scroll()
                .w(px(200.))
                .h(px(40.))
                .child(div().w(px(800.)).h(px(40.)).bg(rgb(0x123456)))
                .into_any_element()
        });
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        assert_eq!(reasons(cx, window), [(LayerBypassReason::Nested, 1)]);
    }

    /// A scroll container stuck to the top of the one around it: sticky
    /// positioning moves it with that one's offset, not only with its own.
    struct Stuck {
        outer: ScrollHandle,
        inner: ScrollHandle,
    }

    impl Render for Stuck {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0xffffff)).child(
                div()
                    .id("outer")
                    .overflow_y_scroll()
                    .track_scroll(&self.outer)
                    .w(viewport().size.width)
                    .h(viewport().size.height)
                    .child(div().h(px(30.)))
                    .child(
                        div()
                            .id("scroller")
                            .sticky()
                            .top_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.inner)
                            .w(px(100.))
                            .h(px(50.))
                            .children(
                                (0..40)
                                    .map(|index| div().h(px(20.)).bg(rgb(0x100000 + index * 0x10))),
                            ),
                    )
                    .child(div().h(px(600.))),
            )
        }
    }

    #[crate::test]
    fn a_container_sticky_positioning_displaced_is_moved(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = add_window(cx, |_, _| Stuck {
            outer: ScrollHandle::new(),
            inner: ScrollHandle::new(),
        });
        let window: AnyWindowHandle = handle.into();
        draw(cx, window);
        draw(cx, window);
        // Scrolled past the inner container, which sticks to the top.
        let owner = handle.entity(cx).unwrap().entity_id();
        let outer = handle
            .read_with(cx, |stuck, _| stuck.outer.clone())
            .unwrap();
        frame_after(cx, window, |cx| {
            crate::fast::tests::layer_support::publish_scrollbar(
                cx,
                &outer,
                owner,
                point(px(0.), px(-80.)),
            );
        });
        draw(cx, window);
        let before = handle
            .read_with(cx, |stuck, _| stuck.inner.offset())
            .unwrap();
        for _ in 0..3 {
            scroll_at(cx, window, point(px(20.), px(20.)), -20.);
            assert_eq!(
                counted(cx, window, LayerBypassReason::Moved),
                1,
                "{:?}",
                reasons(cx, window)
            );
        }
        assert_ne!(
            handle
                .read_with(cx, |stuck, _| stuck.inner.offset())
                .unwrap(),
            before,
            "the wheel scrolled the stuck container"
        );
    }
}
