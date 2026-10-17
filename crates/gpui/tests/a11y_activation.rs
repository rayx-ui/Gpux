//! Accessibility activation sources and modes: a window builds its tree while the platform
//! source (an adapter that activated it), the inspection source or `AccessibilityMode::Always`
//! asks for it, and sends it to the platform adapter only for frames the platform source covered.
//! The test window records every update a platform adapter would receive.

use gpux::{
    AccessibilityMode, AnyWindowHandle, Application, Context, IntoElement, Render, Role,
    TestAccessibilityAdapter, TestAppContext, TestDispatcher, VisualTestContext, Window, div,
    prelude::*, px,
};

struct Labelled {
    /// Set by the test: the mode the next render switches to, in the middle of its frame.
    switch_mode_while_rendering: Option<AccessibilityMode>,
    /// Set by the test: the next render activates (`true`) or deactivates the window's
    /// platform adapter, in the middle of its frame.
    switch_adapter_while_rendering: Option<(TestAccessibilityAdapter, bool)>,
}

impl Render for Labelled {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(mode) = self.switch_mode_while_rendering.take() {
            cx.set_accessibility_mode(mode);
        }
        if let Some((adapter, on)) = self.switch_adapter_while_rendering.take() {
            if on {
                adapter.activate();
            } else {
                adapter.deactivate();
            }
        }
        div().size_full().child(
            div()
                .id("save")
                .role(Role::Button)
                .aria_label("Save")
                .w(px(80.))
                .h(px(24.)),
        )
    }
}

fn open(cx: &mut TestAppContext) -> (VisualTestContext, AnyWindowHandle, gpux::Entity<Labelled>) {
    let (view, vcx) = cx.add_window_view(|_, _| Labelled {
        switch_mode_while_rendering: None,
        switch_adapter_while_rendering: None,
    });
    let vcx = vcx.clone();
    let window = vcx.window_handle();
    vcx.run_until_parked();
    (vcx, window, view)
}

fn redraw(vcx: &mut VisualTestContext) {
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
}

/// The labels of every node in `updates`.
fn labels(updates: &[accesskit::TreeUpdate]) -> Vec<String> {
    updates
        .iter()
        .flat_map(|update| update.nodes.iter())
        .filter_map(|(_, node)| node.label().map(str::to_string))
        .collect()
}

fn has_save(update: &accesskit::TreeUpdate) -> bool {
    update
        .nodes
        .iter()
        .any(|(_, node)| node.label() == Some("Save"))
}

/// Whether the window's last frame built a tree holding the button.
fn tree_has_save(vcx: &mut VisualTestContext) -> bool {
    vcx.update(|window, _| window.a11y_tree().is_some_and(has_save))
}

#[gpux::test]
fn no_source_builds_no_tree(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    redraw(&mut vcx);
    assert!(vcx.take_accessibility_updates(window).is_empty());
    assert!(vcx.update(|window, _| window.a11y_tree().is_none()));
    assert!(!vcx.update(|window, _| window.is_a11y_active()));
}

#[gpux::test]
fn the_platform_source_builds_the_tree_and_sends_it(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    vcx.activate_accessibility(window)
        .expect("the test window has an accessibility adapter");
    redraw(&mut vcx);
    let updates = vcx.take_accessibility_updates(window);
    assert!(labels(&updates).iter().any(|label| label == "Save"));
    assert!(tree_has_save(&mut vcx));
}

#[gpux::test]
fn a_frame_built_for_inspection_alone_sends_nothing(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    assert!(!guard.accessibility_disabled());
    vcx.run_until_parked();
    assert!(tree_has_save(&mut vcx), "inspection builds the tree");
    assert!(
        vcx.take_accessibility_updates(window).is_empty(),
        "nothing reaches the platform adapter for an inspection frame"
    );
    drop(guard);
}

#[gpux::test]
fn turning_one_source_off_keeps_the_tree_while_the_other_is_on(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    vcx.activate_accessibility(window).unwrap();
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    redraw(&mut vcx);
    assert!(!vcx.take_accessibility_updates(window).is_empty());

    // The platform source goes; inspection keeps the tree, and the adapter hears nothing.
    vcx.deactivate_accessibility(window);
    redraw(&mut vcx);
    assert!(tree_has_save(&mut vcx));
    assert!(vcx.take_accessibility_updates(window).is_empty());

    // The platform source comes back and inspection goes; the adapter gets the tree again.
    vcx.activate_accessibility(window).unwrap();
    drop(guard);
    redraw(&mut vcx);
    assert!(tree_has_save(&mut vcx));
    assert!(vcx.take_accessibility_updates(window).iter().any(has_save));

    // Both gone: no tree.
    vcx.deactivate_accessibility(window);
    redraw(&mut vcx);
    assert!(vcx.update(|window, _| window.a11y_tree().is_none()));
    assert!(vcx.take_accessibility_updates(window).is_empty());
}

