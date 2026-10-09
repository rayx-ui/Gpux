//! The devtools frame snapshot's element tree: every element's parent is the element it was
//! painted inside, in debug and release builds alike, so focus checks and `within` queries see
//! the real nesting. (Release builds once kept every finished element on the parent stack.)

use gpux::{
    Context, DevtoolsElementSnapshot, DevtoolsFrameSnapshot, FocusHandle, IntoElement, Render,
    TestAppContext, Window, div,
    prelude::*,
    px,
    semantic_query::{self, Query},
};

struct Siblings {
    first: FocusHandle,
    second: FocusHandle,
}

impl Render for Siblings {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("root")
            .size_full()
            .child(div().id("label").child("leaf text"))
            .child(
                div()
                    .id("first")
                    .track_focus(&self.first)
                    .w(px(40.))
                    .h(px(20.))
                    .child(div().id("first-child").w(px(10.)).h(px(10.))),
            )
            .child(
                div()
                    .id("second")
                    .track_focus(&self.second)
                    .w(px(40.))
                    .h(px(20.)),
            )
    }
}

fn element<'a>(snapshot: &'a DevtoolsFrameSnapshot, id: &str) -> &'a DevtoolsElementSnapshot {
    Query::id(id)
        .resolve(snapshot)
        .unwrap_or_else(|error| panic!("{error}"))
}

fn parent<'a>(snapshot: &'a DevtoolsFrameSnapshot, id: &str) -> Option<&'a str> {
    let parent = element(snapshot, id).parent_devtools_node_id?;
    snapshot
        .elements
        .iter()
        .find(|element| element.devtools_node_id == parent)
        .and_then(|element| element.global_id.as_deref())
}

#[gpux::test]
fn elements_are_children_of_the_element_they_paint_inside(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| Siblings {
        first: cx.focus_handle(),
        second: cx.focus_handle(),
    });
    cx.update(|window, cx| {
        let second = view.read(cx).second.clone();
        window.focus(&second, cx);
    });
    cx.run_until_parked();
    let snapshot = cx.update(|window, _| window.devtools_frame_snapshot());

    let root = element(&snapshot, "root").global_id.clone().unwrap();
    for sibling in ["label", "first", "second"] {
        assert_eq!(
            parent(&snapshot, sibling),
            Some(root.as_str()),
            "{sibling} is a child of root"
        );
    }
    assert!(
        parent(&snapshot, "first-child").is_some_and(|parent| parent.ends_with(".first")),
        "first-child is a child of first"
    );

    // Focus on one sibling is not within the other.
    assert!(semantic_query::focus_is_within(
        &snapshot,
        element(&snapshot, "second")
    ));
    assert!(!semantic_query::focus_is_within(
        &snapshot,
        element(&snapshot, "first")
    ));
    assert!(!semantic_query::focus_is_within(
        &snapshot,
        element(&snapshot, "label")
    ));

    // `within` scopes follow the same tree.
    assert!(
        Query::from("id=first >> id=first-child")
            .resolve(&snapshot)
            .is_ok()
    );
    assert!(
        Query::from("id=label >> id=second")
            .resolve(&snapshot)
            .is_err()
    );
    assert!(
        Query::from("id=first >> id=second")
            .resolve(&snapshot)
            .is_err()
    );
}
