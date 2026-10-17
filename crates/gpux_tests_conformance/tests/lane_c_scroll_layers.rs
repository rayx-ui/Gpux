//! Scroll layers in lane C: a window that scrolls its content from GPU tiles shows, frame for
//! frame and byte for byte, what a window without scroll layers shows.
//!
//! Each test opens the same view in two windows of one application, on the platform's headless
//! renderer (the software adapter on Linux and Windows unless `GPUX_WGPU_ADAPTER` picks another,
//! Metal on macOS): one with every fast-mode mechanism on, and one with all of them off and the
//! device-pixel snapping the scroll-layers switch brings forced on, which is the frame scroll
//! layers are held to. Both get the same input, one frame at a time, and every frame is read
//! back from both.
//!
//! No difference is allowed, at the viewport's edges either: a shadow whose box has scrolled
//! out of the viewport still shows the blur that reaches in, drawn from tiles or without them
//! (`fast/edge_shadows.rs` in `gpui`).
//!
//! A test also checks that the layered window did composite tiles, so that a renderer without
//! tiles, which keeps scroll layers off, fails it rather than passing by drawing as before.
//!
//! ```text
//! cargo test -p gpux-tests-conformance --test lane_c_scroll_layers
//! ```

mod scroll_layers_support;

use scroll_layers_support::*;

#[test]
fn text_rows_scroll_from_tiles() {
    let totals = compare(Content::Text, 1., &scrolls());
    assert_composited(&totals, 8);
}

#[test]
fn text_rows_scroll_from_tiles_at_2x() {
    let totals = compare(Content::Text, 2., &scrolls());
    assert_composited(&totals, 8);
}

/// At a fractional scale factor a logical pixel is not a whole number of device pixels: the
/// snapping the reference shares keeps every scroll on device pixels.
#[test]
fn text_rows_scroll_from_tiles_at_a_fractional_scale() {
    let totals = compare(Content::Text, 1.5, &scrolls());
    assert_composited(&totals, 8);
}

/// Cards under a shadow cast downwards: a card's top edge shows at the viewport's bottom while
/// the boxes of its shadows are still below it, and its shadows' blur shows at the viewport's
/// top after their boxes have left.
#[test]
fn cards_with_shadows_and_gradients_scroll_from_tiles() {
    for scale in [1., 2.] {
        let totals = compare(Content::Cards, scale, &scrolls_and_single_pixels());
        assert_composited(&totals, 60);
    }
}

/// Shadowed boxes closer together than their blurs reach, at every one-pixel step of a card's
/// way out of the viewport and into it, and in the scripted scroll: the blur of a shadow whose
/// box is out of the viewport falls on the cards around it, and under them, as the reference
/// draws it.
#[test]
fn close_shadowed_cards_scroll_from_tiles() {
    for content in [Content::TightCards, Content::HeavyShadows] {
        for scale in [1., 2.] {
            let totals = compare(content, scale, &scrolls_and_single_pixels());
            assert_composited(&totals, 90);
        }
    }
}

/// Rows and sections with backgrounds of their own, under shadows that reach past them: the
/// blur of a shadow whose row is wholly out of the viewport falls on the rows in view as it
/// does when its row shows, drawn from tiles or without them, and a tile's edge that falls
/// between a shadow and what it lies on leaves no step in the blur.
#[test]
fn shadows_reaching_over_other_backgrounds_scroll_from_tiles() {
    for content in [Content::ShadowedRows, Content::Sections] {
        for scale in [1., 2.] {
            let mut script = scrolls_and_single_pixels();
            // On past the first tiles' edges, a pixel at a time.
            script.extend([-120., -55., -91.].map(|dy| Step::Wheel(0., dy)));
            script.extend((0..90).map(|_| Step::Wheel(0., -1.)));
            let totals = compare(content, scale, &script);
            assert_composited(&totals, 150);
        }
    }
}

/// What the scene knows of clips one by one, not only their intersection: a card that clips
/// content taller than itself, cut by the viewport, holds nothing under the next card's
/// shadow; a row's border, drawn in a mask of its own, lies under the next row, which reaches
/// over it; and a container with no background lies over the panel on one side and a nest of
/// backgrounds on the other, each behind the rows on its own side.
#[test]
fn shadows_at_clips_within_clips_scroll_from_tiles() {
    for content in [
        Content::ClippedCards,
        Content::OverlappingRows,
        Content::NoBackground,
    ] {
        for scale in [1., 2.] {
            let totals = compare(content, scale, &scrolls_and_single_pixels());
            // A container with no background has none to bake under its tiles, and keeps
            // today's path: its frames are compared all the same.
            if !matches!(content, Content::NoBackground) {
                assert_composited(&totals, 90);
            }
        }
    }
}

