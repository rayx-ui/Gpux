//! The grid bounds tree: the draw orders upstream's R-tree and a brute-force
//! search give, for fresh and replayed fills, in all three bands.
//!
//! Adapted from gpui-fast's tests in `fast/bounds_tree.rs`; every ordering is
//! also compared with the R-tree gpux used before (`crate::bounds_tree`).

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use crate::{Bounds, Point, ScaledPixels, Size, fast::bounds_tree::BoundsTree, point, px};

type RTree = crate::bounds_tree::BoundsTree<f32>;

fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<f32> {
    Bounds {
        origin: Point { x, y },
        size: Size { width, height },
    }
}

/// The ordering comparing `bounds` with every bounds inserted before gives.
fn brute_force(inserted: &[(Bounds<f32>, u32)], bounds: &Bounds<f32>) -> u32 {
    inserted
        .iter()
        .filter(|(other, _)| other.intersects(bounds))
        .map(|(_, order)| *order)
        .max()
        .unwrap_or(0)
        + 1
}

fn random_bounds(rng: &mut StdRng, spread: f32, size: f32) -> Bounds<f32> {
    bounds(
        rng.random_range(-spread..spread),
        rng.random_range(-spread..spread),
        rng.random_range(0.0..size),
        rng.random_range(0.0..size),
    )
}

/// Fills `tree` with `frame` after clearing it, as a scene does each frame,
/// checking every ordering against the brute-force search and a fresh
/// R-tree. Returns how many orderings the grid replayed.
fn fill(tree: &mut BoundsTree<f32>, frame: &[Bounds<f32>]) -> u32 {
    tree.clear();
    let mut reference = RTree::default();
    let mut inserted: Vec<(Bounds<f32>, u32)> = Vec::new();
    for bounds in frame {
        let expected = brute_force(&inserted, bounds);
        assert_eq!(tree.insert(*bounds), expected, "{bounds:?}");
        assert_eq!(
            reference.insert(*bounds),
            expected,
            "the R-tree on {bounds:?}"
        );
        inserted.push((*bounds, expected));
    }
    tree.replayed
}

#[test]
fn inserts_order_like_the_r_tree() {
    let mut tree = BoundsTree::<f32>::default();
    assert_eq!(tree.insert(bounds(0., 0., 10., 10.)), 1);
    assert_eq!(tree.insert(bounds(5., 5., 10., 10.)), 2);
    assert_eq!(tree.insert(bounds(10., 10., 10., 10.)), 3);
    // Bounds that overlap nothing earlier share the first ordering.
    assert_eq!(tree.insert(bounds(20., 20., 10., 10.)), 1);
    assert_eq!(tree.insert(bounds(40., 40., 10., 10.)), 1);
    assert_eq!(tree.insert(bounds(25., 25., 10., 10.)), 2);
}

#[test]
fn random_fills_order_like_the_r_tree_and_brute_force() {
    for seed in 1..=500 {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut tree = BoundsTree::default();
        let frame: Vec<_> = (0..rng.random_range(1..=120))
            .map(|_| random_bounds(&mut rng, 100., 50.))
            .collect();
        fill(&mut tree, &frame);
    }
    // Large enough for the grid to grow and the R-tree to split every level.
    for seed in 1..=8 {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut tree = BoundsTree::default();
        let frame: Vec<_> = (0..2000)
            .map(|_| random_bounds(&mut rng, 4000., 120.))
            .collect();
        fill(&mut tree, &frame);
    }
}

/// A frame inserting the bounds the last one did is replayed whole; one that
/// follows it for a while, or repeats it, or differs from the start, gets
/// what inserting it afresh gives.
#[test]
fn replaying_the_last_fill_gives_what_inserting_it_would() {
    for seed in 1..=300 {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut tree = BoundsTree::default();
        let count = rng.random_range(1..=120);
        let first: Vec<_> = (0..count)
            .map(|_| random_bounds(&mut rng, 100., 50.))
            .collect();
        assert_eq!(
            fill(&mut tree, &first),
            0,
            "a first fill has nothing to replay"
        );
        assert_eq!(
            fill(&mut tree, &first) as usize,
            count,
            "the same fill again is replayed whole"
        );

        let kept = rng.random_range(0..=count);
        let mut second: Vec<_> = first[..kept].to_vec();
        second.extend((0..rng.random_range(0..=60)).map(|_| random_bounds(&mut rng, 100., 50.)));
        fill(&mut tree, &second);
        fill(&mut tree, &second);
        let third: Vec<_> = (0..count)
            .map(|_| random_bounds(&mut rng, 100., 50.))
            .collect();
        fill(&mut tree, &third);
    }
}

/// Bounds moved, inserted or removed here and there are replayed past, and
/// every ordering still equals a fresh search's.
#[test]
fn replaying_past_scattered_changes_gives_what_inserting_it_would() {
    for seed in 1..=300 {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut tree = BoundsTree::default();
        let count = rng.random_range(1..=if seed % 10 == 0 { 800 } else { 150 });
        let mut frame: Vec<_> = (0..count)
            .map(|_| random_bounds(&mut rng, 100., 50.))
            .collect();
        fill(&mut tree, &frame);
        for _ in 0..6 {
            for _ in 0..rng.random_range(0..=count / 8 + 1) {
                let at = rng.random_range(0..frame.len().max(1));
                match rng.random_range(0..10) {
                    0 if !frame.is_empty() => {
                        frame.remove(at);
                    }
                    1 => frame.insert(at.min(frame.len()), random_bounds(&mut rng, 100., 50.)),
                    _ if !frame.is_empty() => {
                        frame[at].origin.x += rng.random_range(-5.0..5.0);
                    }
                    _ => {}
                }
            }
            fill(&mut tree, &frame);
        }
    }
}

