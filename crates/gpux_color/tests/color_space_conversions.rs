use gpux_color::{Color, LinearSrgb};

const EPSILON: f64 = 1e-5;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "expected {actual} to be within {EPSILON} of {expected}"
    );
}

#[test]
fn srgb_transfer_functions_match_reference_breakpoints() {
    assert_close(LinearSrgb::encoded_channel_to_linear(0.0), 0.0);
    assert_close(
        LinearSrgb::encoded_channel_to_linear(0.04045),
        0.04045 / 12.92,
    );
    assert_close(LinearSrgb::linear_channel_to_encoded(0.0), 0.0);
    assert_close(
        LinearSrgb::linear_channel_to_encoded(0.003_130_8),
        0.003_130_8 * 12.92,
    );
}

#[test]
fn rgb_channel_editing_uses_encoded_srgb_coordinates() {
    let color = Color::from_encoded_srgb(0.25, 0.5, 0.75, 0.8).expect("valid encoded sRGB color");

    let [r, g, b] = color.to_encoded_srgb();
    assert_close(r, 0.25);
    assert_close(g, 0.5);
    assert_close(b, 0.75);
    assert_close(color.alpha(), 0.8);

    let edited = color
        .with_encoded_rgb_channel(1, 0.125)
        .expect("valid channel edit");
    let [_, edited_g, _] = edited.to_encoded_srgb();
    assert_close(edited_g, 0.125);
    assert_close(edited.alpha(), 0.8);
}

#[test]
fn hsv_conversion_and_channel_editing_round_trip() {
    let red = Color::from_encoded_srgb(1.0, 0.0, 0.0, 0.6).expect("red");
    let hsv = red.to_hsv();
    assert_close(hsv.hue_degrees, 0.0);
    assert_close(hsv.saturation, 1.0);
    assert_close(hsv.value, 1.0);
    assert_close(hsv.alpha, 0.6);

    let cyan = red.with_hsv_channel(0, 180.0).expect("edit hue");
    let [r, g, b] = cyan.to_encoded_srgb();
    assert_close(r, 0.0);
    assert_close(g, 1.0);
    assert_close(b, 1.0);
    assert_close(cyan.alpha(), 0.6);
}

#[test]
fn hsl_conversion_and_channel_editing_round_trip() {
    let blue = Color::from_encoded_srgb(0.0, 0.0, 1.0, 0.4).expect("blue");
    let hsl = blue.to_hsl();
    assert_close(hsl.hue_degrees, 240.0);
    assert_close(hsl.saturation, 1.0);
    assert_close(hsl.lightness, 0.5);
    assert_close(hsl.alpha, 0.4);

    let yellow = blue.with_hsl_channel(0, 60.0).expect("edit hue");
    let [r, g, b] = yellow.to_encoded_srgb();
    assert_close(r, 1.0);
    assert_close(g, 1.0);
    assert_close(b, 0.0);
    assert_close(yellow.alpha(), 0.4);
}

#[test]
fn hsv_preserves_hue_when_saturation_becomes_powerless() {
    let color = Color::from_encoded_srgb(1.0, 0.0, 0.0, 1.0)
        .expect("red")
        .with_hsv_channel(0, 210.0)
        .expect("set meaningful hue")
        .with_hsv_channel(1, 0.0)
        .expect("desaturate");

    assert_close(color.to_hsv().hue_degrees, 210.0);

    let restored = color.with_hsv_channel(1, 1.0).expect("restore saturation");
    assert_close(restored.to_hsv().hue_degrees, 210.0);
}

#[test]
fn rgb_hsv_hsl_round_trip_representative_colors() {
    for [r, g, b] in [
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.25, 0.5, 0.75],
        [0.9, 0.2, 0.4],
    ] {
        let color = Color::from_encoded_srgb(r, g, b, 1.0).expect("rgb color");

        let hsv = color.to_hsv();
        let from_hsv = Color::from_hsv(hsv.hue_degrees, hsv.saturation, hsv.value, hsv.alpha)
            .expect("hsv color");
        let [hr, hg, hb] = from_hsv.to_encoded_srgb();
        assert_close(hr, r);
        assert_close(hg, g);
        assert_close(hb, b);

        let hsl = color.to_hsl();
        let from_hsl = Color::from_hsl(hsl.hue_degrees, hsl.saturation, hsl.lightness, hsl.alpha)
            .expect("hsl color");
        let [lr, lg, lb] = from_hsl.to_encoded_srgb();
        assert_close(lr, r);
        assert_close(lg, g);
        assert_close(lb, b);
    }
}

#[test]
fn oklab_to_linear_srgb_round_trips_without_clamping() {
    let color = Color::from_linear_srgb(LinearSrgb::new(-0.1, 0.42, 1.2), 0.7)
        .expect("unclamped linear sRGB");
    let oklab = color.to_oklab();
    let round_trip =
        Color::from_oklab(oklab.l, oklab.a, oklab.b, oklab.alpha).expect("oklab round trip");

    assert_close(round_trip.linear_srgb().r, -0.1);
    assert_close(round_trip.linear_srgb().g, 0.42);
    assert_close(round_trip.linear_srgb().b, 1.2);
    assert_close(round_trip.alpha(), 0.7);
}

#[test]
fn oklch_converts_to_and_from_oklab_coordinates() {
    let color = Color::from_encoded_srgb(1.0, 0.0, 0.0, 0.5).expect("red");
    let oklab = color.to_oklab();
    let oklch = color.to_oklch();

    assert_close(oklch.lightness, oklab.l);
    assert_close(oklch.chroma, (oklab.a * oklab.a + oklab.b * oklab.b).sqrt());
    assert_close(oklch.hue_degrees, oklab.b.atan2(oklab.a).to_degrees());
    assert_close(oklch.alpha, 0.5);

    let round_trip = Color::from_oklch(
        oklch.lightness,
        oklch.chroma,
        oklch.hue_degrees,
        oklch.alpha,
    )
    .expect("oklch round trip");
    let [r, g, b] = round_trip.to_encoded_srgb();
    assert_close(r, 1.0);
    assert_close(g, 0.0);
    assert_close(b, 0.0);
}

#[test]
fn oklch_preserves_hue_when_chroma_becomes_powerless() {
    let color = Color::from_oklch(0.7, 0.2, 315.0, 1.0)
        .expect("oklch")
        .with_oklch_channel(1, 0.0)
        .expect("remove chroma");

    assert_close(color.to_oklch().hue_degrees, 315.0);

    let restored = color.with_oklch_channel(1, 0.2).expect("restore chroma");
    assert_close(restored.to_oklch().hue_degrees, 315.0);
}
