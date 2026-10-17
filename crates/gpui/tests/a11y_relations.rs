//! Required, read-only and the relations between elements: a relation names another element's
//! `accessibility_id` and reaches AccessKit as that element's node id, in the order given; an
//! unknown id is dropped.

use accesskit::{NodeId, TreeUpdate};
use gpux::{Context, IntoElement, Render, Role, TestAppContext, Window, div, prelude::*};

struct Form;

impl Render for Form {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                div()
                    .id("name-label")
                    .role(Role::Label)
                    .accessibility_id("name-label")
                    .aria_label("Name"),
            )
            .child(
                div()
                    .id("name-hint")
                    .role(Role::Label)
                    .accessibility_id("name-hint")
                    .aria_label("As on your passport"),
            )
            .child(
                div()
                    .id("name-extra")
                    .role(Role::Label)
                    .accessibility_id("name-extra")
                    .aria_label("Required"),
            )
            .child(
                div()
                    .id("name-error")
                    .role(Role::Label)
                    .accessibility_id("name-error")
                    .aria_label("Enter a name"),
            )
            .child(
                div()
                    .id("name")
                    .role(Role::TextInput)
                    .aria_required(true)
                    .aria_read_only(true)
                    .aria_labelled_by("name-label")
                    .aria_described_by("name-extra")
                    .aria_described_by("name-hint")
                    .aria_described_by("nowhere")
                    .aria_error_message("name-error"),
            )
            .child(
                div()
                    .id("toggle")
                    .role(Role::Button)
                    .aria_label("Details")
                    .aria_controls("details"),
            )
            .child(
                div()
                    .id("details")
                    .role(Role::Group)
                    .accessibility_id("details"),
            )
            .child(div().id("plain").role(Role::Button).aria_label("Plain"))
    }
}

fn tree(cx: &mut TestAppContext) -> TreeUpdate {
    let (_, vcx) = cx.add_window_view(|_, _| Form);
    let mut vcx = vcx.clone();
    vcx.run_until_parked();
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    vcx.run_until_parked();
    let update = vcx.update(|window, _| window.a11y_tree().cloned()).unwrap();
    drop(guard);
    update
}

fn by_author<'a>(update: &'a TreeUpdate, author: &str) -> (NodeId, &'a accesskit::Node) {
    update
        .nodes
        .iter()
        .find(|(_, node)| node.author_id() == Some(author))
        .map(|(id, node)| (*id, node))
        .unwrap_or_else(|| panic!("no node carries {author}"))
}

fn by_label<'a>(update: &'a TreeUpdate, label: &str) -> &'a accesskit::Node {
    update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .map(|(_, node)| node)
        .unwrap()
}

#[gpux::test]
fn required_and_read_only_reach_accesskit(cx: &mut TestAppContext) {
    let update = tree(cx);
    let field = field(&update);
    assert!(field.is_required());
    assert!(field.is_read_only());
    let plain = by_label(&update, "Plain");
    assert!(!plain.is_required());
    assert!(!plain.is_read_only());
}

fn field(update: &TreeUpdate) -> &accesskit::Node {
    update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == accesskit::Role::TextInput)
        .map(|(_, node)| node)
        .unwrap()
}

#[gpux::test]
fn relations_resolve_to_node_ids_in_order(cx: &mut TestAppContext) {
    let update = tree(cx);
    let field = field(&update);
    assert_eq!(field.labelled_by(), &[by_author(&update, "name-label").0]);
    // In the order given, not the order the targets were drawn in.
    assert_eq!(
        field.described_by(),
        &[
            by_author(&update, "name-extra").0,
            by_author(&update, "name-hint").0
        ]
    );
    assert_eq!(
        field.error_message(),
        Some(by_author(&update, "name-error").0)
    );
    // An element without relations has none.
    assert!(by_label(&update, "Plain").labelled_by().is_empty());
}

#[gpux::test]
fn a_relation_to_an_element_drawn_later_resolves(cx: &mut TestAppContext) {
    let update = tree(cx);
    // `details` is drawn after the button that controls it.
    assert_eq!(
        by_label(&update, "Details").controls(),
        &[by_author(&update, "details").0]
    );
}

#[gpux::test]
fn a_relation_to_an_unknown_id_is_dropped(cx: &mut TestAppContext) {
    let update = tree(cx);
    // `nowhere` names no element: the two known targets stay, nothing stands in for it.
    assert_eq!(field(&update).described_by().len(), 2);
    assert!(
        update
            .nodes
            .iter()
            .all(|(_, node)| node.author_id() != Some("nowhere"))
    );
}

#[gpux::test]
fn every_relation_names_a_node_in_the_tree(cx: &mut TestAppContext) {
    let update = tree(cx);
    let ids: Vec<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
    for (_, node) in &update.nodes {
        for target in node
            .labelled_by()
            .iter()
            .chain(node.described_by())
            .chain(node.controls())
            .chain(node.error_message().as_ref())
        {
            assert!(ids.contains(target));
        }
    }
}
