//! Scroll layers at fractional scale factors and under transformations, in lane C: a window
//! that scrolls its content from GPU tiles shows, frame for frame and byte for byte, what a
//! window without scroll layers shows, where a logical pixel is not a whole number of device
//! pixels too.
//!
//! At scale factors 1.25, 1.5 and 1.75 an element's edge, a clip's and a shadow's box are
//! worked out from the scroll offset in logical pixels, a few millionths of a pixel to one
//! side or the other of where they lie. Two things made that show (`fast/layers/paint.rs` in
//! `gpui`): a clip grown to whole pixels took in a row more on the one side, and a shadow's
//! box, which is not snapped, moved its blur by a level in a pixel or two. Content painted
//! transformed is not drawn from tiles at all.
//!
//! The views, the two windows and the comparison are `lane_c_scroll_layers`'s.
//!
//! ```text
//! cargo test -p gpux-conformance --test lane_c_layer_scales
//! ```

mod scroll_layers_support;

use scroll_layers_support::*;

const SCALES: [f32; 3] = [1.25, 1.5, 1.75];

/// Cards and rows under shadows, whose boxes are not snapped to device pixels: every scroll
/// offset leaves each blur where painting it there puts it.
#[test]
fn shadowed_content_scrolls_from_tiles_at_fractional_scales() {
    for content in [
        Content::Cards,
        Content::TightCards,
        Content::HeavyShadows,
        Content::ShadowedRows,
        Content::Sections,
        Content::OverlappingRows,
        Content::BorderedRows,
        Content::UnderABar,
        Content::Stacked,
    ] {
        for scale in SCALES {
            let totals = compare(content, scale, &scrolls_and_single_pixels());
            assert_composited(&totals, 90);
        }
    }
}

/// Cards and containers that clip what they hold: a clip takes in the same rows of pixels
/// wherever the content is scrolled.
#[test]
fn clipped_content_scrolls_from_tiles_at_fractional_scales() {
    for content in [
        Content::ClippedCards,
        Content::ClippedText,
        Content::Nested,
        Content::NoBackground,
    ] {
        for scale in SCALES {
            let totals = compare(content, scale, &scrolls_and_single_pixels());
            // A container with no background keeps today's path.
            if !matches!(content, Content::NoBackground) {
                assert_composited(&totals, 90);
            }
        }
    }
}

/// A scrolling container inside a scrolling container, out of view for part of the scroll,
/// under a shadowed bar painted after it: at 1.3333 and 2.625 what the clips leave of what is
/// out of view is empty, and not the empty box the clips told of (an edge falls between device
/// pixels), which the scene still has to take for them.
#[test]
fn a_container_out_of_view_within_a_container_scrolls_from_tiles_at_fractional_scales() {
    for scale in [1.3333, 2.625] {
        let totals = compare(Content::Nested, scale, &scrolls_and_single_pixels());
        assert_composited(&totals, 90);
    }
}

/// Lists, whose rows are kept one by one.
#[test]
fn lists_scroll_from_tiles_at_fractional_scales() {
    for content in [
        Content::AlternatingList,
        Content::UniformList,
        Content::List,
    ] {
        for scale in SCALES {
            let totals = compare(content, scale, &scrolls_and_single_pixels());
            assert!(totals.composited + totals.repainted > 0);
        }
    }
}

/// Text, paths and content scrolling on both axes, which were equal before, stay so.
#[test]
fn unshadowed_content_scrolls_from_tiles_at_fractional_scales() {
    for content in [Content::Text, Content::Paths, Content::Wide] {
        for scale in SCALES {
            let totals = compare(content, scale, &scrolls_and_single_pixels());
            assert_composited(&totals, 60);
        }
    }
}

/// A card moved or turned by a transformation of its own is shaded from positions a tile
/// cannot reproduce to the last bit: its container is painted, not drawn from tiles, and
/// shows what the reference shows.
#[test]
fn transformed_content_is_painted_and_not_drawn_from_tiles() {
    for content in [Content::TranslatedCards, Content::RotatedCards] {
        for scale in [1., 1.5, 2.] {
            let totals = compare(content, scale, &scrolls_and_single_pixels());
            assert_eq!(
                totals.composited, 0,
                "frames drawn from the tiles of content that is painted transformed"
            );
        }
    }
}
