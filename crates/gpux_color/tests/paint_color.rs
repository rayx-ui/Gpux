use std::mem::{align_of, size_of};

use gpux_color::{Color, DEFAULT_PAINT_GAMUT_POLICY, PaintColor, PaintGamutPolicy, Srgb};

#[test]
fn paint_color_has_fixed_premultiplied_srgb_abi() {
    assert_eq!(size_of::<PaintColor>(), PaintColor::BYTE_SIZE);
    assert_eq!(size_of::<PaintColor>(), 16);
    assert_eq!(align_of::<PaintColor>(), 4);
    assert_eq!(DEFAULT_PAINT_GAMUT_POLICY, PaintGamutPolicy::ClipSrgb);

    let color = Color::from_srgb(Srgb::new(0.8, 0.4, 0.2), 0.5).expect("finite sRGB color");
    assert_eq!(
        color.paint_color(),
        PaintColor {
            r: 0.4,
            g: 0.2,
            b: 0.1,
            a: 0.5,
        }
    );
}

#[test]
fn paint_color_clips_before_premultiplication_and_caches_once() {
    let color = Color::from_srgb(Srgb::new(-0.25, 0.5, 1.25), 0.8).expect("unbounded sRGB color");
    let clone = color.clone();
    assert_eq!(
        clone.paint_color(),
        PaintColor {
            r: 0.0,
            g: 0.4,
            b: 0.8,
            a: 0.8,
        }
    );
    assert_eq!(color.paint_color(), clone.paint_color());
    assert_eq!(color.paint_color_initialization_count(), 1);

    let changed = color.with_alpha(0.25).expect("changed alpha");
    assert_eq!(changed.paint_color_initialization_count(), 0);
    let _ = changed.paint_color();
    assert_eq!(changed.paint_color_initialization_count(), 1);
    assert_eq!(color.paint_color_initialization_count(), 1);
}

#[test]
fn transparent_black_is_one_color_per_thread() {
    let first = Color::transparent_black();
    let second = Color::default();
    assert_eq!(first, second);
    assert!(first.is_transparent());
    // Preparing one prepares both: they are the same value, worked out once.
    let prepared = first.paint_color();
    assert_eq!(second.paint_color_initialization_count(), 1);
    assert_eq!(second.paint_color(), prepared);
    // Another thread has its own.
    std::thread::spawn(|| {
        assert_eq!(
            Color::transparent_black().paint_color_initialization_count(),
            0
        );
    })
    .join()
    .unwrap();
}

#[test]
fn a_color_faded_to_nothing_is_one_value_worked_out_once() {
    let color = Color::from_encoded_srgb(0.2, 0.4, 0.6, 0.8).unwrap();
    let invisible = color.opacity(0.0);
    assert_eq!(invisible, color.with_alpha(0.0).unwrap());
    assert!(invisible.is_transparent());
    // The same value comes back, with its prepared form, not a new one.
    let again = color.opacity(0.0);
    assert_eq!(again.paint_color(), invisible.paint_color());
    assert_eq!(invisible.paint_color_initialization_count(), 1);
    assert_eq!(again.paint_color_initialization_count(), 1);
    // A factor below zero clamps to the same thing; other factors are unchanged.
    assert_eq!(color.opacity(-1.0), invisible);
    assert_eq!(
        color.opacity(0.5),
        color.with_alpha(color.alpha() * 0.5).unwrap()
    );
}
