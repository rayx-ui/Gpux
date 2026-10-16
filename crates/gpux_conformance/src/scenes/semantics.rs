//! Semantics: the accessibility tree, as the platform adapter receives it and as queries read it.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{AnyElement, IntoElement, Role, Toggled, div, px, rgb, semantic_query::Query};

fn form() -> AnyElement {
    let button = |name: &'static str, label: &'static str| {
        div()
            .id(name)
            .role(Role::Button)
            .aria_label(label)
            .w(px(80.))
            .h(px(28.))
            .bg(rgb(GRAY_HEX))
            .child(label)
    };
    root()
        .flex()
        .gap(px(10.))
        .p(px(10.))
        .child(button("submit", "Submit"))
        .child(button("cancel", "Cancel"))
        .child(
            div()
                .id("agree")
                .w(px(20.))
                .h(px(20.))
                .bg(rgb(GREEN_HEX))
                .role(Role::CheckBox)
                .aria_label("Agree")
                .aria_toggled(Toggled::True),
        )
        .into_any_element()
}

pub const ACCESSIBILITY: SceneSpec = SceneSpec {
    id: "semantics-accessibility",
    areas: &[Area::AccessibilityTree, Area::Accessibility],
    size: (260., 50.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F],
    golden: false,
    build: |_, cx| static_view(cx, form),
    script: || {
        vec![
            Step::Expect(Expect::Accessible("Submit".into())),
            Step::Expect(Expect::Accessible("Cancel".into())),
            Step::Expect(Expect::Count(Query::role("button"), 2)),
            Step::Expect(Expect::Exists(Query::role("button").with_name("Submit"))),
            Step::Expect(Expect::Exists(Query::role("checkbox").with_name("Agree"))),
            Step::Expect(Expect::Bounds(
                Query::role("button").with_name("Cancel"),
                (100., 10., 80., 28.),
                0.01,
            )),
            // The buttons' corners away from their labels, and the checked checkbox.
            Step::probe(85., 35., GRAY),
            Step::probe(175., 35., GRAY),
            Step::probe(200., 20., GREEN),
        ]
    },
};

/// The controls of the `accessibility` scene: what a screen reader announces and operates on
/// every platform, and the fixture of the screen-reader lanes and the owner's walkthrough.
struct Controls {
    clicks: usize,
    subscribed: bool,
    volume: f64,
    title: gpux::Entity<gpux::EditableTextState>,
    /// The check box and the slider take keyboard focus, so screen readers and keys reach them.
    subscribe_focus: gpux::FocusHandle,
    volume_focus: gpux::FocusHandle,
    /// Whether the Details disclosure is open.
    expanded: bool,
    details_focus: gpux::FocusHandle,
    /// The Copies spin button's value.
    copies: f64,
    copies_focus: gpux::FocusHandle,
    /// Whether the links are grouped under "Pinned" (a node), or in an unnamed box (none).
    pinned: bool,
    /// The shelf's items: a scroll container shows the first two.
    shelf_focus: Vec<gpux::FocusHandle>,
}

