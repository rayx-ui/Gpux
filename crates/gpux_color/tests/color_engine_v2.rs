use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::sync::{Arc, Barrier};
use std::thread;

use gpux_color::{
    A98Rgb, Color, ColorComponent, ColorSpace, DisplayP3, EncodedColor, HexFallback, HslColor,
    HsvColor, HwbColor, IntoEncodedColor, LabColor, LchColor, LinearSrgb, OklabColor, OklchColor,
    ProPhotoRgb, Rec2020, Srgb, XyzD50, XyzD65,
};

const EPSILON: f64 = 2.0e-6;

static_assertions::assert_impl_all!(Color: Clone, Send, Sync);
static_assertions::assert_not_impl_any!(Color: Copy);

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "expected {actual:.12} to be within {EPSILON} of {expected:.12}"
    );
}

fn assert_xyz_close(actual: XyzD65, expected: XyzD65) {
    assert_close(actual.x, expected.x);
    assert_close(actual.y, expected.y);
    assert_close(actual.z, expected.z);
}

fn encoded(value: impl IntoEncodedColor, alpha: f64) -> EncodedColor {
    EncodedColor::from(value, alpha).expect("finite typed color")
}

#[test]
fn color_is_an_arc_sized_clone_only_xyz_d65_handle() {
    assert_eq!(size_of::<Color>(), size_of::<Arc<()>>());

    let color = Color::from_xyz_d65(XyzD65::new(-0.125, 0.5, 1.25), 0.75)
        .expect("unbounded finite XYZ is valid");
    let clone = color.clone();

    assert_xyz_close(clone.xyz_d65(), XyzD65::new(-0.125, 0.5, 1.25));
    assert_close(clone.alpha(), 0.75);
    assert_eq!(color, clone);
}

#[test]
fn every_dtcg_space_and_hsv_round_trips_through_xyz_d65() {
    let samples = [
        encoded(Srgb::new(0.21, 0.52, 0.83), 0.7),
        encoded(LinearSrgb::new(-0.04, 0.31, 1.12), 0.7),
        encoded(HslColor::new(212.0, 0.67, 0.52), 0.7),
        encoded(HwbColor::new(212.0, 0.12, 0.08), 0.7),
        encoded(LabColor::new(62.0, -18.0, -36.0), 0.7),
        encoded(LchColor::new(62.0, 40.25, 243.435), 0.7),
        encoded(OklabColor::new(0.62, -0.08, -0.18), 0.7),
        encoded(OklchColor::new(0.62, 0.197, 246.04), 0.7),
        encoded(DisplayP3::new(0.15, 0.55, 0.9), 0.7),
        encoded(A98Rgb::new(0.15, 0.55, 0.9), 0.7),
        encoded(ProPhotoRgb::new(0.15, 0.55, 0.9), 0.7),
        encoded(Rec2020::new(0.15, 0.55, 0.9), 0.7),
        encoded(XyzD65::new(0.25, 0.3, 0.6), 0.7),
        encoded(XyzD50::new(0.24, 0.3, 0.46), 0.7),
        encoded(HsvColor::new(212.0, 0.75, 0.83), 0.7),
    ];

    for source in samples {
        let color = Color::from_encoded(source).expect("finite encoded color");
        let round_trip = color.to_encoded(source.space());
        assert_eq!(round_trip.space(), source.space());
        assert_close(round_trip.alpha(), source.alpha());
        for (actual, expected) in round_trip.components().iter().zip(source.components()) {
            match (actual, expected) {
                (ColorComponent::Number(actual), ColorComponent::Number(expected)) => {
                    assert_close(*actual, expected);
                }
                (ColorComponent::None, ColorComponent::None) => {}
                pair => panic!("component semantic mismatch: {pair:?}"),
            }
        }
    }
}

#[test]
fn encoded_color_preserves_none_bits_and_fallback_metadata() {
    let source = EncodedColor::new(
        ColorSpace::Oklch,
        [
            ColorComponent::Number(0.7),
            ColorComponent::Number(0.0),
            ColorComponent::None,
        ],
        0.4,
    )
    .expect("structurally valid encoded color")
    .with_hex_fallback(HexFallback::new(0x33, 0x66, 0x99));

    let color = Color::from_encoded(source).expect("missing hue evaluates using zero");
    let round_trip = color.to_encoded(ColorSpace::Oklch);

    assert_eq!(round_trip, source);
    assert_eq!(round_trip.components()[2], ColorComponent::None);
    assert_eq!(color.origin_space(), ColorSpace::Oklch);
    assert!(color.missing_components().contains(2));
    assert!(!color.missing_components().contains(0));
    assert_eq!(
        round_trip.hex_fallback(),
        Some(HexFallback::new(0x33, 0x66, 0x99))
    );
}

