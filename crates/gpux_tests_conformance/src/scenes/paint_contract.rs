//! The paint contract: inset shadows, gradient interpolation, pattern backgrounds, image alpha and
//! 1 px border anti-aliasing, probed against values derived from the contract itself, so the same
//! numbers judge every renderer and backend.
//!
//! Paint is premultiplied, gamma-encoded sRGB and composites on encoded values ([`over`]). Every
//! probe is placed and valued so the failure mode it guards against (an ignored inset shadow,
//! straight-alpha interpolation, a pattern that fades alpha alone, a straight sample read as
//! premultiplied) lands outside its tolerance; its comment names the value that failure gives.
//! The scenes keep no goldens.

use super::common::*;
use crate::{
    probe::Rgba,
    scene::{Area, Expect, Lane, LogicalRect, SceneSpec, Step},
};
use gpux::prelude::*;
use gpux::{
    AnyElement, BoxShadow, Color, ColorSpace, Context, IntoElement, Render, RenderImage, Window,
    checkerboard, div, hsla, img, linear_color_stop, linear_gradient, pattern_slash, point,
    prepare_gradient_stop_color, px, rgb,
};
use std::sync::{Arc, LazyLock};

const PIXEL_LANES: &[Lane] = &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F];

/// Pixels that nothing translucent paints: untouched backdrop and opaque paint, up to one level
/// of rounding.
const EXACT: u8 = 1;

/// Translucent paint composited over a backdrop: the premultiplication and the blend each round.
const COMPOSITE: u8 = 2;

/// Gradient samples: the composite's rounding plus the gradient dither (±2 levels before
/// rounding), and nothing more.
const GRADIENT: u8 = 4;

/// A straight-alpha color in encoded sRGB, channels and alpha in `0..=1`.
pub type Straight = [f32; 4];

/// `source`, a straight-alpha color, composited over the opaque `backdrop` as the paint contract
/// composites: on encoded sRGB values, `source * alpha + backdrop * (1 - alpha)` per channel,
/// rounded to 8 bits.
pub fn over(source: Straight, backdrop: Rgba) -> Rgba {
    let alpha = source[3].clamp(0., 1.);
    let channel = |index: usize| {
        (source[index].clamp(0., 1.) * 255. * alpha + f32::from(backdrop[index]) * (1. - alpha))
            .round() as u8
    };
    [channel(0), channel(1), channel(2), 255]
}

/// A straight-alpha color as paint.
fn paint(color: Straight) -> gpux::Rgba {
    gpux::Rgba {
        r: color[0],
        g: color[1],
        b: color[2],
        a: color[3],
    }
}

// --- inset shadows ---

/// The cards' fill, mid-grey.
const CARD: Rgba = GRAY;
/// The cards' logical size and their origins.
const CARD_SIZE: f32 = 80.;
const SQUARE_CARD: (f32, f32) = (20., 20.);
const ROUNDED_CARD: (f32, f32) = (120., 20.);
const CARD_RADIUS: f32 = 16.;

fn inset_shadows() -> AnyElement {
    let card = |name: &'static str, (left, top): (f32, f32)| {
        div()
            .id(name)
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(CARD_SIZE))
            .h(px(CARD_SIZE))
            .bg(rgb(GRAY_HEX))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.8).into(),
                offset: point(px(0.), px(0.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: true,
            }])
    };
    root()
        .child(card("inset-square", SQUARE_CARD))
        .child(card("inset-rounded", ROUNDED_CARD).rounded(px(CARD_RADIUS)))
        .into_any_element()
}