#[gpux::test]
fn a_source_switched_during_a_frame_takes_effect_on_the_next(cx: &mut TestAppContext) {
    let (mut vcx, window, view) = open(cx);
    vcx.activate_accessibility(window).unwrap();
    redraw(&mut vcx);
    vcx.take_accessibility_updates(window);

    // The render that switches to `Off` still belongs to a frame the platform source covered,
    // so that frame's full tree is sent; the next frame hides the window.
    vcx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.switch_mode_while_rendering = Some(AccessibilityMode::Off);
            cx.notify();
        })
    });
    vcx.run_until_parked();
    let updates = vcx.take_accessibility_updates(window);
    assert!(
        updates.len() >= 2,
        "the switching frame and the next one: {updates:?}"
    );
    assert!(
        has_save(&updates[0]),
        "the switching frame sends its full tree"
    );
    assert_eq!(
        updates.last().unwrap().nodes.len(),
        1,
        "the next frame sends only the root"
    );
}

/// The platform source counts for a frame only when it is on at the frame's start and its end:
/// an adapter that disconnects while the frame draws gets nothing from it, and one that connects
/// gets the next frame's tree.
#[gpux::test]
fn an_adapter_switched_during_a_frame_gets_only_frames_it_covered(cx: &mut TestAppContext) {
    // Frames are drawn one at a time, so each frame's sends are seen alone (activation asks for
    // a frame of its own, which would otherwise follow at once).
    cx.set_draw_on_flush(false);
    let (mut vcx, window, view) = open(cx);
    let adapter = vcx.accessibility_adapter(window);
    let draw = |vcx: &mut VisualTestContext, switch: Option<bool>| {
        if let Some(on) = switch {
            vcx.update(|_, cx| {
                view.update(cx, |view, cx| {
                    view.switch_adapter_while_rendering = Some((adapter.clone(), on));
                    cx.notify();
                })
            });
        }
        vcx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        vcx.take_accessibility_updates(window)
    };
    draw(&mut vcx, None);

    // Connects while a frame draws: that frame started without it and sends nothing.
    let joined = draw(&mut vcx, Some(true));
    assert!(
        joined.is_empty(),
        "the frame it joined is not sent: {joined:?}"
    );
    assert!(
        draw(&mut vcx, None).iter().any(has_save),
        "the next frame is"
    );

    // Disconnects while a frame draws: that frame ends without it and sends nothing.
    let left = draw(&mut vcx, Some(false));
    assert!(left.is_empty(), "the frame it left is not sent: {left:?}");
    assert!(draw(&mut vcx, None).is_empty());
}

#[gpux::test]
fn always_builds_the_tree_without_assistive_technology(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    vcx.update(|_, cx| cx.set_accessibility_mode(AccessibilityMode::Always));
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_, cx| cx.accessibility_mode()),
        AccessibilityMode::Always
    );
    assert!(tree_has_save(&mut vcx));
    assert!(
        vcx.take_accessibility_updates(window).is_empty(),
        "Always sends nothing to an adapter that has not activated"
    );

    // Once the adapter activates, it receives the tree.
    vcx.activate_accessibility(window).unwrap();
    redraw(&mut vcx);
    assert!(vcx.take_accessibility_updates(window).iter().any(has_save));
}

#[gpux::test]
fn off_sends_an_active_adapter_its_root_then_nothing_and_auto_restores_it(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    vcx.activate_accessibility(window).unwrap();
    redraw(&mut vcx);
    assert!(vcx.take_accessibility_updates(window).iter().any(has_save));

    vcx.update(|_, cx| cx.set_accessibility_mode(AccessibilityMode::Off));
    vcx.run_until_parked();
    let updates = vcx.take_accessibility_updates(window);
    assert_eq!(updates.len(), 1, "one last tree: {updates:?}");
    assert_eq!(updates[0].nodes.len(), 1);
    assert_eq!(updates[0].nodes[0].1.role(), accesskit::Role::Window);

    redraw(&mut vcx);
    assert!(
        vcx.take_accessibility_updates(window).is_empty(),
        "then nothing"
    );
    assert!(vcx.update(|window, _| window.a11y_tree().is_none()));

    // Inspection still builds the tree under `Off`, and still sends nothing.
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    vcx.run_until_parked();
    assert!(tree_has_save(&mut vcx));
    assert!(vcx.take_accessibility_updates(window).is_empty());
    drop(guard);

    vcx.update(|_, cx| cx.set_accessibility_mode(AccessibilityMode::Auto));
    vcx.run_until_parked();
    assert!(
        vcx.take_accessibility_updates(window).iter().any(has_save),
        "leaving Off sends the adapter a full tree again"
    );
}

