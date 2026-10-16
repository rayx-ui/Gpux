//! The AccessKit 0.25 behaviours gpux relies on: fast mode copies nodes, the tree builder clears
//! and sets properties, and the fast-mode oracle compares trees by node equality. The last test
//! checks that every tree update names gpux as its toolkit (`TreeInfo`).

use accesskit::{Action, Invalid, Node, NodeId, Orientation, Rect, Role, Toggled};
use gpux::{
    AnyWindowHandle, Context, IntoElement, Render, TestAppContext, Window, div, prelude::*, px,
};

/// A node carrying every property gpux's tree builder writes (`Interactivity::write_a11y_info`,
/// the text elements, the window's root), and the relations it resolves at the end of a frame.
fn node_with_every_property() -> Node {
    let mut node = Node::new(Role::Button);
    node.set_label("Save");
    node.set_description("Saves the document");
    node.set_value("draft");
    node.set_author_id("save");
    node.set_keyboard_shortcut("Cmd+S");
    node.set_invalid(Invalid::True);
    node.set_placeholder("Name");
    node.set_orientation(Orientation::Horizontal);
    node.set_level(2);
    node.set_row_index(3);
    node.set_column_index(4);
    node.set_row_count(10);
    node.set_column_count(5);
    node.set_bounds(Rect::new(1., 2., 30., 40.));
    node.set_children(vec![NodeId(7), NodeId(8)]);
    node.set_toggled(Toggled::True);
    node.set_disabled();
    node.set_expanded(false);
    node.set_selected(true);
    node.set_numeric_value(3.);
    node.set_min_numeric_value(0.);
    node.set_max_numeric_value(10.);
    node.set_numeric_value_step(1.);
    node.set_size_of_set(4);
    node.set_position_in_set(2);
    node.set_active_descendant(NodeId(8));
    // The relations the builder resolves from `accessibility_id`s (`aria_labelled_by` and the
    // like, s03).
    node.set_labelled_by(vec![NodeId(9)]);
    node.add_action(Action::Click);
    node.add_action(Action::Focus);
    node
}

#[test]
fn a_cloned_node_keeps_every_property_gpux_writes() {
    let node = node_with_every_property();
    let clone = node.clone();
    assert_eq!(clone, node);
    assert_eq!(clone.role(), Role::Button);
    assert_eq!(clone.label(), Some("Save"));
    assert_eq!(clone.description(), Some("Saves the document"));
    assert_eq!(clone.value(), Some("draft"));
    assert_eq!(clone.author_id(), Some("save"));
    assert_eq!(clone.keyboard_shortcut(), Some("Cmd+S"));
    assert_eq!(clone.invalid(), Some(Invalid::True));
    assert_eq!(clone.placeholder(), Some("Name"));
    assert_eq!(clone.orientation(), Some(Orientation::Horizontal));
    assert_eq!(clone.level(), Some(2));
    assert_eq!(clone.row_index(), Some(3));
    assert_eq!(clone.column_index(), Some(4));
    assert_eq!(clone.row_count(), Some(10));
    assert_eq!(clone.column_count(), Some(5));
    assert_eq!(clone.bounds(), Some(Rect::new(1., 2., 30., 40.)));
    assert_eq!(clone.children(), &[NodeId(7), NodeId(8)]);
    assert_eq!(clone.toggled(), Some(Toggled::True));
    assert!(clone.is_disabled());
    assert_eq!(clone.is_expanded(), Some(false));
    assert_eq!(clone.is_selected(), Some(true));
    assert_eq!(clone.numeric_value(), Some(3.));
    assert_eq!(clone.min_numeric_value(), Some(0.));
    assert_eq!(clone.max_numeric_value(), Some(10.));
    assert_eq!(clone.numeric_value_step(), Some(1.));
    assert_eq!(clone.size_of_set(), Some(4));
    assert_eq!(clone.position_in_set(), Some(2));
    assert_eq!(clone.active_descendant(), Some(NodeId(8)));
    assert_eq!(clone.labelled_by(), &[NodeId(9)]);
    assert!(clone.supports_action(Action::Click));
    assert!(clone.supports_action(Action::Focus));
    assert!(!clone.supports_action(Action::Expand));

    // A clone shares nothing observable with its original: changing one leaves the other.
    let mut changed = node.clone();
    changed.set_label("Saved");
    changed.push_child(NodeId(10));
    assert_eq!(node.label(), Some("Save"));
    assert_eq!(node.children(), &[NodeId(7), NodeId(8)]);
}

#[test]
fn a_property_cleared_and_set_again_holds_its_new_value() {
    let mut node = node_with_every_property();
    node.clear_label();
    node.clear_children();
    node.clear_labelled_by();
    assert_eq!(node.label(), None);
    assert!(node.children().is_empty());

    node.set_label("Open");
    node.push_child(NodeId(11));
    node.push_labelled_by(NodeId(12));
    assert_eq!(node.label(), Some("Open"));
    assert_eq!(node.children(), &[NodeId(11)]);
    assert_eq!(node.labelled_by(), &[NodeId(12)]);

    // The flags gpux clears and sets again (`aria_disabled`, `aria_invalid`), and the bounds
    // fast mode moves when it carries a node.
    node.clear_disabled();
    node.clear_invalid();
    assert!(!node.is_disabled());
    assert_eq!(node.invalid(), None);
    node.set_disabled();
    node.set_invalid(Invalid::Spelling);
    node.set_bounds(Rect::new(5., 6., 7., 8.));
    assert!(node.is_disabled());
    assert_eq!(node.invalid(), Some(Invalid::Spelling));
    assert_eq!(node.bounds(), Some(Rect::new(5., 6., 7., 8.)));
}

#[test]
fn node_equality_does_not_depend_on_the_order_properties_were_set() {
    let mut first = Node::new(Role::CheckBox);
    first.set_label("Wrap lines");
    first.set_toggled(Toggled::False);
    first.set_bounds(Rect::new(0., 0., 10., 10.));
    first.add_action(Action::Click);

    let mut second = Node::new(Role::CheckBox);
    second.add_action(Action::Click);
    second.set_bounds(Rect::new(0., 0., 10., 10.));
    second.set_toggled(Toggled::False);
    second.set_label("Wrap lines");
    assert_eq!(first, second);

    // And a real difference still makes them unequal.
    second.set_toggled(Toggled::True);
    assert_ne!(first, second);
}

struct Labelled;

impl Render for Labelled {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("save")
                .role(gpux::Role::Button)
                .aria_label("Save")
                .w(px(80.))
                .h(px(24.)),
        )
    }
}

#[gpux::test]
fn tree_updates_name_gpux_as_their_toolkit(cx: &mut TestAppContext) {
    let (_, vcx) = cx.add_window_view(|_, _| Labelled);
    let mut vcx = vcx.clone();
    let window: AnyWindowHandle = vcx.window_handle();
    vcx.run_until_parked();

    let initial = vcx
        .activate_accessibility(window)
        .expect("the test window has an accessibility adapter");
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    let updates = vcx.take_accessibility_updates(window);
    assert!(!updates.is_empty(), "an activated window sends its tree");

    for update in std::iter::once(&initial).chain(updates.iter()) {
        let tree = update
            .tree
            .as_ref()
            .expect("every gpux update carries its tree info");
        assert_eq!(tree.root, NodeId(0));
        assert_eq!(tree.toolkit_name.as_deref(), Some("gpux"));
        assert_eq!(
            tree.toolkit_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
    }
}
