//! The scene catalog: enough scenes, every required area covered, unique ids, and complete
//! metadata on every scene.

use gpux_conformance::{Area, CATALOG, Lane, Step, lane_scenes, scene};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn the_catalog_holds_at_least_thirty_scenes() {
    assert!(CATALOG.len() >= 30, "{} scenes", CATALOG.len());
}

#[test]
fn every_required_area_has_a_scene() {
    let mut covered: BTreeMap<Area, Vec<&str>> = BTreeMap::new();
    for scene in CATALOG {
        for area in scene.areas {
            covered.entry(*area).or_default().push(scene.id);
        }
    }
    let missing: Vec<_> = Area::REQUIRED
        .iter()
        .filter(|area| !covered.contains_key(area))
        .collect();
    assert!(missing.is_empty(), "areas without a scene: {missing:?}");
    let unlisted: Vec<_> = covered
        .keys()
        .filter(|area| !Area::REQUIRED.contains(area))
        .collect();
    assert!(
        unlisted.is_empty(),
        "areas missing from Area::REQUIRED: {unlisted:?}"
    );
}

#[test]
fn scene_ids_are_unique_kebab_case_and_resolvable() {
    let mut seen = BTreeSet::new();
    for spec in CATALOG {
        assert!(seen.insert(spec.id), "scene id {} is used twice", spec.id);
        assert!(
            !spec.id.is_empty()
                && !spec.id.starts_with('-')
                && !spec.id.ends_with('-')
                && spec
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "scene id {:?} is not kebab-case",
            spec.id
        );
        assert!(std::ptr::eq(scene(spec.id).unwrap(), spec));
    }
    assert!(scene("no-such-scene").is_none());
    for lane in Lane::ALL {
        assert!(lane_scenes(lane).all(|scene| scene.runs_in(lane)));
    }
}

#[test]
fn every_scene_declares_complete_metadata() {
    let mut problems = Vec::new();
    let mut check = |holds: bool, problem: String| {
        if !holds {
            problems.push(problem);
        }
    };
    for scene in CATALOG {
        let id = scene.id;
        check(!scene.areas.is_empty(), format!("{id} covers no area"));
        let unique_areas: BTreeSet<_> = scene.areas.iter().collect();
        check(
            unique_areas.len() == scene.areas.len(),
            format!("{id} repeats an area"),
        );
        let (width, height) = scene.size;
        check(
            width > 0. && height > 0. && width <= 1024. && height <= 1024.,
            format!("{id} has size {:?}", scene.size),
        );
        check(
            scene.scales.contains(&1.) && scene.scales.iter().all(|s| matches!(*s, 1. | 2.)),
            format!(
                "{id} renders at {:?}; scenes render at 1x, optionally also 2x",
                scene.scales
            ),
        );
        check(scene.runs_in(Lane::A), format!("{id} skips lane A"));

        let script = (scene.script)();
        check(
            script.iter().any(|step| matches!(step, Step::Expect(_))),
            format!("{id} declares no semantic expectation"),
        );
        let probes = script.iter().any(|step| {
            matches!(
                step,
                Step::Probe(..)
                    | Step::ProbeUniform(..)
                    | Step::ProbeDiffer(..)
                    | Step::ChangedSince(..)
            )
        });
        check(
            probes || !scene.lanes.iter().any(|lane| lane.has_pixels()),
            format!("{id} runs in a pixel lane but declares no pixel probe"),
        );
        let golden_step = script.iter().any(Step::is_golden);
        check(
            scene.golden == golden_step,
            format!("{id}: `golden` and its script's Golden step disagree"),
        );
        check(
            !scene.golden || scene.runs_in(Lane::C),
            format!("{id} keeps goldens outside lane C"),
        );
        for step in &script {
            if let Step::GoldenWithin(tolerance) = step {
                check(
                    tolerance.validate().is_ok(),
                    format!("{id}: golden tolerance override {tolerance:?} is invalid"),
                );
            }
            if let Step::ChangedSince(name, _) = step {
                check(
                    script
                        .iter()
                        .any(|other| matches!(other, Step::Capture(captured) if captured == name)),
                    format!("{id} compares against capture {name:?} it never takes"),
                );
            }
        }
    }
    assert!(
        problems.is_empty(),
        "incomplete scenes:
{}",
        problems.join(
            "
"
        )
    );
}
