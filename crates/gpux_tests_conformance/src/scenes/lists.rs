//! Lists: `uniform_list`, `list` and `virtual_list` virtualization.

use super::common::*;
use crate::scene::{Area, Expect, Lane, SceneSpec, Step};
use gpux::prelude::*;
use gpux::{
    AnyView, App, Context, IntoElement, ListAlignment, ListState, Render, Role, SharedString,
    VirtualListScrollHandle, Window, div, list, px, semantic_query::Query, size, uniform_list,
    v_virtual_list,
};
use std::rc::Rc;

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

/// A `virtual_list` of 1000 rows of three heights, every size known up front.
struct Virtual {
    sizes: Rc<Vec<gpux::Size<gpux::Pixels>>>,
    handle: VirtualListScrollHandle,
}

fn virtual_height(index: usize) -> f32 {
    20. + (index % 3) as f32 * 10.
}

impl Render for Virtual {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        root().child(
            v_virtual_list(cx.entity(), "rows", self.sizes.clone(), |_, range, _, _| {
                range
                    .map(|index| row(format!("Entry {index}"), virtual_height(index)))
                    .collect::<Vec<_>>()
            })
            .track_scroll(&self.handle)
            .h(px(100.)),
        )
    }
}

fn virtual_rows(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|_| Virtual {
        sizes: Rc::new(
            (0..1000)
                .map(|index| size(px(200.), px(virtual_height(index))))
                .collect(),
        ),
        handle: VirtualListScrollHandle::new(),
    })
    .into()
}

pub const VIRTUAL_LIST: SceneSpec = SceneSpec {
    id: "list-virtual",
    areas: &[Area::VirtualList],
    size: (200., 150.),
    scales: &[1., 2.],
    lanes: ALL_LANES,
    golden: false,
    build: virtual_rows,
    script: || {
        // Rows repeat 20, 30 and 40 px tall: every three rows take 90 px, so ten 90 px wheel
        // steps put "Entry 30" at the top.
        let mut steps = vec![
            // Only the visible rows of 1000 are built.
            Step::Expect(Expect::Exists(item("Entry 0"))),
            Step::Expect(Expect::Exists(item("Entry 3"))),
            Step::Expect(Expect::Missing(item("Entry 30"))),
            Step::Expect(Expect::Missing(item("Entry 999"))),
            Step::ProbeDiffer((0., 0., 40., 100.), (150., 0., 40., 100.), 40),
            Step::Capture("top"),
        ];
        steps.extend((0..10).map(|_| Step::Wheel(Query::role("listitem").nth(0), 0., -90.)));
        steps.extend([
            Step::Expect(Expect::Exists(item("Entry 30"))),
            Step::Expect(Expect::Exists(item("Entry 31"))),
            Step::Expect(Expect::Missing(item("Entry 0"))),
            Step::Expect(Expect::Missing(item("Entry 100"))),
            Step::ChangedSince("top", (0., 0., 100., 100.)),
        ]);
        steps
    },
};
