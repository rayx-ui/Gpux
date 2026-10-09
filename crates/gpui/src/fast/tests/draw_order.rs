//! The scene's draw-order sorts: a counting sort and its packed fallback that
//! equal a stable sort by draw order, sprites grouped by texture, equal keys
//! kept in painting order in every family, and the already-sorted fast path.
//!
//! Adapted from gpui-fast's tests in `fast/scene.rs`.

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use crate::{
    AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds, ContentMask, DevicePixels, DrawOrder,
    MonochromeSprite, MonochromeSpriteKind, PolychromeSprite, Quad, ScaledPixels, Scene, TileId,
    TransformationMatrix,
    fast::scene::{MAX_ORDERS_PER_PRIMITIVE, Permutation, sort_by_draw_order, sort_sprites},
    point, size,
};

/// A primitive as the sorts see it, and where it was painted.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Item {
    order: DrawOrder,
    texture: u32,
    tile: u32,
    painted: usize,
}

fn items(rng: &mut StdRng, len: usize, orders: DrawOrder) -> Vec<Item> {
    (0..len)
        .map(|painted| Item {
            order: rng.random_range(0..orders),
            texture: rng.random_range(0..3),
            tile: rng.random_range(0..8),
            painted,
        })
        .collect()
}

/// No items, one, orders already in order, orders counted, and orders spread
/// too wide to count, as gpux's positioned and top-layer bands spread them.
fn cases(rng: &mut StdRng) -> Vec<Vec<Item>> {
    let mut cases = vec![Vec::new(), items(rng, 1, 5)];
    for _ in 0..200 {
        let len = rng.random_range(2..300);
        let spread = match rng.random_range(0..5) {
            0 => 1,
            1 => rng.random_range(2..30),
            2 => (len * MAX_ORDERS_PER_PRIMITIVE + 1) as DrawOrder,
            3 => rng.random_range(len * MAX_ORDERS_PER_PRIMITIVE + 2..100_000) as DrawOrder,
            _ => (1 << 31) + 64,
        };
        let mut case = items(rng, len, spread);
        let greatest = rng.random_range(0..len);
        case[greatest].order = spread - 1;
        if rng.random_ratio(1, 5) {
            case.sort_by_key(|item| item.order);
        }
        cases.push(case);
    }
    cases
}

#[test]
fn counting_and_packed_sorts_equal_a_stable_sort_by_draw_order() {
    let mut rng = StdRng::seed_from_u64(7);
    let mut permutation = Permutation::default();
    let mut gathered = Vec::new();
    let (mut counted, mut packed) = (0, 0);
    for case in cases(&mut rng) {
        let mut expected = case.clone();
        expected.sort_by_key(|item| item.order);
        let mut sorted = case.clone();
        sort_by_draw_order(&mut sorted, &mut permutation, &mut gathered, |item| {
            item.order
        });
        assert_eq!(sorted, expected, "sorting {case:?}");

        // Which path sorted it.
        let mut scratch = Permutation::default();
        let mut again = case.clone();
        let in_order = case.is_sorted_by_key(|item| item.order);
        let greatest = case.iter().map(|item| item.order).max().unwrap_or(0) as usize;
        sort_by_draw_order(&mut again, &mut scratch, &mut gathered, |item| item.order);
        if !in_order && case.len() > 1 {
            if greatest > case.len() * MAX_ORDERS_PER_PRIMITIVE {
                assert_eq!((scratch.counts.len(), scratch.keys.len()), (0, case.len()));
                packed += 1;
            } else {
                assert_eq!(
                    (scratch.counts.len(), scratch.keys.len()),
                    (greatest + 1, 0)
                );
                counted += 1;
            }
        }
    }
    assert!(
        counted > 0 && packed > 0,
        "{counted} counted, {packed} packed"
    );
}

/// Primitives already in order are left as they are: nothing is counted,
/// packed or gathered.
#[test]
fn an_already_sorted_family_is_left_as_it_is() {
    let mut permutation = Permutation::default();
    let mut gathered: Vec<Item> = Vec::new();
    let mut sorted: Vec<Item> = (0..100)
        .map(|painted| Item {
            order: painted as DrawOrder / 3,
            texture: 0,
            tile: 0,
            painted,
        })
        .collect();
    let before = sorted.clone();
    sort_by_draw_order(&mut sorted, &mut permutation, &mut gathered, |item| {
        item.order
    });
    assert_eq!(sorted, before);
    assert!(permutation.counts.is_empty() && permutation.keys.is_empty() && gathered.is_empty());
    sort_sprites(&mut sorted, &mut permutation, &mut gathered, |item| {
        (item.order, item.texture, item.tile)
    });
    assert_eq!(sorted, before);
    assert!(gathered.is_empty(), "sorted sprites were gathered");

    // The scratch is reused rather than grown again.
    let mut rng = StdRng::seed_from_u64(3);
    let mut shuffled = items(&mut rng, 200, 50);
    sort_by_draw_order(&mut shuffled, &mut permutation, &mut gathered, |item| {
        item.order
    });
    let capacity = gathered.capacity();
    let mut shuffled = items(&mut rng, 200, 50);
    sort_by_draw_order(&mut shuffled, &mut permutation, &mut gathered, |item| {
        item.order
    });
    assert_eq!(gathered.capacity(), capacity, "the scratch was reallocated");
}

