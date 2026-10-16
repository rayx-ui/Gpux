//! Fast mode with the accessibility tree built: cached view replay, retained paint and splice run
//! as with it off and carry what the tree recorded for the reused content (nodes and their links,
//! bounds, focusable entries, focus, action handlers, text leaves, inspection containers and the
//! id map), and the randomized oracle holds every frame's tree equal to a window drawing from
//! scratch. A prepaint that is tried again leaves nothing of its first try in the tree.

use super::oracle_support::{Change, OracleWindow, run_lockstep_with, seeds_and_steps};
use crate::{
    AnyElement, App, Bounds, Context, Element, ElementId, FastMode, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Render, Role, Window, div, prelude::*, px,
};

const VIEWS: FastMode = FastMode {
    layout_retention: true,
    view_retention: true,
    ..FastMode::OFF
};

const SPLICE: FastMode = FastMode {
    layout_retention: true,
    view_retention: true,
    splice: true,
    scroll_layers: false,
};

#[test]
fn views_mode_with_accessibility_matches_frames_drawn_from_scratch() {
    let (seeds, steps) = seeds_and_steps(8, 40);
    for seed in seeds {
        let stats = run_lockstep_with(seed, steps, VIEWS, true).incremental;
        // Views are drawn from last frame with the tree built (the history's drags and
        // refreshes still build theirs, as with the tree off).
        assert!(stats.views_reused > 0, "seed {seed}: {stats:?}");
    }
}

#[test]
fn splice_mode_with_accessibility_matches_frames_drawn_from_scratch() {
    let (seeds, steps) = seeds_and_steps(8, 40);
    let mut spliced = 0;
    for seed in seeds {
        let stats = run_lockstep_with(seed, steps, SPLICE, true).incremental;
        assert!(stats.views_reused > 0, "seed {seed}: {stats:?}");
        spliced += stats.views_spliced;
    }
    assert!(spliced > 0, "the runs spliced views");
}

/// Draws `changes` in an incremental window in `mode` and a window drawing from scratch, both
/// building their trees, and panics on the first frame whose descriptions differ. Returns the
/// incremental window, and its counters for each frame.
fn lockstep(mode: FastMode, changes: &[Change]) -> (OracleWindow, Vec<crate::FastStats>) {
    let mut incremental = OracleWindow::open_with(0, mode, true);
    let mut reference = OracleWindow::open_with(0, FastMode::OFF, true);
    let mut stats = Vec::new();
    for (step, change) in changes.iter().enumerate() {
        incremental.apply(change);
        reference.apply(change);
        let (expected, _) = reference.draw(true);
        let (actual, frame) = incremental.draw(false);
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!("frame {step} after {change:?}: {difference}");
        }
        stats.push(frame);
    }
    (incremental, stats)
}

/// Changes beside reused views move them, and with them their nodes' bounds and the links
/// between their nodes and the nodes built anew around them.
#[test]
fn reused_views_carry_their_nodes_bounds_and_links_when_content_around_them_changes() {
    for mode in [VIEWS, SPLICE] {
        let (_, stats) = lockstep(
            mode,
            &[
                Change::Hover { x: 4000., y: 4000. },
                Change::UpdatePanel { panel: 0, value: 3 },
                Change::InsertChip { at: 0 },
                Change::RotateChips { by: 1 },
                Change::RemoveChip { at: 0 },
                Change::SwapText,
                Change::Badge,
                Change::Redraw,
            ],
        );
        assert!(
            stats.iter().skip(1).all(|frame| frame.views_reused > 0),
            "{mode:?}: {stats:?}"
        );
    }
}

/// Hover and covering redraw reused content's paint alone; its part of the tree is carried
/// with it.
#[test]
fn paint_only_changes_carry_the_tree_of_reused_content() {
    lockstep(
        VIEWS,
        &[
            Change::Hover { x: 4000., y: 4000. },
            Change::ToggleCellHover { cell: 0 },
            Change::HoverCell { cell: 0 },
            Change::HoverCell { cell: 1 },
            Change::CoverCells,
            Change::HoverCell { cell: 0 },
            Change::CoverCells,
        ],
    );
}

/// The action handlers a reused view registered last frame still run when assistive
/// technology asks for them.
#[test]
fn a_reused_view_s_action_handlers_still_run() {
    let (mut incremental, stats) = lockstep(
        VIEWS,
        &[
            Change::Hover { x: 4000., y: 4000. },
            Change::Redraw,
            Change::Redraw,
        ],
    );
    assert!(
        stats[2].views_reused > 0 && stats[2].views_built == 0,
        "{stats:?}"
    );
    let ran = incremental
        .cx
        .update_window(incremental.window.into(), |_, window, cx| {
            let tree = window.a11y_tree().cloned().unwrap();
            let (button, _) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.supports_action(accesskit::Action::Increment))
                .expect("the press target advertises Increment");
            window.perform_a11y_action(
                accesskit::ActionRequest {
                    action: accesskit::Action::Increment,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: *button,
                    data: None,
                },
                cx,
            )
        })
        .unwrap();
    assert!(ran, "the carried handler ran");
}

