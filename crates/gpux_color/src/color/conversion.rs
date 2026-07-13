use crate::color::hsl::{hsl_to_rgb, rgb_to_hsl};
use crate::color::hsv::{hsv_to_rgb, rgb_to_hsv};
use crate::color::oklab::{linear_srgb_to_oklab, oklab_to_linear_srgb};
use crate::color::oklch::{oklab_to_oklch, oklch_to_oklab};
use crate::color::{ColorError, ColorSpace, EncodedColor, HueMemory, LinearSrgb, XyzD50, XyzD65};

type Matrix = [[f64; 3]; 3];

const SRGB_TO_XYZ: Matrix = [
    [0.412_390_799_3, 0.357_584_339_4, 0.180_480_788_4],
    [0.212_639_005_9, 0.715_168_678_8, 0.072_192_315_4],
    [0.019_330_818_7, 0.119_194_779_8, 0.950_532_152_2],
];
const P3_TO_XYZ: Matrix = [
    [0.486_570_948_6, 0.265_667_693_2, 0.198_217_285_2],
    [0.228_974_564_1, 0.691_738_521_8, 0.079_286_914_1],
    [0.0, 0.045_113_381_9, 1.043_944_368_9],
];
const A98_TO_XYZ: Matrix = [
    [0.576_669_042_9, 0.185_558_237_9, 0.188_228_646_2],
    [0.297_344_975_3, 0.627_363_566_3, 0.075_291_458_5],
    [0.027_031_361_4, 0.070_688_852_5, 0.991_337_536_8],
];
const PROPHOTO_TO_XYZ_D50: Matrix = [
    [0.797_766_644_9, 0.135_181_297_4, 0.031_347_734_1],
    [0.288_074_828_8, 0.711_835_234_2, 0.000_089_936_9],
    [0.0, 0.0, 0.825_104_602_5],
];
const REC2020_TO_XYZ: Matrix = [
    [0.636_958_048_3, 0.144_616_903_6, 0.168_880_975_2],
    [0.262_700_212_0, 0.677_998_071_5, 0.059_301_716_5],
    [0.0, 0.028_072_693_0, 1.060_985_057_7],
];
const D50_TO_D65: Matrix = [
    [0.955_473_421_5, -0.023_098_454_9, 0.063_259_243_2],
    [-0.028_369_709_3, 1.009_995_398_1, 0.021_041_441_2],
    [0.012_314_014_9, -0.020_507_649_3, 1.330_365_926_2],
];
const D65_TO_D50: Matrix = [
    [1.047_929_820_8, 0.022_946_793_3, -0.050_192_229_5],
    [0.029_627_815_7, 0.990_434_484_6, -0.017_073_825_0],
    [-0.009_243_058_2, 0.015_055_144_9, 0.751_874_281_4],
];

pub(crate) fn encoded_to_xyz(encoded: EncodedColor) -> Result<XyzD65, ColorError> {
    let [a, b, c] = encoded.values();
    let xyz = match encoded.space() {
        ColorSpace::Srgb => rgb_to_xyz([a, b, c], SRGB_TO_XYZ, srgb_decode),
        ColorSpace::LinearSrgb => vector_to_xyz(mul(SRGB_TO_XYZ, [a, b, c])),
        ColorSpace::Hsv => {
            let rgb = hsv_to_rgb(a, b, c);
            rgb_to_xyz(rgb, SRGB_TO_XYZ, srgb_decode)
        }
        ColorSpace::Hsl => {
            let rgb = hsl_to_rgb(a, b, c);
            rgb_to_xyz(rgb, SRGB_TO_XYZ, srgb_decode)
        }
        ColorSpace::Hwb => {
            let mut rgb = hsv_to_rgb(a, 1.0, 1.0);
            if b + c >= 1.0 {
                let gray = if b + c == 0.0 { 0.0 } else { b / (b + c) };
                rgb = [gray; 3];
            } else {
                for channel in &mut rgb {
                    *channel = *channel * (1.0 - b - c) + b;
                }
            }
            rgb_to_xyz(rgb, SRGB_TO_XYZ, srgb_decode)
        }
        ColorSpace::Lab => xyz_d50_to_d65(lab_to_xyz_d50(a, b, c)),
        ColorSpace::Lch => {
            let hue = c.to_radians();
            xyz_d50_to_d65(lab_to_xyz_d50(a, b * hue.cos(), b * hue.sin()))
        }
        ColorSpace::Oklab => {
            let linear = oklab_to_linear_srgb(a, b, c);
            vector_to_xyz(mul(SRGB_TO_XYZ, [linear.r, linear.g, linear.b]))
        }
        ColorSpace::Oklch => {
            let oklab = oklch_to_oklab(a, b, c, encoded.alpha());
            let linear = oklab_to_linear_srgb(oklab.l, oklab.a, oklab.b);
            vector_to_xyz(mul(SRGB_TO_XYZ, [linear.r, linear.g, linear.b]))
        }
        ColorSpace::DisplayP3 => rgb_to_xyz([a, b, c], P3_TO_XYZ, srgb_decode),
        ColorSpace::A98Rgb => rgb_to_xyz([a, b, c], A98_TO_XYZ, a98_decode),
        ColorSpace::ProPhotoRgb => {
            let xyz_d50 = vector_to_d50(mul(
                PROPHOTO_TO_XYZ_D50,
                [prophoto_decode(a), prophoto_decode(b), prophoto_decode(c)],
            ));
            xyz_d50_to_d65(xyz_d50)
        }
        ColorSpace::Rec2020 => rgb_to_xyz([a, b, c], REC2020_TO_XYZ, rec2020_decode),
        ColorSpace::XyzD65 => XyzD65::new(a, b, c),
        ColorSpace::XyzD50 => xyz_d50_to_d65(XyzD50::new(a, b, c)),
    };
    if xyz.is_finite() {
        Ok(xyz)
    } else {
        Err(ColorError::InvalidColorCoordinate)
    }
}

