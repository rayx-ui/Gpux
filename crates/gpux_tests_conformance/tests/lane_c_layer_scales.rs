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
//! cargo test -p gpux-tests-conformance --test lane_c_layer_scales
//! ```

mod scroll_layers_support;

use gpux::prelude::FluentBuilder as _;
use gpux::{
    AnyWindowHandle, AppContext as _, Context, FastMode, IntoElement, ParentElement, Render,
    RenderImage, Styled, TestAppContext, TestWindowOptions, VisualTestContext, Window, div, img,
    point, px, rgb, size,
};
use scroll_layers_support::*;
use std::sync::{Arc, LazyLock};

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

/// Rounded, bordered, shadowed cards turned, moved or scaled, in rows of a container that is
/// tried again for a layer every so often and found to hold what no tile can. A transformed
/// element's clip mask is the bounding box of what its transformation makes of the clip's
/// rectangle, and while a layer's content is painted the clip was cut by the painted region,
/// not the viewport: the box came out looser, and a border painted in four strips let through
/// of its ring what the reference holds back, in the frames of the retry.
#[test]
fn transformed_bordered_content_is_equal_to_the_reference() {
    let mut script = scrolls_and_single_pixels();
    script.extend([Step::Wheel(0., -300.), Step::Wheel(0., -211.)]);
    script.extend((0..170).map(|_| Step::Wheel(0., -1.)));
    for scale in [1., 1.5, 2.] {
        compare(Content::TransformedBorders, scale, &script);
    }
}

