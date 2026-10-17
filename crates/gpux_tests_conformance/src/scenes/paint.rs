//! Media and paint: raster images, SVG, canvas paths, borders, shadows, gradients, opacity,
//! clipping, and sRGB compositing.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyElement, Background, BoxShadow, IntoElement, RenderImage, canvas, div, hsla, img,
    linear_color_stop, linear_gradient, point, px, rgb, rgba, svg,
};
use std::sync::{Arc, LazyLock};

const PIXEL_LANES: &[Lane] = &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F];

/// A 16×16 image with red, green, blue and white quadrants.
static QUADRANTS: LazyLock<Arc<RenderImage>> = LazyLock::new(|| {
    let mut pixels = image::RgbaImage::new(16, 16);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        // RenderImage frames are BGRA.
        *pixel = image::Rgba(match (x < 8, y < 8) {
            (true, true) => [0, 0, 255, 255],
            (false, true) => [0, 255, 0, 255],
            (true, false) => [255, 0, 0, 255],
            (false, false) => [255, 255, 255, 255],
        });
    }
    Arc::new(RenderImage::new([image::Frame::new(pixels)]))
});

fn image_scene() -> AnyElement {
    root()
        .child(
            div()
                .id("photo")
                .absolute()
                .top(px(10.))
                .left(px(10.))
                .child(img(QUADRANTS.clone()).w(px(64.)).h(px(64.))),
        )
        .into_any_element()
}

pub const IMAGE: SceneSpec = SceneSpec {
    id: "media-image",
    areas: &[Area::RasterImage],
    size: (100., 90.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, image_scene),
    script: || {
        vec![
            Step::Expect(Expect::Size(id("photo"), 64., 64., 0.01)),
            Step::probe(26., 26., RED),
            Step::probe(58., 26., GREEN),
            Step::probe(26., 58., BLUE),
            Step::probe(58., 58., WHITE),
        ]
    },
};

const CIRCLE_SVG: &[u8] =
    b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><circle cx='5' cy='5' r='5'/></svg>";

fn svg_scene() -> AnyElement {
    root()
        .child(
            div()
                .id("icon")
                .absolute()
                .top(px(10.))
                .left(px(10.))
                .child(
                    svg()
                        .data(CIRCLE_SVG)
                        .w(px(60.))
                        .h(px(60.))
                        .text_color(rgb(BLUE_HEX)),
                ),
        )
        .into_any_element()
}

pub const SVG: SceneSpec = SceneSpec {
    id: "media-svg",
    areas: &[Area::Svg],
    size: (100., 90.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, svg_scene),
    script: || {
        vec![
            Step::Expect(Expect::Size(id("icon"), 60., 60., 0.01)),
            // SVGs paint as a mask in the text color: the circle's center, not its corners.
            Step::probe(40., 40., BLUE),
            Step::probe(12., 12., WHITE),
            Step::Golden,
        ]
    },
};

fn canvas_scene() -> AnyElement {
    root()
        .child(
            div().id("canvas").size_full().child(
                canvas(
                    |_, _, _| {},
                    |bounds, _, window, _| {
                        let origin = bounds.origin;
                        let mut path = gpux::Path::new(origin + point(px(10.), px(70.)));
                        path.line_to(origin + point(px(90.), px(70.)));
                        path.line_to(origin + point(px(50.), px(10.)));
                        path.line_to(origin + point(px(10.), px(70.)));
                        window.paint_path(path, rgb(RED_HEX));
                    },
                )
                .size_full(),
            ),
        )
        .into_any_element()
}

pub const CANVAS: SceneSpec = SceneSpec {
    id: "paint-canvas-path",
    areas: &[Area::CanvasPath],
    size: (100., 80.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, canvas_scene),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(id("canvas"), (0., 0., 100., 80.), 0.01)),
            Step::probe(50., 55., RED),
            Step::probe(15., 20., WHITE),
            Step::probe(85., 20., WHITE),
            Step::Golden,
        ]
    },
};

