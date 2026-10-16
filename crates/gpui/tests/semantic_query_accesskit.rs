//! Semantic queries over the AccessKit tree: every query term (roles from the one role table,
//! names computed as screen readers compute them, text, values, element and accessibility ids,
//! states, `nth` and `>>` scopes), the removed `type=` term, the screen-reader view, locators
//! starting inspection and acting at node bounds in window pixels, and a tree whose nesting holds
//! in release builds as in debug ones.

use gpux::{
    Context, FocusHandle, IntoElement, Render, Role, TestAppContext, TestWindowOptions, Toggled,
    VisualTestContext, Window, div,
    prelude::*,
    px,
    semantic_query::{self, Checked, Query, QueryErrorKind, QueryView},
    size,
};
use std::{cell::Cell, rc::Rc};

struct Board {
    clicks: Rc<Cell<usize>>,
    focus: FocusHandle,
    /// Adds a third button.
    extra: bool,
}

impl Render for Board {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .id("root")
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("toolbar")
                    .flex()
                    // Named by its text, as screen readers name it.
                    .child(
                        div()
                            .id("save")
                            .role(Role::Button)
                            .w(px(60.))
                            .h(px(20.))
                            .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
                            .child("Save"),
                    )
                    .child(
                        div()
                            .id("open")
                            .role(Role::Button)
                            .accessibility_id("open-button")
                            .aria_label("Open file")
                            .aria_disabled(true)
                            .w(px(60.))
                            .h(px(20.)),
                    )
                    .when(self.extra, |toolbar| {
                        toolbar.child(
                            div()
                                .id("extra")
                                .role(Role::Button)
                                .aria_label("Extra")
                                .w(px(60.))
                                .h(px(20.)),
                        )
                    }),
            )
            .child(
                div()
                    .id("options")
                    .flex()
                    .child(check("wrap", "Wrap", Toggled::True))
                    .child(check("tabs", "Tabs", Toggled::False))
                    .child(check("all", "All", Toggled::Mixed)),
            )
            .child(
                div()
                    .id("volume")
                    .role(Role::Slider)
                    .aria_label("Volume")
                    .aria_numeric_value(5.)
                    .w(px(100.))
                    .h(px(10.)),
            )
            .child(
                div()
                    .id("folder")
                    .role(Role::TreeItem)
                    .aria_label("Folder")
                    .aria_expanded(true)
                    .aria_selected(true)
                    .h(px(10.)),
            )
            .child(
                div()
                    .id("field")
                    .role(Role::TextInput)
                    .aria_label("Title")
                    .aria_value("Draft")
                    .track_focus(&self.focus)
                    .w(px(100.))
                    .h(px(20.)),
            )
            .child(
                div()
                    .id("first")
                    .child(div().id("first-child").w(px(10.)).h(px(10.)))
                    .child("first text"),
            )
            .child(div().id("second").child("second text"))
    }
}

fn check(id: &'static str, label: &'static str, toggled: Toggled) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::CheckBox)
        .aria_label(label)
        .aria_toggled(toggled)
        .w(px(20.))
        .h(px(20.))
}

fn open(cx: &mut TestAppContext, scale: f32) -> (VisualTestContext, Rc<Cell<usize>>) {
    let clicks = Rc::new(Cell::new(0));
    let (_, vcx) = cx.add_window_view_with_options(
        TestWindowOptions::new(size(px(400.), px(300.))).scale_factor(scale),
        {
            let clicks = clicks.clone();
            move |window, cx| {
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                Board {
                    clicks,
                    focus,
                    extra: false,
                }
            }
        },
    );
    let vcx = vcx.clone();
    vcx.run_until_parked();
    (vcx, clicks)
}

#[gpux::test]
fn roles_come_from_the_role_table(cx: &mut TestAppContext) {
    let (mut cx, _) = open(cx, 1.);
    assert_eq!(cx.locator("role=button").count(), 2);
    assert_eq!(cx.locator("role=checkbox").count(), 3);
    assert_eq!(cx.locator("role=slider").count(), 1);
    assert_eq!(cx.locator("role=treeitem").count(), 1);
    assert_eq!(cx.locator("role=textbox").count(), 1);
    assert_eq!(cx.locate("text=Save").unwrap().role, "text");
    assert_eq!(cx.locate("id=toolbar").unwrap().role, "generic");
    assert_eq!(gpux::query_role_name(gpux::Role::Label), "text");
    assert_eq!(
        gpux::query_role_name(gpux::Role::GenericContainer),
        "generic"
    );
    assert_eq!(gpux::query_role_name(gpux::Role::Button), "button");
    assert_eq!(gpux::aria_role(gpux::Role::CheckBox), Some("checkbox"));
    assert_eq!(gpux::aria_role(gpux::Role::Label), None);
}

