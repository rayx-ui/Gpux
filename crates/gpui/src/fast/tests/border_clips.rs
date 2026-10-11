//! A bordered element tells the scene one clip (`fast/border_clips.rs`): the four quads of a
//! border, each in its side's strip, are painted in one clip told to the scene, and the scene
//! orders them as it did with each strip a clip of its own.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::border_clips::
//! ```

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use super::edge_shadows::{Item, orders, paint_with, sp, viewport, window};
use crate::fast::edge_shadows::ClipOp;
use crate::{
    Bounds, DrawOrder, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    ScaledPixels, Scene, Styled as _, TestAppContext, div,
    fast::tests::layer_support::{add_window, with_window},
    px, rgb,
    scene::PaintOperation,
};

/// How many clips of each kind a scene was told of.
fn told(scene: &Scene) -> (usize, usize, usize) {
    let count = |wanted: fn(&ClipOp) -> bool| {
        scene
            .paint_operations
            .iter()
            .filter(|operation| matches!(operation, PaintOperation::FastClip(clip) if wanted(clip)))
            .count()
    };
    (
        count(|clip| matches!(clip, ClipOp::Push(_))),
        count(|clip| matches!(clip, ClipOp::Border { .. })),
        count(|clip| matches!(clip, ClipOp::Pop)),
    )
}

/// What a scene holds of the quads it keeps: bounds, mask and draw order, in the order they
/// were kept.
fn quads(scene: &Scene) -> Vec<(Bounds<ScaledPixels>, Bounds<ScaledPixels>, DrawOrder)> {
    scene
        .quads
        .iter()
        .map(|quad| (quad.bounds, quad.content_mask.bounds, quad.order))
        .collect()
}

/// A bordered element is told to the scene as one clip, where each of its four strips was a
/// clip: two paint operations and no set of entries of its own, for eight operations and four
/// sets.
#[test]
fn a_border_tells_one_clip_and_makes_no_set_of_entries_of_its_own() {
    let rows = 200;
    let items: Vec<Item> = std::iter::once(Item::Push(viewport()))
        .chain((0..rows).flat_map(|row| {
            let bounds = sp(20., 24. * row as f32 - 2000., 360., 20.);
            [Item::Quad(bounds), Item::Border(bounds, 1., 0.)]
        }))
        .chain([Item::Pop])
        .collect();
    let (each, _) = paint_with(&items, false);
    let (one, primitives) = paint_with(&items, true);
    let (strip_clips, _, _) = told(&each);
    assert_eq!(strip_clips, 4 * rows + 1);
    // The viewport's clip, and a border's.
    assert_eq!(told(&one), (1, rows, rows + 1));
    // Eight operations a border less six: a clip told and closed, for four told and closed.
    assert!(
        one.paint_operations.len() + 6 * rows <= each.paint_operations.len(),
        "{} operations against {}",
        one.paint_operations.len(),
        each.paint_operations.len()
    );
    // A border cut by the viewport leaves an entry at its edge, in a set of the
    // viewport's; none lies in a set of a border's.
    assert!(
        each.fast_edge_shadows.sets() >= 4 * 20,
        "{} sets with a clip for each strip",
        each.fast_edge_shadows.sets()
    );
    assert!(
        one.fast_edge_shadows.sets() <= 1,
        "{} sets with a clip for a border",
        one.fast_edge_shadows.sets()
    );
    assert_eq!(orders(&one, &primitives), orders(&each, &primitives));
    assert_eq!(quads(&one), quads(&each));
}

