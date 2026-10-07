//! Golden images: a curated subset of scenes compared against committed per-backend PNGs.
//!
//! Goldens live in `tests/goldens/<backend>/<scene>@<scale>x.<backend>.png` and are only kept
//! for software rasterizers whose output is stable (`lavapipe`, `llvmpipe`, `warp`), Metal and
//! SwiftShader WebGPU; hardware adapters never get committed goldens. A comparison allows a
//! per-channel tolerance and a ratio of differing pixels. `GPUX_UPDATE_SNAPSHOTS=1` rewrites
//! goldens instead of comparing. A failed comparison writes the actual, expected and diff images
//! under the lane's artifacts directory.

use crate::{probe::DEFAULT_TOLERANCE, scene::SceneSpec};
use image::{
    ExtendedColorType, ImageEncoder, Rgba as ImagePixel, RgbaImage,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// The environment variable that turns comparisons into golden updates.
pub const UPDATE_ENV_VAR: &str = "GPUX_UPDATE_SNAPSHOTS";

/// The environment variable that overrides where failure artifacts go.
pub const ARTIFACTS_ENV_VAR: &str = "GPUX_ARTIFACTS_DIR";

/// Backend ids that may have committed goldens.
pub const GOLDEN_BACKENDS: &[&str] = &[
    "lavapipe",
    "llvmpipe",
    "warp",
    "metal",
    "swiftshader-webgpu",
];

/// The largest PNG a golden may be.
pub const MAX_GOLDEN_BYTES: u64 = 32 * 1024;

/// The total size all goldens together may take.
pub const GOLDEN_BUDGET_BYTES: u64 = 2 * 1024 * 1024;

/// Whether `value` (of [`UPDATE_ENV_VAR`]) asks for updates: `1`, `true` or `yes`.
pub fn update_requested(value: Option<&str>) -> bool {
    matches!(
        value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// The name of one golden: a scene at a scale on a backend.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GoldenName {
    /// The scene id.
    pub scene: String,
    /// The integer scale factor.
    pub scale: u32,
    /// The backend id, one of [`GOLDEN_BACKENDS`].
    pub backend: String,
}

impl GoldenName {
    /// A golden name; the backend must be one of [`GOLDEN_BACKENDS`].
    pub fn new(scene: impl Into<String>, scale: u32, backend: impl Into<String>) -> Self {
        Self {
            scene: scene.into(),
            scale,
            backend: backend.into(),
        }
    }

    /// `<scene>@<scale>x.<backend>.png`.
    pub fn file_name(&self) -> String {
        format!("{}@{}x.{}.png", self.scene, self.scale, self.backend)
    }

    /// The path below the goldens root: `<backend>/<file name>`.
    pub fn relative_path(&self) -> PathBuf {
        Path::new(&self.backend).join(self.file_name())
    }

    /// Parses a golden file name, checking the naming convention: a lowercase kebab-case scene
    /// id, scale 1 or 2, and a golden backend.
    pub fn parse(file_name: &str) -> Option<Self> {
        let rest = file_name.strip_suffix(".png")?;
        let (scene, rest) = rest.split_once('@')?;
        let (scale, backend) = rest.split_once("x.")?;
        let scale: u32 = scale.parse().ok()?;
        let valid_scene = !scene.is_empty()
            && !scene.starts_with('-')
            && !scene.ends_with('-')
            && scene
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        (valid_scene && matches!(scale, 1 | 2) && GOLDEN_BACKENDS.contains(&backend))
            .then(|| Self::new(scene, scale, backend))
    }
}

impl fmt::Display for GoldenName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.file_name())
    }
}

/// How strictly two images must match.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompareOptions {
    /// The largest per-channel difference a pixel may have and still match.
    pub channel_tolerance: u8,
    /// The largest share of pixels (0–1) that may differ beyond the tolerance.
    pub max_differing_ratio: f64,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            channel_tolerance: DEFAULT_TOLERANCE,
            max_differing_ratio: 0.001,
        }
    }
}