fn borders() -> AnyElement {
    let card = |name: &'static str| {
        div()
            .id(name)
            .w(px(60.))
            .h(px(60.))
            .bg(rgb(WHITE_HEX))
            .border(px(4.))
            .border_color(rgb(BLUE_HEX))
    };
    root()
        .flex()
        .gap(px(10.))
        .p(px(10.))
        .child(card("solid"))
        .child(card("dashed").border_dashed())
        .child(card("rounded").rounded(px(16.)))
        .into_any_element()
}

pub const BORDERS: SceneSpec = SceneSpec {
    id: "paint-borders",
    areas: &[Area::Borders, Area::DashedBorders, Area::BorderRadii],
    size: (230., 80.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, borders),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(id("solid"), (10., 10., 60., 60.), 0.01)),
            Step::Expect(Expect::Bounds(id("dashed"), (80., 10., 60., 60.), 0.01)),
            Step::Expect(Expect::Bounds(id("rounded"), (150., 10., 60., 60.), 0.01)),
            // Solid: a continuous border and a white inside.
            Step::ProbeUniform((10., 10., 60., 4.), BLUE, 3),
            Step::probe(40., 40., WHITE),
            // Dashed: the top edge alternates dashes and gaps.
            Step::ProbeDiffer((80., 10., 6., 4.), (88., 10., 6., 4.), 40),
            // Rounded: the corner lies outside the curve, the edge midpoint on the border.
            Step::probe(151., 11., WHITE),
            Step::probe(180., 11., BLUE),
            Step::Golden,
        ]
    },
};

/// Borders over a black backdrop, which shows what a border leaves uncovered: a 1 px pill at odd
/// device-pixel edges (its top and bottom border strips meet in the middle of a 2×2 pixel block
/// at scale 1) and a 4 px dashed box whose gaps must show the backdrop, not the border color.
fn border_strips() -> AnyElement {
    root()
        .bg(rgb(BLACK_HEX))
        .child(
            div()
                .id("pill")
                .absolute()
                .left(px(41.))
                .top(px(41.))
                .w(px(55.))
                .h(px(32.))
                .rounded_full()
                .border_1()
                .border_color(rgb(BLUE_HEX)),
        )
        .child(
            div()
                .id("dashed")
                .absolute()
                .left(px(120.))
                .top(px(10.))
                .w(px(60.))
                .h(px(60.))
                .border(px(4.))
                .border_dashed()
                .border_color(rgb(WHITE_HEX)),
        )
        .into_any_element()
}

pub const BORDER_STRIPS: SceneSpec = SceneSpec {
    id: "paint-border-strips",
    areas: &[Area::Borders, Area::DashedBorders, Area::BorderRadii],
    size: (200., 90.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, border_strips),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(id("pill"), (41., 41., 55., 32.), 0.01)),
            // The pill's inside, along the middle where its top and bottom strips meet and down
            // a column near its left end, shows the backdrop.
            Step::ProbeUniform((46., 55., 44., 4.), BLACK, 3),
            Step::ProbeUniform((45., 50., 2., 14.), BLACK, 3),
            // Outside the rounded corner, on the strips' top row and left column, is backdrop.
            Step::probe(44., 42., BLACK),
            Step::probe(42., 45., BLACK),
            // The dashed box's top edge: a dash, and a gap that shows the backdrop.
            Step::probe(124., 12., WHITE),
            Step::probe(130., 12., BLACK),
        ]
    },
};

fn shadows_gradients() -> AnyElement {
    root()
        .flex()
        .gap(px(30.))
        .p(px(20.))
        .child(
            div()
                .id("shadowed")
                .w(px(60.))
                .h(px(60.))
                .bg(rgb(WHITE_HEX))
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.8).into(),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(8.),
                    spread_radius: px(0.),
                    inset: false,
                }]),
        )
        .child(
            div()
                .id("gradient")
                .w(px(100.))
                .h(px(60.))
                .bg(linear_gradient(
                    90.,
                    [
                        linear_color_stop(rgb(RED_HEX), 0.),
                        linear_color_stop(rgb(BLUE_HEX), 1.),
                    ],
                )),
        )
        .into_any_element()
}

