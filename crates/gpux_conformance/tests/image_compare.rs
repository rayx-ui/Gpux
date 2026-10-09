//! Image comparison on synthetic images: relative probes, golden comparison with a per-channel
//! tolerance and a differing-pixel ratio, per-backend golden names, the update mode and the
//! failure artifacts.

use gpux::GpuSpecs;
use gpux_conformance::{
    backend::BackendId,
    compare::{
        self, CompareOptions, GoldenName, GoldenOutcome, GoldenTolerance, Goldens, compare_images,
        update_requested,
    },
    probe::{self, DEFAULT_TOLERANCE, PixelRect},
    report::{Outcome, ResultLine},
};
use image::{Rgba, RgbaImage};
use std::path::Path;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn filled(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(width, height, Rgba(color))
}

/// A 20x10 image: red on the left half, blue on the right.
fn halves() -> RgbaImage {
    RgbaImage::from_fn(20, 10, |x, _| Rgba(if x < 10 { RED } else { BLUE }))
}

fn goldens(root: &Path, artifacts: &Path, update: bool) -> Goldens {
    Goldens {
        root: root.to_path_buf(),
        artifacts: artifacts.to_path_buf(),
        update,
        options: CompareOptions::default(),
    }
}

#[test]
fn color_probes_hold_within_the_tolerance() {
    let mut image = halves();
    image.put_pixel(3, 3, Rgba([252, 3, 2, 255]));
    probe::color_at(&image, 3, 3, RED, DEFAULT_TOLERANCE).unwrap();
    probe::color_at(&image, 15, 5, BLUE, 0).unwrap();

    let error = probe::color_at(&image, 3, 3, RED, 2).unwrap_err();
    assert!(error.0.contains("(3, 3)"), "{error}");
    let error = probe::color_at(&image, 20, 0, RED, 255).unwrap_err();
    assert!(error.0.contains("outside"), "{error}");
}

#[test]
fn region_probes_check_uniformity_equality_and_difference() {
    let image = halves();
    let left = PixelRect::new(0, 0, 10, 10);
    let right = PixelRect::new(10, 0, 10, 10);

    assert_eq!(probe::region_uniform(&image, left, 0).unwrap(), RED);
    assert!(probe::region_uniform(&image, PixelRect::new(5, 0, 10, 10), 0).is_err());
    probe::regions_equal(
        &image,
        PixelRect::new(0, 0, 5, 10),
        PixelRect::new(5, 0, 5, 10),
        0,
    )
    .unwrap();
    probe::regions_differ(&image, left, right, DEFAULT_TOLERANCE).unwrap();
    assert!(probe::regions_equal(&image, left, right, DEFAULT_TOLERANCE).is_err());
    assert!(probe::regions_differ(&image, left, left, DEFAULT_TOLERANCE).is_err());

    // Regions must fit the image and match in size.
    assert!(probe::region_uniform(&image, PixelRect::new(15, 0, 10, 10), 0).is_err());
    assert!(probe::region_uniform(&image, PixelRect::new(0, 0, 0, 10), 0).is_err());
    assert!(probe::regions_equal(&image, left, PixelRect::new(10, 0, 5, 10), 0).is_err());
}

#[test]
fn change_probes_compare_a_region_before_and_after() {
    let before = halves();
    let mut after = halves();
    for y in 0..4 {
        after.put_pixel(2, y, Rgba(BLUE));
    }
    let touched = PixelRect::new(0, 0, 5, 5);
    let untouched = PixelRect::new(10, 0, 10, 10);

    probe::changed(&before, &after, touched, DEFAULT_TOLERANCE).unwrap();
    probe::unchanged(&before, &after, untouched, DEFAULT_TOLERANCE).unwrap();
    assert!(probe::changed(&before, &after, untouched, DEFAULT_TOLERANCE).is_err());
    assert!(probe::unchanged(&before, &after, touched, DEFAULT_TOLERANCE).is_err());
}

