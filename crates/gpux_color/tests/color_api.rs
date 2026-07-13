use gpux_color::{
    AlphaEncoding, Color, ColorError, ColorOutputPolicy, ColorSpace, DisplayColorSpace,
    GamutMappingPolicy, GamutStatus, HexRepresentation, HueMemory, LinearSrgb, QuantizationPolicy,
};

#[test]
fn color_can_represent_transparent_black() {
    let color = Color::transparent_black();

    assert_eq!(color.linear_srgb(), LinearSrgb::BLACK);
    assert_eq!(color.alpha(), 0.0);
    assert_eq!(color.hue_memory(), HueMemory::EMPTY);
}

#[test]
fn color_preserves_unclamped_linear_srgb_channels() {
    let color = Color::from_linear_srgb(LinearSrgb::new(-0.25, 0.5, 1.25), 0.75)
        .expect("finite alpha should be accepted");

    assert_eq!(color.linear_srgb(), LinearSrgb::new(-0.25, 0.5, 1.25));
    assert_eq!(color.alpha(), 0.75);
    assert_eq!(
        color
            .to_output(ColorOutputPolicy::new(
                DisplayColorSpace::Srgb,
                GamutMappingPolicy::Clip,
                QuantizationPolicy::Uint8,
                AlphaEncoding::Straight,
            ))
            .expect("output")
            .bytes(),
        Some([0, 188, 255, 191])
    );
    assert_eq!(color.display_gamut_status(), GamutStatus::ClampedForDisplay);
}

#[test]
fn color_rejects_invalid_alpha() {
    assert!(Color::from_linear_srgb(LinearSrgb::BLACK, -0.1).is_err());
    assert!(Color::from_linear_srgb(LinearSrgb::BLACK, 1.1).is_err());
    assert!(Color::from_linear_srgb(LinearSrgb::BLACK, f64::NAN).is_err());
}

#[test]
fn color_space_and_gamut_types_cover_picker_modes() {
    assert_eq!(
        ColorSpace::ALL,
        [
            ColorSpace::Srgb,
            ColorSpace::LinearSrgb,
            ColorSpace::Hsv,
            ColorSpace::Hsl,
            ColorSpace::Hwb,
            ColorSpace::Lab,
            ColorSpace::Lch,
            ColorSpace::Oklab,
            ColorSpace::Oklch,
            ColorSpace::DisplayP3,
            ColorSpace::A98Rgb,
            ColorSpace::ProPhotoRgb,
            ColorSpace::Rec2020,
            ColorSpace::XyzD65,
            ColorSpace::XyzD50,
        ]
    );

    assert_eq!(
        Color::from_linear_srgb(LinearSrgb::new(0.0, 0.5, 1.0), 1.0)
            .expect("in-gamut color")
            .gamut_status(),
        GamutStatus::InSrgb
    );
    assert_eq!(
        Color::from_linear_srgb(LinearSrgb::new(-0.1, 0.5, 1.1), 1.0)
            .expect("out-of-gamut color should still be represented")
            .gamut_status(),
        GamutStatus::OutOfSrgb
    );
}

#[test]
fn hue_memory_keeps_recent_meaningful_hues() {
    let memory = HueMemory::default()
        .with_hsv(Some(32.0))
        .with_hsl(Some(64.0))
        .with_oklch(Some(128.0));

    assert_eq!(memory.hsv(), Some(32.0));
    assert_eq!(memory.hsl(), Some(64.0));
    assert_eq!(memory.oklch(), Some(128.0));
}

#[test]
fn error_and_hex_metadata_types_are_public_api() {
    let error: &dyn std::error::Error = &ColorError::InvalidHex;

    assert_eq!(error.to_string(), "hex color text is invalid");
    assert_eq!(HexRepresentation::Rgba, HexRepresentation::Rgba);
    assert_eq!(HexRepresentation::Argb, HexRepresentation::Argb);
}
