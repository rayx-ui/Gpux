//! Command-line parsing, lane selection and the exact `--list` selection.

use gpux_testkit::{
    LANES,
    cli::{Command, LaneArgs, Selection, parse},
    lanes::{LaneId, OutputKind, plan},
};
use gpux_tests_conformance::{CATALOG, Lane, platform_checks::PLATFORM_CHECKS};

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

#[test]
fn lane_commands_parse_lanes_and_selection() {
    assert_eq!(parse(args("")).unwrap(), Command::Help);
    assert_eq!(parse(args("--help")).unwrap(), Command::Help);
    assert_eq!(
        parse(args("lane a C a --scene paint-borders --list")).unwrap(),
        Command::Lane(LaneArgs {
            lanes: vec![LaneId::A, LaneId::C],
            list: true,
            selection: Selection {
                scene: Some("paint-borders".into()),
                grep: None,
            },
            headed: false,
            full: false,
            jobs: 1,
        })
    );
    assert_eq!(
        parse(args("lane d --grep platform")).unwrap(),
        Command::Lane(LaneArgs {
            lanes: vec![LaneId::D],
            list: false,
            selection: Selection {
                scene: None,
                grep: Some("platform".into()),
            },
            headed: false,
            full: false,
            jobs: 1,
        })
    );
}

#[test]
fn bad_command_lines_fail_with_actionable_messages() {
    let error = parse(args("lane q")).unwrap_err().to_string();
    assert!(
        error.contains("unknown lane `q`") && error.contains("d (desktop windows)"),
        "{error}"
    );
    assert!(
        parse(args("lane"))
            .unwrap_err()
            .to_string()
            .contains("at least one lane")
    );
    assert!(
        parse(args("lane a --scene"))
            .unwrap_err()
            .to_string()
            .contains("needs a value")
    );
    assert!(
        parse(args("lane a --grep x --grep y"))
            .unwrap_err()
            .to_string()
            .contains("twice")
    );
    assert!(
        parse(args("lane a --fast"))
            .unwrap_err()
            .to_string()
            .contains("unknown option")
    );
    assert!(
        parse(args("run a"))
            .unwrap_err()
            .to_string()
            .contains("unknown command")
    );
}

#[test]
fn lane_a_selects_every_lane_a_scene_and_the_catalog_groups() {
    let plan = plan(LaneId::A, &Selection::default()).unwrap();
    let scenes = CATALOG
        .iter()
        .filter(|scene| scene.runs_in(Lane::A))
        .count();
    assert_eq!(plan.tests.len(), scenes + 8);
    assert!(plan.tests.contains(&"lane_a::layout_flex".to_string()));
    assert!(plan.tests.contains(&"golden_policy::*".to_string()));
    assert!(plan.tests.contains(&"executor_clock::*".to_string()));
    let invocation = &plan.invocations[0];
    assert_eq!(invocation.program, "cargo");
    assert_eq!(invocation.output, OutputKind::Libtest);
    assert_eq!(
        invocation.command_line(),
        "cargo test -p gpux-tests-conformance --test lane_a --test catalog_coverage --test image_compare \
         --test golden_policy --test executor_clock --test host_capabilities --test rgb565_tolerance \
         --test screen_shows --test lane_a_fast_parity --no-fail-fast"
    );
}

