//! Accessibility actions: a div advertises exactly the actions it has handlers for, custom
//! actions are listed and dispatched by id, gpux scrolls a node into view through its scroll
//! containers, and `Window::perform_a11y_action` reports whether a handler ran.

use accesskit::{Action, ActionData, ActionRequest, NodeId, TreeId, TreeUpdate};
use gpux::{
    AnyWindowHandle, Context, FocusHandle, IntoElement, Render, Role, ScrollHandle, TestAppContext,
    TestWindowOptions, VisualTestContext, Window, div, prelude::*, px, size,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Log(Rc<RefCell<Vec<String>>>);

struct Controls {
    log: Rc<RefCell<Vec<String>>>,
    outer: ScrollHandle,
    field: FocusHandle,
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let log = |entry: &'static str, log: &Rc<RefCell<Vec<String>>>| {
            let log = log.clone();
            move |_: Option<&ActionData>, _: &mut Window, _: &mut gpux::App| {
                log.borrow_mut().push(entry.to_string())
            }
        };
        let custom = |entry: &'static str, log: &Rc<RefCell<Vec<String>>>| {
            let log = log.clone();
            move |_: &mut Window, _: &mut gpux::App| log.borrow_mut().push(entry.to_string())
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("volume")
                    .role(Role::Slider)
                    .aria_label("Volume")
                    .h(px(10.))
                    .on_a11y_action(Action::SetValue, {
                        let log = self.log.clone();
                        move |data, _, _| {
                            if let Some(ActionData::NumericValue(value)) = data {
                                log.borrow_mut().push(format!("set {value}"));
                            }
                        }
                    })
                    .on_a11y_action(Action::Increment, log("increment", &self.log))
                    .on_a11y_action(Action::Decrement, log("decrement", &self.log)),
            )
            .child(
                div()
                    .id("tree-row")
                    .role(Role::TreeItem)
                    .aria_label("Folder")
                    .h(px(10.))
                    .on_a11y_action(Action::Expand, log("expand", &self.log))
                    .on_a11y_action(Action::Collapse, log("collapse", &self.log)),
            )
            .child(
                div()
                    .id("card")
                    .role(Role::Group)
                    .aria_label("Card")
                    .h(px(10.))
                    .on_a11y_custom_action(1, "Archive", custom("archive", &self.log))
                    .on_a11y_custom_action(2, "Pin", custom("pin", &self.log)),
            )
            .child(
                div()
                    .id("go")
                    .role(Role::Button)
                    .aria_label("Go")
                    .h(px(10.))
                    .on_click({
                        let log = self.log.clone();
                        move |_, _, _| log.borrow_mut().push("click".into())
                    }),
            )
            .child(
                div()
                    .id("field")
                    .role(Role::TextInput)
                    .aria_label("Field")
                    .h(px(10.))
                    .track_focus(&self.field),
            )
            .child(
                div()
                    .id("label")
                    .role(Role::Label)
                    .aria_label("Inert")
                    .h(px(10.)),
            )
            // Nested scroll containers: the outer one tracked by a handle, the inner one
            // keeping its own offset. The target sits low in both.
            .child(
                div()
                    .id("outer")
                    .h(px(100.))
                    .w(px(200.))
                    .overflow_y_scroll()
                    .track_scroll(&self.outer)
                    .child(div().h(px(300.)))
                    .child(
                        div()
                            .id("inner")
                            .h(px(80.))
                            .overflow_y_scroll()
                            .child(div().h(px(200.)))
                            .child(
                                div()
                                    .id("target")
                                    .role(Role::Button)
                                    .aria_label("Deep")
                                    .h(px(20.)),
                            ),
                    )
                    .child(div().h(px(300.))),
            )
    }
}

struct Opened {
    vcx: VisualTestContext,
    window: AnyWindowHandle,
    log: Rc<RefCell<Vec<String>>>,
    outer: ScrollHandle,
    field: FocusHandle,
    _guard: gpux::A11yInspection,
}

fn open(cx: &mut TestAppContext) -> Opened {
    let log = Log::default().0;
    let outer = ScrollHandle::new();
    let field = cx.update(|cx| cx.focus_handle());
    let (_, vcx) = cx.add_window_view_with_options(
        TestWindowOptions::new(size(px(240.), px(200.))).scale_factor(1.),
        {
            let log = log.clone();
            let outer = outer.clone();
            let field = field.clone();
            move |_, _| Controls { log, outer, field }
        },
    );
    let mut vcx = vcx.clone();
    let window = vcx.window_handle();
    vcx.run_until_parked();
    let guard = vcx.update(|window, _| window.begin_a11y_inspection());
    vcx.run_until_parked();
    Opened {
        vcx,
        window,
        log,
        outer,
        field,
        _guard: guard,
    }
}

fn tree(vcx: &mut VisualTestContext) -> TreeUpdate {
    vcx.update(|window, _| window.a11y_tree().cloned()).unwrap()
}

fn node(update: &TreeUpdate, label: &str) -> (NodeId, accesskit::Node) {
    update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .cloned()
        .unwrap_or_else(|| panic!("no node labelled {label}"))
}

fn perform(opened: &mut Opened, target: NodeId, action: Action, data: Option<ActionData>) -> bool {
    let ran = opened.vcx.update(|window, cx| {
        window.perform_a11y_action(
            ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: target,
                data,
            },
            cx,
        )
    });
    opened.vcx.run_until_parked();
    ran
}