#[test]
fn comparison_allows_the_per_channel_tolerance() {
    let expected = filled(10, 10, [100, 100, 100, 255]);
    let options = CompareOptions {
        channel_tolerance: 3,
        max_differing_ratio: 0.,
    };

    let close = filled(10, 10, [103, 97, 100, 255]);
    let comparison = compare_images(&close, &expected, options);
    assert!(comparison.matches);
    assert_eq!(comparison.differing_pixels, 0);
    assert_eq!(comparison.max_channel_difference, 3);

    let far = filled(10, 10, [104, 100, 100, 255]);
    let comparison = compare_images(&far, &expected, options);
    assert!(!comparison.matches);
    assert_eq!(comparison.differing_pixels, 100);
    assert_eq!(comparison.max_channel_difference, 4);
    // The diff marks differing pixels red.
    assert_eq!(comparison.diff.get_pixel(0, 0).0, RED);
}

#[test]
fn comparison_allows_a_ratio_of_differing_pixels() {
    let expected = filled(100, 100, BLUE);
    let options = CompareOptions::default();
    assert_eq!(options.channel_tolerance, 3);
    assert_eq!(options.max_differing_ratio, 0.001);

    // 10 of 10,000 pixels is exactly the default 0.1 %.
    let mut actual = expected.clone();
    for x in 0..10 {
        actual.put_pixel(x, 0, Rgba(RED));
    }
    let comparison = compare_images(&actual, &expected, options);
    assert!(comparison.matches);
    assert_eq!(comparison.differing_pixels, 10);
    assert!((comparison.differing_ratio() - 0.001).abs() < 1e-12);
    // Unchanged pixels show a faded copy of the expected image.
    assert_eq!(comparison.diff.get_pixel(50, 50).0, [191, 191, 254, 255]);

    actual.put_pixel(10, 0, Rgba(RED));
    assert!(!compare_images(&actual, &expected, options).matches);

    // Images of different sizes never match.
    let comparison = compare_images(&filled(100, 99, BLUE), &expected, options);
    assert!(!comparison.matches);
    assert_eq!(comparison.differing_ratio(), 1.);
}

#[test]
fn golden_names_carry_scene_scale_and_backend() {
    let name = GoldenName::new("paint-borders", 2, "warp");
    assert_eq!(name.file_name(), "paint-borders@2x.warp.png");
    assert_eq!(
        name.relative_path(),
        Path::new("warp").join("paint-borders@2x.warp.png")
    );
    assert_eq!(GoldenName::parse(&name.file_name()), Some(name));

    for backend in compare::GOLDEN_BACKENDS {
        let file_name = format!("text-runs@1x.{backend}.png");
        assert_eq!(
            GoldenName::parse(&file_name),
            Some(GoldenName::new("text-runs", 1, *backend)),
            "{file_name}"
        );
    }
    for bad in [
        "paint-borders.warp.png",
        "paint-borders@2x.png",
        "paint-borders@3x.warp.png",
        "paint-borders@2x.nvidia.png",
        "paint-borders@2x.warp.jpg",
        "Paint-Borders@2x.warp.png",
        "paint_borders@2x.warp.png",
        "-borders@2x.warp.png",
        "@2x.warp.png",
    ] {
        assert_eq!(GoldenName::parse(bad), None, "{bad}");
    }
}

