//! Lists: `uniform_list` and `list` virtualization.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyView, App, Context, IntoElement, ListAlignment, ListState, Render, Role, SharedString,
    Window, div, list, px, semantic_query::Query, uniform_list,
};

const ALL_LANES: &[Lane] = &[Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F];

fn row(name: String, height: f32) -> impl IntoElement {
    div()
        .id(SharedString::from(name.clone()))
        .role(Role::ListItem)
        // A list item is named by its author, not by its text (as in ARIA).
        .aria_label(name.clone())
        .h(px(height))
        .child(name)
}

struct Uniform;

impl Render for Uniform {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root().child(
            uniform_list("rows", 1000, |range, _, _| {
                range
                    .map(|index| row(format!("Row {index}"), 20.))
                    .collect::<Vec<_>>()
            })
            .h(px(100.)),
        )
    }
}

fn item(name: &str) -> Query {
    Query::role("listitem").with_name(name)
}

pub const UNIFORM_LIST: SceneSpec = SceneSpec {
    id: "list-uniform",
    areas: &[Area::UniformList],
    size: (200., 150.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: |_, cx| cx.new(|_| Uniform).into(),
    script: || {
        vec![
            // Only the visible rows of 1000 are built.
            Step::Expect(Expect::Exists(item("Row 0"))),
            Step::Expect(Expect::Exists(item("Row 4"))),
            Step::Expect(Expect::Missing(item("Row 30"))),
            Step::Expect(Expect::AtLeast(Query::role("listitem"), 5)),
            // Row labels paint at the left; the rest of each row is blank.
            Step::ProbeDiffer((0., 0., 40., 100.), (150., 0., 40., 100.), 40),
            Step::Capture("top"),
            Step::Wheel(Query::text("Row 2"), 0., -400.),
            Step::Expect(Expect::Exists(item("Row 22"))),
            Step::Expect(Expect::Missing(item("Row 0"))),
            Step::ChangedSince("top", (0., 0., 100., 100.)),
        ]
    },
};

/// A `list` of 500 rows of three heights.
struct Variable {
    state: ListState,
}

impl Render for Variable {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root().child(
            list(self.state.clone(), |index, _, _| {
                row(format!("Item {index}"), 20. + (index % 3) as f32 * 10.).into_any_element()
            })
            .h(px(100.)),
        )
    }
}

fn variable(_: &mut Window, cx: &mut App) -> AnyView {
    // `list` sums only measured rows into its scroll range, and a wheel that reaches the end of
    // that range anchors the list to its last item, so lazily measured rows would jump to the
    // end on the first wheel. Measuring every row keeps the scroll range exact.
    cx.new(|_| Variable {
        state: ListState::new(500, ListAlignment::Top, px(50.)).measure_all(),
    })
    .into()
}

pub const LIST: SceneSpec = SceneSpec {
    id: "list-variable",
    areas: &[Area::ListVirtualization],
    size: (200., 150.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: variable,
    script: || {
        // Rows repeat 20, 30 and 40 px tall: every three rows take 90 px, so ten 90 px wheel
        // steps put "Item 30" at the top.
        let mut steps = vec![
            Step::Expect(Expect::Exists(item("Item 0"))),
            Step::Expect(Expect::Missing(item("Item 100"))),
            Step::ProbeDiffer((0., 0., 40., 100.), (150., 0., 40., 100.), 40),
            Step::Capture("top"),
        ];
        steps.extend((0..10).map(|_| Step::Wheel(Query::role("listitem").nth(0), 0., -90.)));
        steps.extend([
            Step::Expect(Expect::Exists(item("Item 30"))),
            Step::Expect(Expect::Exists(item("Item 31"))),
            Step::Expect(Expect::Missing(item("Item 0"))),
            Step::ChangedSince("top", (0., 0., 100., 100.)),
        ]);
        steps
    },
};