/// However far the content is scrolled, and however many clips were painted above the
/// viewport on the way (a border is four), the shadows at its edges are stacked alike.
#[test]
fn shadows_far_down_the_content_scroll_from_tiles() {
    for content in [Content::BorderedRows, Content::Cards] {
        for scale in [1., 2.] {
            let mut script = vec![Step::Wheel(0., -700.), Step::Wheel(0., -800.)];
            script.extend(scrolls_and_single_pixels());
            let totals = compare(content, scale, &script);
            assert_composited(&totals, 90);
        }
    }
}

/// What is painted after a scrolling container, over its edge, lies over everything in it,
/// the blurs of its shadows too, drawn from tiles or without them.
#[test]
fn a_bar_painted_over_a_containers_edge_lies_over_its_shadows() {
    for scale in [1., 2.] {
        let totals = compare(Content::UnderABar, scale, &scrolls_and_single_pixels());
        assert_composited(&totals, 90);
    }
}

/// Two containers laid one over the other clip alike, and something is painted between them:
/// the upper one's shadows are stacked by what it holds, not by what the lower one dropped
/// where it stands.
#[test]
fn containers_laid_over_one_another_scroll_from_tiles() {
    for scale in [1., 2.] {
        let totals = compare(Content::Stacked, scale, &scrolls_and_single_pixels());
        assert_composited(&totals, 90);
    }
}

/// A scrolling container within the scrolled content, cut by the page's viewport as it comes
/// and goes: the bar painted after it lies over the blurs of its shadows, where they show and
/// where the page's viewport cuts them.
#[test]
fn a_scrolling_container_within_scrolled_content_scrolls_from_tiles() {
    for scale in [1., 2.] {
        let totals = compare(Content::Nested, scale, &scrolls_and_single_pixels());
        assert_composited(&totals, 90);
    }
}

/// A list's rows are scenes of their own, ordered when the content is put together: a
/// change of those orders alone, as a row whose background reaches over the row before
/// comes and goes, draws the tiles it reaches again.
#[test]
fn shadowed_list_rows_reaching_over_one_another_scroll_from_tiles() {
    for scale in [1., 2.] {
        let totals = compare(
            Content::AlternatingList,
            scale,
            &scrolls_and_single_pixels(),
        );
        assert!(totals.composited + totals.repainted > 0);
    }
}

/// The content begins below the container's top, by its first card's margin: scrolled a few
/// pixels at a time from the top, the layer still covers what shows, and composites.
#[test]
fn small_scrolls_from_the_top_of_content_with_a_margin_composite() {
    let script: Vec<Step> = (0..40).map(|_| Step::Wheel(0., -3.)).collect();
    let totals = compare(Content::Cards, 1., &script);
    assert_composited(&totals, 34);
}

/// A layer's paths are drawn into the frame over its tiles.
#[test]
fn paths_scroll_over_the_tiles() {
    for scale in [1., 2.] {
        let totals = compare(Content::Paths, scale, &scrolls());
        assert_composited(&totals, 8);
    }
}

#[test]
fn content_scrolls_on_both_axes_from_tiles() {
    let script: Vec<Step> = [
        (0., -20.),
        (0., -20.),
        (0., -20.),
        (-33., 0.),
        (-90.5, -41.),
        (-200., 0.),
        (17., -350.),
        (160., 120.),
        (-5., -5.),
        (-5., -5.),
    ]
    .into_iter()
    .map(|(dx, dy)| Step::Wheel(dx, dy))
    .collect();
    let totals = compare(Content::Wide, 1., &script);
    assert_composited(&totals, 5);
}

/// Content that changes while it is scrolled is painted again, and the tiles the change touched
/// are rasterized again; the frames after it come from tiles again.
#[test]
fn a_change_while_scrolled_shows_and_scrolling_goes_on_from_tiles() {
    let mut script = scrolls();
    script.insert(6, Step::Tick);
    script.extend([-20., -20., -20., -20., -20., -20.].map(|dy| Step::Wheel(0., dy)));
    for content in [Content::Text, Content::Cards] {
        let totals = compare(content, 1., &script);
        assert_composited(&totals, 10);
    }
}

/// A `uniform_list` and a `list` keep their rows in their layer one by one, and extend it by
/// the rows a scroll uncovers: every frame shows what the lists draw without a layer.
#[test]
fn list_rows_scroll_from_tiles() {
    for content in [Content::UniformList, Content::List] {
        for scale in [1., 1.5] {
            let totals = compare(content, scale, &scrolls());
            assert_composited(&totals, 8);
        }
    }
}

/// Rows that change while a list is scrolled show, and the list scrolls on from its tiles.
#[test]
fn a_change_while_a_list_is_scrolled_shows_and_scrolling_goes_on_from_tiles() {
    let mut script = scrolls();
    script.insert(6, Step::Tick);
    script.extend([-20., -20., -20., -20., -20., -20.].map(|dy| Step::Wheel(0., dy)));
    for content in [Content::UniformList, Content::List] {
        let totals = compare(content, 1., &script);
        assert_composited(&totals, 10);
    }
}