#[gpux::test]
fn names_text_and_values(cx: &mut TestAppContext) {
    let (mut cx, _) = open(cx, 1.);
    // A button without a label is named by its text.
    let save = cx.locate("role=button name=Save").unwrap();
    assert!(
        save.element_id
            .as_deref()
            .unwrap()
            .ends_with(".toolbar.save")
    );
    assert_eq!(
        cx.locate("role=button name~=file").unwrap().name.as_deref(),
        Some("Open file")
    );
    assert_eq!(
        cx.locate("text~=first").unwrap().text.as_deref(),
        Some("first text")
    );
    assert_eq!(
        cx.locate("role=slider value=5").unwrap().name.as_deref(),
        Some("Volume")
    );
    assert_eq!(
        cx.locate("role=textbox value=Draft")
            .unwrap()
            .text
            .as_deref(),
        Some("Draft")
    );
}

#[gpux::test]
fn ids_match_element_ids_and_accessibility_ids(cx: &mut TestAppContext) {
    let (mut cx, _) = open(cx, 1.);
    let open = cx.locate("id=open").unwrap();
    assert_eq!(cx.locate("id=open-button").unwrap().node_id, open.node_id);
    assert!(cx.locate("id=first-child").is_ok());
    // Whole segments only.
    assert_eq!(cx.locator("id=pen").count(), 0);
}

#[gpux::test]
fn state_terms(cx: &mut TestAppContext) {
    let (mut cx, _) = open(cx, 1.);
    assert_eq!(
        cx.locate("checked=true").unwrap().name.as_deref(),
        Some("Wrap")
    );
    assert_eq!(
        cx.locate("checked=false").unwrap().name.as_deref(),
        Some("Tabs")
    );
    assert_eq!(
        cx.locate(Query::role("checkbox").checked(Checked::Mixed))
            .unwrap()
            .name
            .as_deref(),
        Some("All")
    );
    assert_eq!(
        cx.locate("expanded=true").unwrap().name.as_deref(),
        Some("Folder")
    );
    assert_eq!(
        cx.locate("role=treeitem selected=true")
            .unwrap()
            .name
            .as_deref(),
        Some("Folder")
    );
    assert_eq!(
        cx.locate("disabled=true").unwrap().name.as_deref(),
        Some("Open file")
    );
    assert_eq!(
        cx.locate("focused=true").unwrap().name.as_deref(),
        Some("Title")
    );
    assert_eq!(cx.locator("role=button disabled=false").count(), 1);
}

#[gpux::test]
fn scopes_and_nth(cx: &mut TestAppContext) {
    let (mut cx, _) = open(cx, 1.);
    assert_eq!(cx.locator("id=toolbar >> role=button").count(), 2);
    assert_eq!(
        cx.locate("id=toolbar >> role=button nth=1")
            .unwrap()
            .name
            .as_deref(),
        Some("Open file")
    );
    assert_eq!(cx.locator("id=options >> role=button").count(), 0);
    let error = cx.locate("role=button nth=4").unwrap_err();
    assert_eq!(
        error.kind,
        QueryErrorKind::NthOutOfRange { index: 4, count: 2 }
    );
}

#[gpux::test]
fn the_text_form_carries_states_and_the_view(_cx: &mut TestAppContext) {
    let query = Query::role("checkbox")
        .checked(Checked::Mixed)
        .expanded(false)
        .selected(true)
        .disabled(false)
        .focused(true)
        .for_screen_readers()
        .nth(2);
    let text = query.to_string();
    assert_eq!(
        text,
        "role=checkbox checked=mixed expanded=false selected=true disabled=false focused=true view=screen-reader nth=2"
    );
    assert_eq!(text.parse::<Query>().unwrap(), query);
    assert_eq!(
        "view=screen-reader".parse::<Query>().unwrap().view,
        QueryView::ScreenReader
    );
    for bad in ["checked=maybe", "expanded=yes", "view=dom", "checked~=true"] {
        assert!(bad.parse::<Query>().is_err(), "{bad:?} should not parse");
    }
}