#[test]
fn scene_and_grep_narrow_lanes_to_exact_tests() {
    let plan_c = plan(
        LaneId::C,
        &Selection {
            scene: Some("paint-borders".into()),
            grep: None,
        },
    )
    .unwrap();
    assert_eq!(
        plan_c.tests,
        vec![
            "lane_c::at_1x::paint_borders",
            "lane_c::at_2x::paint_borders"
        ]
    );
    assert!(
        plan_c.invocations[0]
            .command_line()
            .ends_with("-- --test-threads=1 --exact at_1x::paint_borders at_2x::paint_borders")
    );

    let plan_a = plan(
        LaneId::A,
        &Selection {
            scene: None,
            grep: Some("scroll".into()),
        },
    )
    .unwrap();
    assert!(!plan_a.tests.is_empty());
    assert!(
        plan_a
            .tests
            .iter()
            .all(|test| test.starts_with("lane_a::scroll"))
    );

    let unknown = plan(
        LaneId::A,
        &Selection {
            scene: Some("paint-border".into()),
            grep: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        unknown.contains("similar scenes: paint-borders"),
        "{unknown}"
    );
    let none = plan(
        LaneId::C,
        &Selection {
            scene: None,
            grep: Some("no-such".into()),
        },
    )
    .unwrap();
    assert!(none.tests.is_empty() && none.invocations.is_empty());
}

#[test]
fn lane_d_runs_the_desktop_host_with_platform_checks_and_scenes() {
    let plan = plan(LaneId::D, &Selection::default()).unwrap();
    for check in PLATFORM_CHECKS {
        assert!(plan.tests.contains(&format!("desktop_host::{}", check.id)));
    }
    let scenes = CATALOG
        .iter()
        .filter(|scene| scene.runs_in(Lane::D))
        .count();
    assert_eq!(plan.tests.len(), PLATFORM_CHECKS.len() + scenes);
    assert_eq!(plan.invocations[0].output, OutputKind::ResultLines);
    assert_eq!(
        plan.invocations[0].command_line(),
        "cargo run -p gpux-tests-conformance --no-default-features --features app-host --example desktop_host --"
    );
    let scene = plan_scene(LaneId::D, "layout-flex");
    assert_eq!(scene.tests, vec!["desktop_host::layout-flex"]);
    assert!(
        scene.invocations[0]
            .command_line()
            .ends_with("-- --scene layout-flex")
    );
}

fn plan_scene(lane: LaneId, scene: &str) -> gpux_testkit::lanes::LanePlan {
    plan(
        lane,
        &Selection {
            scene: Some(scene.into()),
            grep: None,
        },
    )
    .unwrap()
}

#[test]
fn libtest_lanes_list_every_target_they_run() {
    let everything = plan(LaneId::A, &Selection::default()).unwrap();
    let targets: Vec<&str> = everything
        .listings
        .iter()
        .map(|listing| listing.target.as_str())
        .collect();
    assert_eq!(
        targets,
        [
            "lane_a",
            "catalog_coverage",
            "image_compare",
            "golden_policy",
            "executor_clock",
            "host_capabilities",
            "rgb565_tolerance",
            "screen_shows",
            "lane_a_fast_parity"
        ]
    );
    for listing in &everything.listings {
        assert!(
            listing
                .invocation
                .command_line()
                .ends_with(&format!("--test {} -- --list", listing.target)),
            "{}",
            listing.invocation.command_line()
        );
    }
    let lane_c = plan(LaneId::C, &Selection::default()).unwrap();
    let targets: Vec<&str> = lane_c
        .listings
        .iter()
        .map(|listing| listing.target.as_str())
        .collect();
    assert_eq!(
        targets,
        [
            "lane_c",
            "lane_g",
            "lane_c_fast_parity",
            "lane_c_scroll_layers",
            "lane_c_layer_scales",
            "lane_c_list_hover",
            "lane_c_profile",
            "lane_c_compare",
            "lane_c_sampling"
        ]
    );
    assert!(
        lane_c.listings[1]
            .invocation
            .command_line()
            .ends_with("--test lane_g -- --list")
    );
    assert_eq!(
        lane_c.invocations[0].command_line(),
        "cargo test -p gpux-tests-conformance --test lane_c --test lane_g --test lane_c_fast_parity \
         --test lane_c_scroll_layers --test lane_c_layer_scales --test lane_c_list_hover \
         --test lane_c_profile --test lane_c_compare --test lane_c_sampling --no-fail-fast -- --test-threads=1"
    );
    assert!(lane_c.tests.contains(&"lane_g::*".to_string()));
    assert!(
        plan(LaneId::D, &Selection::default())
            .unwrap()
            .listings
            .is_empty()
    );
}

#[test]
fn lane_d_plans_exactly_what_the_desktop_host_selects() {
    let selections = [
        (None, None),
        (None, Some("layout_flex")),
        (None, Some("layout-flex")),
        (None, Some("platform")),
        (Some("layout-flex"), None),
        (Some("layout-flex"), Some("text")),
        (Some("layout-flex"), Some("flex")),
    ];
    for (scene, grep) in selections {
        let selection = Selection {
            scene: scene.map(str::to_string),
            grep: grep.map(str::to_string),
        };
        let planned = plan(LaneId::D, &selection).unwrap().tests;
        // The desktop host runs the platform checks, then the lane-D scenes, that
        // `gpux_tests_conformance::selects` keeps.
        let host: Vec<String> = PLATFORM_CHECKS
            .iter()
            .map(|check| check.id)
            .chain(
                CATALOG
                    .iter()
                    .filter(|scene| scene.runs_in(Lane::D))
                    .map(|scene| scene.id),
            )
            .filter(|id| gpux_tests_conformance::selects(id, scene, grep))
            .map(|id| format!("desktop_host::{id}"))
            .collect();
        assert_eq!(planned, host, "scene {scene:?}, grep {grep:?}");
    }
    assert!(
        plan(
            LaneId::D,
            &Selection {
                scene: None,
                grep: Some("layout_flex".into()),
            }
        )
        .unwrap()
        .tests
        .contains(&"desktop_host::layout-flex".to_string())
    );
}

#[test]
fn lanes_without_a_cargo_plan_name_their_runner() {
    for lane in LANES {
        let result = plan(lane, &Selection::default());
        let runner = match lane {
            LaneId::A | LaneId::C | LaneId::D => {
                assert!(result.is_ok(), "lane {lane}");
                continue;
            }
            // Lane B runs Playwright specs through `web e2e`.
            LaneId::B => "web e2e",
            // Lane E runs on a device through `android run`.
            LaneId::E => "android run",
            // Lane F runs on the iOS Simulator through `ios run --simulator`.
            LaneId::F => "ios run --simulator",
            // Lane G runs lanes B, C and D and the benchmarks through `gpux-testkit lane g`.
            LaneId::G => "`gpux-testkit lane g` runs them",
        };
        let error = result.unwrap_err().to_string();
        assert!(error.contains(runner), "lane {lane}: {error}");
    }
}
