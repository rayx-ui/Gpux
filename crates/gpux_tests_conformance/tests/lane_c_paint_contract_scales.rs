//! The `paint-contract-*` scenes' probes at the scales mobile hosts render at: 3 (the iOS
//! Simulator) and 2.625 (a common Android density). The catalog renders scenes at 1 and 2 only,
//! but the app hosts of lanes E and F run pixel steps at the device's own scale, so the probes,
//! pattern cells and stripes included, must hold there too. Goldens are skipped: probes only.
//!
//! ```text
//! cargo test -p gpux-tests-conformance --test lane_c_paint_contract_scales -- --test-threads=1
//! ```

use gpux_tests_conformance::{
    CATALOG, SceneSpec,
    backend::EXPECT_BACKEND_ENV_VAR,
    host::{Host, describe, run_scene},
    report::Outcome,
    scene::flatten,
    test_host::TestHost,
};

fn paint_contract_scenes() -> Vec<&'static SceneSpec> {
    let scenes: Vec<_> = CATALOG
        .iter()
        .filter(|scene| scene.id.starts_with("paint-contract-"))
        .collect();
    assert_eq!(scenes.len(), 5, "the paint-contract scenes changed");
    scenes
}

/// Fails unless `host` rendered on the backend `GPUX_EXPECT_BACKEND` names, when it names one.
fn assert_backend(host: &TestHost, id: &str) {
    if let Some(expected) = std::env::var(EXPECT_BACKEND_ENV_VAR)
        .ok()
        .filter(|value| !value.is_empty())
    {
        let label = host
            .backend()
            .map(|backend| backend.label())
            .unwrap_or_else(|| "no renderer".to_string());
        assert!(
            host.backend().is_some_and(|backend| backend.is(&expected)),
            "{id} rendered on {label}, but {EXPECT_BACKEND_ENV_VAR} expects {expected}"
        );
    }
}

#[test]
fn paint_contract_scenes_hold_at_3x() {
    let mut failures = Vec::new();
    for scene in paint_contract_scenes() {
        let mut host = TestHost::pixels().probes_only();
        let result = run_scene(&mut host, scene, 3.);
        println!("{result}");
        if result.outcome != Outcome::Pass {
            failures.push(result.to_string());
        }
        assert_backend(&host, scene.id);
    }
    assert!(failures.is_empty(), "at 3x:\n{}", failures.join("\n"));
}

/// At a fractional scale layout snaps to device pixels, so element bounds move by a fraction of
/// a logical pixel; the app hosts allow one device pixel for that and the test host none. Only
/// the pixel steps run here, which is what the mobile scales put at risk.
#[test]
fn paint_contract_probes_hold_at_2_625x() {
    let mut failures = Vec::new();
    for scene in paint_contract_scenes() {
        let mut host = TestHost::pixels().probes_only();
        host.open(scene, 2.625).expect("the scene opens");
        let script = (scene.script)();
        let mut ran = 0;
        for step in flatten(&script) {
            if !step.needs_pixels() || step.is_golden() {
                continue;
            }
            ran += 1;
            if let Err(error) = host.step(scene, step) {
                failures.push(format!("{} {}: {error:#}", scene.id, describe(step)));
            }
        }
        assert!(ran > 0, "{} has no pixel step", scene.id);
        println!("{}@2.625x: {ran} pixel steps", scene.id);
        assert_backend(&host, scene.id);
        host.close();
    }
    assert!(failures.is_empty(), "at 2.625x:\n{}", failures.join("\n"));
}