/// The probes of one card with its top-left corner at (`left`, `top`).
fn inset_card_probes((left, top): (f32, f32), steps: &mut Vec<Step>) {
    let (right, bottom) = (left + CARD_SIZE, top + CARD_SIZE);
    let (center_x, center_y) = (left + CARD_SIZE / 2., top + CARD_SIZE / 2.);
    // Five blur radii from every edge, the shadow does not reach the centre. An inset shadow
    // painted without its blurred hole covers the whole card and darkens it to about 26.
    steps.push(Step::Probe(center_x, center_y, CARD, EXACT));
    // Just inside each edge the shadow darkens the card to about 85; an ignored inset shadow
    // leaves it the centre's grey, a difference of 0.
    let column: fn(f32, f32) -> LogicalRect = |x, y| (x, y - 2., 2., 4.);
    let row: fn(f32, f32) -> LogicalRect = |x, y| (x - 2., y, 4., 2.);
    for (inside, centre) in [
        (column(left + 1., center_y), column(center_x - 1., center_y)),
        (
            column(right - 3., center_y),
            column(center_x - 1., center_y),
        ),
        (row(center_x, top + 1.), row(center_x, center_y - 1.)),
        (row(center_x, bottom - 3.), row(center_x, center_y - 1.)),
    ] {
        steps.push(Step::ProbeDiffer(inside, centre, 16));
    }
    // Just outside each edge is the white backdrop; an inset shadow drawn outside its element
    // darkens it to about 140.
    for (x, y) in [
        (left - 2., center_y),
        (right + 1., center_y),
        (center_x, top - 2.),
        (center_x, bottom + 1.),
    ] {
        steps.push(Step::Probe(x, y, WHITE, EXACT));
    }
}

pub const INSET_SHADOWS: SceneSpec = SceneSpec {
    id: "paint-contract-inset-shadows",
    areas: &[Area::InsetShadows],
    size: (220., 120.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, inset_shadows),
    script: || {
        let mut steps = vec![
            Step::Expect(Expect::Bounds(
                id("inset-square"),
                (SQUARE_CARD.0, SQUARE_CARD.1, CARD_SIZE, CARD_SIZE),
                0.01,
            )),
            Step::Expect(Expect::Bounds(
                id("inset-rounded"),
                (ROUNDED_CARD.0, ROUNDED_CARD.1, CARD_SIZE, CARD_SIZE),
                0.01,
            )),
        ];
        inset_card_probes(SQUARE_CARD, &mut steps);
        inset_card_probes(ROUNDED_CARD, &mut steps);
        // Inside the rounded card's bounding box but outside its corner curve (4.5 px past it):
        // the backdrop. An inset shadow clipped to the bounding box instead of the element's
        // corner radii darkens it to about 95.
        let (left, top) = ROUNDED_CARD;
        let far = CARD_SIZE - 2.;
        for (x, y) in [(1., 1.), (far, 1.), (1., far), (far, far)] {
            steps.push(Step::Probe(left + x, top + y, WHITE, EXACT));
        }
        steps
    },
};

// --- gradient interpolation ---

/// The gradients' logical size and their left edge. The width is odd, so at scale 1 the probe
/// at its middle samples the pixel whose centre is the middle of the gradient line.
const GRADIENT_SIZE: (f32, f32) = (161., 24.);
const GRADIENT_LEFT: f32 = 20.;
/// The gradients' top edges.
const SRGB_FADE_TOP: f32 = 20.;
const OKLAB_TOP: f32 = 54.;
const OKLAB_FADE_TOP: f32 = 88.;

const TRANSPARENT_BLACK: gpux::Rgba = gpux::Rgba {
    r: 0.,
    g: 0.,
    b: 0.,
    a: 0.,
};