/// Content scattered in and around a viewport, bordered elements among it: in the viewport
/// or in cards that clip what they hold, scrolling or not, with shadows whose blurs reach
/// over what is bordered.
fn random_bordered_content(rng: &mut StdRng, scrolling: bool) -> Vec<Item> {
    let mut items = vec![Item::Push(viewport())];
    if scrolling {
        items.push(Item::BeginScroll);
    }
    items.push(Item::Quad(sp(20., -500., 360., 2000.)));
    let element = |rng: &mut StdRng, items: &mut Vec<Item>, around: Bounds<ScaledPixels>| {
        // Large enough that a strip is a strip: elements as small as a border is wide
        // overlap their own strips.
        let (width, radius) = (
            rng.random_range(1..5) as f32,
            rng.random_range(0..10) as f32,
        );
        let reach = 2. * width.max(radius) + 4.;
        let bounds = sp(
            (around.left().0 + rng.random_range(-40.0..around.size.width.0)).round(),
            (around.top().0 + rng.random_range(-40.0..around.size.height.0)).round(),
            rng.random_range(reach..200.0f32).round(),
            rng.random_range(reach..120.0f32).round(),
        );
        items.push(Item::Quad(bounds));
        items.push(Item::Border(bounds, width, radius));
        if rng.random_range(0..3) == 0 {
            let inner = sp(
                bounds.left().0 + 2.,
                bounds.top().0 + 2.,
                (bounds.size.width.0 - 4.).max(1.),
                (bounds.size.height.0 - 4.).max(1.),
            );
            items.push(Item::Quad(inner));
        }
    };
    let everywhere = sp(-20., -200., 440., 640.);
    for _ in 0..rng.random_range(6..40) {
        match rng.random_range(0..6) {
            0 => {
                // A card clipping what it holds, a bordered element or two in it.
                let card = sp(
                    rng.random_range(-20.0..300.0f32).round(),
                    rng.random_range(-200.0..400.0f32).round(),
                    rng.random_range(60.0..220.0f32).round(),
                    rng.random_range(40.0..160.0f32).round(),
                );
                items.push(Item::Quad(card));
                items.push(Item::Push(card));
                let scrolls = scrolling && rng.random_range(0..2) == 0;
                if scrolls {
                    items.push(Item::BeginScroll);
                }
                for _ in 0..rng.random_range(1..4) {
                    element(rng, &mut items, card);
                }
                if scrolls {
                    items.push(Item::EndScroll);
                }
                items.push(Item::Pop);
            }
            1 => {
                let bounds = sp(
                    rng.random_range(-40.0..360.0f32).round(),
                    rng.random_range(-200.0..440.0f32).round(),
                    rng.random_range(20.0..160.0f32).round(),
                    rng.random_range(20.0..100.0f32).round(),
                );
                items.push(Item::Shadow(bounds, rng.random_range(2.0..24.0)));
            }
            2 => {
                let bounds = sp(
                    rng.random_range(-40.0..360.0f32).round(),
                    rng.random_range(-200.0..440.0f32).round(),
                    rng.random_range(10.0..200.0f32).round(),
                    rng.random_range(10.0..120.0f32).round(),
                );
                items.push(Item::Quad(bounds));
            }
            _ => element(rng, &mut items, everywhere),
        }
    }
    if scrolling {
        items.push(Item::EndScroll);
    }
    items.push(Item::Pop);
    items
}

/// A border told as one clip is ordered primitive for primitive as it was with a clip for
/// each strip: the bounds, masks and draw orders the scene keeps are the same.
#[test]
fn bordered_content_is_ordered_as_with_a_clip_for_each_strip() {
    let mut rng = StdRng::seed_from_u64(0xb0_4d3e);
    let (mut kept, mut cut, mut dropped) = (0, 0, 0);
    for run in 0..1000 {
        let items = random_bordered_content(&mut rng, run % 2 == 1);
        let (each, primitives) = paint_with(&items, false);
        let (one, _) = paint_with(&items, true);
        let (expected, got) = (orders(&each, &primitives), orders(&one, &primitives));
        assert_eq!(got, expected, "run {run}");
        assert_eq!(quads(&one), quads(&each), "run {run}");
        assert_eq!(one.shadows.len(), each.shadows.len(), "run {run}");
        for (a, b) in one.shadows.iter().zip(&each.shadows) {
            assert_eq!(a.order, b.order, "run {run}");
        }
        for (primitive, order) in primitives.iter().zip(&expected) {
            let whole = primitive.bounds();
            match order {
                Some(_) => {
                    kept += 1;
                    cut += (whole.intersect(&primitive.content_mask().bounds) != whole) as usize;
                }
                None => dropped += 1,
            }
        }
    }
    assert!(kept >= 10_000, "{kept} primitives were kept");
    assert!(cut >= 1_000, "{cut} were cut by a clip");
    assert!(dropped >= 5_000, "{dropped} were dropped");
}