pub const SHADOWS_GRADIENTS: SceneSpec = SceneSpec {
    id: "paint-shadows-gradients",
    areas: &[Area::Shadows, Area::Gradients],
    size: (240., 100.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, shadows_gradients),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(id("shadowed"), (20., 20., 60., 60.), 0.01)),
            Step::Expect(Expect::Bounds(id("gradient"), (110., 20., 100., 60.), 0.01)),
            // The shadow darkens the backdrop just outside the card, not far away.
            Step::ProbeDiffer((14., 40., 4., 4.), (2., 90., 4., 4.), 20),
            Step::probe(50., 50., WHITE),
            // A left-to-right gradient: red at the start, blue at the end.
            Step::Probe(113., 50., RED, 30),
            Step::Probe(207., 50., BLUE, 30),
            Step::ProbeDiffer((140., 40., 4., 4.), (180., 40., 4., 4.), 20),
            Step::Golden,
        ]
    },
};

fn opacity_clipping() -> AnyElement {
    root()
        .flex()
        .gap(px(20.))
        .p(px(20.))
        .child(
            div()
                .id("translucent")
                .w(px(60.))
                .h(px(60.))
                .bg(rgb(RED_HEX))
                .opacity(0.5),
        )
        .child(
            div()
                .id("rounded-clip")
                .w(px(60.))
                .h(px(60.))
                .rounded(px(30.))
                .overflow_hidden()
                .child(div().size_full().bg(rgb(BLUE_HEX))),
        )
        .into_any_element()
}

pub const OPACITY_CLIPPING: SceneSpec = SceneSpec {
    id: "paint-opacity-clipping",
    areas: &[Area::Opacity, Area::Clipping],
    size: (200., 100.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, opacity_clipping),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(
                id("translucent"),
                (20., 20., 60., 60.),
                0.01,
            )),
            Step::Expect(Expect::Bounds(
                id("rounded-clip"),
                (100., 20., 60., 60.),
                0.01,
            )),
            // 50% red over white composites in encoded sRGB.
            Step::Probe(50., 50., [255, 128, 128, 255], 3),
            // The rounded clip turns the square child into a circle.
            Step::probe(130., 50., BLUE),
            Step::probe(102., 22., WHITE),
            Step::Golden,
        ]
    },
};

fn swatches() -> AnyElement {
    let swatch = |name: &'static str, backdrop: u32, paint: gpux::Rgba| {
        div()
            .id(name)
            .w(px(40.))
            .h(px(40.))
            .bg(rgb(backdrop))
            .child(div().size_full().bg(paint))
    };
    root()
        .flex()
        .gap(px(10.))
        .p(px(10.))
        .child(swatch("half-white-on-black", BLACK_HEX, rgba(0xffffff80)))
        .child(swatch("faint-white-on-ink", 0x0a0a0a, rgba(0xffffff0b)))
        .child(swatch("half-blue-on-red", RED_HEX, rgba(0x0000ff80)))
        .child(
            div()
                .id("background-prepared")
                .w(px(40.))
                .h(px(40.))
                .bg(Background::from(rgb(GREEN_HEX))),
        )
        .into_any_element()
}

pub const SRGB: SceneSpec = SceneSpec {
    id: "color-srgb-compositing",
    areas: &[Area::SrgbCompositing],
    size: (210., 60.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: true,
    build: |_, cx| static_view(cx, swatches),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(
                id("half-white-on-black"),
                (10., 10., 40., 40.),
                0.01,
            )),
            Step::Expect(Expect::Bounds(
                id("background-prepared"),
                (160., 10., 40., 40.),
                0.01,
            )),
            // Blending happens on encoded sRGB values, as in Figma and CSS: 50% white over
            // black is 128, not the 188 linear-light blending would give.
            Step::Probe(30., 30., [128, 128, 128, 255], 2),
            // White at 4.3% over #0a0a0a stays near #151515, not #3e3e3e.
            Step::Probe(80., 30., [21, 21, 21, 255], 2),
            Step::Probe(130., 30., [127, 0, 128, 255], 2),
            Step::probe(180., 30., GREEN),
            Step::Golden,
        ]
    },
};