pub(crate) fn xyz_to_encoded(
    xyz: XyzD65,
    alpha: f64,
    space: ColorSpace,
    hues: HueMemory,
) -> EncodedColor {
    let values = match space {
        ColorSpace::Srgb => xyz_to_rgb(xyz, SRGB_TO_XYZ, srgb_encode),
        ColorSpace::LinearSrgb => mul(inverse(SRGB_TO_XYZ), [xyz.x, xyz.y, xyz.z]),
        ColorSpace::Hsv => {
            let [r, g, b] = xyz_to_rgb(xyz, SRGB_TO_XYZ, srgb_encode);
            let value = rgb_to_hsv(r, g, b, alpha, hues.hsv().unwrap_or(0.0));
            [value.hue_degrees, value.saturation, value.value]
        }
        ColorSpace::Hsl => {
            let [r, g, b] = xyz_to_rgb(xyz, SRGB_TO_XYZ, srgb_encode);
            let value = rgb_to_hsl(r, g, b, alpha, hues.hsl().unwrap_or(0.0));
            [value.hue_degrees, value.saturation, value.lightness]
        }
        ColorSpace::Hwb => {
            let [r, g, b] = xyz_to_rgb(xyz, SRGB_TO_XYZ, srgb_encode);
            let hsv = rgb_to_hsv(r, g, b, alpha, hues.hsv().unwrap_or(0.0));
            [hsv.hue_degrees, r.min(g).min(b), 1.0 - r.max(g).max(b)]
        }
        ColorSpace::Lab => {
            let lab = xyz_d50_to_lab(xyz_d65_to_d50(xyz));
            [lab.lightness, lab.a, lab.b]
        }
        ColorSpace::Lch => {
            let lab = xyz_d50_to_lab(xyz_d65_to_d50(xyz));
            let chroma = (lab.a * lab.a + lab.b * lab.b).sqrt();
            let hue = if chroma <= 1.0e-7 {
                hues.oklch().unwrap_or(0.0)
            } else {
                lab.b.atan2(lab.a).to_degrees().rem_euclid(360.0)
            };
            [lab.lightness, chroma, hue]
        }
        ColorSpace::Oklab => {
            let [r, g, b] = mul(inverse(SRGB_TO_XYZ), [xyz.x, xyz.y, xyz.z]);
            let value = linear_srgb_to_oklab(LinearSrgb::new(r, g, b), alpha);
            [value.l, value.a, value.b]
        }
        ColorSpace::Oklch => {
            let [r, g, b] = mul(inverse(SRGB_TO_XYZ), [xyz.x, xyz.y, xyz.z]);
            let lab = linear_srgb_to_oklab(LinearSrgb::new(r, g, b), alpha);
            let value = oklab_to_oklch(lab, hues.oklch().unwrap_or(0.0));
            [value.lightness, value.chroma, value.hue_degrees]
        }
        ColorSpace::DisplayP3 => xyz_to_rgb(xyz, P3_TO_XYZ, srgb_encode),
        ColorSpace::A98Rgb => xyz_to_rgb(xyz, A98_TO_XYZ, a98_encode),
        ColorSpace::ProPhotoRgb => {
            let d50 = xyz_d65_to_d50(xyz);
            mul(inverse(PROPHOTO_TO_XYZ_D50), [d50.x, d50.y, d50.z]).map(prophoto_encode)
        }
        ColorSpace::Rec2020 => xyz_to_rgb(xyz, REC2020_TO_XYZ, rec2020_encode),
        ColorSpace::XyzD65 => [xyz.x, xyz.y, xyz.z],
        ColorSpace::XyzD50 => {
            let value = xyz_d65_to_d50(xyz);
            [value.x, value.y, value.z]
        }
    };
    EncodedColor::numeric(space, values, alpha)
}