#[test]
fn sprites_sort_by_order_then_texture_then_tile_stably() {
    let mut rng = StdRng::seed_from_u64(11);
    let mut permutation = Permutation::default();
    let mut gathered = Vec::new();
    for case in cases(&mut rng) {
        let mut expected = case.clone();
        expected.sort_by_key(|item| (item.order, item.texture, item.tile));
        let mut sorted = case.clone();
        sort_sprites(&mut sorted, &mut permutation, &mut gathered, |item| {
            (item.order, item.texture, item.tile)
        });
        assert_eq!(sorted, expected, "sorting {case:?}");
        // Batches differ from the old (order, tile) sort only by texture
        // grouping: within one texture, the order is the old one.
        let mut old = case.clone();
        old.sort_by_key(|item| (item.order, item.tile));
        for texture in 0..3 {
            let of = |items: &[Item]| -> Vec<Item> {
                items
                    .iter()
                    .copied()
                    .filter(|item| item.texture == texture)
                    .collect()
            };
            assert_eq!(of(&sorted), of(&old), "texture {texture} of {case:?}");
        }
    }
    // Every bit of each key counts.
    let item = |order, texture, tile, painted| Item {
        order,
        texture,
        tile,
        painted,
    };
    let mut sorted = vec![
        item(u32::MAX, 0, 0, 0),
        item(0, u32::MAX, 0, 1),
        item(0, 0, u32::MAX, 2),
        item(0, 0, 0, 3),
    ];
    sort_sprites(&mut sorted, &mut permutation, &mut gathered, |item| {
        (item.order, item.texture, item.tile)
    });
    let painted: Vec<usize> = sorted.iter().map(|item| item.painted).collect();
    assert_eq!(painted, [3, 2, 1, 0]);
}

fn quad(order: DrawOrder, painted: usize) -> Quad {
    Quad {
        order,
        bounds: Bounds::new(
            point(ScaledPixels(painted as f32), ScaledPixels(0.)),
            size(ScaledPixels(1.), ScaledPixels(1.)),
        ),
        ..Quad::default()
    }
}

fn tile(texture: u32, tile: u32) -> AtlasTile {
    AtlasTile {
        texture_id: AtlasTextureId {
            index: texture,
            kind: AtlasTextureKind::Monochrome,
        },
        tile_id: TileId(tile),
        padding: 0,
        bounds: Bounds::<DevicePixels>::default(),
    }
}

/// A finished scene keeps primitives of one draw order in the order they
/// were painted in every family, the monochrome sprites included, and groups
/// sprites of one order by texture.
#[test]
fn scene_finish_keeps_equal_keys_in_painting_order_in_every_family() {
    let mut rng = StdRng::seed_from_u64(5);
    let mut scene = Scene::default();
    let mask = ContentMask::<ScaledPixels>::default();
    for painted in 0..300 {
        let order = rng.random_range(0..12);
        scene.quads.push(quad(order, painted));
        let marked = |painted: usize| {
            Bounds::new(
                point(ScaledPixels(painted as f32), ScaledPixels(0.)),
                size(ScaledPixels(1.), ScaledPixels(1.)),
            )
        };
        scene.monochrome_sprites.push(MonochromeSprite {
            order,
            kind: MonochromeSpriteKind::Glyph,
            bounds: marked(painted),
            content_mask: mask.clone(),
            color: Default::default(),
            tile: tile(rng.random_range(0..3), rng.random_range(0..4)),
            transformation: TransformationMatrix::unit(),
        });
        scene.polychrome_sprites.push(PolychromeSprite {
            order,
            pad: 0,
            grayscale: false,
            opacity: 1.,
            bounds: marked(painted),
            content_mask: mask.clone(),
            corner_radii: Default::default(),
            tile: tile(rng.random_range(0..3), rng.random_range(0..4)),
            transformation: TransformationMatrix::unit(),
        });
    }
    let mut expected_quads = scene.quads.clone();
    expected_quads.sort_by_key(|quad| quad.order);
    let mut expected_mono = scene.monochrome_sprites.clone();
    expected_mono.sort_by_key(|sprite| {
        (
            sprite.order,
            sprite.tile.texture_id.index,
            sprite.tile.tile_id,
        )
    });
    let mut expected_poly = scene.polychrome_sprites.clone();
    expected_poly.sort_by_key(|sprite| {
        (
            sprite.order,
            sprite.tile.texture_id.index,
            sprite.tile.tile_id,
        )
    });
    scene.finish();
    let painted = |bounds: &Bounds<ScaledPixels>| bounds.origin.x.0 as usize;
    assert_eq!(
        scene
            .quads
            .iter()
            .map(|q| painted(&q.bounds))
            .collect::<Vec<_>>(),
        expected_quads
            .iter()
            .map(|q| painted(&q.bounds))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        scene
            .monochrome_sprites
            .iter()
            .map(|s| painted(&s.bounds))
            .collect::<Vec<_>>(),
        expected_mono
            .iter()
            .map(|s| painted(&s.bounds))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        scene
            .polychrome_sprites
            .iter()
            .map(|s| painted(&s.bounds))
            .collect::<Vec<_>>(),
        expected_poly
            .iter()
            .map(|s| painted(&s.bounds))
            .collect::<Vec<_>>()
    );
}

/// The oracle's cached-view histories draw the same frames with the new
/// sorts as a window drawing from scratch does.
#[test]
fn incremental_frames_still_match_with_the_new_sorts() {
    let run = super::oracle_support::run_lockstep(21, 40, crate::FastMode::OFF);
    assert!(run.incremental.cached_views_replayed > 0);
}
