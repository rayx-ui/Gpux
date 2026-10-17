//! Lane G's lane C run: the developer's GPU matches no committed golden, so goldens are switched
//! off and the relative probes still run, and `GPUX_EXPECT_BACKEND=hardware` accepts any GPU
//! while rejecting the software rasterizers.

use gpux_tests_conformance::{
    backend::BackendId,
    compare::{CompareOptions, Goldens},
    host::run_scene,
    report::Outcome,
    test_host::TestHost,
};

/// A host whose goldens live in an empty folder: every golden comparison fails as missing.
fn without_goldens(root: &std::path::Path) -> TestHost {
    TestHost::pixels().with_goldens(Goldens {
        root: root.join("goldens"),
        artifacts: root.join("artifacts"),
        update: false,
        options: CompareOptions::default(),
    })
}

#[test]
fn probes_only_skips_goldens_and_keeps_the_probes() {
    let scene = gpux_tests_conformance::scene("color-srgb-compositing").unwrap();
    assert!(scene.golden, "the scene must be in the golden subset");
    let dir = tempfile::tempdir().unwrap();

    let mut host = without_goldens(dir.path());
    let compared = run_scene(&mut host, scene, 1.);
    let backend = host.backend().cloned();
    if backend
        .as_ref()
        .is_some_and(|backend| backend.golden_id().is_some())
    {
        // A software backend compares, and finds no golden.
        assert_eq!(compared.outcome, Outcome::Fail, "{compared}");
    }

    let mut host = without_goldens(dir.path()).probes_only();
    let probed = run_scene(&mut host, scene, 1.);
    assert_eq!(probed.outcome, Outcome::Pass, "{probed}");
    assert!(
        probed.detail.contains("relative probes only"),
        "{probed} [{backend:?}]"
    );
    // The probes ran: only the golden step was skipped.
    assert!(probed.detail.contains("1 skipped"), "{probed}");
}

#[test]
fn the_hardware_expectation_accepts_any_gpu_and_no_software_rasterizer() {
    let gpu = BackendId::Hardware("NVIDIA GeForce RTX 4070".into());
    assert!(gpu.is("hardware"));
    assert!(!gpu.is("warp"));
    for software in [
        BackendId::Warp,
        BackendId::Lavapipe,
        BackendId::Llvmpipe,
        BackendId::SwiftShaderWebGpu,
    ] {
        assert!(!software.is("hardware"), "{software:?}");
        assert!(software.is(&software.label()), "{software:?}");
    }
    assert!(BackendId::Metal.is("metal"));
}