#[test]
fn backend_ids_follow_the_adapter_identity() {
    let specs = |device: &str, backend: &str| GpuSpecs {
        device_name: device.into(),
        backend: backend.into(),
        ..GpuSpecs::default()
    };
    let cases = [
        (
            specs("llvmpipe (LLVM 19.1.1, 256 bits)", "Vulkan"),
            Some("lavapipe"),
        ),
        (
            specs("llvmpipe (LLVM 19.1.1, 256 bits)", "Gl"),
            Some("llvmpipe"),
        ),
        (specs("Microsoft Basic Render Driver", "Dx12"), Some("warp")),
        (specs("Apple M2", "Metal"), Some("metal")),
        (
            specs("Google SwiftShader", "BrowserWebGpu"),
            Some("swiftshader-webgpu"),
        ),
        (specs("NVIDIA GeForce RTX 4080", "Dx12"), None),
        (specs("AMD Radeon RX 7900", "Vulkan"), None),
    ];
    for (specs, expected) in cases {
        let id = BackendId::from_specs(&specs);
        assert_eq!(id.golden_id(), expected, "{specs:?}");
    }
    assert_eq!(
        BackendId::from_specs(&specs("NVIDIA GeForce RTX 4080", "Dx12")).label(),
        "hardware (NVIDIA GeForce RTX 4080)"
    );
    assert_eq!(BackendId::Warp.label(), "warp");
}

#[test]
fn update_mode_writes_goldens_that_then_match() {
    assert!(update_requested(Some("1")));
    assert!(update_requested(Some(" TRUE ")));
    assert!(update_requested(Some("yes")));
    assert!(!update_requested(Some("0")));
    assert!(!update_requested(Some("")));
    assert!(!update_requested(None));
    assert_eq!(compare::UPDATE_ENV_VAR, "GPUX_UPDATE_SNAPSHOTS");

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("goldens");
    let artifacts = dir.path().join("artifacts");
    let name = GoldenName::new("paint-borders", 1, "lavapipe");
    let image = halves();

    let outcome = goldens(&root, &artifacts, true)
        .check(&name, &image)
        .unwrap();
    let path = root.join("lavapipe").join("paint-borders@1x.lavapipe.png");
    assert_eq!(outcome, GoldenOutcome::Updated(path.clone()));
    assert_eq!(image::open(&path).unwrap().to_rgba8(), image);

    let outcome = goldens(&root, &artifacts, false)
        .check(&name, &image)
        .unwrap();
    assert_eq!(outcome, GoldenOutcome::Matched);
    assert!(!artifacts.exists(), "a match writes no artifacts");
}

#[test]
fn failed_comparisons_write_actual_expected_and_diff() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("goldens");
    let artifacts = dir.path().join("artifacts-temp").join("testkit").join("c");
    let name = GoldenName::new("paint-borders", 2, "warp");
    goldens(&root, &artifacts, true)
        .check(&name, &filled(20, 10, RED))
        .unwrap();

    let error = goldens(&root, &artifacts, false)
        .check(&name, &halves())
        .unwrap_err();
    assert!(
        error
            .message
            .contains("100 of 200 pixels (50.000%) differ by more than 3"),
        "{error}"
    );
    let expected_artifacts: Vec<_> = ["actual", "expected", "diff"]
        .iter()
        .map(|kind| artifacts.join(format!("paint-borders@2x.warp.{kind}.png")))
        .collect();
    assert_eq!(error.artifacts, expected_artifacts);
    assert_eq!(
        image::open(&expected_artifacts[0]).unwrap().to_rgba8(),
        halves()
    );
    let diff = image::open(&expected_artifacts[2]).unwrap().to_rgba8();
    assert_eq!(diff.get_pixel(15, 5).0, RED);
    assert_ne!(diff.get_pixel(5, 5).0, RED);
}