#[test]
fn css_color_4_reference_vectors_match() {
    let red = Color::from_srgb(Srgb::new(1.0, 0.0, 0.0), 1.0).expect("red");
    assert_xyz_close(
        red.xyz_d65(),
        XyzD65::new(0.412_390_799_3, 0.212_639_005_9, 0.019_330_818_7),
    );

    let p3_red =
        Color::from_display_p3(DisplayP3::new(1.0, 0.0, 0.0), 1.0).expect("display-p3 red");
    assert_xyz_close(
        p3_red.xyz_d65(),
        XyzD65::new(0.486_570_948_6, 0.228_974_564_1, 0.0),
    );

    let d50_white = Color::from_xyz_d50(XyzD50::new(0.964_295_676_4, 1.0, 0.825_104_602_5), 1.0)
        .expect("D50 white");
    assert_xyz_close(
        d50_white.xyz_d65(),
        XyzD65::new(0.950_455_927_1, 1.0, 1.089_057_750_8),
    );

    let a98_red = Color::from_encoded(encoded(A98Rgb::new(1.0, 0.0, 0.0), 1.0)).expect("A98 red");
    assert_xyz_close(
        a98_red.xyz_d65(),
        XyzD65::new(0.576_669_042_9, 0.297_344_975_3, 0.027_031_361_4),
    );

    let rec2020_red =
        Color::from_encoded(encoded(Rec2020::new(1.0, 0.0, 0.0), 1.0)).expect("Rec. 2020 red");
    assert_xyz_close(
        rec2020_red.xyz_d65(),
        XyzD65::new(0.636_958_048_3, 0.262_700_212_0, 0.0),
    );

    let prophoto_red =
        Color::from_prophoto_rgb(ProPhotoRgb::new(1.0, 0.0, 0.0), 1.0).expect("ProPhoto red");
    assert_xyz_close(
        prophoto_red.xyz_d65(),
        XyzD65::new(0.755_590_742_3, 0.268_321_843_6, 0.003_915_972_8),
    );

    let lab_white =
        Color::from_encoded(encoded(LabColor::new(100.0, 0.0, 0.0), 1.0)).expect("Lab white");
    assert_xyz_close(
        lab_white.xyz_d65(),
        XyzD65::new(0.950_455_927_1, 1.0, 1.089_057_750_8),
    );

    let oklab_red = red.to_oklab();
    assert_close(oklab_red.l, 0.627_955_360_6);
    assert_close(oklab_red.a, 0.224_863_061_1);
    assert_close(oklab_red.b, 0.125_846_298_5);
}

#[test]
fn extended_transfer_functions_preserve_negative_coordinates() {
    for value in [-1.25, -0.5, -0.04, 0.0, 0.04, 0.5, 1.25] {
        let linear = LinearSrgb::encoded_channel_to_linear(value);
        assert_close(LinearSrgb::linear_channel_to_encoded(linear), value);
    }
}

#[test]
fn dtcg_keys_cover_fourteen_spaces_and_exclude_hsv() {
    let keys = ColorSpace::ALL
        .into_iter()
        .filter_map(ColorSpace::dtcg_key)
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 14);
    assert_eq!(ColorSpace::Hsv.dtcg_key(), None);
    assert!(keys.contains(&"display-p3"));
    assert!(keys.contains(&"xyz-d50"));
}

#[test]
fn clones_share_thread_safe_lazy_conversion_caches() {
    let color =
        Color::from_prophoto_rgb(ProPhotoRgb::new(0.2, 0.7, 0.4), 0.8).expect("prophoto color");
    let barrier = Arc::new(Barrier::new(8));

    let threads = (0..8)
        .map(|_| {
            let color = color.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                (color.to_srgb(), color.to_display_p3(), color.to_lab())
            })
        })
        .collect::<Vec<_>>();

    let first = threads[0].thread().id();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().expect("conversion thread"))
        .collect::<Vec<_>>();
    assert_ne!(first, thread::current().id());
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(color.cache_initialization_count(ColorSpace::Srgb), 1);
    assert_eq!(color.cache_initialization_count(ColorSpace::DisplayP3), 1);
    assert_eq!(color.cache_initialization_count(ColorSpace::Lab), 1);
}

#[test]
fn source_representation_seeds_its_cache_without_conversion() {
    let source = Srgb::new(0.25, 0.5, 0.75);
    let color = Color::from_srgb(source, 1.0).expect("sRGB");

    assert_eq!(color.to_srgb(), source);
    assert_eq!(color.cache_initialization_count(ColorSpace::Srgb), 0);
}

#[test]
fn semantic_hash_normalizes_negative_zero_and_with_methods_own_fresh_caches() {
    let positive_zero =
        Color::from_xyz_d65(XyzD65::new(0.0, 0.5, 0.25), 1.0).expect("positive zero XYZ");
    let negative_zero =
        Color::from_xyz_d65(XyzD65::new(-0.0, 0.5, 0.25), 1.0).expect("negative zero XYZ");
    assert_eq!(positive_zero, negative_zero);

    fn hash(color: &Color) -> u64 {
        let mut hasher = DefaultHasher::new();
        color.hash(&mut hasher);
        hasher.finish()
    }
    assert_eq!(hash(&positive_zero), hash(&negative_zero));

    let original =
        Color::from_prophoto_rgb(ProPhotoRgb::new(0.2, 0.7, 0.4), 0.8).expect("prophoto color");
    let _ = original.to_srgb();
    assert_eq!(original.cache_initialization_count(ColorSpace::Srgb), 1);

    let changed = original.with_alpha(0.5).expect("updated alpha");
    assert_eq!(changed.cache_initialization_count(ColorSpace::Srgb), 0);
    let _ = changed.to_srgb();
    assert_eq!(changed.cache_initialization_count(ColorSpace::Srgb), 1);
    assert_eq!(original.cache_initialization_count(ColorSpace::Srgb), 1);
}