/// The color of a two-stop gradient from `from` to `to` at `t` along its line, as the paint
/// contract defines it: the stops prepared for interpolation as the renderers prepare them
/// (`prepare_gradient_stop_color`), mixed premultiplied, and an OKLab mix unpremultiplied,
/// converted to encoded sRGB with `gpux_color`'s conversion and clamped to the display gamut.
pub fn gradient_at(
    from: impl Into<Color>,
    to: impl Into<Color>,
    space: ColorSpace,
    t: f32,
) -> Straight {
    let [from, to] =
        [from.into(), to.into()].map(|color| prepare_gradient_stop_color(&color, space));
    let mixed: [f32; 4] = std::array::from_fn(|index| from[index] + (to[index] - from[index]) * t);
    let alpha = mixed[3];
    if alpha <= 0. {
        return [0.; 4];
    }
    let [first, second, third] = [mixed[0], mixed[1], mixed[2]].map(|channel| channel / alpha);
    match space {
        ColorSpace::Srgb => [first, second, third, alpha],
        ColorSpace::Oklab => {
            let color = Color::from_oklab(first.into(), second.into(), third.into(), 1.)
                .expect("a mix of finite OKLab stops is finite");
            let mut linear = color.to_linear_srgb();
            linear.r = linear.r.clamp(0., 1.);
            linear.g = linear.g.clamp(0., 1.);
            linear.b = linear.b.clamp(0., 1.);
            let [red, green, blue] = linear.to_encoded_srgb();
            [red as f32, green as f32, blue as f32, alpha]
        }
    }
}

fn gradients() -> AnyElement {
    let bar = |name: &'static str, top: f32, background: gpux::Background| {
        div()
            .id(name)
            .absolute()
            .left(px(GRADIENT_LEFT))
            .top(px(top))
            .w(px(GRADIENT_SIZE.0))
            .h(px(GRADIENT_SIZE.1))
            .bg(background)
    };
    let red = || linear_color_stop(rgb(RED_HEX), 0.);
    root()
        .child(bar(
            "gradient-srgb-fade",
            SRGB_FADE_TOP,
            linear_gradient(90., [red(), linear_color_stop(TRANSPARENT_BLACK, 1.)]),
        ))
        .child(bar(
            "gradient-oklab",
            OKLAB_TOP,
            linear_gradient(90., [red(), linear_color_stop(rgb(BLUE_HEX), 1.)])
                .color_space(ColorSpace::Oklab),
        ))
        .child(bar(
            "gradient-oklab-fade",
            OKLAB_FADE_TOP,
            linear_gradient(90., [red(), linear_color_stop(TRANSPARENT_BLACK, 1.)])
                .color_space(ColorSpace::Oklab),
        ))
        .into_any_element()
}

pub const GRADIENTS: SceneSpec = SceneSpec {
    id: "paint-contract-gradients",
    areas: &[Area::GradientInterpolation],
    size: (201., 132.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, gradients),
    script: || {
        let middle = GRADIENT_LEFT + GRADIENT_SIZE.0 / 2.;
        let at_middle = |top: f32| top + GRADIENT_SIZE.1 / 2.;
        let red = rgb(RED_HEX);
        let mut steps: Vec<Step> = [
            ("gradient-srgb-fade", SRGB_FADE_TOP),
            ("gradient-oklab", OKLAB_TOP),
            ("gradient-oklab-fade", OKLAB_FADE_TOP),
        ]
        .into_iter()
        .map(|(name, top)| {
            Step::Expect(Expect::Bounds(
                id(name),
                (GRADIENT_LEFT, top, GRADIENT_SIZE.0, GRADIENT_SIZE.1),
                0.01,
            ))
        })
        .collect();
        // Red to transparent black in sRGB, interpolated premultiplied as CSS specifies: red at
        // half alpha over white, about (255, 128, 128). Straight-alpha interpolation mixes in
        // the black and gives about (191, 128, 128).
        steps.push(Step::Probe(
            middle,
            at_middle(SRGB_FADE_TOP),
            over(
                gradient_at(red, TRANSPARENT_BLACK, ColorSpace::Srgb, 0.5),
                WHITE,
            ),
            GRADIENT,
        ));
        // Red to blue in OKLab: the OKLab midpoint, about (140, 83, 162). Interpolating in sRGB
        // instead gives (128, 0, 128).
        steps.push(Step::Probe(
            middle,
            at_middle(OKLAB_TOP),
            over(
                gradient_at(red, rgb(BLUE_HEX), ColorSpace::Oklab, 0.5),
                WHITE,
            ),
            GRADIENT,
        ));
        // Red to transparent black in OKLab, premultiplied: unpremultiplied, the mix is red
        // again, at half alpha over white, about (255, 128, 128). Mixing straight OKLab values
        // darkens it toward black, to about (177, 128, 128).
        steps.push(Step::Probe(
            middle,
            at_middle(OKLAB_FADE_TOP),
            over(
                gradient_at(red, TRANSPARENT_BLACK, ColorSpace::Oklab, 0.5),
                WHITE,
            ),
            GRADIENT,
        ));
        steps
    },
};

