//! The scroll-layer groups' frame comparison: `assert_same` compares the two images' bytes first
//! and only locates the differing pixels on a mismatch, then reports what it always reported.
//! Each test seeds a known difference, so a comparison that stopped looking would fail here.
//!
//! ```text
//! cargo test -p gpux-conformance --test lane_c_compare
//! ```

use image::{Rgba, RgbaImage};
use scroll_layers_support::{assert_same, difference};
use std::panic::{AssertUnwindSafe, catch_unwind};

mod scroll_layers_support;

fn image(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([(x * 7 % 256) as u8, (y * 11 % 256) as u8, 40, 255])
    })
}

/// The panic message of `assert_same(actual, expected, 3, 1.5)`, which must panic.
fn failure(actual: &RgbaImage, expected: &RgbaImage) -> String {
    let error = catch_unwind(AssertUnwindSafe(|| assert_same(actual, expected, 3, 1.5)))
        .expect_err("the images differ, so the comparison must fail");
    error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| error.downcast_ref::<&str>().map(|text| text.to_string()))
        .expect("a text panic message")
}

#[test]
fn equal_images_pass() {
    let reference = image(40, 30);
    assert_same(&reference.clone(), &reference, 0, 1.);
    assert!(difference(&reference.clone(), &reference).is_none());
}

#[test]
fn one_differing_pixel_fails_with_its_place_and_both_colors() {
    let expected = image(40, 30);
    let mut actual = expected.clone();
    actual.put_pixel(17, 9, Rgba([1, 2, 3, 255]));
    let message = failure(&actual, &expected);
    assert_eq!(
        message,
        format!(
            "at 1.5x, after step 3: 1 pixels differ from the reference, within (17, 9)..=(17, 9); \
             first {:?} against {:?}",
            Rgba([1u8, 2, 3, 255]),
            expected.get_pixel(17, 9)
        )
    );
}

#[test]
fn a_differing_corner_is_found() {
    let expected = image(40, 30);
    for corner in [(0, 0), (39, 0), (0, 29), (39, 29)] {
        let mut actual = expected.clone();
        actual.put_pixel(corner.0, corner.1, Rgba([9, 9, 9, 255]));
        let difference = difference(&actual, &expected).expect("the corner differs");
        assert_eq!(
            (difference.count, difference.first, difference.last),
            (1, corner, corner)
        );
        assert!(failure(&actual, &expected).contains(&format!(
            "within ({}, {})..=({}, {})",
            corner.0, corner.1, corner.0, corner.1
        )));
    }
}

#[test]
fn a_differing_row_reports_its_count_and_bounding_box() {
    let expected = image(40, 30);
    let mut actual = expected.clone();
    for x in 0..40 {
        actual.put_pixel(x, 12, Rgba([200, 0, 0, 255]));
    }
    // A second, lower pixel stretches the box the message reports.
    actual.put_pixel(5, 20, Rgba([200, 0, 0, 255]));
    let message = failure(&actual, &expected);
    assert!(
        message.contains("41 pixels differ from the reference, within (0, 12)..=(39, 20)"),
        "{message}"
    );
    assert!(message.starts_with("at 1.5x, after step 3: "), "{message}");
}

#[test]
fn a_difference_in_the_alpha_byte_alone_fails() {
    let expected = image(40, 30);
    let mut actual = expected.clone();
    let mut pixel = *expected.get_pixel(3, 4);
    pixel.0[3] = 254;
    actual.put_pixel(3, 4, pixel);
    assert!(failure(&actual, &expected).contains("1 pixels differ"));
}

#[test]
#[should_panic(expected = "assertion `left == right` failed")]
fn images_of_different_dimensions_fail_as_before() {
    assert_same(&image(40, 30), &image(40, 31), 0, 1.);
}