/// A scene's own golden tolerance, replacing the defaults for that scene. Every override
/// records why the scene needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GoldenTolerance {
    /// The largest per-channel difference a pixel may have and still match.
    pub channel_tolerance: u8,
    /// The largest share of pixels (0–1) that may differ beyond the tolerance.
    pub max_differing_ratio: f64,
    /// Why the defaults do not fit this scene.
    pub reason: &'static str,
}

impl GoldenTolerance {
    /// The comparison options this override sets.
    pub fn options(&self) -> CompareOptions {
        CompareOptions {
            channel_tolerance: self.channel_tolerance,
            max_differing_ratio: self.max_differing_ratio,
        }
    }

    /// Checks that the override gives a reason and a ratio between 0 and 1.
    pub fn validate(&self) -> Result<(), String> {
        if self.reason.trim().is_empty() {
            return Err("a golden tolerance override needs a reason".into());
        }
        if !(0.0..=1.0).contains(&self.max_differing_ratio) {
            return Err(format!(
                "the differing-pixel ratio {} is outside 0..=1",
                self.max_differing_ratio
            ));
        }
        Ok(())
    }
}

/// The result of comparing two images.
#[derive(Clone, Debug)]
pub struct Comparison {
    /// Pixels differing beyond the tolerance.
    pub differing_pixels: u64,
    /// All pixels compared.
    pub total_pixels: u64,
    /// The largest per-channel difference anywhere.
    pub max_channel_difference: u8,
    /// Whether the images matched under the options.
    pub matches: bool,
    /// Differing pixels in red over a faded copy of the expected image.
    pub diff: RgbaImage,
}

impl Comparison {
    /// The share of pixels that differ.
    pub fn differing_ratio(&self) -> f64 {
        if self.total_pixels == 0 {
            0.
        } else {
            self.differing_pixels as f64 / self.total_pixels as f64
        }
    }
}

/// Compares `actual` with `expected`. Images of different sizes never match.
pub fn compare_images(
    actual: &RgbaImage,
    expected: &RgbaImage,
    options: CompareOptions,
) -> Comparison {
    if actual.dimensions() != expected.dimensions() {
        let total_pixels = u64::from(actual.width()) * u64::from(actual.height());
        return Comparison {
            differing_pixels: total_pixels,
            total_pixels,
            max_channel_difference: u8::MAX,
            matches: false,
            diff: actual.clone(),
        };
    }
    let mut differing_pixels = 0;
    let mut max_channel_difference = 0;
    let mut diff = RgbaImage::new(actual.width(), actual.height());
    for ((actual_pixel, expected_pixel), diff_pixel) in actual
        .pixels()
        .zip(expected.pixels())
        .zip(diff.pixels_mut())
    {
        let difference = actual_pixel
            .0
            .iter()
            .zip(expected_pixel.0)
            .map(|(actual, expected)| actual.abs_diff(expected))
            .max()
            .unwrap_or(0);
        max_channel_difference = max_channel_difference.max(difference);
        *diff_pixel = if difference > options.channel_tolerance {
            differing_pixels += 1;
            ImagePixel([255, 0, 0, 255])
        } else {
            let [r, g, b, _] = expected_pixel.0;
            ImagePixel([r / 4 + 191, g / 4 + 191, b / 4 + 191, 255])
        };
    }
    let total_pixels = u64::from(actual.width()) * u64::from(actual.height());
    let ratio = if total_pixels == 0 {
        0.
    } else {
        differing_pixels as f64 / total_pixels as f64
    };
    Comparison {
        differing_pixels,
        total_pixels,
        max_channel_difference,
        matches: ratio <= options.max_differing_ratio,
        diff,
    }
}

/// What a golden check did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenOutcome {
    /// The image matched the committed golden.
    Matched,
    /// Update mode wrote the image as the golden.
    Updated(PathBuf),
}

/// A golden check that failed, with the artifacts it wrote.
#[derive(Clone, Debug)]
pub struct GoldenError {
    /// What went wrong.
    pub message: String,
    /// The actual, expected and diff images written for inspection.
    pub artifacts: Vec<PathBuf>,
}

impl fmt::Display for GoldenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        for artifact in &self.artifacts {
            write!(f, "\n  {}", artifact.display())?;
        }
        Ok(())
    }
}

impl std::error::Error for GoldenError {}