/// Text painted under a transformation: a card that clips a line of text and is turned or
/// scaled shows, glyph for glyph, what the reference shows, at every scale. Each scale is
/// compared on its own, so a failure names every scale it fails at.
#[test]
fn rotated_text_is_equal_to_the_reference() {
    let failures: Vec<String> = [1., 1.5, 2.]
        .into_iter()
        .flat_map(|scale| {
            [Content::RotatedText, Content::NestedRotatedText].map(move |content| (content, scale))
        })
        .filter_map(|(content, scale)| {
            std::panic::catch_unwind(|| {
                compare(content, scale, &scrolls_and_single_pixels());
            })
            .err()
            .map(|error| {
                let message = error
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| error.downcast_ref::<&str>().map(|text| text.to_string()))
                    .unwrap_or_default();
                format!("{content:?} at {scale}x: {message}")
            })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Text whose turn goes away and comes back, in a container that composites while it is
/// straight and is painted while it is turned.
#[test]
fn text_turned_again_after_a_straight_phase_is_equal_to_the_reference() {
    let mut script = Vec::new();
    for round in 0..4 {
        script.push(Step::Tick);
        script
            .extend((0..40).map(|i| Step::Wheel(0., if (i + round) % 7 == 0 { -3. } else { -1. })));
    }
    script.push(Step::Tick);
    script.extend((0..300).map(|i| Step::Wheel(0., if i % 11 == 0 { -3. } else { -1. })));
    for scale in [1., 1.5, 2.] {
        compare(Content::TurnedAndStraightText, scale, &script);
    }
}

/// A line of text moved to its place by a transformation of its own, a whole number of device
/// pixels, is filtered by hand within its tile where the same line laid out there is sampled:
/// at texel centres the two give the same pixels, to a level (the filter's weights at a texel
/// centre are zero to the last bits of a float, which a software rasterizer's arithmetic can
/// turn into one level where the coverage meets a rounding boundary). This pins the filter to
/// the sampler's picture (its origin, its half texel and its axes), which the layer tests,
/// comparing two windows that both run it, cannot: a half texel off differs by dozens of levels.
#[test]
fn translated_text_is_the_text_drawn_where_it_lands() {
    for scale in [1., 2.] {
        let mut cx = app();
        let native = Run::open(&mut cx, Content::TextNative, scale, false);
        let translated = Run::open(&mut cx, Content::TextTranslated, scale, false);
        let (expected, _) = native.frame(&mut cx);
        let (actual, _) = translated.frame(&mut cx);
        assert_eq!(actual.dimensions(), expected.dimensions());
        let worst = actual
            .pixels()
            .zip(expected.pixels())
            .map(|(a, e)| {
                a.0.iter()
                    .zip(e.0)
                    .map(|(a, e)| a.abs_diff(e))
                    .max()
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        assert!(
            worst <= 1,
            "at {scale}x a line moved by a transformation differs from the line laid out there \
             by {worst} levels"
        );
        cx.quit();
    }
}

/// An image of 16 by 16 texels with alpha, and one of 64 by 64: a gradient with a ring of
/// transparent texels.
fn gradient_image(side: u32) -> Arc<RenderImage> {
    let mut pixels = image::RgbaImage::new(side, side);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let alpha = if (x + y) % 7 == 0 { 96 } else { 255 };
        *pixel = image::Rgba([
            (x * 255 / (side - 1)) as u8,
            (y * 255 / (side - 1)) as u8,
            200,
            alpha,
        ]);
    }
    Arc::new(RenderImage::new([image::Frame::new(pixels)]))
}

static SMALL_IMAGE: LazyLock<Arc<RenderImage>> = LazyLock::new(|| gradient_image(16));
static LARGE_IMAGE: LazyLock<Arc<RenderImage>> = LazyLock::new(|| gradient_image(64));

type Neighbours = &'static [(u32, u32, [u8; 4])];

/// Bright opaque images painted beside the two under test, before them and after them, in
/// sizes that put them next to the images in the atlas on every side (the allocator packs
/// images of one height in a row, and rows above and below): (before, after) of each window.
const CROWDS: [(Neighbours, Neighbours); 4] = [
    // One flat image before: it can sit above the 16 by 16 source.
    (&[(16, 8, [0, 255, 0, 255])], &[]),
    // Wide and tall images after: they can sit below it, and to its right.
    (
        &[],
        &[(48, 16, [255, 128, 0, 255]), (32, 32, [0, 128, 255, 255])],
    ),
    // Squares of the sources' sizes on both sides.
    (
        &[(16, 16, [255, 240, 0, 255]), (64, 64, [255, 255, 255, 255])],
        &[(16, 16, [0, 255, 255, 255]), (64, 64, [255, 0, 255, 255])],
    ),
    // All of them.
    (
        &[
            (16, 16, [255, 240, 0, 255]),
            (64, 64, [255, 255, 255, 255]),
            (16, 8, [0, 255, 0, 255]),
        ],
        &[
            (16, 16, [0, 255, 255, 255]),
            (64, 64, [255, 0, 255, 255]),
            (48, 16, [255, 128, 0, 255]),
            (32, 32, [0, 128, 255, 255]),
        ],
    ),
];

fn solid_image(width: u32, height: u32, color: [u8; 4]) -> Arc<RenderImage> {
    let mut pixels = image::RgbaImage::new(width, height);
    for pixel in pixels.pixels_mut() {
        *pixel = image::Rgba(color);
    }
    Arc::new(RenderImage::new([image::Frame::new(pixels)]))
}

/// Two images, one shown at four times its size and one at a third, each laid out where it
/// shows or moved there by a transformation of its own (a whole number of device pixels). With
/// `crowd`, the bright opaque images of that entry of [`CROWDS`] are painted before them and
/// after them, elsewhere in the window, so that the atlas holds them beside the two.
struct ImageStrip {
    moved: bool,
    crowd: Option<usize>,
}

impl ImageStrip {
    fn place(&self, x: f32, y: f32, image: gpux::Img) -> gpux::AnyElement {
        if self.moved {
            div()
                .absolute()
                .left(px(x - 3.))
                .top(px(y - 5.))
                .child(image.translate(point(px(3.), px(5.))))
                .into_any_element()
        } else {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .child(image)
                .into_any_element()
        }
    }

    /// The bright images of `sizes`, laid out in a row from (`x`, `y`) in the window's lower
    /// part, a fresh image each time.
    fn bright(&self, x: f32, y: f32, sizes: &[(u32, u32, [u8; 4])]) -> gpux::AnyElement {
        let mut at = x;
        div()
            .children(sizes.iter().map(|&(width, height, color)| {
                let element = div()
                    .absolute()
                    .left(px(at))
                    .top(px(y))
                    .child(
                        img(solid_image(width, height, color))
                            .w(px(width as f32))
                            .h(px(height as f32)),
                    )
                    .into_any_element();
                at += width as f32 + 4.;
                element
            }))
            .into_any_element()
    }
}

/// Where the images show, in logical pixels: (left, top, side).
const IMAGES: [(f32, f32, f32); 2] = [(20., 20., 64.), (110., 20., 24.)];

impl Render for ImageStrip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (small, large) = (IMAGES[0], IMAGES[1]);
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .when_some(self.crowd, |root, crowd| {
                root.child(self.bright(10., 96., CROWDS[crowd].0))
            })
            .child(self.place(
                small.0,
                small.1,
                img(SMALL_IMAGE.clone()).w(px(small.2)).h(px(small.2)),
            ))
            .child(self.place(
                large.0,
                large.1,
                img(LARGE_IMAGE.clone()).w(px(large.2)).h(px(large.2)),
            ))
            .when_some(self.crowd, |root, crowd| {
                root.child(self.bright(10., 170., CROWDS[crowd].1))
            })
    }
}

/// A window of `ImageStrip` at `scale`, and its frame.
fn strip_frame(
    cx: &mut TestAppContext,
    scale: f32,
    moved: bool,
    crowd: Option<usize>,
) -> image::RgbaImage {
    let handle = cx.add_window_with_options(
        TestWindowOptions::new(size(px(300.), px(250.))).scale_factor(scale),
        move |_, _| ImageStrip { moved, crowd },
    );
    let window: AnyWindowHandle = handle.into();
    cx.update_window(window, |_, window, _| {
        window.set_fast_mode(FastMode::OFF);
        window.activate_window();
    })
    .expect("the window is open");
    cx.run_until_parked();
    VisualTestContext::from_window(window, cx)
        .capture_screenshot()
        .expect("the window rendered")
}

/// The most levels the pixels of `rect` (logical left, top, width, height) differ by between
/// two frames, `margin` device pixels in from its edges.
fn worst_difference(
    a: &image::RgbaImage,
    b: &image::RgbaImage,
    scale: f32,
    (left, top, side): (f32, f32, f32),
    margin: f32,
) -> u8 {
    let (x0, y0) = (
        (left * scale + margin) as u32,
        (top * scale + margin) as u32,
    );
    let (x1, y1) = (
        ((left + side) * scale - margin) as u32,
        ((top + side) * scale - margin) as u32,
    );
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (a, b) = (a.get_pixel(x, y).0, b.get_pixel(x, y).0);
            a.iter()
                .zip(b)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0)
}