#[gpux::test]
fn a_div_advertises_exactly_its_handled_actions(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let update = tree(&mut opened.vcx);
    let (_, volume) = node(&update, "Volume");
    for action in [Action::SetValue, Action::Increment, Action::Decrement] {
        assert!(volume.supports_action(action), "{action:?}");
    }
    for action in [Action::Expand, Action::Collapse, Action::CustomAction] {
        assert!(!volume.supports_action(action), "{action:?}");
    }
    let (_, row) = node(&update, "Folder");
    assert!(row.supports_action(Action::Expand));
    assert!(row.supports_action(Action::Collapse));
    assert!(!row.supports_action(Action::SetValue));
    let (_, inert) = node(&update, "Inert");
    for action in [
        Action::SetValue,
        Action::Increment,
        Action::Expand,
        Action::CustomAction,
    ] {
        assert!(!inert.supports_action(action));
    }
}

#[gpux::test]
fn handlers_run_through_perform_a11y_action(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let update = tree(&mut opened.vcx);
    let volume = node(&update, "Volume").0;
    let row = node(&update, "Folder").0;
    assert!(perform(
        &mut opened,
        volume,
        Action::SetValue,
        Some(ActionData::NumericValue(7.))
    ));
    assert!(perform(&mut opened, volume, Action::Increment, None));
    assert!(perform(&mut opened, volume, Action::Decrement, None));
    assert!(perform(&mut opened, row, Action::Expand, None));
    assert!(perform(&mut opened, row, Action::Collapse, None));
    assert_eq!(
        *opened.log.borrow(),
        ["set 7", "increment", "decrement", "expand", "collapse"]
    );

    // A request nothing handles does nothing and says so.
    let inert = node(&update, "Inert").0;
    assert!(!perform(&mut opened, inert, Action::Increment, None));
    assert!(!perform(&mut opened, NodeId(424242), Action::Click, None));
    assert_eq!(opened.log.borrow().len(), 5);
}

#[gpux::test]
fn custom_actions_are_listed_and_dispatched_by_id(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let update = tree(&mut opened.vcx);
    let (card, node) = node(&update, "Card");
    assert!(node.supports_action(Action::CustomAction));
    let listed: Vec<(i32, String)> = node
        .custom_actions()
        .iter()
        .map(|action| (action.id, action.description.clone()))
        .collect();
    assert_eq!(listed, [(1, "Archive".to_string()), (2, "Pin".to_string())]);

    assert!(perform(
        &mut opened,
        card,
        Action::CustomAction,
        Some(ActionData::CustomAction(2))
    ));
    assert!(perform(
        &mut opened,
        card,
        Action::CustomAction,
        Some(ActionData::CustomAction(1))
    ));
    assert!(!perform(
        &mut opened,
        card,
        Action::CustomAction,
        Some(ActionData::CustomAction(3))
    ));
    assert!(!perform(&mut opened, card, Action::CustomAction, None));
    assert_eq!(*opened.log.borrow(), ["pin", "archive"]);
}

#[gpux::test]
fn scroll_into_view_moves_every_scroll_container_around_the_node(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let update = tree(&mut opened.vcx);
    let (deep, before) = node(&update, "Deep");
    let before = before.bounds().unwrap();
    assert!(
        before.y0 > 200.,
        "the node starts below the window: {before:?}"
    );

    assert!(perform(&mut opened, deep, Action::ScrollIntoView, None));
    opened.vcx.run_until_parked();
    let update = tree(&mut opened.vcx);
    let after = node(&update, "Deep").1.bounds().unwrap();
    // Visible inside both scrollports.
    assert!(
        opened.outer.offset().y < px(0.),
        "the tracked outer container scrolled"
    );
    assert!(
        after.y0 >= 0. && after.y1 <= 200.,
        "the node is on screen: {after:?}"
    );
    let (_, outer_node) = update
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == accesskit::Role::GenericContainer
                && node
                    .bounds()
                    .is_some_and(|b| (b.y1 - b.y0 - 100.).abs() < 0.5)
        })
        .cloned()
        .expect("the outer scroll container's node");
    let port = outer_node.bounds().unwrap();
    assert!(
        after.y0 >= port.y0 - 0.5 && after.y1 <= port.y1 + 0.5,
        "{after:?} in {port:?}"
    );

    // A node outside any scroll container has nothing to scroll.
    let volume = node(&update, "Volume").0;
    assert!(!perform(&mut opened, volume, Action::ScrollIntoView, None));
    let _ = opened.window;
}

#[gpux::test]
fn click_and_focus_run_the_element_s_own_handlers(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let update = tree(&mut opened.vcx);
    let (go, go_node) = node(&update, "Go");
    let (field, field_node) = node(&update, "Field");
    assert!(go_node.supports_action(Action::Click));
    assert!(!go_node.supports_action(Action::Focus));
    assert!(field_node.supports_action(Action::Focus));
    assert!(!field_node.supports_action(Action::Click));

    assert!(perform(&mut opened, go, Action::Click, None));
    assert_eq!(*opened.log.borrow(), ["click"]);
    assert!(perform(&mut opened, field, Action::Focus, None));
    let focused = opened.vcx.update(|window, cx| window.focused(cx));
    assert_eq!(focused.as_ref(), Some(&opened.field));
}

#[gpux::test]
fn scroll_into_view_outside_scroll_containers_does_nothing(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let update = tree(&mut opened.vcx);
    let go = node(&update, "Go").0;
    assert!(!perform(&mut opened, go, Action::ScrollIntoView, None));
    assert_eq!(opened.outer.offset().y, px(0.));
}