/// Golden storage for one lane: where goldens live, where failure artifacts go, and whether
/// to update instead of comparing.
#[derive(Clone, Debug)]
pub struct Goldens {
    /// The goldens root (`tests/goldens` of this crate by default).
    pub root: PathBuf,
    /// Where failure artifacts go.
    pub artifacts: PathBuf,
    /// Whether to write goldens instead of comparing.
    pub update: bool,
    /// How strictly images must match.
    pub options: CompareOptions,
}

impl Goldens {
    /// The committed goldens of this crate, with artifacts under
    /// `artifacts-temp/testkit/<lane>/` of the workspace (or [`ARTIFACTS_ENV_VAR`]) and the
    /// update mode from [`UPDATE_ENV_VAR`].
    pub fn for_lane(lane: &str) -> Self {
        Self {
            root: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("goldens"),
            artifacts: artifacts_dir(lane),
            update: update_requested(std::env::var(UPDATE_ENV_VAR).ok().as_deref()),
            options: CompareOptions::default(),
        }
    }

    /// The path of a golden.
    pub fn path(&self, name: &GoldenName) -> PathBuf {
        self.root.join(name.relative_path())
    }

    /// Compares `actual` with the golden `name` under the default options, or writes it in
    /// update mode.
    pub fn check(
        &self,
        name: &GoldenName,
        actual: &RgbaImage,
    ) -> Result<GoldenOutcome, GoldenError> {
        self.check_with(name, actual, self.options)
    }

    /// Compares `actual` with the golden `name` under `options` (a scene's override), or
    /// writes it in update mode.
    pub fn check_with(
        &self,
        name: &GoldenName,
        actual: &RgbaImage,
        options: CompareOptions,
    ) -> Result<GoldenOutcome, GoldenError> {
        let path = self.path(name);
        if self.update {
            let written = (|| -> anyhow::Result<()> {
                std::fs::create_dir_all(path.parent().unwrap_or(&self.root))?;
                save_png(actual, &path)
            })();
            return written
                .map(|()| GoldenOutcome::Updated(path.clone()))
                .map_err(|error| GoldenError {
                    message: format!("could not write golden {}: {error:#}", path.display()),
                    artifacts: Vec::new(),
                });
        }
        let expected = match image::open(&path) {
            Ok(expected) => expected.to_rgba8(),
            Err(error) => {
                let artifacts = self.write_artifacts(name, actual, None, None);
                return Err(GoldenError {
                    message: format!(
                        "golden {} is missing or unreadable ({error}); rerun with {UPDATE_ENV_VAR}=1 to create it",
                        path.display()
                    ),
                    artifacts,
                });
            }
        };
        let comparison = compare_images(actual, &expected, options);
        if comparison.matches {
            return Ok(GoldenOutcome::Matched);
        }
        let artifacts = self.write_artifacts(name, actual, Some(&expected), Some(&comparison.diff));
        Err(GoldenError {
            message: format!(
                "{} differs from its golden: {} of {} pixels ({:.3}%) differ by more than {}, largest difference {}",
                name,
                comparison.differing_pixels,
                comparison.total_pixels,
                comparison.differing_ratio() * 100.,
                options.channel_tolerance,
                comparison.max_channel_difference
            ),
            artifacts,
        })
    }

    fn write_artifacts(
        &self,
        name: &GoldenName,
        actual: &RgbaImage,
        expected: Option<&RgbaImage>,
        diff: Option<&RgbaImage>,
    ) -> Vec<PathBuf> {
        let stem = name.file_name().trim_end_matches(".png").to_string();
        let mut written = Vec::new();
        if std::fs::create_dir_all(&self.artifacts).is_err() {
            return written;
        }
        for (suffix, image) in [
            ("actual", Some(actual)),
            ("expected", expected),
            ("diff", diff),
        ] {
            if let Some(image) = image {
                let path = self.artifacts.join(format!("{stem}.{suffix}.png"));
                if save_png(image, &path).is_ok() {
                    written.push(path);
                }
            }
        }
        written
    }
}

