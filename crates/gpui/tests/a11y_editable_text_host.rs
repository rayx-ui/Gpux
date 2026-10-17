//! A control built around an `EditableText`: the host tracks the field's focus handle and has no
//! role of its own, and the field carries what the host says of it (name, description,
//! required, invalid, relations, SetValue). The control is exactly one named text input, the
//! node that holds the focus, and an assistive technology's SetValue reaches the field or the
//! host's handler.
//!
//! `cargo test -p gpux --test a11y_editable_text_host`

use accesskit::{Action, ActionData, ActionRequest, Role as AkRole, TreeId};
use gpux::{
    AppContext as _, Context, EditableText, EditableTextState, Entity, IntoElement, Render, Role,
    TestAppContext, Window, WindowHandle, div,
    prelude::*,
    px,
    semantic_query::{Query, SemanticSnapshot},
};
use std::{cell::RefCell, rc::Rc};

struct Control {
    field: Entity<EditableTextState>,
    /// Values the host's SetValue handler received, when it has one.
    set_values: Option<Rc<RefCell<Vec<String>>>>,
}

impl Render for Control {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.field.read(cx).focus_handle();
        let mut editable = EditableText::new(&self.field)
            .aria_label("Name")
            .aria_description("Your full name")
            .aria_required(true)
            .aria_invalid(true)
            .accessibility_id("name-field")
            .aria_labelled_by("name-label")
            .aria_error_message("name-error");
        if let Some(values) = self.set_values.clone() {
            editable = editable.on_set_value(move |value, _, _| {
                values.borrow_mut().push(value.to_string());
            });
        }
        div()
            .id("root")
            .size_full()
            .child(
                div()
                    .id("name-label")
                    .accessibility_id("name-label")
                    .role(Role::Label)
                    .child("Name"),
            )
            .child(
                // The host: it tracks the field's focus and adds no node.
                div()
                    .id("host")
                    .track_focus(&focus)
                    .w(px(200.))
                    .h(px(24.))
                    .child(editable),
            )
            .child(
                div()
                    .id("name-error")
                    .accessibility_id("name-error")
                    .role(Role::Alert)
                    .child("Required"),
            )
    }
}

fn open(
    cx: &mut TestAppContext,
    set_values: Option<Rc<RefCell<Vec<String>>>>,
) -> (WindowHandle<Control>, gpux::A11yInspection) {
    let window = cx.add_window(|_, cx| Control {
        field: cx.new(|cx| EditableTextState::new(cx)),
        set_values,
    });
    let guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    let focus = window
        .update(cx, |control, _, cx| control.field.read(cx).focus_handle())
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        focus.focus(window, cx);
        window.refresh();
    })
    .unwrap();
    cx.run_until_parked();
    (window, guard)
}

fn snapshot(cx: &mut TestAppContext, window: WindowHandle<Control>) -> SemanticSnapshot {
    cx.update_window(window.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        SemanticSnapshot::of(window).expect("an inspected tree")
    })
    .unwrap()
}

fn text_inputs(snapshot: &SemanticSnapshot) -> Vec<&gpux::semantic_query::SemanticNode> {
    snapshot
        .nodes
        .iter()
        .filter(|node| node.ak_role == AkRole::TextInput)
        .collect()
}

fn set_value(cx: &mut TestAppContext, window: WindowHandle<Control>, value: &str) -> bool {
    let snapshot = snapshot(cx, window);
    let target = text_inputs(&snapshot)[0].node_id;
    let ran = cx
        .update_window(window.into(), |_, window, cx| {
            window.perform_a11y_action(
                ActionRequest {
                    action: Action::SetValue,
                    target_tree: TreeId::ROOT,
                    target_node: target,
                    data: Some(ActionData::Value(value.into())),
                },
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();
    ran
}

#[gpux::test]
fn the_control_is_one_named_text_input_that_holds_the_focus(cx: &mut TestAppContext) {
    let (window, _guard) = open(cx, None);
    let snapshot = snapshot(cx, window);
    let inputs = text_inputs(&snapshot);
    assert_eq!(inputs.len(), 1, "one text input: {inputs:?}");
    let field = inputs[0];
    assert_eq!(field.name.as_deref(), Some("Name"));
    let focused_node = snapshot.focus.map(|index| &snapshot.nodes[index]);
    assert!(
        field.focused,
        "the field holds the focus; focused: {:?}",
        focused_node.map(|node| (node.ak_role, node.name.clone(), node.element_id.clone()))
    );
    assert!(
        field.actions.contains(&Action::SetValue),
        "{:?}",
        field.actions
    );
    let focused = Query::role("textbox").focused(true);
    assert_eq!(focused.matches(&snapshot).len(), 1);
}

#[gpux::test]
fn the_field_carries_the_host_s_states_and_relations(cx: &mut TestAppContext) {
    let (window, _guard) = open(cx, None);
    let snapshot = snapshot(cx, window);
    let field = text_inputs(&snapshot)[0];
    assert!(field.required);
    assert_eq!(field.description.as_deref(), Some("Your full name"));
    assert_eq!(field.accessibility_id.as_deref(), Some("name-field"));
    let tree = cx
        .update_window(window.into(), |_, window, _| window.a11y_tree().cloned())
        .unwrap()
        .expect("the tree");
    let node = &tree
        .nodes
        .iter()
        .find(|(id, _)| *id == field.node_id)
        .expect("the field's node")
        .1;
    assert_eq!(node.invalid(), Some(accesskit::Invalid::True));
    let label = snapshot
        .nodes
        .iter()
        .find(|node| node.accessibility_id.as_deref() == Some("name-label"))
        .expect("the label");
    let error = snapshot
        .nodes
        .iter()
        .find(|node| node.accessibility_id.as_deref() == Some("name-error"))
        .expect("the error");
    assert_eq!(node.labelled_by(), [label.node_id]);
    assert_eq!(node.error_message(), Some(error.node_id));
}

#[gpux::test]
fn set_value_replaces_the_field_s_text(cx: &mut TestAppContext) {
    let (window, _guard) = open(cx, None);
    window
        .update(cx, |control, _, cx| {
            control.field.update(cx, |state, cx| {
                state.replace_selection("old text", cx);
            })
        })
        .unwrap();
    assert!(set_value(cx, window, "Ada Lovelace"));
    let text = window
        .update(cx, |control, _, cx| control.field.read(cx).text())
        .unwrap();
    assert_eq!(text, "Ada Lovelace");
}

#[gpux::test]
fn the_host_s_handler_takes_set_value_when_it_has_one(cx: &mut TestAppContext) {
    let values = Rc::new(RefCell::new(Vec::new()));
    let (window, _guard) = open(cx, Some(values.clone()));
    assert!(set_value(cx, window, "Grace"));
    assert_eq!(*values.borrow(), ["Grace"]);
    let text = window
        .update(cx, |control, _, cx| control.field.read(cx).text())
        .unwrap();
    assert!(
        text.is_empty(),
        "the handler, not the field, decides: {text:?}"
    );
}
