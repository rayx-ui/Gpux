//! Plain text in the accessibility tree: text drawn without an element id becomes a `Label`
//! leaf under the nearest accessibility node, so buttons, links and check boxes get their names
//! from their text (AccessKit's name-from-descendants rule), and leaf ids stay stable while the
//! order of the text does not change.

use accesskit::{NodeId, Role as AkRole, TreeUpdate};
use accesskit_consumer::{NodeRef, Tree};
use gpux::{
    AnyWindowHandle, Context, Entity, InteractiveText, IntoElement, Render, Role, SharedString,
    StyledText, TestAppContext, VisualTestContext, Window, div, prelude::*, px, text,
};

struct Texts {
    swapped: bool,
}

impl Render for Texts {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (first, second): (SharedString, SharedString) = if self.swapped {
            ("Beta".into(), "Alpha".into())
        } else {
            ("Alpha".into(), "Beta".into())
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            // A button named only by its child text, in each text element kind.
            .child(
                div()
                    .id("save")
                    .role(Role::Button)
                    .w(px(120.))
                    .h(px(24.))
                    .child("Save")
                    .child(StyledText::new("draft")),
            )
            .child(
                div()
                    .id("docs")
                    .role(Role::Link)
                    .child(SharedString::from("Read the docs")),
            )
            .child(
                div()
                    .id("wrap")
                    .role(Role::CheckBox)
                    .aria_toggled(accesskit::Toggled::False)
                    .child("Wrap lines"),
            )
            // An authored label wins over the text.
            .child(
                div()
                    .id("close")
                    .role(Role::Button)
                    .aria_label("Close dialog")
                    .child("×"),
            )
            // Text with no accessibility node around it sits under the window's root.
            .child(div().child(first).child(second))
            // Text with an id keeps its own node; interactive text always has one.
            .child(text!(id = "status", "Ready"))
            .child(InteractiveText::new(
                "link-text",
                StyledText::new("Click here"),
            ))
            // Opting out stays inaccessible.
            .child(gpux::Text::new_inaccessible("hidden".into()))
    }
}

fn open(cx: &mut TestAppContext) -> (VisualTestContext, AnyWindowHandle, Entity<Texts>) {
    let (view, vcx) = cx.add_window_view(|_, _| Texts { swapped: false });
    let vcx = vcx.clone();
    let window = vcx.window_handle();
    vcx.run_until_parked();
    (vcx, window, view)
}

/// The tree screen readers get, read once the platform source is on.
fn platform_tree(vcx: &mut VisualTestContext, window: AnyWindowHandle) -> TreeUpdate {
    vcx.activate_accessibility(window).unwrap();
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    vcx.update(|window, _| window.a11y_tree().cloned())
        .expect("the platform source builds the tree")
}

/// The computed names (`accesskit_consumer`'s labels) of every node with `role`, in tree order.
fn names(node: NodeRef, role: AkRole, found: &mut Vec<Option<String>>) {
    if node.role() == role {
        found.push(node.label());
    }
    for child in node.children() {
        names(child, role, found);
    }
}

fn labels(update: &TreeUpdate) -> Vec<(NodeId, String)> {
    update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == AkRole::Label)
        .filter_map(|(id, node)| node.value().map(|value| (*id, value.to_string())))
        .collect()
}

fn leaf_id(update: &TreeUpdate, text: &str) -> NodeId {
    labels(update)
        .into_iter()
        .find(|(_, value)| value == text)
        .unwrap_or_else(|| panic!("no label leaf holds {text:?}"))
        .0
}

#[gpux::test]
fn every_text_kind_becomes_a_label_leaf(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    let update = platform_tree(&mut vcx, window);
    let values: Vec<String> = labels(&update)
        .into_iter()
        .map(|(_, value)| value)
        .collect();
    for text in [
        "Save",
        "draft",
        "Read the docs",
        "Wrap lines",
        "×",
        "Alpha",
        "Beta",
        "Ready",
        "Click here",
    ] {
        assert_eq!(
            values.iter().filter(|value| *value == text).count(),
            1,
            "{text:?} appears once as a label: {values:?}"
        );
    }
    assert!(!values.iter().any(|value| value == "hidden"));
}

#[gpux::test]
fn buttons_links_and_check_boxes_are_named_by_their_text(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    let update = platform_tree(&mut vcx, window);
    let tree = Tree::new(update, true);
    let root = tree.state().root();
    let named = |role: AkRole| {
        let mut found = Vec::new();
        names(root, role, &mut found);
        found
    };
    assert_eq!(
        named(AkRole::Button),
        vec![
            Some("Save draft".to_string()),
            Some("Close dialog".to_string())
        ]
    );
    assert_eq!(named(AkRole::Link), vec![Some("Read the docs".to_string())]);
    assert_eq!(
        named(AkRole::CheckBox),
        vec![Some("Wrap lines".to_string())]
    );
}

#[gpux::test]
fn leaves_sit_under_the_nearest_node_or_the_root(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    let update = platform_tree(&mut vcx, window);
    let parent_of = |child: NodeId| {
        update
            .nodes
            .iter()
            .find(|(_, node)| node.children().contains(&child))
            .map(|(id, node)| (*id, node.role()))
            .expect("every leaf has a parent")
    };
    assert_eq!(parent_of(leaf_id(&update, "Save")).1, AkRole::Button);
    assert_eq!(
        parent_of(leaf_id(&update, "Alpha")),
        (NodeId(0), AkRole::Window)
    );
    // Leaves are never focusable and carry no actions.
    for (id, _) in labels(&update) {
        let node = &update.nodes.iter().find(|(node, _)| *node == id).unwrap().1;
        assert!(!node.supports_action(accesskit::Action::Focus));
        assert!(!node.supports_action(accesskit::Action::Click));
    }
}

#[gpux::test]
fn leaf_ids_are_stable_across_frames(cx: &mut TestAppContext) {
    let (mut vcx, window, _) = open(cx);
    let first = platform_tree(&mut vcx, window);
    for _ in 0..2 {
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
        let again = vcx.update(|window, _| window.a11y_tree().cloned()).unwrap();
        assert_eq!(
            labels(&first),
            labels(&again),
            "ids stay equal across frames"
        );
    }
}

#[gpux::test]
fn leaf_ids_follow_the_text_order(cx: &mut TestAppContext) {
    let (mut vcx, window, view) = open(cx);
    let first = platform_tree(&mut vcx, window);
    vcx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.swapped = true;
            cx.notify();
        })
    });
    vcx.run_until_parked();
    let swapped = vcx.update(|window, _| window.a11y_tree().cloned()).unwrap();
    assert_ne!(
        leaf_id(&first, "Alpha"),
        leaf_id(&swapped, "Alpha"),
        "reordering the text moves its leaf id"
    );
    assert_eq!(leaf_id(&first, "Alpha"), leaf_id(&swapped, "Beta"));
    assert_eq!(leaf_id(&first, "Save"), leaf_id(&swapped, "Save"));
}

#[gpux::test]
fn no_leaf_is_built_without_a_source(cx: &mut TestAppContext) {
    let (mut vcx, _, _) = open(cx);
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    assert!(vcx.update(|window, _| window.a11y_tree().is_none()));

    // Inspection builds the same leaves.
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    vcx.run_until_parked();
    let update = vcx.update(|window, _| window.a11y_tree().cloned()).unwrap();
    assert!(labels(&update).iter().any(|(_, value)| value == "Save"));
    drop(guard);
}