impl gpux::Render for Controls {
    fn render(&mut self, _: &mut gpux::Window, cx: &mut gpux::Context<Self>) -> impl IntoElement {
        let subscribed = self.subscribed;
        let volume = self.volume;
        // Accessibility actions on the slider move its value as its keys would.
        let entity = cx.entity().downgrade();
        let set_volume = move |change: fn(f64, Option<&gpux::accesskit::ActionData>) -> f64| {
            let entity = entity.clone();
            move |data: Option<&gpux::accesskit::ActionData>,
                  _: &mut gpux::Window,
                  cx: &mut gpux::App| {
                entity
                    .update(cx, |this: &mut Self, cx| {
                        this.volume = change(this.volume, data).clamp(0., 10.);
                        cx.notify();
                    })
                    .ok();
            }
        };
        root()
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(status(format!(
                "clicks {} subscribed {} volume {}",
                self.clicks, subscribed, volume
            )))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    // Named by its text.
                    .child(
                        div()
                            .id("activate")
                            .role(Role::Button)
                            .w(px(90.))
                            .h(px(28.))
                            .bg(rgb(GRAY_HEX))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clicks += 1;
                                cx.notify();
                            }))
                            .child("Activate"),
                    )
                    .child(
                        div()
                            .id("subscribe")
                            .role(Role::CheckBox)
                            .aria_label("Subscribe")
                            .aria_toggled(if subscribed {
                                Toggled::True
                            } else {
                                Toggled::False
                            })
                            .w(px(28.))
                            .h(px(28.))
                            .bg(rgb(if subscribed { GREEN_HEX } else { GRAY_HEX }))
                            .track_focus(&self.subscribe_focus)
                            // A click, or Space while it has focus, toggles it.
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.subscribed = !this.subscribed;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("volume")
                            .role(Role::Slider)
                            .aria_label("Volume")
                            .aria_numeric_value(volume)
                            .aria_min_numeric_value(0.)
                            .aria_max_numeric_value(10.)
                            .aria_numeric_value_step(1.)
                            .track_focus(&self.volume_focus)
                            .w(px(120.))
                            .h(px(28.))
                            .bg(rgb(GRAY_HEX))
                            .on_a11y_action(
                                gpux::AccessibleAction::Increment,
                                set_volume(|volume, _| volume + 1.),
                            )
                            .on_a11y_action(
                                gpux::AccessibleAction::Decrement,
                                set_volume(|volume, _| volume - 1.),
                            )
                            .on_a11y_action(
                                gpux::AccessibleAction::SetValue,
                                set_volume(|volume, data| match data {
                                    Some(gpux::accesskit::ActionData::NumericValue(value)) => {
                                        *value
                                    }
                                    _ => volume,
                                }),
                            ),
                    ),
            )
            // A text field to type into.
            .child(
                div()
                    .w(px(240.))
                    .child(gpux::EditableText::new(&self.title).aria_label("Title")),
            )
            // A field labelled and described by other elements.
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("email-label")
                            .role(Role::Label)
                            .accessibility_id("email-label")
                            .aria_value("Email")
                            .child("Email"),
                    )
                    .child(
                        div()
                            .id("email")
                            .role(Role::TextInput)
                            .aria_labelled_by("email-label")
                            .aria_described_by("email-hint")
                            .aria_required(true)
                            .aria_value("ada@example.com")
                            .w(px(160.))
                            .h(px(20.))
                            .bg(rgb(GRAY_HEX)),
                    )
                    .child(
                        div()
                            .id("email-hint")
                            .role(Role::Label)
                            .accessibility_id("email-hint")
                            .aria_value("We never share it")
                            .child("We never share it"),
                    ),
            )
            .child(
                div()
                    .id("fruits")
                    .role(Role::List)
                    .aria_label("Fruits")
                    .flex()
                    .gap(px(8.))
                    .children(["Apple", "Pear", "Plum"].into_iter().enumerate().map(
                        |(index, fruit)| {
                            div()
                                .id(fruit)
                                .role(Role::ListItem)
                                .aria_position_in_set(index + 1)
                                .aria_size_of_set(3)
                                .aria_selected(fruit == "Pear")
                                .child(fruit)
                        },
                    )),
            )
            .child(self.more(cx))
    }
}