/// Bounds on cell edges, covering cells whole, reaching past the grid,
/// without width or height, negative, infinite or not a number are ordered
/// as comparing them with every bounds before them orders them, fresh and
/// replayed.
#[test]
fn degenerate_and_nan_bounds_order_like_brute_force() {
    let edge = 64.;
    let coordinate = |rng: &mut StdRng| match rng.random_range(0..12) {
        0 => f32::INFINITY,
        1 => f32::NEG_INFINITY,
        2 => f32::NAN,
        3 => rng.random_range(-4..260) as f32 * edge,
        4..8 => rng.random_range(-3..8) as f32 * edge,
        _ => rng.random_range(-2.0 * edge..6.0 * edge),
    };
    let length = |rng: &mut StdRng| match rng.random_range(0..10) {
        0 => 0.,
        1 => -rng.random_range(0.0..edge),
        2 => f32::INFINITY,
        3 => f32::NAN,
        4..6 => rng.random_range(0..4) as f32 * edge,
        _ => rng.random_range(0.0..3.0 * edge),
    };
    for seed in 1..=400 {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut tree = BoundsTree::default();
        let mut frame: Vec<Bounds<f32>> = (0..rng.random_range(1..200))
            .map(|_| Bounds {
                origin: Point {
                    x: coordinate(&mut rng),
                    y: coordinate(&mut rng),
                },
                size: Size {
                    width: length(&mut rng),
                    height: length(&mut rng),
                },
            })
            .collect();
        for _ in 0..4 {
            tree.clear();
            let mut inserted: Vec<(Bounds<f32>, u32)> = Vec::new();
            for bounds in &frame {
                let expected = brute_force(&inserted, bounds);
                assert_eq!(tree.insert(*bounds), expected, "seed {seed}: {bounds:?}");
                inserted.push((*bounds, expected));
            }
            let at = rng.random_range(0..frame.len());
            frame[at].origin.x = coordinate(&mut rng);
        }
    }
}

/// A frame whose changes reach more bounds than replaying may compare
/// against stops replaying, builds the grid from what it recorded, and
/// orders the rest by searching it, still exactly.
#[test]
fn the_comparison_budget_falls_back_to_building_the_grid() {
    let mut rng = StdRng::seed_from_u64(7);
    let mut tree = BoundsTree::default();
    // Thousands of bounds over a small area, so every change meets many.
    let mut frame: Vec<_> = (0..4000)
        .map(|_| random_bounds(&mut rng, 200., 60.))
        .collect();
    fill(&mut tree, &frame);
    for bounds in frame.iter_mut().step_by(3) {
        bounds.origin.y += 1.;
    }
    let replayed = fill(&mut tree, &frame);
    assert!(
        replayed < 4000,
        "a frame changed throughout was replayed whole ({replayed})"
    );
    // The frame after, unchanged again, is replayed whole.
    assert_eq!(fill(&mut tree, &frame), 4000);
}

/// The scene orders primitives in all three bands with the grid: the
/// positioned and top-layer bands keep their bases, and a frame painting what
/// the last one did replays every ordering. A cached view replayed by
/// `Scene::replay` replays its orderings too.
#[test]
fn every_band_replays_and_cached_views_replay_their_orderings() {
    use crate::{Context, Render, Window, div, prelude::*};

    struct Bands;
    impl Render for Bands {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(div().w(px(40.)).h(px(40.)).bg(crate::red()))
                .child(
                    div()
                        .id("scroller")
                        .w(px(100.))
                        .h(px(100.))
                        .overflow_y_scroll()
                        .child(
                            div()
                                .id("pinned")
                                .fixed()
                                .top(px(5.))
                                .w(px(20.))
                                .h(px(20.))
                                .bg(crate::blue()),
                        )
                        .children(
                            (0..30usize)
                                .map(|row| div().id(("row", row)).h(px(20.)).bg(crate::green())),
                        ),
                )
        }
    }

    let mut cx = super::oracle_support::oracle_app(0);
    let window = cx.add_window(|_, _| Bands);
    let draw = |cx: &mut crate::TestAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            let scene = &window.rendered_frame.scene;
            (
                window.fast_frame_stats(),
                scene
                    .quads
                    .iter()
                    .map(|quad| quad.order)
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap()
    };
    draw(&mut cx);
    let (first, first_orders) = draw(&mut cx);
    let (second, second_orders) = draw(&mut cx);
    assert_eq!(first_orders, second_orders);
    assert!(
        first_orders
            .iter()
            .any(|order| *order >= crate::scene::POSITIONED_DRAW_ORDER_BASE),
        "nothing painted in the positioned band"
    );
    // Frames alternate between two scenes, so the second identical frame
    // after the first replays every ordering.
    assert_eq!(second.draw_orders_computed, 0, "{second:?}");
    assert!(second.draw_orders_replayed > 0, "{second:?}");
    assert!(first.draw_orders_replayed + first.draw_orders_computed > 0);

    // Cached views: the grid sees the same inserts a fresh frame makes.
    let mut oracle = super::oracle_support::OracleWindow::open(4, crate::FastMode::OFF);
    oracle.draw(false);
    oracle.draw(false);
    let (_, replay) = oracle.draw(false);
    assert!(replay.cached_views_replayed > 0);
    assert!(replay.draw_orders_replayed > 0, "{replay:?}");

    // The scaled bounds the scene inserts are ordered like any others.
    let mut scaled = BoundsTree::<ScaledPixels>::default();
    let a = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        crate::size(ScaledPixels(10.), ScaledPixels(10.)),
    );
    assert_eq!((scaled.insert(a), scaled.insert(a)), (1, 2));
    let _ = cx.update(|_| ());
}