// --- pattern backgrounds ---

/// What the patterns paint: blue at half alpha, over a red backdrop.
const PATTERN_PAINT: Straight = [0., 0., 1., 0.5];
const PATTERN_BACKDROP: Rgba = RED;
/// The checkerboard's cell and the slash pattern's stripe and gap, in logical pixels. Pattern
/// geometry is in device pixels and does not scale with the window, so the scene multiplies it
/// by the window's scale factor: the cells and stripes then sit at the same logical places on
/// every host, whose scale may be 3 or fractional (2.625) on mobile devices. The probes below
/// stay at least two logical pixels inside one cell or stripe at every such scale.
const CHECKER_CELL: f32 = 16.;
const STRIPE: f32 = 10.;
const STRIPE_GAP: f32 = 16.;
const CHECKERBOARD: (f32, f32) = (20., 20.);
const SLASHES: (f32, f32) = (72., 20.);

/// The patterns, rebuilt at the window's current scale factor.
struct Patterns;

impl Render for Patterns {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        patterns(window.scale_factor())
    }
}

fn patterns(scale: f32) -> AnyElement {
    let swatch = |name: &'static str, (left, top): (f32, f32), width: f32| {
        div()
            .id(name)
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(32.))
    };
    root()
        .bg(rgb(RED_HEX))
        .child(
            swatch("checkerboard", CHECKERBOARD, 32.)
                .bg(checkerboard(paint(PATTERN_PAINT), CHECKER_CELL * scale)),
        )
        .child(swatch("slashes", SLASHES, 64.).bg(pattern_slash(
            paint(PATTERN_PAINT),
            STRIPE * scale,
            STRIPE_GAP * scale,
        )))
        .into_any_element()
}