impl Controls {
    /// The scene's second half: a disclosure and what it controls, a spin button, a field
    /// marked invalid, a disabled button, a table, a group that comes and goes, and a scroll
    /// container holding items out of view.
    fn more(&self, cx: &mut gpux::Context<Self>) -> impl IntoElement {
        let expanded = self.expanded;
        let entity = cx.entity().downgrade();
        let set = move |change: fn(&mut Self)| {
            let entity = entity.clone();
            move |_: Option<&gpux::accesskit::ActionData>,
                  _: &mut gpux::Window,
                  cx: &mut gpux::App| {
                entity
                    .update(cx, |this: &mut Self, cx| {
                        change(this);
                        cx.notify();
                    })
                    .ok();
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .id("details")
                            .role(Role::Button)
                            .aria_label("Details")
                            .aria_expanded(expanded)
                            .aria_controls("details-panel")
                            .track_focus(&self.details_focus)
                            .w(px(90.))
                            .h(px(24.))
                            .bg(rgb(GRAY_HEX))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.expanded = !this.expanded;
                                cx.notify();
                            }))
                            .on_a11y_action(
                                gpux::AccessibleAction::Expand,
                                set(|this| this.expanded = true),
                            )
                            .on_a11y_action(
                                gpux::AccessibleAction::Collapse,
                                set(|this| this.expanded = false),
                            ),
                    )
                    .child(
                        div()
                            .id("copies")
                            .role(Role::SpinButton)
                            .aria_label("Copies")
                            .aria_numeric_value(self.copies)
                            .aria_min_numeric_value(1.)
                            .aria_max_numeric_value(9.)
                            .track_focus(&self.copies_focus)
                            .w(px(60.))
                            .h(px(24.))
                            .bg(rgb(GRAY_HEX))
                            .on_a11y_action(
                                gpux::AccessibleAction::Increment,
                                set(|this| this.copies = (this.copies + 1.).min(9.)),
                            )
                            .on_a11y_action(
                                gpux::AccessibleAction::Decrement,
                                set(|this| this.copies = (this.copies - 1.).max(1.)),
                            ),
                    )
                    .child(
                        div()
                            .id("delete")
                            .role(Role::Button)
                            .aria_label("Delete")
                            .aria_disabled(true)
                            .w(px(60.))
                            .h(px(24.))
                            .bg(rgb(GRAY_HEX)),
                    ),
            )
            .when(expanded, |more| {
                more.child(
                    div()
                        .id("details-panel")
                        .role(Role::Region)
                        .aria_label("Details panel")
                        .accessibility_id("details-panel")
                        .child("More about it"),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("code")
                            .role(Role::TextInput)
                            .aria_label("Code")
                            .aria_value("12")
                            .aria_invalid(true)
                            .aria_read_only(true)
                            .aria_error_message("code-error")
                            .w(px(80.))
                            .h(px(20.))
                            .bg(rgb(GRAY_HEX)),
                    )
                    .child(
                        div()
                            .id("code-error")
                            .role(Role::Label)
                            .accessibility_id("code-error")
                            .aria_value("Six digits")
                            .child("Six digits"),
                    ),
            )
            .child(
                div()
                    .id("scores")
                    .role(Role::Table)
                    .aria_label("Scores")
                    .aria_row_count(2)
                    .aria_column_count(2)
                    .children([["Ada", "3"], ["Max", "5"]].into_iter().enumerate().map(
                        |(row, cells)| {
                            div()
                                .id(("score-row", row))
                                .role(Role::Row)
                                .aria_row_index(row)
                                .flex()
                                .gap(px(8.))
                                .children(cells.into_iter().enumerate().map(
                                    move |(column, cell)| {
                                        div()
                                            .id(("score", row * 2 + column))
                                            .role(Role::Cell)
                                            .aria_column_index(column)
                                            .child(cell)
                                    },
                                ))
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("pin")
                            .role(Role::Button)
                            .aria_label("Pin")
                            .w(px(40.))
                            .h(px(20.))
                            .bg(rgb(GRAY_HEX))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pinned = !this.pinned;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("pins")
                            .flex()
                            .gap(px(8.))
                            .when(self.pinned, |pins| {
                                pins.role(Role::Group).aria_label("Pinned")
                            })
                            .children(["Alpha", "Beta"].map(|name| {
                                div().id(name).role(Role::Link).aria_label(name).child(name)
                            })),
                    ),
            )
            .child(
                div()
                    .id("shelf")
                    .role(Role::List)
                    .aria_label("Shelf")
                    .h(px(40.))
                    .w(px(120.))
                    .overflow_y_scroll()
                    .children(self.shelf_focus.iter().enumerate().map(|(index, focus)| {
                        div()
                            .id(("shelf-item", index))
                            .role(Role::ListItem)
                            .aria_label(gpux::SharedString::from(format!("Item {}", index + 1)))
                            .track_focus(focus)
                            .h(px(20.))
                            .child(gpux::SharedString::from(format!("Item {}", index + 1)))
                    })),
            )
    }
}

fn controls(_: &mut gpux::Window, cx: &mut gpux::App) -> gpux::AnyView {
    use gpux::AppContext as _;
    cx.new(|cx| {
        let title = cx.new(|cx| {
            let mut title = gpux::EditableTextState::new(cx);
            title.set_placeholder("Untitled", cx);
            title
        });
        cx.observe(&title, |_, _, cx| cx.notify()).detach();
        Controls {
            clicks: 0,
            subscribed: false,
            volume: 5.,
            title,
            subscribe_focus: cx.focus_handle(),
            volume_focus: cx.focus_handle(),
            expanded: false,
            details_focus: cx.focus_handle(),
            copies: 1.,
            copies_focus: cx.focus_handle(),
            pinned: false,
            shelf_focus: (0..5).map(|_| cx.focus_handle()).collect(),
        }
    })
    .into()
}

/// The status line of the `accessibility` scene.
fn state_is(clicks: usize, subscribed: bool, volume: u32) -> Step {
    Step::Expect(Expect::Exists(status_is(format!(
        "clicks {clicks} subscribed {subscribed} volume {volume}"
    ))))
}

pub const CONTROLS: SceneSpec = SceneSpec {
    id: "accessibility",
    areas: &[Area::AccessibilityTree, Area::Accessibility],
    size: (360., 420.),
    scales: &[1., 2.],
    lanes: &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F],
    golden: false,
    build: controls,
    script: || {
        vec![
            Step::Expect(Expect::Exists(Query::role("button").with_name("Activate"))),
            Step::Expect(Expect::Exists(Query::from(
                "role=checkbox name=Subscribe checked=false",
            ))),
            Step::Expect(Expect::Exists(Query::from(
                "role=slider name=Volume value=5",
            ))),
            Step::Expect(Expect::Exists(Query::from("role=textbox name=Title"))),
            // Named by the label it names, valued, and required.
            Step::Expect(Expect::Exists(Query::from(
                "role=textbox name=Email value=ada@example.com",
            ))),
            Step::Expect(Expect::Exists(Query::role("list").with_name("Fruits"))),
            Step::Expect(Expect::Count(
                Query::role("listitem").within(Query::role("list").with_name("Fruits")),
                3,
            )),
            Step::Expect(Expect::Exists(Query::role("status"))),
            Step::Expect(Expect::Accessible("Subscribe".into())),
            state_is(0, false, 5),
            Step::Click(Query::role("button").with_name("Activate")),
            state_is(1, false, 5),
            Step::Click(Query::role("checkbox").with_name("Subscribe")),
            state_is(1, true, 5),
            Step::Expect(Expect::Exists(Query::from(
                "role=checkbox name=Subscribe checked=true",
            ))),
            Step::Type(Query::from("role=textbox name=Title"), "Hello".into()),
            Step::Expect(Expect::Exists(Query::from(
                "role=textbox name=Title value=Hello",
            ))),
            // The button's corner away from its label.
            Step::probe(14., 46., GRAY),
        ]
    },
};
