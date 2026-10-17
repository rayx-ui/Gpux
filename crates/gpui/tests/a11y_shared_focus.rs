//! One focus handle, tracked by nested elements that both have a node (a control's host and the
//! text field inside it): the focused handle's innermost node holds the tree's focus, and
//! building the tree does not panic as two claims of the window's focus would.
//!
//! `cargo test -p gpux --test a11y_shared_focus`

use gpux::{
    AppContext as _, Context, FocusHandle, IntoElement, Render, Role, TestAppContext, Window, div,
    prelude::*, px, semantic_query::SemanticSnapshot,
};

struct Field {
    focus: FocusHandle,
    /// Whether the inner element tracks the same handle as its host.
    inner_tracks: bool,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let inner = div()
            .id("input")
            .role(Role::TextInput)
            .aria_label("Name")
            .w(px(100.))
            .h(px(20.));
        div().id("root").size_full().child(
            div()
                .id("host")
                .role(Role::Group)
                .aria_label("Name field")
                .track_focus(&self.focus)
                .child(if self.inner_tracks {
                    inner.track_focus(&self.focus)
                } else {
                    inner
                }),
        )
    }
}

fn focused_name(cx: &mut TestAppContext, inner_tracks: bool) -> Option<String> {
    let window = cx.add_window(|_, cx| Field {
        focus: cx.focus_handle(),
        inner_tracks,
    });
    let _guard = window
        .update(cx, |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    let focus = window
        .update(cx, |field, _, _| field.focus.clone())
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        focus.focus(window, cx);
        window.refresh();
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let snapshot = SemanticSnapshot::of(window).expect("an inspected tree");
        snapshot
            .focus
            .and_then(|index| snapshot.nodes[index].name.clone())
    })
    .unwrap()
}

#[gpux::test]
fn the_innermost_node_of_a_shared_focus_handle_holds_the_focus(cx: &mut TestAppContext) {
    assert_eq!(focused_name(cx, true).as_deref(), Some("Name"));
}

#[gpux::test]
fn a_handle_tracked_once_focuses_its_own_node(cx: &mut TestAppContext) {
    assert_eq!(focused_name(cx, false).as_deref(), Some("Name field"));
}