#[gpux::test]
fn the_type_term_was_removed(_cx: &mut TestAppContext) {
    let error = "type=Div".parse::<Query>().unwrap_err().to_string();
    assert!(error.contains("`type=` was removed"), "{error}");
    // Inside a scope too.
    let error = "id=toolbar >> type=Div"
        .parse::<Query>()
        .unwrap_err()
        .to_string();
    assert!(error.contains("`type=` was removed"), "{error}");
}

#[gpux::test]
fn locators_read_the_tree_of_the_latest_frame(cx: &mut TestAppContext) {
    let (mut cx, _) = open(cx, 1.);
    assert_eq!(cx.locator("role=button").count(), 2);
    let board = cx.update(|window, _| window.root::<Board>().flatten().unwrap());
    cx.update(|_, cx| {
        board.update(cx, |board, cx| {
            board.extra = true;
            cx.notify();
        })
    });
    assert_eq!(cx.locator("role=button").count(), 3);
    assert!(cx.locate("role=button name=Extra").is_ok());
}

#[gpux::test]
fn the_screen_reader_view_matches_what_screen_readers_see(cx: &mut TestAppContext) {
    let (mut cx, _) = open(cx, 1.);
    assert!(cx.locator("role=generic").count() > 0);
    assert_eq!(cx.locator("role=generic view=screen-reader").count(), 0);
    assert_eq!(cx.locator("role=button view=screen-reader").count(), 2);
    // The scope follows the tree screen readers see: the window holds the buttons directly.
    assert_eq!(
        cx.locator("role=window view=screen-reader >> role=button view=screen-reader")
            .count(),
        2
    );
    assert_eq!(
        cx.locator("id=toolbar >> role=button view=screen-reader")
            .count(),
        0
    );
}

#[gpux::test]
fn locators_start_inspection_and_act_at_node_bounds(cx: &mut TestAppContext) {
    let (mut cx, clicks) = open(cx, 2.);
    assert!(
        cx.update(|window, _| window.a11y_tree().is_none()),
        "nothing asked for a tree yet"
    );
    let save = cx.locate("role=button name=Save").unwrap();
    assert!(
        cx.update(|window, _| window.a11y_tree().is_some()),
        "the locator started inspection"
    );
    // Window pixels at scale 2: the button's layout bounds, not device pixels.
    assert_eq!((save.bounds.width, save.bounds.height), (60., 20.));
    cx.locator("role=button name=Save").click().unwrap();
    assert_eq!(clicks.get(), 1);
}

#[gpux::test]
fn a_window_without_accessibility_cannot_be_queried(cx: &mut TestAppContext) {
    cx.disable_accessibility();
    let (mut cx, _) = open(cx, 1.);
    let error = cx.locate("role=button").unwrap_err();
    assert_eq!(error.kind, QueryErrorKind::NoTree);
    assert!(error.to_string().contains("accessibility is disabled"));
}

/// Every node's parent is the node of the element it was drawn inside, and focus and `>>`
/// scopes follow that nesting, in release builds as in debug ones.
#[gpux::test]
fn the_tree_nests_as_elements_do(cx: &mut TestAppContext) {
    let (mut cx, _) = open(cx, 1.);
    let snapshot = cx.settled_semantic_snapshot().unwrap();
    let node = |id: &str| Query::id(id).resolve(&snapshot).unwrap();
    let parent_id = |id: &str| {
        let parent = node(id).parent.unwrap();
        snapshot.nodes[parent].element_id.clone().unwrap()
    };
    for child in ["toolbar", "options", "first", "second"] {
        assert!(
            parent_id(child).ends_with(".root"),
            "{child} is a child of root"
        );
    }
    assert!(parent_id("first-child").ends_with(".first"));
    assert!(semantic_query::focus_is_within(&snapshot, node("root")));
    assert!(!semantic_query::focus_is_within(&snapshot, node("first")));
    assert!(
        Query::from("id=first >> id=first-child")
            .resolve(&snapshot)
            .is_ok()
    );
    assert!(
        Query::from("id=second >> id=first-child")
            .resolve(&snapshot)
            .is_err()
    );
}
