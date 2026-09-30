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