/// Writes `image` as a compact PNG: RGB when it is opaque, at the best compression with
/// adaptive filtering, so goldens stay under [`MAX_GOLDEN_BYTES`].
pub fn save_png(image: &RgbaImage, path: &Path) -> anyhow::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let encoder = PngEncoder::new_with_quality(file, CompressionType::Best, FilterType::Adaptive);
    if image.pixels().all(|pixel| pixel.0[3] == u8::MAX) {
        let rgb: Vec<u8> = image
            .pixels()
            .flat_map(|pixel| [pixel.0[0], pixel.0[1], pixel.0[2]])
            .collect();
        encoder.write_image(&rgb, image.width(), image.height(), ExtendedColorType::Rgb8)?;
    } else {
        encoder.write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )?;
    }
    Ok(())
}

/// A golden file that breaks the storage policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyViolation {
    /// The offending file, or the goldens root for the total budget.
    pub path: PathBuf,
    /// What is wrong with it.
    pub problem: String,
}

impl fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.problem)
    }
}

/// Audits the goldens below `root`: every file is a PNG named `<scene>@<scale>x.<backend>.png`
/// inside its backend's directory, at most [`MAX_GOLDEN_BYTES`], for a golden scene of `scenes`
/// rendered at that scale, and all of them together fit [`GOLDEN_BUDGET_BYTES`]. A missing root
/// holds no goldens.
pub fn audit_goldens(root: &Path, scenes: &[SceneSpec]) -> std::io::Result<Vec<PolicyViolation>> {
    let mut violations = Vec::new();
    if !root.exists() {
        return Ok(violations);
    }
    let mut violation = |path: &Path, problem: String| {
        violations.push(PolicyViolation {
            path: path.to_path_buf(),
            problem,
        })
    };
    let mut total = 0;
    for backend_dir in std::fs::read_dir(root)? {
        let backend_dir = backend_dir?.path();
        let backend = backend_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        if !backend_dir.is_dir() {
            violation(&backend_dir, "is not inside a backend directory".into());
            continue;
        }
        if !GOLDEN_BACKENDS.contains(&backend.as_str()) {
            violation(
                &backend_dir,
                format!("`{backend}` is not a golden backend ({GOLDEN_BACKENDS:?})"),
            );
            continue;
        }
        for file in std::fs::read_dir(&backend_dir)? {
            let file = file?;
            let path = file.path();
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                violation(&path, "is not a golden file".into());
                continue;
            }
            total += metadata.len();
            if metadata.len() > MAX_GOLDEN_BYTES {
                violation(
                    &path,
                    format!(
                        "is {} bytes, over the {MAX_GOLDEN_BYTES}-byte cap",
                        metadata.len()
                    ),
                );
            }
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let Some(name) = GoldenName::parse(file_name) else {
                violation(
                    &path,
                    "does not follow `<scene>@<scale>x.<backend>.png`".into(),
                );
                continue;
            };
            if name.backend != backend {
                violation(
                    &path,
                    format!("names backend `{}` but sits in `{backend}`", name.backend),
                );
            }
            let referenced = scenes.iter().any(|scene| {
                scene.id == name.scene
                    && scene.golden
                    && scene.scales.contains(&(name.scale as f32))
            });
            if !referenced {
                violation(
                    &path,
                    format!(
                        "no golden scene `{}` renders at {}x",
                        name.scene, name.scale
                    ),
                );
            }
        }
    }
    if total > GOLDEN_BUDGET_BYTES {
        violation(
            root,
            format!("goldens take {total} bytes, over the {GOLDEN_BUDGET_BYTES}-byte budget"),
        );
    }
    Ok(violations)
}

/// `artifacts-temp/testkit/<lane>/` under the workspace root (the nearest ancestor of the
/// working directory holding `Cargo.lock`), or under [`ARTIFACTS_ENV_VAR`] when set.
pub fn artifacts_dir(lane: &str) -> PathBuf {
    if let Ok(dir) = std::env::var(ARTIFACTS_ENV_VAR) {
        return PathBuf::from(dir).join(lane);
    }
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = start
        .ancestors()
        .find(|dir| dir.join("Cargo.lock").is_file())
        .map(Path::to_path_buf)
        .unwrap_or(start);
    root.join("artifacts-temp").join("testkit").join(lane)
}
