//! Command-line parsing, lane selection and the exact `--list` selection.

use gpux_conformance::{CATALOG, Lane, platform_checks::PLATFORM_CHECKS};
use gpux_testkit::{
    LANES,
    cli::{Command, LaneArgs, Selection, parse},
    lanes::{LaneId, OutputKind, plan},
};

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
    assert_eq!(plan.tests.len(), scenes + 3);
    assert!(plan.tests.contains(&"lane_a::layout_flex".to_string()));
    assert!(plan.tests.contains(&"golden_policy::*".to_string()));
    let invocation = &plan.invocations[0];
    assert_eq!(invocation.program, "cargo");
    assert_eq!(invocation.output, OutputKind::Libtest);
    assert_eq!(
        invocation.command_line(),
        "cargo test -p gpux-conformance --test lane_a --test catalog_coverage --test image_compare --test golden_policy"
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
        "cargo run -p gpux-conformance --no-default-features --features app-host --example desktop_host --"
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
            "golden_policy"
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
    assert_eq!(lane_c.listings.len(), 1);
    assert!(
        lane_c.listings[0]
            .invocation
            .command_line()
            .ends_with("--test lane_c -- --list")
    );
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
        // `gpux_conformance::selects` keeps.
        let host: Vec<String> = PLATFORM_CHECKS
            .iter()
            .map(|check| check.id)
            .chain(
                CATALOG
                    .iter()
                    .filter(|scene| scene.runs_in(Lane::D))
                    .map(|scene| scene.id),
            )
            .filter(|id| gpux_conformance::selects(id, scene, grep))
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
fn lanes_not_wired_yet_fail_clearly() {
    for lane in LANES {
        let result = plan(lane, &Selection::default());
        match lane {
            LaneId::A | LaneId::C | LaneId::D => assert!(result.is_ok(), "lane {lane}"),
            // Lane B runs Playwright specs through `web e2e`, not a cargo plan.
            LaneId::B => assert!(result.unwrap_err().to_string().contains("web e2e")),
            _ => {
                let error = result.unwrap_err().to_string();
                assert!(
                    error.contains("is not available yet") && error.contains("run lanes a, c or d"),
                    "{error}"
                );
            }
        }
    }
}