/// A transformed image (a polychrome sprite, magnified and minified) moved by a whole number of
/// device pixels shows the image laid out there, to a few levels (the filter's zero weights are
/// zero to the last bits of a float, and a software rasterizer's blend of a half transparent
/// texel can turn that into two or three), inside the outermost texel, where the untransformed
/// sampler bleeds into the tile next to it and the filter by hand does not. A wrong tile size
/// or position differs by about two hundred.
#[test]
fn translated_images_are_the_images_laid_out_there() {
    for scale in [1., 2.] {
        let mut cx = app();
        let expected = strip_frame(&mut cx, scale, false, None);
        let actual = strip_frame(&mut cx, scale, true, None);
        assert_eq!(actual.dimensions(), expected.dimensions());
        for image in IMAGES {
            // The outer half texel of the source is `side / texels / 2` logical pixels wide
            // at most: two logical pixels for the magnified image, none for the minified.
            let margin = if image.2 > 32. {
                2. * scale + 1.
            } else {
                scale + 1.
            };
            let worst = worst_difference(&actual, &expected, scale, image, margin);
            assert!(
                worst <= 3,
                "at {scale}x the image at ({}, {}) of {} moved by a transformation differs from \
                 the image laid out there by {worst} levels",
                image.0,
                image.1,
                image.2
            );
        }
        cx.quit();
    }
}

/// A transformed image looks the same whatever the atlas holds beside it, its outermost texels
/// included: the filter by hand reads only its own tile (the sampler read the tile next to it
/// at the edge). The same moved images, once alone in the atlas and once with bright images
/// painted before and after them, differ by nothing.
#[test]
fn a_transformed_image_does_not_depend_on_what_the_atlas_holds_beside_it() {
    for scale in [1., 2.] {
        let mut cx = app();
        let alone = strip_frame(&mut cx, scale, true, None);
        for crowd in 0..CROWDS.len() {
            let crowded = strip_frame(&mut cx, scale, true, Some(crowd));
            assert_eq!(alone.dimensions(), crowded.dimensions());
            for image in IMAGES {
                let worst = worst_difference(&crowded, &alone, scale, image, 0.);
                assert!(
                    worst <= 1,
                    "at {scale}x the image at ({}, {}) of {} moved by a transformation differs \
                     by {worst} levels with the images of crowd {crowd} in the atlas beside it",
                    image.0,
                    image.1,
                    image.2
                );
            }
        }
        cx.quit();
    }
}
