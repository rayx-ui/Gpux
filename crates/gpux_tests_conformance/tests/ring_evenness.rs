//! The ring-evenness step (`Step::EvenRing`) on synthetic frames: RayX's beading metric passes an
//! evenly anti-aliased 1 px ring at scales 1 and 2 whatever its sub-pixel phase, and fails a
//! beaded ring and an undrawn one with the coefficient and limit in its message.
//!
//! The rings are the 1 px border of a 48 px `rounded_full` element over `#0a0a0a`, as on RayX's
//! border-antialiasing page. Each pixel's coverage of the border's annulus is integrated
//! analytically (by subsampling); an even ring lets each pixel add light in proportion to its
//! coverage, which is what the renderers' border-edge coverage correction approximates.

use gpux_tests_conformance::evaluate::{
    DeviceRect, RING_BINS, even_ring, ring_evenness, ring_limit,
};
use image::{Rgba, RgbaImage};
use std::f64::consts::{PI, TAU};

/// The dark theme backdrop of RayX's border-antialiasing page.
const BACKDROP: [u8; 3] = [0x0a, 0x0a, 0x0a];
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];

/// RayX's limits for its white ring at scale 1, and for every ring at scale 2.
const WHITE_AT_1X: f32 = 0.15;
const AT_2X: f32 = 0.06;

/// The ring element's logical diameter and the frame's logical size.
const DIAMETER: f32 = 48.;
const FRAME: f32 = 80.;

/// Subsamples per device pixel along each axis when integrating coverage.
const SUBSAMPLES: u32 = 16;

/// How a ring's coverage becomes a pixel.
#[derive(Clone, Copy)]
enum Shading {
    /// Each pixel adds light in proportion to its coverage: an evenly anti-aliased ring.
    Even,
    /// [`Shading::Even`] with the coverage of every other angular bin cut to 40%: a beaded ring.
    Beaded,
    /// Coverage blended on encoded sRGB values with no correction, which over a dark backdrop
    /// adds too little light where a pixel is partly covered.
    Uncorrected,
}