#[test]
fn scenes_override_the_golden_tolerance_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("goldens");
    let artifacts = dir.path().join("artifacts");
    let name = GoldenName::new("paint-shadows-gradients", 1, "warp");
    goldens(&root, &artifacts, true)
        .check(&name, &filled(10, 10, [100, 100, 100, 255]))
        .unwrap();
    let blurred = filled(10, 10, [106, 100, 100, 255]);

    // The defaults (±3) reject a 6-level difference everywhere...
    assert!(
        goldens(&root, &artifacts, false)
            .check(&name, &blurred)
            .is_err()
    );
    // ...a scene's override with a reason accepts it.
    let tolerance = GoldenTolerance {
        channel_tolerance: 6,
        max_differing_ratio: 0.001,
        reason: "blur radii round differently per rasterizer",
    };
    tolerance.validate().unwrap();
    assert_eq!(
        tolerance.options(),
        CompareOptions {
            channel_tolerance: 6,
            max_differing_ratio: 0.001,
        }
    );
    let outcome = goldens(&root, &artifacts, false)
        .check_with(&name, &blurred, tolerance.options())
        .unwrap();
    assert_eq!(outcome, GoldenOutcome::Matched);

    // An override needs a reason and a ratio between 0 and 1.
    let unexplained = GoldenTolerance {
        reason: "  ",
        ..tolerance
    };
    assert!(unexplained.validate().unwrap_err().contains("reason"));
    let impossible = GoldenTolerance {
        max_differing_ratio: 1.5,
        ..tolerance
    };
    assert!(impossible.validate().is_err());
}

#[test]
fn missing_goldens_fail_with_the_actual_image_and_the_update_hint() {
    let dir = tempfile::tempdir().unwrap();
    let artifacts = dir.path().join("artifacts");
    let name = GoldenName::new("media-svg", 1, "metal");
    let error = goldens(&dir.path().join("goldens"), &artifacts, false)
        .check(&name, &halves())
        .unwrap_err();
    assert!(error.message.contains("GPUX_UPDATE_SNAPSHOTS=1"), "{error}");
    assert_eq!(
        error.artifacts,
        vec![artifacts.join("media-svg@1x.metal.actual.png")]
    );
}

#[test]
fn failure_artifacts_default_to_artifacts_temp_testkit() {
    let dir = compare::artifacts_dir("c");
    match std::env::var(compare::ARTIFACTS_ENV_VAR) {
        Ok(base) => assert_eq!(dir, Path::new(&base).join("c")),
        Err(_) => {
            assert!(
                dir.ends_with(Path::new("artifacts-temp").join("testkit").join("c")),
                "{}",
                dir.display()
            );
            let workspace = dir.ancestors().nth(3).unwrap();
            assert!(workspace.join("Cargo.lock").is_file(), "{}", dir.display());
        }
    }
    let goldens = Goldens::for_lane("c");
    assert_eq!(goldens.artifacts, dir);
    assert!(goldens.root.ends_with(Path::new("tests").join("goldens")));
}

#[test]
fn result_lines_round_trip_through_any_log_prefix() {
    let line = ResultLine::new(Outcome::Fail, "paint-borders@2x", "step 3 probe:\nmismatch");
    let printed = line.to_string();
    assert_eq!(
        printed,
        "gpux-conformance: FAIL paint-borders@2x step 3 probe: mismatch"
    );
    let parsed =
        ResultLine::parse(&format!("10-07 12:00:01.123 I/gpux( 4242): {printed}")).unwrap();
    assert_eq!(parsed.outcome, Outcome::Fail);
    assert_eq!(parsed.scene, "paint-borders@2x");
    assert_eq!(parsed.detail, "step 3 probe: mismatch");

    let pass = ResultLine::parse("gpux-conformance: PASS layout-flex").unwrap();
    assert_eq!(pass, ResultLine::new(Outcome::Pass, "layout-flex", ""));
    assert_eq!(pass.to_string(), "gpux-conformance: PASS layout-flex");
    assert_eq!(
        ResultLine::parse("console: gpux-conformance: SKIP media-svg no renderer")
            .unwrap()
            .outcome,
        Outcome::Skip
    );
    for not_a_result in [
        "",
        "test layout_flex ... ok",
        "gpux-conformance: MAYBE layout-flex",
        "gpux-conformance: PASS",
    ] {
        assert_eq!(ResultLine::parse(not_a_result), None, "{not_a_result:?}");
    }
}
