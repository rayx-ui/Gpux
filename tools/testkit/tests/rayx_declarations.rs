//! What gpux declares for rayx (RayX.CLI's `docs/app-project.md` owns the contract): `gpux-fonts`
//! is a rayx asset package, `gpux-mobile` names its Android Java sources, and the workspace pins
//! the Android SDK pieces. Cargo ignores these declarations and gpux never reads them; these tests
//! keep them equal to what gpux's own lanes use (lane E's pins and Java directory).

use std::path::{Path, PathBuf};

use gpux_testkit::android;
use toml::{Table, Value};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn parse(path: &str) -> Table {
    std::fs::read_to_string(root().join(path))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
        .parse::<Table>()
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn get<'a>(table: &'a Table, path: &[&str]) -> &'a Value {
    let (last, parents) = path.split_last().unwrap();
    let mut current = table;
    for key in parents {
        current = current
            .get(*key)
            .and_then(Value::as_table)
            .unwrap_or_else(|| panic!("missing table `{key}` of {path:?}"));
    }
    current
        .get(*last)
        .unwrap_or_else(|| panic!("missing `{last}` of {path:?}"))
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect()
}

#[test]
fn gpux_fonts_is_a_rayx_font_asset_package() {
    let manifest = parse("crates/gpux_fonts/Cargo.toml");
    let declared = get(&manifest, &["package", "metadata", "rayx", "assets"])
        .as_table()
        .unwrap();
    assert_eq!(declared.len(), 1, "{declared:?}");
    assert_eq!(
        declared["manifest"].as_str(),
        Some("rayx.assets.toml"),
        "[package.metadata.rayx.assets] manifest"
    );

    let assets = parse("crates/gpux_fonts/rayx.assets.toml");
    let mut keys: Vec<&String> = assets.keys().collect();
    keys.sort();
    assert_eq!(keys, ["asset_package", "assets", "version"]);
    assert_eq!(assets["version"].as_integer(), Some(1));
    let package = assets["asset_package"].as_table().unwrap();
    assert_eq!(package.len(), 3, "{package:?}");
    assert_eq!(package["id"].as_str(), Some("gpux-fonts"));
    assert_eq!(package["kind"].as_str(), Some("fonts"));
    assert_eq!(package["namespace"].as_str(), Some(""));
    let roots = assets["assets"].as_table().unwrap();
    assert_eq!(roots.len(), 1, "{roots:?}");
    assert_eq!(strings(&roots["roots"]), ["assets"]);

    // The logical `fonts/...` paths gpux's SVG renderer loads through the app's `AssetSource`.
    for family in ["fonts/ibm-plex-sans", "fonts/lilex"] {
        let dir = root().join("crates/gpux_fonts/assets").join(family);
        let fonts = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "ttf" || extension == "otf")
            })
            .count();
        assert!(fonts > 0, "{} holds font files", dir.display());
    }
}

#[test]
fn gpux_mobile_declares_the_java_sources_lane_e_compiles() {
    let manifest = parse("crates/gpui_mobile/Cargo.toml");
    let rayx = get(&manifest, &["package", "metadata", "rayx"])
        .as_table()
        .unwrap();
    assert_eq!(rayx.len(), 1, "{rayx:?}");
    let dirs = strings(&rayx["android_java_dirs"]);
    assert_eq!(dirs, ["java"]);
    for dir in dirs {
        // Relative to the manifest, it is the directory lane E hands to javac.
        assert_eq!(
            Path::new("crates/gpui_mobile").join(dir),
            Path::new(android::JAVA_SOURCES)
        );
        let sources = root().join("crates/gpui_mobile").join(dir);
        let java: Vec<_> = std::fs::read_dir(sources.join("dev/gpux/mobile"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "java")
            })
            .collect();
        assert!(
            java.iter()
                .any(|path| path.ends_with("GpuxNativeActivity.java")),
            "{java:?}"
        );
    }
}

#[test]
fn the_workspace_pins_lane_e_android_tools_for_rayx() {
    let manifest = parse("Cargo.toml");
    let pins = get(&manifest, &["workspace", "metadata", "rayx"])
        .as_table()
        .unwrap();
    let platform = format!("android-{}", android::COMPILE_SDK);
    let expected = [
        ("android-platform", platform.as_str()),
        ("android-build-tools", android::BUILD_TOOLS_VERSION),
        ("android-ndk", android::NDK_VERSION),
    ];
    // No `android-system-image`: rayx keeps deriving it, as for the old gpux-checkout profile.
    assert_eq!(pins.len(), expected.len(), "{pins:?}");
    for (key, value) in expected {
        assert_eq!(
            pins[key].as_str(),
            Some(value),
            "[workspace.metadata.rayx] {key}"
        );
    }

    // lane-e.yml installs the same pieces on the CI emulator host.
    let workflow = std::fs::read_to_string(root().join(".github/workflows/lane-e.yml")).unwrap();
    let packages: Vec<&str> = workflow
        .lines()
        .skip_while(|line| line.trim() != "ANDROID_PACKAGES: >-")
        .nth(1)
        .expect("lane-e.yml sets ANDROID_PACKAGES")
        .split_whitespace()
        .collect();
    for piece in [
        format!("ndk;{}", android::NDK_VERSION),
        format!("build-tools;{}", android::BUILD_TOOLS_VERSION),
        format!("platforms;{platform}"),
    ] {
        assert!(
            packages.contains(&piece.as_str()),
            "{piece} in {packages:?}"
        );
    }
}