fn rgb_to_xyz(values: [f64; 3], matrix: Matrix, decode: fn(f64) -> f64) -> XyzD65 {
    vector_to_xyz(mul(matrix, values.map(decode)))
}

fn xyz_to_rgb(xyz: XyzD65, matrix: Matrix, encode: fn(f64) -> f64) -> [f64; 3] {
    mul(inverse(matrix), [xyz.x, xyz.y, xyz.z]).map(encode)
}

fn mul(matrix: Matrix, value: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row[0] * value[0] + row[1] * value[1] + row[2] * value[2])
}

fn inverse(m: Matrix) -> Matrix {
    let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let inv = 1.0 / determinant;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ]
}

fn vector_to_xyz(value: [f64; 3]) -> XyzD65 {
    XyzD65::new(value[0], value[1], value[2])
}
fn vector_to_d50(value: [f64; 3]) -> XyzD50 {
    XyzD50::new(value[0], value[1], value[2])
}
fn xyz_d50_to_d65(value: XyzD50) -> XyzD65 {
    vector_to_xyz(mul(D50_TO_D65, [value.x, value.y, value.z]))
}
fn xyz_d65_to_d50(value: XyzD65) -> XyzD50 {
    vector_to_d50(mul(D65_TO_D50, [value.x, value.y, value.z]))
}

fn srgb_decode(value: f64) -> f64 {
    LinearSrgb::encoded_channel_to_linear(value)
}
fn srgb_encode(value: f64) -> f64 {
    LinearSrgb::linear_channel_to_encoded(value)
}
fn a98_decode(value: f64) -> f64 {
    value.signum() * value.abs().powf(563.0 / 256.0)
}
fn a98_encode(value: f64) -> f64 {
    value.signum() * value.abs().powf(256.0 / 563.0)
}
fn prophoto_decode(value: f64) -> f64 {
    if value.abs() <= 16.0 / 512.0 {
        value / 16.0
    } else {
        value.signum() * value.abs().powf(1.8)
    }
}
fn prophoto_encode(value: f64) -> f64 {
    if value.abs() <= 1.0 / 512.0 {
        value * 16.0
    } else {
        value.signum() * value.abs().powf(1.0 / 1.8)
    }
}
fn rec2020_decode(value: f64) -> f64 {
    const ALPHA: f64 = 1.099_296_826_8;
    const BETA: f64 = 0.018_053_968_5;
    if value.abs() < BETA * 4.5 {
        value / 4.5
    } else {
        value.signum() * ((value.abs() + ALPHA - 1.0) / ALPHA).powf(1.0 / 0.45)
    }
}
fn rec2020_encode(value: f64) -> f64 {
    const ALPHA: f64 = 1.099_296_826_8;
    const BETA: f64 = 0.018_053_968_5;
    if value.abs() < BETA {
        value * 4.5
    } else {
        value.signum() * (ALPHA * value.abs().powf(0.45) - (ALPHA - 1.0))
    }
}

fn lab_to_xyz_d50(lightness: f64, a: f64, b: f64) -> XyzD50 {
    const EPSILON: f64 = 216.0 / 24_389.0;
    const KAPPA: f64 = 24_389.0 / 27.0;
    let f1 = (lightness + 16.0) / 116.0;
    let f0 = a / 500.0 + f1;
    let f2 = f1 - b / 200.0;
    let finv = |value: f64| {
        let cube = value * value * value;
        if cube > EPSILON {
            cube
        } else {
            (116.0 * value - 16.0) / KAPPA
        }
    };
    XyzD50::new(
        finv(f0) * 0.964_295_676_4,
        finv(f1),
        finv(f2) * 0.825_104_602_5,
    )
}

fn xyz_d50_to_lab(xyz: XyzD50) -> LabColorInternal {
    const EPSILON: f64 = 216.0 / 24_389.0;
    const KAPPA: f64 = 24_389.0 / 27.0;
    let f = |value: f64| {
        if value > EPSILON {
            value.cbrt()
        } else {
            (KAPPA * value + 16.0) / 116.0
        }
    };
    let f0 = f(xyz.x / 0.964_295_676_4);
    let f1 = f(xyz.y);
    let f2 = f(xyz.z / 0.825_104_602_5);
    LabColorInternal {
        lightness: 116.0 * f1 - 16.0,
        a: 500.0 * (f0 - f1),
        b: 200.0 * (f1 - f2),
    }
}

struct LabColorInternal {
    lightness: f64,
    a: f64,
    b: f64,
}
