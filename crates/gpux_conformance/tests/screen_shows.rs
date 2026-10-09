//! Screenshot checks of a whole screen (lane E's startup and lifecycle checks) look for a known
//! pixel of the app, so a screen that shows only the system bars, or nothing, cannot pass, while
//! a 16-bit screen's color rounding can.

use gpux_conformance::evaluate::screen_shows;
use image::{Rgba, RgbaImage};

const RED: [u8; 4] = [255, 0, 0, 255];

/// A 360x640 logical screen at scale 2 with the gesture-navigation pill at the bottom.
fn screen(background: [u8; 4]) -> RgbaImage {
    let mut frame = RgbaImage::from_pixel(720, 1280, Rgba(background));
    for y in 1250..1258 {
        for x in 256..464 {
            frame.put_pixel(x, y, Rgba([102, 102, 102, 255]));
        }
    }
    frame
}

/// `frame` with a block from logical (40, 40) to (160, 120) in `color`.
fn with_block(mut frame: RgbaImage, color: [u8; 4]) -> RgbaImage {
    for y in 80..240 {
        for x in 80..320 {
            frame.put_pixel(x, y, Rgba(color));
        }
    }
    frame
}

#[test]
fn system_bars_alone_or_an_empty_screen_do_not_pass() {
    // Only the system's own pixels: the app drew nothing.
    assert!(screen_shows(&screen([255, 255, 255, 255]), 2., 100., 80., RED).is_err());
    // A black screen, as a lost surface leaves it.
    assert!(screen_shows(&screen([0, 0, 0, 255]), 2., 100., 80., RED).is_err());
    // Another color where the app's block should be.
    let blue = with_block(screen([255, 255, 255, 255]), [0, 0, 255, 255]);
    assert!(screen_shows(&blue, 2., 100., 80., RED).is_err());
}

#[test]
fn the_apps_known_pixel_passes_at_full_or_16_bit_depth() {
    let full = with_block(screen([255, 255, 255, 255]), RED);
    screen_shows(&full, 2., 100., 80., RED).unwrap();
    // A gray block on a 16-bit screen: 128 shows as (132, 130, 132).
    let gray = [128, 128, 128, 255];
    let rounded = with_block(screen([255, 255, 255, 255]), [132, 130, 132, 255]);
    screen_shows(&rounded, 2., 100., 80., gray).unwrap();
    // The same rounding on a screen that is not 16-bit is a different color: one full-depth
    // neighbor in the probed area, and the 4-level difference exceeds the ordinary tolerance.
    let mut not_rgb565 = rounded;
    not_rgb565.put_pixel(201, 160, Rgba([133, 130, 132, 255]));
    assert_eq!(not_rgb565.get_pixel(200, 160).0, [132, 130, 132, 255]);
    assert!(screen_shows(&not_rgb565, 2., 100., 80., gray).is_err());
}