/// Replayed paint operations give the same scene, whole and in ranges cut anywhere, inside
/// a border as well: the ranges' clips go on where the last left them, and so do the quads
/// they have counted.
#[test]
fn a_border_replayed_gives_the_same_scene() {
    let mut rng = StdRng::seed_from_u64(0xb0_4d3f);
    let mut inside = 0;
    for run in 0..400 {
        let items = random_bordered_content(&mut rng, run % 2 == 1);
        let (scene, primitives) = paint_with(&items, true);
        let expected = orders(&scene, &primitives);
        let count = scene.paint_operations.len();

        let mut whole = Scene::default();
        whole.replay(0..count, &scene);
        assert_eq!(orders(&whole, &primitives), expected, "run {run}");
        assert_eq!(quads(&whole), quads(&scene), "run {run}");

        // Cut somewhere, now and then inside a border.
        let borders: Vec<usize> = scene
            .paint_operations
            .iter()
            .enumerate()
            .filter(|(_, operation)| {
                matches!(operation, PaintOperation::FastClip(ClipOp::Border { .. }))
            })
            .map(|(index, _)| index)
            .collect();
        let middle = match (run % 3, borders.is_empty()) {
            (0, false) => {
                // After the border's clip, and after some of its quads.
                let border = borders[rng.random_range(0..borders.len())];
                (border + 1 + rng.random_range(0..4)).min(count - 1)
            }
            _ => rng.random_range(1..count),
        };
        let mut split = Scene::default();
        split.replay_balanced(0..middle, &scene).unwrap();
        // Whether the cut fell inside a border's clip.
        let mut open = Vec::new();
        for operation in &scene.paint_operations[..middle] {
            if let PaintOperation::FastClip(clip) = operation {
                match clip {
                    ClipOp::Pop | ClipOp::EndScroll => {
                        open.pop();
                    }
                    other => open.push(matches!(other, ClipOp::Border { .. })),
                }
            }
        }
        inside += open.last().copied().unwrap_or(false) as usize;
        split.replay_balanced(middle..count, &scene).unwrap();
        assert_eq!(
            orders(&split, &primitives),
            expected,
            "run {run}, split at {middle} of {count}"
        );
        assert_eq!(quads(&split), quads(&scene), "run {run}");
    }
    assert!(inside >= 20, "{inside} splits fell inside a border");
}

/// A page of bordered elements.
struct Page {
    rows: usize,
    bordered: bool,
}

impl Render for Page {
    fn render(&mut self, _: &mut crate::Window, _: &mut crate::Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .children((0..self.rows).map(|row| {
                let element = div()
                    .id(("row", row))
                    .w(px(100.))
                    .h(px(20.))
                    .bg(rgb(0xe9e4d8));
                if self.bordered {
                    element.border_1().border_color(rgb(0x8a8272))
                } else {
                    element
                }
            }))
    }
}

/// A window tells the scene one clip for each bordered element it paints, and the border's
/// quads are the same ones.
#[crate::test]
fn a_window_tells_the_scene_one_clip_for_a_border(cx: &mut TestAppContext) {
    let rows = 5;
    let paint = |cx: &mut TestAppContext, bordered: bool| {
        let window = add_window(cx, move |_, _| Page { rows, bordered });
        with_window(cx, window.into(), |window, cx| {
            window.draw(cx).clear(cx);
            window.draw(cx).clear(cx);
            let scene = &window.rendered_frame.scene;
            (
                told(scene),
                scene.fast_edge_shadows.sets(),
                scene.quads.len(),
            )
        })
    };
    let (plain, plain_sets, plain_quads) = paint(cx, false);
    let (bordered, sets, quad_count) = paint(cx, true);
    // Each border: one clip, with its pop, and its four quads.
    assert_eq!(bordered.0, plain.0, "clips pushed for a strip each");
    assert_eq!(bordered.1, plain.1 + rows, "border clips");
    assert_eq!(bordered.2, plain.2 + rows, "clips popped");
    assert_eq!(quad_count, plain_quads + 4 * rows, "border quads");
    assert!(
        sets <= plain_sets + rows,
        "{sets} sets of entries for {rows} borders"
    );
}

#[test]
fn a_border_tells_the_clip_it_was_given() {
    // The window's bounds, which the strips are inside, are a clip's too.
    let (scene, _) = paint_with(
        &[
            Item::Quad(window()),
            Item::Border(sp(10., 10., 100., 30.), 2., 4.),
        ],
        true,
    );
    assert_eq!(told(&scene), (0, 1, 1));
    assert_eq!(scene.quads.len(), 5);
}