#[gpux::test]
fn set_accessibility_mode_switches_open_windows_at_runtime(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    let (mut other, other_window, _) = open(cx);
    assert!(!tree_has_save(&mut vcx));

    // Every open window follows the application's mode, without a redraw of its own.
    vcx.update(|_, cx| cx.set_accessibility_mode(AccessibilityMode::Always));
    vcx.run_until_parked();
    assert!(tree_has_save(&mut vcx));
    assert!(tree_has_save(&mut other));

    // Back to Auto, with no other source on, the windows drop their trees.
    vcx.update(|_, cx| cx.set_accessibility_mode(AccessibilityMode::Auto));
    vcx.run_until_parked();
    redraw(&mut vcx);
    redraw(&mut other);
    assert!(!tree_has_save(&mut vcx));
    assert!(!tree_has_save(&mut other));
    assert!(vcx.take_accessibility_updates(window).is_empty());
    assert!(other.take_accessibility_updates(other_window).is_empty());
}

#[gpux::test]
fn an_inaccessible_application_refuses_every_source(cx: &mut TestAppContext) {
    cx.disable_accessibility();
    let (mut vcx, window, _) = open(cx);
    assert!(
        vcx.activate_accessibility(window).is_none(),
        "no adapter is set up"
    );
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    assert!(guard.accessibility_disabled());
    vcx.update(|_, cx| cx.set_accessibility_mode(AccessibilityMode::Always));
    redraw(&mut vcx);
    assert!(vcx.update(|window, _| window.a11y_tree().is_none()));
    assert!(vcx.take_accessibility_updates(window).is_empty());
    assert!(!vcx.update(|window, _| window.is_a11y_active()));
}

#[test]
fn accessibility_mode_values_parse() {
    assert_eq!(
        AccessibilityMode::parse("auto"),
        Some(AccessibilityMode::Auto)
    );
    assert_eq!(
        AccessibilityMode::parse("Always"),
        Some(AccessibilityMode::Always)
    );
    assert_eq!(
        AccessibilityMode::parse(" OFF "),
        Some(AccessibilityMode::Off)
    );
    assert_eq!(AccessibilityMode::parse("sometimes"), None);
    assert_eq!(AccessibilityMode::default(), AccessibilityMode::Auto);
}

/// Set in the child process `the_environment_variable_is_read_when_the_app_is_created` starts.
const CHILD: &str = "GPUX_A11Y_ACTIVATION_ENV_CHILD";

/// Runs `child_reads_the_environment` in a process of its own with `GPUX_ACCESSIBILITY` set to
/// `value`, so the variable never reaches the other tests' applications.
// A test may block on its child process: the environment cannot be set for one test alone.
#[allow(clippy::disallowed_methods)]
fn run_child(value: &str, expected: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_reads_the_environment", "--nocapture"])
        .env(AccessibilityMode::ENV, value)
        .env(CHILD, expected)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "GPUX_ACCESSIBILITY={value}: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("child checked"),
        "the child ran its checks: {stdout}"
    );
}

#[test]
fn the_environment_variable_is_read_when_the_app_is_created() {
    run_child("always", "always");
    run_child("off", "off");
    run_child("auto", "auto");
    // An unknown value reads as auto.
    run_child("loud", "auto");
}

#[test]
fn child_reads_the_environment() {
    let Ok(expected) = std::env::var(CHILD) else {
        return;
    };
    let expected = AccessibilityMode::parse(&expected).unwrap();
    let cx = TestAppContext::build(TestDispatcher::new(0), None);
    assert_eq!(cx.update(|cx| cx.accessibility_mode()), expected);
    // The builder wins over the variable, whatever it holds.
    for mode in [
        AccessibilityMode::Off,
        AccessibilityMode::Always,
        AccessibilityMode::Auto,
    ] {
        let app =
            Application::on_test_platform(TestDispatcher::new(0)).with_accessibility_mode(mode);
        assert_eq!(app.accessibility_mode(), mode);
    }
    // And an application that does not set one reads it.
    assert_eq!(
        Application::on_test_platform(TestDispatcher::new(0)).accessibility_mode(),
        expected
    );
    // A later call wins over the variable.
    cx.update(|cx| cx.set_accessibility_mode(AccessibilityMode::Auto));
    assert_eq!(
        cx.update(|cx| cx.accessibility_mode()),
        AccessibilityMode::Auto
    );
    println!("child checked");
}