fn decode(value: u8) -> f64 {
    let value = f64::from(value) / 255.;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(light: f64) -> u8 {
    let value = if light <= 0.003_130_8 {
        light * 12.92
    } else {
        1.055 * light.powf(1. / 2.4) - 0.055
    };
    (value * 255.).round().clamp(0., 255.) as u8
}

/// A frame at `scale` showing the 1 px border of a `rounded_full` element of [`DIAMETER`]
/// logical pixels with its top-left corner at logical (`x`, `y`), in `color`. Returns the frame
/// and the element's device-pixel bounds.
fn ring_frame(
    scale: f32,
    (x, y): (f32, f32),
    color: [u8; 3],
    shading: Shading,
) -> (RgbaImage, DeviceRect) {
    let size = (FRAME * scale) as u32;
    let mut frame = RgbaImage::from_pixel(
        size,
        size,
        Rgba([BACKDROP[0], BACKDROP[1], BACKDROP[2], 255]),
    );
    let bounds = (x * scale, y * scale, DIAMETER * scale, DIAMETER * scale);
    let center = (
        f64::from(bounds.0 + bounds.2 / 2.),
        f64::from(bounds.1 + bounds.3 / 2.),
    );
    let outer = f64::from(bounds.2 / 2.);
    let inner = outer - f64::from(scale);
    for (column, row, pixel) in frame.enumerate_pixels_mut() {
        let (left, top) = (f64::from(column), f64::from(row));
        let distance = (left + 0.5 - center.0).hypot(top + 0.5 - center.1);
        if distance < inner - 1. || distance > outer + 1. {
            continue;
        }
        let mut covered = 0;
        for sub_row in 0..SUBSAMPLES {
            for sub_column in 0..SUBSAMPLES {
                let dx = left + (f64::from(sub_column) + 0.5) / f64::from(SUBSAMPLES) - center.0;
                let dy = top + (f64::from(sub_row) + 0.5) / f64::from(SUBSAMPLES) - center.1;
                if (inner..=outer).contains(&dx.hypot(dy)) {
                    covered += 1;
                }
            }
        }
        let mut coverage = f64::from(covered) / f64::from(SUBSAMPLES * SUBSAMPLES);
        if let Shading::Beaded = shading {
            let angle = (top + 0.5 - center.1).atan2(left + 0.5 - center.0);
            let bin = ((angle + PI) / TAU * RING_BINS as f64) as usize % RING_BINS;
            if bin % 2 == 1 {
                coverage *= 0.4;
            }
        }
        for channel in 0..3 {
            pixel.0[channel] = match shading {
                Shading::Even | Shading::Beaded => encode(
                    decode(color[channel]) * coverage + decode(BACKDROP[channel]) * (1. - coverage),
                ),
                Shading::Uncorrected => (f64::from(color[channel]) * coverage
                    + f64::from(BACKDROP[channel]) * (1. - coverage))
                    .round() as u8,
            };
        }
    }
    (frame, bounds)
}

fn limit(scale: f32) -> f32 {
    ring_limit(scale, WHITE_AT_1X, AT_2X)
}

#[test]
fn an_even_ring_passes_at_scales_1_and_2() {
    assert_eq!(limit(1.), WHITE_AT_1X);
    assert_eq!(limit(2.), AT_2X);
    for scale in [1., 2.] {
        for color in [WHITE, [0x0e, 0x74, 0x90]] {
            let (frame, bounds) = ring_frame(scale, (10., 10.), color, Shading::Even);
            even_ring(&frame, bounds, scale, limit(scale)).unwrap_or_else(|error| {
                panic!("an even ring at {scale}x in {color:?} fails: {error:#}")
            });
        }
    }
}

#[test]
fn the_metric_does_not_depend_on_sub_pixel_phase() {
    for scale in [1., 2.] {
        for origin in [(10.25, 10.), (10.5, 10.5), (10.3, 10.7)] {
            let (frame, bounds) = ring_frame(scale, origin, WHITE, Shading::Even);
            even_ring(&frame, bounds, scale, limit(scale)).unwrap_or_else(|error| {
                panic!("an even ring at {scale}x with its corner at {origin:?} fails: {error:#}")
            });
        }
    }
}

#[test]
fn a_beaded_ring_fails_with_its_coefficient_and_the_limit() {
    for scale in [1., 2.] {
        let (frame, bounds) = ring_frame(scale, (10., 10.), WHITE, Shading::Beaded);
        let coefficient = ring_evenness(&frame, bounds, scale).unwrap();
        assert!(
            coefficient >= 0.3,
            "the seeded beading measures {coefficient} at {scale}x"
        );
        let error = even_ring(&frame, bounds, scale, limit(scale))
            .expect_err("a beaded ring passes")
            .to_string();
        assert!(
            error.contains(&format!("{coefficient:.3}"))
                && error.contains(&limit(scale).to_string()),
            "the failure names neither the coefficient {coefficient:.3} nor the limit: {error}"
        );
    }
}

#[test]
fn an_undrawn_ring_fails() {
    for scale in [1., 2.] {
        let size = (FRAME * scale) as u32;
        let frame = RgbaImage::from_pixel(
            size,
            size,
            Rgba([BACKDROP[0], BACKDROP[1], BACKDROP[2], 255]),
        );
        let bounds = (10. * scale, 10. * scale, DIAMETER * scale, DIAMETER * scale);
        let error = even_ring(&frame, bounds, scale, limit(scale))
            .expect_err("an empty frame passes")
            .to_string();
        assert!(error.contains("no light"), "{error}");
    }
}

#[test]
fn uncorrected_coverage_beads_on_the_pixel_grid_at_1x() {
    // RayX measured 0.23 for the white ring with uncorrected encoded-sRGB edge coverage: partly
    // covered pixels add too little light, so the ring's diagonals, where every pixel is partly
    // covered, come out dimmer than its axes.
    let (frame, bounds) = ring_frame(1., (10., 10.), WHITE, Shading::Uncorrected);
    let coefficient = ring_evenness(&frame, bounds, 1.).unwrap();
    assert!(
        coefficient > f64::from(WHITE_AT_1X),
        "uncorrected coverage measures only {coefficient}"
    );
}

#[test]
fn a_ring_outside_the_frame_fails() {
    let (frame, _) = ring_frame(1., (10., 10.), WHITE, Shading::Even);
    let error = ring_evenness(&frame, (60., 60., 48., 48.), 1.)
        .expect_err("a ring outside the frame is measured")
        .to_string();
    assert!(error.contains("outside"), "{error}");
}