pub const PATTERNS: SceneSpec = SceneSpec {
    id: "paint-contract-patterns",
    areas: &[Area::PatternBackgrounds],
    size: (156., 72.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: false,
    build: |_, cx| cx.new(|_| Patterns).into(),
    script: || {
        let painted = over(PATTERN_PAINT, PATTERN_BACKDROP);
        let mut steps = vec![
            Step::Expect(Expect::Bounds(
                id("checkerboard"),
                (CHECKERBOARD.0, CHECKERBOARD.1, 32., 32.),
                0.01,
            )),
            Step::Expect(Expect::Bounds(
                id("slashes"),
                (SLASHES.0, SLASHES.1, 64., 32.),
                0.01,
            )),
        ];
        // A painted cell or stripe is half-alpha blue over red, (128, 0, 128). An unpainted cell
        // or gap is exactly the backdrop; a pattern that fades alpha alone leaves the paint's
        // premultiplied blue there and tints it to (255, 0, 128).
        let (left, top) = CHECKERBOARD;
        for (x, y, expected, tolerance) in [
            (2., 2., PATTERN_BACKDROP, EXACT),
            (26., 2., painted, COMPOSITE),
            (2., 26., painted, COMPOSITE),
            (26., 26., PATTERN_BACKDROP, EXACT),
        ] {
            steps.push(Step::ProbeUniform(
                (left + x, top + y, 4., 4.),
                expected,
                tolerance,
            ));
        }
        let (left, top) = SLASHES;
        for (x, y, expected, tolerance) in [
            // Stripe centres lie where x + y is a multiple of the 26 px period, gap centres
            // half a period further.
            (13., 13., painted, COMPOSITE),
            (40., 12., painted, COMPOSITE),
            (6., 7., PATTERN_BACKDROP, EXACT),
            (25., 14., PATTERN_BACKDROP, EXACT),
        ] {
            steps.push(Step::Probe(left + x, top + y, expected, tolerance));
        }
        steps
    },
};

// --- image alpha ---

/// The image's quadrants as straight-alpha colors: opaque red, red at alpha 128, white at alpha
/// 64 and green at alpha 0.
const QUADRANT_COLORS: [[u8; 4]; 4] = [
    [255, 0, 0, 255],
    [255, 0, 0, 128],
    [255, 255, 255, 64],
    [0, 255, 0, 0],
];

/// A 16×16 image of four 8×8 quadrants in [`QUADRANT_COLORS`] (top left, top right, bottom left,
/// bottom right), in the straight-alpha BGRA frames images deliver.
static TRANSLUCENT_QUADRANTS: LazyLock<Arc<RenderImage>> = LazyLock::new(|| {
    let mut pixels = image::RgbaImage::new(16, 16);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let [red, green, blue, alpha] =
            QUADRANT_COLORS[usize::from(y >= 8) * 2 + usize::from(x >= 8)];
        *pixel = image::Rgba([blue, green, red, alpha]);
    }
    Arc::new(RenderImage::new([image::Frame::new(pixels)]))
});

const IMAGE_ORIGIN: f32 = 10.;
const IMAGE_SIZE: f32 = 96.;

fn image_alpha() -> AnyElement {
    root()
        .bg(rgb(BLACK_HEX))
        .child(
            div()
                .id("translucent-image")
                .absolute()
                .left(px(IMAGE_ORIGIN))
                .top(px(IMAGE_ORIGIN))
                .child(
                    img(TRANSLUCENT_QUADRANTS.clone())
                        .w(px(IMAGE_SIZE))
                        .h(px(IMAGE_SIZE)),
                ),
        )
        .into_any_element()
}

pub const IMAGE_ALPHA: SceneSpec = SceneSpec {
    id: "paint-contract-image-alpha",
    areas: &[Area::ImageAlpha],
    size: (116., 116.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, image_alpha),
    script: || {
        let mut steps = vec![Step::Expect(Expect::Bounds(
            id("translucent-image"),
            (IMAGE_ORIGIN, IMAGE_ORIGIN, IMAGE_SIZE, IMAGE_SIZE),
            0.01,
        ))];
        // Quadrant centres, far from the filtered edges between quadrants. Each straight sample
        // composites over black: red at 128 is about (128, 0, 0) and white at 64 about
        // (64, 64, 64), and transparent green leaves the backdrop. A sprite that reads the
        // straight sample as premultiplied gives (255, 0, 0), (255, 255, 255) and (0, 255, 0).
        let near = IMAGE_ORIGIN + IMAGE_SIZE / 4.;
        let far = IMAGE_ORIGIN + IMAGE_SIZE * 3. / 4.;
        for ((x, y), [red, green, blue, alpha]) in
            [(near, near), (far, near), (near, far), (far, far)]
                .into_iter()
                .zip(QUADRANT_COLORS)
        {
            let source = [red, green, blue, alpha].map(|channel| f32::from(channel) / 255.);
            let tolerance = if alpha == 0 || alpha == 255 {
                EXACT
            } else {
                COMPOSITE
            };
            steps.push(Step::Probe(x, y, over(source, BLACK), tolerance));
        }
        steps
    },
};

// --- border anti-aliasing ---

