//! The golden storage policy: committed goldens are small PNGs named after a golden scene,
//! scale and backend, they fit a total budget, and the published package leaves them out.

use gpux_conformance::{
    CATALOG,
    compare::{GOLDEN_BUDGET_BYTES, GoldenName, MAX_GOLDEN_BYTES, audit_goldens},
};
use std::path::{Path, PathBuf};

fn committed_goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
}

fn write(path: &Path, bytes: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![0u8; bytes]).unwrap();
}

#[test]
fn committed_goldens_follow_the_policy() {
    let violations = audit_goldens(&committed_goldens(), CATALOG).unwrap();
    assert!(
        violations.is_empty(),
        "golden policy violations:\n{}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_audit_rejects_oversize_misnamed_misplaced_and_orphan_goldens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let golden_scene = CATALOG
        .iter()
        .find(|scene| scene.golden && scene.scales.contains(&2.))
        .expect("a golden scene at 2x");
    let plain_scene = CATALOG
        .iter()
        .find(|scene| !scene.golden)
        .expect("a scene without goldens");

    let good = root
        .join("warp")
        .join(GoldenName::new(golden_scene.id, 2, "warp").file_name());
    write(&good, 1024);
    let oversize = root
        .join("lavapipe")
        .join(GoldenName::new(golden_scene.id, 2, "lavapipe").file_name());
    write(&oversize, MAX_GOLDEN_BYTES as usize + 1);
    let misnamed = root
        .join("warp")
        .join(format!("{}-2x.png", golden_scene.id));
    write(&misnamed, 10);
    let wrong_directory = root
        .join("metal")
        .join(GoldenName::new(golden_scene.id, 2, "warp").file_name());
    write(&wrong_directory, 10);
    let unknown_scene = root
        .join("warp")
        .join(GoldenName::new("no-such-scene", 1, "warp").file_name());
    write(&unknown_scene, 10);
    let not_golden = root
        .join("warp")
        .join(GoldenName::new(plain_scene.id, 1, "warp").file_name());
    write(&not_golden, 10);
    let hardware = root
        .join("nvidia")
        .join(format!("{}@2x.nvidia.png", golden_scene.id));
    write(&hardware, 10);
    let loose = root.join(GoldenName::new(golden_scene.id, 2, "warp").file_name());
    write(&loose, 10);

    let violations = audit_goldens(root, CATALOG).unwrap();
    let problem = |path: &Path| {
        violations
            .iter()
            .filter(|violation| violation.path == path)
            .map(|violation| violation.problem.as_str())
            .collect::<Vec<_>>()
    };
    assert!(problem(&good).is_empty(), "{violations:#?}");
    assert!(problem(&oversize)[0].contains("cap"), "{violations:#?}");
    assert!(
        problem(&misnamed)[0].contains("<scene>@<scale>x.<backend>.png"),
        "{violations:#?}"
    );
    assert!(
        problem(&wrong_directory)[0].contains("sits in `metal`"),
        "{violations:#?}"
    );
    assert!(
        problem(&unknown_scene)[0].contains("no golden scene"),
        "{violations:#?}"
    );
    assert!(
        problem(&not_golden)[0].contains("no golden scene"),
        "{violations:#?}"
    );
    assert!(
        problem(&root.join("nvidia"))[0].contains("not a golden backend"),
        "{violations:#?}"
    );
    assert!(
        problem(&loose)[0].contains("backend directory"),
        "{violations:#?}"
    );
    assert_eq!(violations.len(), 7, "{violations:#?}");
}

#[test]
fn the_audit_enforces_the_total_budget() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Fill the budget with valid goldens of every golden scene at every scale and backend,
    // then go past it.
    let mut written = 0;
    'fill: for backend in [
        "warp",
        "lavapipe",
        "llvmpipe",
        "metal",
        "swiftshader-webgpu",
    ] {
        for scene in CATALOG.iter().filter(|scene| scene.golden) {
            for scale in scene.scales {
                let name = GoldenName::new(scene.id, *scale as u32, backend);
                write(
                    &root.join(backend).join(name.file_name()),
                    MAX_GOLDEN_BYTES as usize,
                );
                written += MAX_GOLDEN_BYTES;
                if written > GOLDEN_BUDGET_BYTES {
                    break 'fill;
                }
            }
        }
    }
    assert!(
        written > GOLDEN_BUDGET_BYTES,
        "the catalog cannot exceed the budget"
    );
    let violations = audit_goldens(root, CATALOG).unwrap();
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(violations[0].path, root);
    assert!(violations[0].problem.contains("budget"), "{violations:#?}");

    // A missing goldens directory holds no goldens.
    assert!(
        audit_goldens(&root.join("missing"), CATALOG)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_package_excludes_goldens_and_git_lfs_is_unused() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let exclude = manifest
        .lines()
        .find(|line| line.trim_start().starts_with("exclude"))
        .expect("the manifest excludes files from the package");
    assert!(
        exclude.contains("\"tests/goldens/**\""),
        "goldens must be excluded from the package: {exclude}"
    );

    let workspace = manifest_dir
        .ancestors()
        .find(|dir| dir.join("Cargo.lock").is_file())
        .expect("the workspace root");
    let goldens = committed_goldens();
    for attributes in [manifest_dir, workspace, goldens.as_path()]
        .map(|dir| dir.join(".gitattributes"))
        .into_iter()
        .filter(|path| path.is_file())
    {
        let text = std::fs::read_to_string(&attributes).unwrap();
        assert!(
            !text.contains("filter=lfs"),
            "{} routes files through Git LFS",
            attributes.display()
        );
    }
}