/// With nothing changing, every view is drawn from last frame and the tree is still the
/// tree drawn from scratch: nodes, focus, focusables, handlers, containers and the id map.
#[test]
fn a_still_window_carries_its_whole_tree() {
    a_still_window_carries_its_whole_tree_in(VIEWS);
}

/// [`a_still_window_carries_its_whole_tree`], with splice on.
#[test]
fn a_still_spliced_window_carries_its_whole_tree() {
    a_still_window_carries_its_whole_tree_in(SPLICE);
}

fn a_still_window_carries_its_whole_tree_in(mode: FastMode) {
    let mut incremental = OracleWindow::open_with(0, mode, true);
    let mut reference = OracleWindow::open_with(0, FastMode::OFF, true);
    for step in 0..5 {
        let change = match step {
            0 => Change::Hover { x: 4000., y: 4000. },
            // Focus the key target, so a focused node is carried.
            1 => Change::Focus { target: 0 },
            _ => Change::Redraw,
        };
        incremental.apply(&change);
        reference.apply(&change);
        let (expected, _) = reference.draw(true);
        let (actual, stats) = incremental.draw(false);
        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            panic!("still frame {step}: {difference}");
        }
        let tree = |lines: &[String]| {
            lines
                .iter()
                .filter(|line| line.starts_with("a11y "))
                .count()
        };
        assert!(tree(&actual) > 10, "the tree is described: {actual:#?}");
        // The frame after the focus moved still builds the views that read it.
        if step > 2 {
            assert_eq!(stats.views_built, 0, "still frame {step}: {stats:?}");
            assert!(stats.views_reused > 0, "still frame {step}: {stats:?}");
            assert!(
                actual.iter().any(|line| line.starts_with("a11y element")),
                "the id map is carried"
            );
        }
    }
}

/// Prepaints its child in a transaction that fails, then again for real, as a list does when
/// it scrolls to reveal what asked for it.
struct Retrying {
    tries: [Option<AnyElement>; 2],
}

impl IntoElement for Retrying {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Retrying {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let children: Vec<LayoutId> = self
            .tries
            .iter_mut()
            .flatten()
            .map(|child| child.request_layout(window, cx))
            .collect();
        (window.request_layout(Default::default(), children, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut first = self.tries[0].take().unwrap();
        let result: Result<(), ()> = window.transact(|window| {
            first.prepaint(window, cx);
            Err(())
        });
        assert!(result.is_err());
        self.tries[1].as_mut().unwrap().prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.tries[1].as_mut().unwrap().paint(window, cx);
    }
}

struct Retried;

impl Render for Retried {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let child = |label: &'static str| {
            div()
                .id("retried")
                .role(Role::Button)
                .aria_label(label)
                .w(px(40.))
                .h(px(20.))
                .child("text")
                .into_any_element()
        };
        div().size_full().child(Retrying {
            tries: [Some(child("first try")), Some(child("second try"))],
        })
    }
}

#[test]
fn a_rolled_back_prepaint_leaves_nothing_in_the_tree() {
    let mut cx = super::oracle_support::oracle_app(0);
    let window = cx.add_window(|_, _| Retried);
    let _guard = cx
        .update_window(window.into(), |_, window, _| window.begin_a11y_inspection())
        .unwrap();
    let tree = cx
        .update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.a11y_tree().cloned().unwrap()
        })
        .unwrap();
    let labels: Vec<&str> = tree
        .nodes
        .iter()
        .filter_map(|(_, node)| node.label())
        .collect();
    assert!(labels.contains(&"second try"), "{labels:?}");
    assert!(!labels.contains(&"first try"), "{labels:?}");
    let texts = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.value() == Some("text"))
        .count();
    assert_eq!(texts, 1, "the first try's text leaf is gone");
    // Every child link names a node of the tree, and the button is one of them once.
    let ids: Vec<_> = tree.nodes.iter().map(|(id, _)| *id).collect();
    for (_, node) in &tree.nodes {
        for child in node.children() {
            assert!(ids.contains(child));
        }
    }
    let parents = tree
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.children().iter().any(|child| {
                tree.nodes
                    .iter()
                    .any(|(id, n)| id == child && n.role() == accesskit::Role::Button)
            })
        })
        .count();
    assert_eq!(parents, 1);
}

/// A view whose tree holds what the replay carries beside nodes: an active descendant of a
/// focused list box, relations between elements, and synthetic children.
struct Rich {
    list: crate::FocusHandle,
}

