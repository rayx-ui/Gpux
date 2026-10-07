//! Semantic locators: queries over the devtools frame snapshot by role, name, text, value, id,
//! type and enclosing element, actions at the located element, and errors that list candidates.

use gpux::{
    AppContext as _, Context, DevtoolsSemantics, EditableText, EditableTextState, Empty, Entity,
    InspectableElementExt as _, IntoElement, Render, ScrollHandle, TestAppContext,
    VisualTestContext, Window, div, point,
    prelude::*,
    px,
    semantic_query::{Query, QueryErrorKind, TextMatch},
};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Log {
    clicks: Vec<String>,
    hovered: Vec<(String, bool)>,
    dropped: Vec<String>,
}

#[derive(Clone, Debug)]
struct Card(String);

struct CardPreview;

impl Render for CardPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct Form {
    log: Rc<RefCell<Log>>,
    name: Entity<EditableTextState>,
    list: ScrollHandle,
}

fn button(id: &'static str, label: &'static str, log: &Rc<RefCell<Log>>) -> impl IntoElement {
    let clicks = log.clone();
    let hovers = log.clone();
    div()
        .id(id)
        .w(px(80.))
        .h(px(24.))
        .on_click(move |_, _, _| clicks.borrow_mut().clicks.push(id.to_string()))
        .on_hover(move |hovered, _, _| hovers.borrow_mut().hovered.push((id.to_string(), *hovered)))
        .devtools_semantics(DevtoolsSemantics::new().role("button").name(label))
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.name.read(cx).text();
        let dropped = self.log.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("toolbar")
                    .flex()
                    .child(button("save", "Save", &self.log))
                    .child(button("save-as", "Save as", &self.log)),
            )
            .child(
                div()
                    .id("sidebar")
                    .flex()
                    .child(button("save", "Save", &self.log)),
            )
            .child(div().id("label").child("Hello locators"))
            .child(
                div().w(px(300.)).child(
                    EditableText::new(&self.name).devtools_semantics(
                        DevtoolsSemantics::new()
                            .role("textbox")
                            .name("Name")
                            .value(value),
                    ),
                ),
            )
            .child(
                div()
                    .id("list")
                    .w(px(100.))
                    .h(px(100.))
                    .overflow_scroll()
                    .track_scroll(&self.list)
                    .child(div().w(px(100.)).h(px(1000.))),
            )
            .child(
                div()
                    .flex()
                    .child(
                        div()
                            .id("card")
                            .w(px(40.))
                            .h(px(40.))
                            .on_drag(Card("ace".into()), |_, _, _, cx| cx.new(|_| CardPreview))
                            .devtools_semantics(
                                DevtoolsSemantics::new().role("listitem").name("Ace"),
                            ),
                    )
                    .child(
                        div()
                            .id("pile")
                            .ml(px(100.))
                            .w(px(60.))
                            .h(px(60.))
                            .on_drop(move |card: &Card, _, _| {
                                dropped.borrow_mut().dropped.push(card.0.clone())
                            })
                            .devtools_semantics(DevtoolsSemantics::new().role("list").name("Pile")),
                    ),
            )
    }
}

fn open_form(cx: &mut TestAppContext) -> (Rc<RefCell<Log>>, Entity<Form>, &mut VisualTestContext) {
    let log = Rc::new(RefCell::new(Log::default()));
    let (form, cx) = cx.add_window_view({
        let log = log.clone();
        move |_, cx| Form {
            log,
            name: cx.new(|cx| EditableTextState::with_text("Grace", cx)),
            list: ScrollHandle::new(),
        }
    });
    (log, form, cx)
}

#[gpux::test]
fn role_and_exact_name(cx: &mut TestAppContext) {
    let (_, _, cx) = open_form(cx);
    let save_as = cx
        .locate(Query::role("button").with_name("Save as"))
        .unwrap();
    assert!(save_as.global_id.unwrap().ends_with(".toolbar.save-as"));
    assert_eq!(cx.locator(Query::role("button")).count(), 3);
}

