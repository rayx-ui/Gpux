use gpux_color::{
    AlphaEncoding, Color, ColorComponent, ColorOutputPolicy, ColorSpace, DisplayColorSpace,
    EncodedColor, GamutMappingPolicy, HslColor, HueInterpolationMethod, InterpolationOptions,
    InterpolationSpace, QuantizationPolicy, Srgb,
};

const EPSILON: f64 = 2.0e-5;

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "{actual} != {expected}"
    );
}

#[test]
fn parses_numeric_css_color_4_forms_and_rejects_palette_syntax() {
    let rgb = Color::parse_css("rgb(255 0 128 / 50%)").expect("modern rgb");
    close(rgb.to_srgb().r, 1.0);
    close(rgb.to_srgb().g, 0.0);
    close(rgb.to_srgb().b, 128.0 / 255.0);
    close(rgb.alpha(), 0.5);

    let legacy = Color::parse_css("rgba(10, 20, 30, 0.25)").expect("legacy rgba");
    close(legacy.to_srgb().r, 10.0 / 255.0);
    close(legacy.alpha(), 0.25);

    for text in [
        "hsl(210 50% 40% / 75%)",
        "hwb(210 10% 20% / .8)",
        "lab(60% -20 30 / .7)",
        "lch(60% 40 210 / .7)",
        "oklab(.6 -.1 .15 / .7)",
        "oklch(.6 .2 210 / .7)",
        "color(display-p3 1 .2 0 / .8)",
        "color(a98-rgb .2 .4 .8)",
        "color(prophoto-rgb .2 .4 .8)",
        "color(rec2020 .2 .4 .8)",
        "color(xyz-d50 .2 .3 .4)",
    ] {
        Color::parse_css(text).unwrap_or_else(|error| panic!("failed to parse {text}: {error}"));
    }

    for text in ["red-500", "blue/30", "rebeccapurple", "rgb(foo 0 0)"] {
        assert!(
            Color::parse_css(text).is_err(),
            "palette/identifier syntax must fail: {text}"
        );
    }
    assert!(Color::parse_css(&"0".repeat(4097)).is_err());
}

#[test]
fn interpolation_distinguishes_missing_hue_from_numeric_zero() {
    let missing = Color::from_encoded(
        EncodedColor::new(
            ColorSpace::Hsl,
            [ColorComponent::None, 1.0.into(), 0.5.into()],
            1.0,
        )
        .expect("missing hue"),
    )
    .expect("color");
    let zero =
        Color::from_encoded(EncodedColor::from(HslColor::new(0.0, 1.0, 0.5), 1.0).expect("hsl"))
            .expect("color");
    let blue =
        Color::from_encoded(EncodedColor::from(HslColor::new(240.0, 1.0, 0.5), 1.0).expect("hsl"))
            .expect("color");
    let options = InterpolationOptions::new(InterpolationSpace::Hsl)
        .hue_method(HueInterpolationMethod::Shorter);

    let carried = missing
        .interpolate(&blue, 0.5, options)
        .expect("interpolate");
    let numeric = zero.interpolate(&blue, 0.5, options).expect("interpolate");

    close(carried.to_hsl().hue_degrees, 240.0);
    close(numeric.to_hsl().hue_degrees, 300.0);
}

#[test]
fn interpolation_uses_premultiplied_alpha_and_explicit_hue_paths() {
    let transparent_red = Color::from_srgb(Srgb::new(1.0, 0.0, 0.0), 0.0).expect("red");
    let blue = Color::from_srgb(Srgb::new(0.0, 0.0, 1.0), 1.0).expect("blue");
    let midpoint = transparent_red
        .interpolate(
            &blue,
            0.5,
            InterpolationOptions::new(InterpolationSpace::Srgb),
        )
        .expect("interpolate");
    close(midpoint.alpha(), 0.5);
    close(midpoint.to_srgb().r, 0.0);
    close(midpoint.to_srgb().b, 1.0);

    let red =
        Color::from_encoded(EncodedColor::from(HslColor::new(0.0, 1.0, 0.5), 1.0).expect("hsl"))
            .expect("red");
    let blue_hsl =
        Color::from_encoded(EncodedColor::from(HslColor::new(240.0, 1.0, 0.5), 1.0).expect("hsl"))
            .expect("blue");
    let increasing = red
        .interpolate(
            &blue_hsl,
            0.5,
            InterpolationOptions::new(InterpolationSpace::Hsl)
                .hue_method(HueInterpolationMethod::Increasing),
        )
        .expect("increasing");
    close(increasing.to_hsl().hue_degrees, 120.0);
}

#[test]
fn output_requires_explicit_gamut_quantization_and_alpha_policy() {
    let wide = Color::from_srgb(Srgb::new(1.4, -0.2, 0.5), 0.5).expect("wide color");
    let strict = ColorOutputPolicy::new(
        DisplayColorSpace::Srgb,
        GamutMappingPolicy::PreserveOrError,
        QuantizationPolicy::Uint8,
        AlphaEncoding::Straight,
    );
    assert!(wide.to_output(strict).is_err());

    let clip = wide
        .to_output(ColorOutputPolicy::new(
            DisplayColorSpace::Srgb,
            GamutMappingPolicy::Clip,
            QuantizationPolicy::Uint8,
            AlphaEncoding::Straight,
        ))
        .expect("clip");
    assert_eq!(clip.bytes(), Some([255, 0, 128, 128]));

    let mapped = wide
        .to_output(ColorOutputPolicy::new(
            DisplayColorSpace::Srgb,
            GamutMappingPolicy::CssColor4,
            QuantizationPolicy::Float,
            AlphaEncoding::Premultiplied,
        ))
        .expect("gamut map");
    assert!(
        [mapped.r, mapped.g, mapped.b]
            .into_iter()
            .all(|value| (0.0..=1.0).contains(&value))
    );
    assert!(
        [mapped.r, mapped.g, mapped.b]
            .into_iter()
            .all(|value| value <= mapped.a)
    );
}
