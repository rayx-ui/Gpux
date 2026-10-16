//! Semantics: the accessibility tree and devtools semantics.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{AnyElement, DevtoolsSemantics, IntoElement, Role, div, px, rgb, semantic_query::Query};

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
            .devtools_semantics(DevtoolsSemantics::new().role("button").name(label))
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
                .devtools_semantics(
                    DevtoolsSemantics::new()
                        .role("checkbox")
                        .name("Agree")
                        .checked(true),
                ),
        )
        .into_any_element()
}

pub const ACCESSIBILITY: SceneSpec = SceneSpec {
    id: "semantics-accessibility",
    areas: &[Area::AccessibilityTree, Area::DevtoolsSemantics],
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
