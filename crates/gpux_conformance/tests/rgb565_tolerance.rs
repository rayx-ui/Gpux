//! Frames shown at 16-bit depth (RGB565, as on Android emulators' GL surfaces) round colors by
//! up to [`RGB565_ROUNDING`] per channel. Color probes allow that rounding on such frames; frames
//! with full 8-bit color and difference checks keep their own tolerances.

use gpux_conformance::{
    Step,
    evaluate::{RGB565_ROUNDING, pixel_step, pixel_step_within, shows_rgb565},
    probe::PixelRect,
};
use image::{Rgba, RgbaImage};
use std::collections::HashMap;

/// `value` rounded to `bits` and widened back to 8 bits by repeating the high bits, as most
/// displays do.
fn through(value: u8, bits: u32) -> u8 {
    let max = (1u32 << bits) - 1;
    let narrow = (u32::from(value) * max + 127) / 255;
    ((narrow << (8 - bits)) | (narrow >> (2 * bits - 8))) as u8
}

/// `value` rounded to `bits` and widened back by rounding to the nearest 8-bit level, as the
/// Android emulator's screenshots do.
fn through_rounding(value: u8, bits: u32) -> u8 {
    let max = (1u32 << bits) - 1;
    let narrow = (u32::from(value) * max + 127) / 255;
    ((narrow * 255 + max / 2) / max) as u8
}

fn rgb565(color: [u8; 4]) -> [u8; 4] {
    [
        through(color[0], 5),
        through(color[1], 6),
        through(color[2], 5),
        color[3],
    ]
}

const GREY: [u8; 4] = [128, 128, 128, 255];

#[test]
fn rgb565_rounding_stays_within_the_bound() {
    for value in 0..=255u8 {
        for bits in [5, 6] {
            for shown in [through(value, bits), through_rounding(value, bits)] {
                assert!(
                    value.abs_diff(shown) <= RGB565_ROUNDING,
                    "{value} shows as {shown} at {bits} bits"
                );
            }
        }
    }
}

#[test]
fn frames_of_widened_rgb565_colors_are_recognised() {
    let shown = RgbaImage::from_fn(16, 16, |x, y| {
        Rgba(rgb565([(x * 16) as u8, (y * 16) as u8, (x * y) as u8, 255]))
    });
    let whole = PixelRect::new(0, 0, 16, 16);
    assert!(shows_rgb565(&shown, whole));
    assert!(!shows_rgb565(
        &RgbaImage::from_pixel(4, 4, Rgba(GREY)),
        whole
    ));
    let mut one_full_color = shown;
    one_full_color.put_pixel(3, 3, Rgba(GREY));
    assert!(!shows_rgb565(&one_full_color, whole));
    // Only the scene's area counts, and the part of it inside the frame.
    assert!(shows_rgb565(&one_full_color, PixelRect::new(4, 0, 40, 40)));

    // The emulator widens 3/31 to 25 where repeating bits gives 24.
    let rounded = RgbaImage::from_pixel(4, 4, Rgba([25, 20, 25, 255]));
    assert_eq!(through_rounding(21, 5), 25);
    assert!(shows_rgb565(&rounded, whole));
}

#[test]
fn color_probes_allow_rgb565_rounding() {
    let frame = RgbaImage::from_pixel(20, 20, Rgba(rgb565(GREY)));
    let mut captures = HashMap::new();
    let probe = Step::Probe(5., 5., GREY, 0);
    let uniform = Step::ProbeUniform((0., 0., 10., 10.), GREY, 0);
    for step in [&probe, &uniform] {
        assert!(pixel_step(step, &frame, 1., &mut captures).is_err());
        pixel_step_within(step, &frame, 1., &mut captures, RGB565_ROUNDING).unwrap();
    }
    let far = Step::Probe(5., 5., [140, 128, 128, 255], 0);
    assert!(pixel_step_within(&far, &frame, 1., &mut captures, RGB565_ROUNDING).is_err());
}

#[test]
fn difference_checks_keep_their_tolerance() {
    let frame = RgbaImage::from_fn(20, 10, |x, _| {
        Rgba(if x < 10 {
            [100, 100, 100, 255]
        } else {
            [103, 100, 100, 255]
        })
    });
    let differ = Step::ProbeDiffer((0., 0., 10., 10.), (10., 0., 10., 10.), 2);
    pixel_step_within(&differ, &frame, 1., &mut HashMap::new(), RGB565_ROUNDING).unwrap();
}
