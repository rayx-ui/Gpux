//! The conformance hosts on the AccessKit tree: every host resolves scene queries through the
//! window's accessibility tree, read while it inspects the window (`evaluate::inspect`,
//! `evaluate::keep_inspecting`, `evaluate::snapshot`, the functions the app, desktop, web, iOS
//! and Android hosts share); the test host runs the scenes on it; `Area::Accessibility` names
//! the scenes about it; and the status helper publishes a status node with its value.
//!
//! `cargo test -p gpux-conformance --test conformance_accessibility`

use gpux::{
    AnyView, AppContext as _, Context, IntoElement, Render, TestAppContext, TestDispatcher, Window,
    prelude::*, semantic_query::Query,
};
use gpux_conformance::{
    Area, CATALOG, Lane, evaluate,
    host::run_scene,
    report::Outcome,
    scene::{Expect, Step, flatten},
    scenes::common::{status, status_is},
    test_host::TestHost,
};

fn scene(id: &str) -> &'static gpux_conformance::SceneSpec {
    gpux_conformance::scene(id).unwrap_or_else(|| panic!("no scene {id}"))
}

/// The view a scene builds, as a window's root.
struct SceneRoot(AnyView);

impl Render for SceneRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

/// Opens `id` in a window of the test platform, inspecting it as the hosts that open their own
/// windows do when `inspect`, and draws its first frame.
fn open(id: &str, inspect: bool) -> (TestAppContext, gpux::AnyWindowHandle) {
    let spec = scene(id);
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
    );
    let window = cx.add_window(move |window, cx| {
        if inspect {
            evaluate::keep_inspecting(window);
        }
        SceneRoot((spec.build)(window, cx))
    });
    cx.run_until_parked();
    (cx, window.into())
}

#[test]
fn hosts_resolve_scene_queries_through_the_accesskit_tree() {
    for id in ["accessibility", "semantics-accessibility"] {
        let (mut cx, window) = open(id, true);
        // Every expectation of the scene's opening state, as the app hosts check them.
        let opening: Vec<Expect> = flatten(&(scene(id).script)())
            .into_iter()
            .take_while(|step| matches!(step, Step::Expect(_)))
            .filter_map(|step| match step {
                Step::Expect(expect) => Some(expect.clone()),
                _ => None,
            })
            .collect();
        assert!(opening.len() > 3, "{id} opens with expectations");
        cx.update_window(window, |_, window, _| {
            let snapshot = evaluate::snapshot(window).expect("the window builds its tree");
            for expect in &opening {
                evaluate::expect(&snapshot, expect, 0.)
                    .unwrap_or_else(|error| panic!("{id}: {expect:?}: {error:#}"));
            }
        })
        .unwrap();
        evaluate::stop_inspecting(window);
    }
}

#[test]
fn a_window_not_inspected_has_no_tree_to_query() {
    let (mut cx, window) = open("accessibility", false);
    let error = cx
        .update_window(window, |_, window, _| {
            evaluate::snapshot(window).unwrap_err()
        })
        .unwrap();
    assert!(
        error.to_string().contains("no accessibility tree"),
        "{error}"
    );
}

#[test]
fn the_test_host_runs_the_accessibility_scenes() {
    for id in ["accessibility", "semantics-accessibility"] {
        let mut host = TestHost::behaviour();
        let result = run_scene(&mut host, scene(id), 1.);
        assert_eq!(result.outcome, Outcome::Pass, "{result}");
    }
}

#[test]
fn expect_accessible_reads_the_tree() {
    let (mut cx, window) = open("accessibility", true);
    cx.update_window(window, |_, window, _| {
        let snapshot = evaluate::snapshot(window).unwrap();
        evaluate::expect(&snapshot, &Expect::Accessible("Subscribe".into()), 0.).unwrap();
        let error =
            evaluate::expect(&snapshot, &Expect::Accessible("Unsubscribe".into()), 0.).unwrap_err();
        assert!(
            error.to_string().contains("Subscribe"),
            "names are listed: {error}"
        );
    })
    .unwrap();
}

struct Status;

impl Render for Status {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpux_conformance::scenes::common::root().child(status("ready 3"))
    }
}

#[test]
fn the_status_helper_publishes_a_status_node_with_its_value() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|window, _| {
        evaluate::keep_inspecting(window);
        Status
    });
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, _| {
        let snapshot = evaluate::snapshot(window).unwrap();
        let node = status_is("ready 3").resolve(&snapshot).unwrap();
        assert_eq!(node.role, "status");
        assert_eq!(node.value.as_deref(), Some("ready 3"));
        assert!(
            Query::text("ready 3").resolve(&snapshot).is_ok(),
            "its text shows too"
        );
    })
    .unwrap();
}

#[test]
fn accessibility_scenes_are_catalogued_under_their_area() {
    let ids: Vec<&str> = CATALOG
        .iter()
        .filter(|scene| scene.areas.contains(&Area::Accessibility))
        .map(|scene| scene.id)
        .collect();
    assert_eq!(ids, ["semantics-accessibility", "accessibility"]);
    let controls = scene("accessibility");
    for lane in [Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F] {
        assert!(
            controls.runs_in(lane),
            "the accessibility scene runs in lane {lane:?}"
        );
    }
    // The scenes annotate through AccessKit only.
    for source in [
        include_str!("../src/scenes/semantics.rs"),
        include_str!("../src/scenes/common.rs"),
        include_str!("../src/scenes/editing.rs"),
        include_str!("../src/scenes/lists.rs"),
    ] {
        assert!(!source.contains("devtools_semantics"));
        assert!(!source.contains("DevtoolsSemantics"));
    }
}