#[gpux::test]
fn name_contains_text_and_text_contains(cx: &mut TestAppContext) {
    let (_, _, cx) = open_form(cx);
    assert_eq!(
        cx.locator(Query::role("button").with_name_containing("Sav"))
            .count(),
        3
    );
    let label = cx.locate(Query::text("Hello locators")).unwrap();
    assert_eq!(label.semantics.role.as_deref(), Some("text"));
    let by_substring = cx
        .locate(Query::default().with_text_containing("locators"))
        .unwrap();
    assert_eq!(by_substring.devtools_node_id, label.devtools_node_id);
}

#[gpux::test]
fn value_queries(cx: &mut TestAppContext) {
    let (_, _, cx) = open_form(cx);
    let textbox = cx
        .locate(Query::role("textbox").with_value("Grace"))
        .unwrap();
    assert_eq!(textbox.semantics.name.as_deref(), Some("Name"));
    assert!(
        cx.locate(Query::default().with_value_containing("rac"))
            .is_ok()
    );
}

#[gpux::test]
fn id_suffixes_and_element_types(cx: &mut TestAppContext) {
    let (_, _, cx) = open_form(cx);
    let save_as = cx.locate(Query::id("save-as")).unwrap();
    assert_eq!(save_as.semantics.name.as_deref(), Some("Save as"));
    let toolbar = cx.locate(Query::id("toolbar")).unwrap();
    assert!(
        toolbar
            .type_name
            .ends_with("Stateful<gpux::elements::div::Div>")
    );
    // Suffixes match whole id segments only.
    assert_eq!(cx.locator(Query::id("ave-as")).count(), 0);
    // `type` names the element type without its module path or generic parameters.
    // toolbar, sidebar, label and list; the buttons, card and pile are wrapped by their
    // semantics and report `InspectableElement`.
    assert_eq!(cx.locator(Query::type_name("Stateful")).count(), 4);
    assert_eq!(
        cx.locator(Query::type_name("InspectableElement")).count(),
        6
    );
    assert!(cx.locator(Query::type_name("Div")).count() > 0);
}

#[gpux::test]
fn within_scopes_and_nth_picks(cx: &mut TestAppContext) {
    let (_, _, cx) = open_form(cx);
    let in_sidebar = cx
        .locate(Query::role("button").within(Query::id("sidebar")))
        .unwrap();
    assert!(in_sidebar.global_id.unwrap().ends_with(".sidebar.save"));

    let first = cx
        .locate(Query::role("button").with_name("Save").nth(0))
        .unwrap();
    let second = cx
        .locate(Query::role("button").with_name("Save").nth(1))
        .unwrap();
    assert!(first.global_id.unwrap().ends_with(".toolbar.save"));
    assert!(second.global_id.unwrap().ends_with(".sidebar.save"));

    let all = cx.locator(Query::role("button").with_name("Save")).all();
    assert_eq!(all.len(), 2);
}

#[gpux::test]
fn ambiguous_queries_fail_and_list_the_matches(cx: &mut TestAppContext) {
    let (_, _, cx) = open_form(cx);
    let error = cx
        .locate(Query::role("button").with_name("Save"))
        .unwrap_err();
    assert_eq!(error.kind, QueryErrorKind::Ambiguous { count: 2 });
    assert_eq!(error.candidates.len(), 2);
    let message = error.to_string();
    assert!(
        message.contains("2 elements match `role=button name=Save`"),
        "{message}"
    );
    assert!(
        message.contains(".toolbar.save role=button name=Save"),
        "{message}"
    );
    assert!(
        message.contains(".sidebar.save role=button name=Save"),
        "{message}"
    );

    let error = cx
        .locate(Query::role("button").with_name("Save").nth(5))
        .unwrap_err();
    assert_eq!(
        error.kind,
        QueryErrorKind::NthOutOfRange { index: 5, count: 2 }
    );
}

#[gpux::test]
fn missing_elements_fail_and_list_candidates(cx: &mut TestAppContext) {
    let (_, _, cx) = open_form(cx);
    let error = cx.locate(Query::role("checkbox")).unwrap_err();
    assert_eq!(error.kind, QueryErrorKind::NotFound);
    let message = error.to_string();
    assert!(
        message.starts_with("no element matches `role=checkbox`"),
        "{message}"
    );
    assert!(
        message.contains("role=button name=\"Save as\""),
        "{message}"
    );
    assert!(
        message.contains("role=textbox name=Name value=Grace"),
        "{message}"
    );
}

