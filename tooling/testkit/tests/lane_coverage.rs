//! Every conformance test target runs in a lane: a target no lane plans never runs in CI.

use std::collections::BTreeSet;

use gpux_testkit::{
    cli::Selection,
    lanes::{LaneId, plan},
};

/// The `crates/gpux_conformance/tests/*.rs` targets.
fn conformance_targets() -> BTreeSet<String> {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/gpux_conformance/tests");
    std::fs::read_dir(&tests)
        .unwrap_or_else(|error| panic!("{}: {error}", tests.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// The test targets a lane's full run builds: each one it lists, checked against its command.
fn planned(lane: LaneId) -> Vec<String> {
    let plan = plan(lane, &Selection::default()).unwrap();
    let command = format!("{} ", plan.invocations[0].command_line());
    plan.listings
        .iter()
        .map(|listing| {
            assert!(
                command.contains(&format!("--test {} ", listing.target)),
                "lane {lane} lists `{}` but does not run it: {command}",
                listing.target
            );
            listing.target.clone()
        })
        .collect()
}

#[test]
fn every_conformance_test_target_runs_in_lane_a_or_c() {
    let targets = conformance_targets();
    assert!(
        targets.contains("lane_a") && targets.contains("lane_c"),
        "{targets:?}"
    );
    let mut planned_targets = BTreeSet::new();
    for target in planned(LaneId::A).into_iter().chain(planned(LaneId::C)) {
        assert!(
            planned_targets.insert(target.clone()),
            "`{target}` runs in lanes a and c"
        );
    }
    let unplanned: Vec<&String> = targets.difference(&planned_targets).collect();
    assert!(
        unplanned.is_empty(),
        "conformance test targets no lane runs: {unplanned:?}; add each to lane a's groups (no \
         renderer) or lane c's (renderer) in tooling/testkit/src/lanes.rs"
    );
    let missing: Vec<&String> = planned_targets.difference(&targets).collect();
    assert!(
        missing.is_empty(),
        "lanes plan targets that do not exist: {missing:?}"
    );
}
