//! Siblings with the same element id share an element path, which element state tolerates;
//! the accessibility tree gives each of them a node of its own. Both are in the tree, keep
//! their node ids across frames, an action on either runs that element's own handler, and
//! fast mode draws such a scene as it does without the tree (it builds the view holding the
//! duplicates, whose element state they share, every frame).
//!
//! `cargo test -p gpux --features test-support --test a11y_duplicate_ids`
#![cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]

use accesskit::{Action, ActionRequest, NodeId, TreeId};
use gpux::{
    AccessibilityMode, AppContext as _, Context, Entity, FastMode, IntoElement, Render, Role,
    SharedString, TestAppContext, TestDispatcher, TestWindowOptions, VisualTestContext, Window,
    WindowHandle, div, prelude::*, px, rgb, size,
};
use std::{cell::RefCell, rc::Rc};

type Log = Rc<RefCell<Vec<&'static str>>>;

/// Two buttons with the same element id.
struct Pair {
    log: Log,
}

impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let button = |label: &'static str, log: Log| {
            div()
                .id("item")
                .role(Role::Button)
                .aria_label(label)
                .h(px(20.))
                .bg(rgb(0xdddddd))
                .on_a11y_action(Action::Click, move |_, _, _| log.borrow_mut().push(label))
        };
        div()
            .id("pair")
            .flex()
            .flex_col()
            .child(button("First", self.log.clone()))
            .child(button("Second", self.log.clone()))
    }
}

struct Counter {
    value: usize,
}

impl Render for Counter {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("counter")
            .role(Role::Status)
            .aria_label(SharedString::from(format!("count {}", self.value)))
            .h(px(20.))
    }
}

struct Board {
    pair: Entity<Pair>,
    counter: Entity<Counter>,
}

impl Render for Board {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .flex()
            .flex_col()
            .child(self.pair.clone())
            .child(self.counter.clone())
    }
}

struct Opened {
    cx: TestAppContext,
    window: WindowHandle<Board>,
    log: Log,
}

fn open(fast: FastMode) -> Opened {
    open_in(fast, AccessibilityMode::Always)
}

fn open_in(fast: FastMode, mode: AccessibilityMode) -> Opened {
    let mut cx = TestAppContext::build_with_renderer(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
        gpui_platform::current_headless_renderer,
    );
    cx.set_draw_on_flush(false);
    cx.update(|cx| cx.set_accessibility_mode(mode));
    let log = Log::default();
    let window = cx.add_window_with_options(
        TestWindowOptions::new(size(px(200.), px(200.))).scale_factor(1.),
        {
            let log = log.clone();
            move |_, cx| Board {
                pair: cx.new(|_| Pair { log }),
                counter: cx.new(|_| Counter { value: 0 }),
            }
        },
    );
    cx.update_window(window.into(), |_, window, _| {
        window.set_fast_mode(fast);
        window.activate_window();
    })
    .unwrap();
    Opened { cx, window, log }
}

fn frame(opened: &mut Opened) -> gpux::FastStats {
    VisualTestContext::from_window(opened.window.into(), &opened.cx)
        .capture_screenshot()
        .expect("the window rendered");
    opened
        .cx
        .update_window(opened.window.into(), |_, window, _| {
            window.fast_frame_stats()
        })
        .unwrap()
}

/// The node id of each node labelled `First` or `Second`, in tree order.
fn buttons(opened: &mut Opened) -> Vec<(String, NodeId)> {
    opened
        .cx
        .update_window(opened.window.into(), |_, window, _| {
            window
                .a11y_tree()
                .expect("the tree is built on every frame")
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == accesskit::Role::Button)
                .map(|(id, node)| (node.label().unwrap_or_default().to_string(), *id))
                .collect()
        })
        .unwrap()
}

fn click(opened: &mut Opened, target: NodeId) -> bool {
    let ran = opened
        .cx
        .update_window(opened.window.into(), |_, window, cx| {
            window.perform_a11y_action(
                ActionRequest {
                    action: Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node: target,
                    data: None,
                },
                cx,
            )
        })
        .unwrap();
    opened.cx.run_until_parked();
    ran
}

fn id_of(buttons: &[(String, NodeId)], label: &str) -> NodeId {
    buttons
        .iter()
        .find(|(name, _)| name == label)
        .unwrap_or_else(|| panic!("no {label} button in {buttons:?}"))
        .1
}

#[test]
fn both_duplicates_have_a_node_of_their_own() {
    let mut opened = open(FastMode::OFF);
    frame(&mut opened);
    let buttons = buttons(&mut opened);
    let first = id_of(&buttons, "First");
    let second = id_of(&buttons, "Second");
    assert_ne!(first, second, "{buttons:?}");
}

#[test]
fn duplicate_nodes_keep_their_ids_across_frames() {
    let mut opened = open(FastMode::OFF);
    frame(&mut opened);
    let before = buttons(&mut opened);
    opened
        .cx
        .update_window(opened.window.into(), |_, window, _| window.refresh())
        .unwrap();
    frame(&mut opened);
    assert_eq!(buttons(&mut opened), before);
}

#[test]
fn an_action_on_a_duplicate_runs_its_own_handler() {
    let mut opened = open(FastMode::OFF);
    frame(&mut opened);
    let buttons = buttons(&mut opened);
    assert!(click(&mut opened, id_of(&buttons, "Second")));
    assert!(click(&mut opened, id_of(&buttons, "First")));
    assert_eq!(*opened.log.borrow(), ["Second", "First"]);
}

/// The views built by a frame where only the counter changes, after three settling frames.
fn built_when_the_counter_changes(opened: &mut Opened) -> u64 {
    for _ in 0..3 {
        frame(opened);
    }
    let counter = opened
        .window
        .read_with(&opened.cx, |board, _| board.counter.clone())
        .unwrap();
    counter.update(&mut opened.cx, |counter, cx| {
        counter.value += 1;
        cx.notify();
    });
    frame(opened).views_built
}

#[test]
fn fast_mode_draws_duplicates_as_it_does_without_the_tree() {
    let without =
        built_when_the_counter_changes(&mut open_in(FastMode::ALL, AccessibilityMode::Off));
    let mut opened = open(FastMode::ALL);
    frame(&mut opened);
    let before = buttons(&mut opened);
    assert_eq!(built_when_the_counter_changes(&mut opened), without);
    assert_eq!(buttons(&mut opened), before);
    assert!(click(&mut opened, id_of(&before, "Second")));
    assert_eq!(*opened.log.borrow(), ["Second"]);
}