/// The dark theme backdrop of RayX's border-antialiasing page.
const BEADING_BACKDROP_HEX: u32 = 0x0a0a0a;
/// The accent ring's color.
const ACCENT_HEX: u32 = 0x0e7490;
/// The rings' and the thick box's logical size, and their top edge.
const RING_SIZE: f32 = 48.;
const RING_TOP: f32 = 24.;
/// The left edges of the white, accent and white-at-15% rings and of the thick box.
const RING_LEFTS: [f32; 3] = [24., 96., 168.];
const THICK_LEFT: f32 = 240.;

/// RayX's beading limits (its `rayx-border-antialiasing.spec.ts`), measured on WebGPU with the
/// border-edge coverage correction the renderers apply: the largest coefficient of variation of
/// each ring's per-angle energy at scale 1, and of every ring at scale 2. Uncorrected
/// encoded-sRGB coverage measured 0.23, 0.20 and 0.16 at scale 1. Change them only with the
/// owner's agreement, recorded here.
const RING_LIMITS: [(&str, f32); 3] = [
    ("ring-white", 0.15),
    ("ring-accent", 0.17),
    ("ring-neutral-15", 0.12),
];
const RING_LIMIT_AT_2X: f32 = 0.06;

/// RayX's border-antialiasing page: 1 px rounded borders on the dark theme backdrop, whose edge
/// coverage must keep each ring evenly bright around its whole circumference, and a 4 px border
/// that must stay solid.
fn border_beading() -> AnyElement {
    let ring = |name: &'static str, left: f32, color: gpux::Hsla| {
        div()
            .id(name)
            .absolute()
            .left(px(left))
            .top(px(RING_TOP))
            .w(px(RING_SIZE))
            .h(px(RING_SIZE))
            .rounded_full()
            .border_1()
            .border_color(color)
    };
    let [white, accent, neutral] = RING_LEFTS;
    root()
        .bg(rgb(BEADING_BACKDROP_HEX))
        .child(ring("ring-white", white, hsla(0., 0., 1., 1.)))
        .child(ring("ring-accent", accent, rgb(ACCENT_HEX).into()))
        .child(ring("ring-neutral-15", neutral, hsla(0., 0., 1., 0.15)))
        .child(
            div()
                .id("thick-white")
                .absolute()
                .left(px(THICK_LEFT))
                .top(px(RING_TOP))
                .w(px(RING_SIZE))
                .h(px(RING_SIZE))
                .rounded(px(8.))
                .border_4()
                .border_color(rgb(WHITE_HEX)),
        )
        .into_any_element()
}

pub const BORDER_BEADING: SceneSpec = SceneSpec {
    id: "paint-contract-border-beading",
    areas: &[Area::BorderAntialiasing],
    size: (312., 96.),
    scales: &[1., 2.],
    lanes: PIXEL_LANES,
    golden: false,
    build: |_, cx| static_view(cx, border_beading),
    script: || {
        let mut steps: Vec<Step> = RING_LIMITS
            .iter()
            .zip(RING_LEFTS)
            .map(|((name, _), left)| {
                Step::Expect(Expect::Bounds(
                    id(name),
                    (left, RING_TOP, RING_SIZE, RING_SIZE),
                    0.01,
                ))
            })
            .collect();
        steps.push(Step::Expect(Expect::Bounds(
            id("thick-white"),
            (THICK_LEFT, RING_TOP, RING_SIZE, RING_SIZE),
            0.01,
        )));
        // Beading: uncorrected coverage dims each ring along its diagonals, where every pixel
        // is partly covered, to about 0.2 at scale 1.
        for (name, limit) in RING_LIMITS {
            steps.push(Step::EvenRing(id(name), limit, RING_LIMIT_AT_2X));
        }
        // The middle of the 4 px border is fully covered and keeps the exact border color. A
        // border drawn thinner, or only partly covering its middle, lets the #0a0a0a backdrop
        // through, down to (10, 10, 10).
        steps.push(Step::Probe(
            THICK_LEFT + 2.,
            RING_TOP + RING_SIZE / 2.,
            WHITE,
            EXACT,
        ));
        steps
    },
};