#[gpux::test]
fn text_form_round_trips_and_rejects_bad_input(cx: &mut TestAppContext) {
    let query = Query::role("button")
        .with_name("Save as")
        .with_text_containing("a \"quoted\" \\ value")
        .with_id("save-as")
        .with_type_name("Stateful")
        .within(Query::id("toolbar"))
        .nth(1);
    let text = query.to_string();
    assert_eq!(
        text,
        r#"id=toolbar >> role=button name="Save as" text~="a \"quoted\" \\ value" id=save-as type=Stateful nth=1"#
    );
    assert_eq!(text.parse::<Query>().unwrap(), query);
    assert_eq!("*".parse::<Query>().unwrap(), Query::default());
    assert_eq!(
        "name~=Sav".parse::<Query>().unwrap().name,
        Some(TextMatch::Contains("Sav".into()))
    );

    for bad in [
        "",
        "colour=red",
        "role~=button",
        "name=\"unterminated",
        "nth=first",
        "role=button >>",
        "role",
    ] {
        assert!(bad.parse::<Query>().is_err(), "{bad:?} should not parse");
    }

    // The text form drives the same resolution.
    let (_, _, cx) = open_form(cx);
    let from_text = cx.locate("id=sidebar >> role=button").unwrap();
    assert!(from_text.global_id.unwrap().ends_with(".sidebar.save"));
}

#[gpux::test]
fn click_and_hover_act_at_the_element_center(cx: &mut TestAppContext) {
    let (log, _, cx) = open_form(cx);
    cx.locator("id=sidebar >> role=button").click().unwrap();
    cx.locator(Query::role("button").with_name("Save as"))
        .click()
        .unwrap();
    assert_eq!(log.borrow().clicks, ["save", "save-as"]);

    cx.locator(Query::role("button").with_name("Save").nth(0))
        .hover()
        .unwrap();
    assert_eq!(
        log.borrow().hovered.last(),
        Some(&("save".to_string(), true))
    );
}

#[gpux::test]
fn drag_to_another_locator_or_a_point(cx: &mut TestAppContext) {
    let (log, _, cx) = open_form(cx);
    cx.locator(Query::role("listitem").with_name("Ace"))
        .drag_to(Query::role("list").with_name("Pile"))
        .unwrap();
    assert_eq!(log.borrow().dropped, ["ace"]);

    let pile = cx.locate("role=list name=Pile").unwrap();
    let inside_pile = point(px(pile.bounds.x + 5.), px(pile.bounds.y + 5.));
    cx.locator("role=listitem").drag_to(inside_pile).unwrap();
    assert_eq!(log.borrow().dropped, ["ace", "ace"]);
}

#[gpux::test]
fn scroll_by_a_delta_over_the_element(cx: &mut TestAppContext) {
    let (_, form, cx) = open_form(cx);
    cx.locator(Query::id("list"))
        .scroll(point(px(0.), px(-150.)))
        .unwrap();
    let offset = form.read_with(cx, |form, _| form.list.offset());
    assert_eq!(offset, point(px(0.), px(-150.)));
}

#[gpux::test]
fn focus_type_and_press(cx: &mut TestAppContext) {
    let (_, form, cx) = open_form(cx);
    let name = form.read_with(cx, |form, _| form.name.clone());

    let textbox = cx.locator("role=textbox name=Name").focus().unwrap();
    assert!(!textbox.focused, "the snapshot was taken before the click");
    assert!(name.read_with(cx, |state, _| state.is_focused()));

    name.update(cx, |state, cx| state.move_to_end(cx));
    cx.locator("role=textbox").type_text(" Hopper").unwrap();
    assert_eq!(name.read_with(cx, |state, _| state.text()), "Grace Hopper");
    assert!(cx.locate("role=textbox value=\"Grace Hopper\"").is_ok());

    cx.locator("role=textbox")
        .press("backspace backspace")
        .unwrap();
    assert_eq!(name.read_with(cx, |state, _| state.text()), "Grace Hopp");
}

#[test]
fn locators_read_only_public_snapshot_data() {
    // Release-mode tests must resolve queries too, so nothing may use the debug-only selectors.
    for source in [
        include_str!("../src/semantic_query.rs"),
        include_str!("../src/semantic_query/locator.rs"),
    ] {
        assert!(!source.contains("debug_selector"));
        assert!(!source.contains("debug_bounds"));
    }
}