impl Render for Rich {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("listbox")
                    .role(Role::ListBox)
                    .aria_label("Fruits")
                    .track_focus(&self.list)
                    .child(
                        div()
                            .id("apple")
                            .role(Role::ListBoxOption)
                            .aria_label("Apple")
                            .h(px(10.)),
                    )
                    .child(
                        div()
                            .id("pear")
                            .role(Role::ListBoxOption)
                            .aria_label("Pear")
                            .aria_active_descendant()
                            .h(px(10.)),
                    ),
            )
            .child(
                div()
                    .id("name-label")
                    .role(Role::Label)
                    .accessibility_id("name-label")
                    .aria_label("Name")
                    .h(px(10.)),
            )
            .child(
                div()
                    .id("name")
                    .role(Role::TextInput)
                    .aria_labelled_by("name-label")
                    .h(px(10.)),
            )
            .child(
                div()
                    .id("runs")
                    .role(Role::Paragraph)
                    .a11y_synthetic_children(|builder| {
                        for index in 0..2 {
                            let mut run = accesskit::Node::new(accesskit::Role::TextRun);
                            run.set_value(format!("run {index}"));
                            let id = builder.synthetic_node_id(index);
                            builder.push_child(id, run);
                        }
                    })
                    .h(px(10.)),
            )
    }
}

/// A root around a [`Rich`] view, which changes on its own, so the view is drawn from last
/// frame.
struct AroundRich {
    rich: crate::Entity<Rich>,
    tick: usize,
}

impl Render for AroundRich {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                crate::AnyView::from(self.rich.clone())
                    .cached(crate::StyleRefinement::default().size_full()),
            )
            .child(crate::SharedString::from(format!("tick {}", self.tick)))
    }
}

/// The active descendant, the relations and the synthetic children of a view drawn from last
/// frame are carried: its frames equal those of a window drawing from scratch.
#[test]
fn a_reused_view_carries_its_active_descendant_relations_and_synthetic_children() {
    use crate::AppContext as _;
    for mode in [VIEWS, SPLICE] {
        let open = |mode: FastMode| {
            let mut cx = super::oracle_support::oracle_app(0);
            let window = cx.add_window(|window, cx| {
                let rich = cx.new(|cx| Rich {
                    list: cx.focus_handle(),
                });
                let list = rich.read(cx).list.clone();
                window.focus(&list, cx);
                AroundRich { rich, tick: 0 }
            });
            cx.simulate_window_resize(window.into(), crate::size(px(300.), px(200.)));
            cx.set_draw_on_flush(false);
            let guard = cx
                .update_window(window.into(), |_, window, _| {
                    window.set_fast_mode(mode);
                    window.begin_a11y_inspection()
                })
                .unwrap();
            (cx, window, guard)
        };
        let (mut incremental, inc_window, _inc_guard) = open(mode);
        let (mut reference, ref_window, _ref_guard) = open(FastMode::OFF);
        for step in 0..4 {
            for (cx, window) in [(&mut incremental, inc_window), (&mut reference, ref_window)] {
                window
                    .update(cx, |around, _, cx| {
                        around.tick = step;
                        cx.notify();
                    })
                    .unwrap();
            }
            let draw = |cx: &mut crate::TestAppContext,
                        window: crate::WindowHandle<AroundRich>,
                        scratch: bool| {
                cx.update_window(window.into(), |_, window, cx| {
                    if scratch {
                        window.forget_retained_state();
                    }
                    window.draw(cx).clear(cx);
                    (
                        crate::fast::verify::describe_frame(window),
                        window.fast_frame_stats(),
                    )
                })
                .unwrap()
            };
            let (expected, _) = draw(&mut reference, ref_window, true);
            let (actual, stats) = draw(&mut incremental, inc_window, false);
            if let Some(difference) =
                crate::fast::verify::first_frame_difference(&actual, &expected)
            {
                panic!("{mode:?} step {step}: {difference}");
            }
            let has = |needle: &str| {
                actual
                    .iter()
                    .any(|line| line.starts_with("a11y") && line.contains(needle))
            };
            assert!(has("run 1"), "synthetic children: {actual:#?}");
            assert!(has("labelled_by"), "the relation: {actual:#?}");
            // The active descendant takes the tree's focus from the focused list box.
            let pear = actual
                .iter()
                .find(|line| line.contains("label: \"Pear\""))
                .and_then(|line| line.split_whitespace().nth(2))
                .expect("the option's node");
            assert!(
                actual
                    .iter()
                    .any(|line| line.starts_with(&format!("a11y focus {pear} "))),
                "the active descendant has the focus: {actual:#?}"
            );
            if step > 0 {
                assert!(
                    stats.views_reused + stats.views_spliced > 0,
                    "{mode:?} step {step}: {stats:?}"
                );
            }
        }
    }
}
