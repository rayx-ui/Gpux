//! The inspection tree: while inspection is on, elements with an id and no role become generic
//! containers so tests and agents reach their subtrees, and the window maps every node to the
//! global element id it belongs to. Screen readers see the same tree with or without them.

use accesskit::{NodeId, Role as AkRole, TreeUpdate};
use accesskit_consumer::{NodeRef, Tree, common_filter};
use gpux::{
    AnyWindowHandle, Context, ElementId, FocusHandle, IntoElement, Render, Role, TestAppContext,
    VisualTestContext, Window, div, prelude::*, px,
};

struct Panel {
    focus: FocusHandle,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id("root").size_full().child(
            div()
                .id("toolbar")
                .flex()
                .child(
                    div()
                        .id("save")
                        .role(Role::Button)
                        .w(px(60.))
                        .h(px(20.))
                        .child("Save"),
                )
                .child(
                    // Focused, with an id and no role: it must not take the screen reader's
                    // focus away from where it is without inspection.
                    div()
                        .id("canvas")
                        .track_focus(&self.focus)
                        .w(px(40.))
                        .h(px(20.))
                        .child("Canvas"),
                )
                // Text after a container, numbered under the same node as the text before.
                .child("After"),
        )
    }
}

fn open(cx: &mut TestAppContext) -> (VisualTestContext, AnyWindowHandle) {
    let (_, vcx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Panel { focus }
    });
    let vcx = vcx.clone();
    let window = vcx.window_handle();
    vcx.run_until_parked();
    (vcx, window)
}

fn tree(vcx: &mut VisualTestContext) -> TreeUpdate {
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    vcx.update(|window, _| window.a11y_tree().cloned())
        .expect("a source is on")
}

fn containers(update: &TreeUpdate) -> Vec<NodeId> {
    update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == AkRole::GenericContainer)
        .map(|(id, _)| *id)
        .collect()
}

/// The tree as a screen reader walks it: AccessKit's common filter, with each node's id (which
/// adapters identify the objects they expose by), role, computed name and value, and the focus.
fn screen_reader_view(update: TreeUpdate) -> (String, Option<String>) {
    fn walk(node: NodeRef, depth: usize, out: &mut String) {
        out.push_str(&format!(
            "{}{:?} {:?} {:?} {:?}\n",
            "  ".repeat(depth),
            node.locate().0,
            node.role(),
            node.label(),
            node.value()
        ));
        for child in node.filtered_children(&common_filter) {
            walk(child, depth + 1, out);
        }
    }
    let tree = Tree::new(update, true);
    let state = tree.state();
    let mut out = String::new();
    walk(state.root(), 0, &mut out);
    let focus = state
        .focus()
        .map(|node| format!("{:?} {:?} {:?}", node.locate().0, node.role(), node.label()));
    (out, focus)
}

#[gpux::test]
fn containers_exist_only_while_inspecting(cx: &mut TestAppContext) {
    let (mut vcx, window) = open(cx);
    vcx.activate_accessibility(window).unwrap();
    let platform_only = tree(&mut vcx);
    assert!(
        containers(&platform_only).is_empty(),
        "the platform source adds none"
    );

    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    let inspected = tree(&mut vcx);
    // The window's root view element, `root`, `toolbar` and `canvas` have ids and no roles.
    assert_eq!(containers(&inspected).len(), 4);
    // What the adapter received is the inspected frame; it still adds nothing a screen
    // reader sees.
    let sent = vcx.take_accessibility_updates(window);
    assert!(!sent.is_empty());

    drop(guard);
    let after = tree(&mut vcx);
    assert!(containers(&after).is_empty());
}

#[gpux::test]
fn screen_readers_see_the_same_tree_with_and_without_containers(cx: &mut TestAppContext) {
    let (mut vcx, window) = open(cx);
    vcx.activate_accessibility(window).unwrap();
    let without = screen_reader_view(tree(&mut vcx));
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    let with = screen_reader_view(tree(&mut vcx));
    assert_eq!(with, without, "the same nodes, ids and focus");
    assert!(with.0.contains("Button Some(\"Save\")"), "{}", with.0);
    assert!(with.0.contains("Some(\"After\")"), "{}", with.0);
    drop(guard);
}

#[gpux::test]
fn the_id_map_names_each_node_s_element(cx: &mut TestAppContext) {
    let (mut vcx, _) = open(cx);
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    let update = tree(&mut vcx);
    let mut path = |node: NodeId| {
        vcx.update(|window, _| {
            window.a11y_element_id(node).map(|id| {
                id.iter()
                    .filter_map(|id| match id {
                        ElementId::Name(name) => Some(name.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
        })
    };
    let node = |role: AkRole, label: Option<&str>, value: Option<&str>| {
        update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == role && node.label() == label && node.value() == value)
            .map(|(id, _)| *id)
            .unwrap()
    };
    let save = node(AkRole::Button, None, None);
    assert_eq!(
        path(save).unwrap(),
        ["root", "toolbar", "save"],
        "an element's node maps to its global id"
    );
    let save_text = node(AkRole::Label, None, Some("Save"));
    assert_eq!(path(save_text), path(save), "a text leaf maps to its owner");
    let canvas_text = node(AkRole::Label, None, Some("Canvas"));
    assert_eq!(path(canvas_text).unwrap(), ["root", "toolbar", "canvas"]);
    assert_eq!(path(NodeId(0)), None, "the root has no element");
    let mapped = vcx.update(|window, _| window.a11y_element_ids().count());
    assert_eq!(
        mapped,
        update.nodes.len() - 1,
        "every node but the root is mapped"
    );
    drop(guard);
}

#[gpux::test]
fn nothing_is_mapped_without_inspection(cx: &mut TestAppContext) {
    let (mut vcx, window) = open(cx);
    vcx.activate_accessibility(window).unwrap();
    let update = tree(&mut vcx);
    vcx.update(|window, _| {
        assert_eq!(window.a11y_element_ids().count(), 0);
        for (id, _) in &update.nodes {
            assert!(window.a11y_element_id(*id).is_none());
        }
    });
}

#[gpux::test]
fn the_id_map_is_dropped_when_inspection_ends(cx: &mut TestAppContext) {
    let (mut vcx, window) = open(cx);
    vcx.activate_accessibility(window).unwrap();
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    tree(&mut vcx);
    assert!(vcx.update(|window, _| window.a11y_element_ids().count()) > 0);
    drop(guard);
    // The platform source keeps the tree; the map goes with the last guard.
    tree(&mut vcx);
    assert_eq!(vcx.update(|window, _| window.a11y_element_ids().count()), 0);
}

#[gpux::test]
fn a_container_never_takes_the_focus(cx: &mut TestAppContext) {
    let (mut vcx, _) = open(cx);
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    let update = tree(&mut vcx);
    let focused = update
        .nodes
        .iter()
        .find(|(id, _)| *id == update.focus)
        .map(|(_, node)| node.role())
        .unwrap();
    // `canvas` holds the keyboard focus, but its container is not what screen readers focus.
    assert_ne!(focused, AkRole::GenericContainer);
    drop(guard);
}
