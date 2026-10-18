//! A measured leaf other than text, whose size on an axis comes from its measurement, keeps that
//! size on every frame view retention keeps its view, with each fast-mode switch combination.
//!
//! Each case puts the leaf in a child view inside a scrolling `div` of the root view, scrolls
//! the parent and changes things beside the child, and compares every frame with a window drawn
//! with fast mode off: the parent's scroll extent, the parent's offset, and the frame itself.
//!
//! `cargo test -p gpux --features test-support --test retained_measured_leaves`

use std::{cell::Cell, rc::Rc};

use gpux::{
    AnyWindowHandle, App, AppContext as _, AvailableSpace, Bounds, Context, Element, ElementId,
    Entity, FastMode, GlobalElementId, InspectorElementId, InteractiveElement as _, IntoElement,
    LayoutId, ListSizingBehavior, ParentElement as _, Pixels, PlatformInput, Render, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _, Style, Styled as _,
    TestAppContext, TestDispatcher, TouchPhase, Window, div, fill, hsla, point, px, size,
    uniform_list,
};

const W: f32 = 300.;
const H: f32 = 200.;
const ROWS: usize = 1000;
const ROW: f32 = 20.;
/// The height the measured element gives itself.
const TALL: f32 = 20_000.;

/// What the child view holds.
#[derive(Clone, Copy, Debug)]
enum Case {
    /// A `uniform_list` sized by `ListSizingBehavior::Infer`, with an auto height.
    InferredList,
    /// An element built on `Window::request_measured_layout`, as a scroll container's content.
    Measured,
    /// The same element as a flex child.
    MeasuredFlexChild,
    /// An element whose measured height depends on the width available to it: the parent's width
    /// changes while the child view is kept.
    WidthDependent,
    /// The measured element with a definite width and height: its measurement decides nothing,
    /// so its view is kept without building.
    DefiniteSize,
    /// A `uniform_list` sized by `ListSizingBehavior::Infer` and `100%` of a parent whose height
    /// the root switches between definite and auto while the child view is kept.
    ParentLosesItsHeight,
    /// The measured element of definite size with an automatic minimum, in the scrolling parent,
    /// which the root switches between block and flex while the child view is kept.
    ParentBecomesFlex,
    /// A measured element sized `100%` of a parent of auto height: the percentage resolves
    /// against no height, and the measurement gives it.
    PercentOfAutoParent,
    /// The measured element of definite size as a flex child taller than its container: its
    /// automatic minimum height comes from its measurement and stops it from shrinking.
    DefiniteFlexChild,
}

/// An element on the public measured-layout path: as wide as it may be, and as tall as
/// `height` makes it for that width.
struct Measured {
    height: fn(Pixels) -> Pixels,
}

impl IntoElement for Measured {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Measured {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let height = self.height;
        let layout =
            window.request_measured_layout(Style::default(), move |known, available, _, _| {
                let width = known.width.unwrap_or(match available.width {
                    AvailableSpace::Definite(width) => width,
                    _ => px(W),
                });
                size(width, known.height.unwrap_or(height(width)))
            });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        // Bands every 100 px, so that the frame shows where the element lies.
        let mut top = bounds.top();
        let mut band = 0;
        while top < bounds.bottom() {
            window.paint_quad(fill(
                Bounds::new(point(bounds.left(), top), size(bounds.size.width, px(100.))),
                hsla((band % 7) as f32 / 7., 0.5, 0.5, 1.),
            ));
            top += px(100.);
            band += 1;
        }
    }
}

/// A measured element with the size its style gives it, measuring as [`W`] by [`TALL`] on an
/// axis the style leaves open.
struct SizedMeasured {
    width: gpux::Length,
    height: gpux::Length,
    /// Whether its minimum size is its size, rather than automatic.
    fixed: bool,
}

impl SizedMeasured {
    /// [`W`] by [`TALL`], with that minimum size too: sized so in every parent.
    fn fixed() -> Self {
        Self {
            fixed: true,
            ..Self::definite()
        }
    }

    /// [`W`] by [`TALL`] with an automatic minimum size, which a flex parent takes from its
    /// measurement.
    fn definite() -> Self {
        Self {
            width: px(W).into(),
            height: px(TALL).into(),
            fixed: false,
        }
    }
}

impl IntoElement for SizedMeasured {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SizedMeasured {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size = size(self.width, self.height);
        if self.fixed {
            style.min_size = style.size;
        }
        let layout = window.request_measured_layout(style, |known, _, _, _| {
            size(
                known.width.unwrap_or(px(W)),
                known.height.unwrap_or(px(TALL)),
            )
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        window.paint_quad(fill(
            Bounds::new(bounds.origin, size(bounds.size.width, px(100.))),
            hsla(0.3, 0.5, 0.5, 1.),
        ));
    }
}

struct Child {
    case: Case,
}

impl Render for Child {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match self.case {
            Case::InferredList => uniform_list("inferred", ROWS, |range, _, _| {
                range
                    .map(|row| {
                        div()
                            .w_full()
                            .h(px(ROW))
                            .bg(hsla((row % 97) as f32 / 97., 0.5, 0.5, 1.))
                    })
                    .collect::<Vec<_>>()
            })
            .with_sizing_behavior(ListSizingBehavior::Infer)
            .w_full()
            .into_any_element(),
            Case::Measured | Case::MeasuredFlexChild => Measured {
                height: |_| px(TALL),
            }
            .into_any_element(),
            Case::WidthDependent => Measured {
                height: |width| px(6_000_000. / width.as_f32()),
            }
            .into_any_element(),
            Case::DefiniteSize => div()
                .w(px(W))
                .h(px(TALL))
                .child(SizedMeasured::fixed())
                .into_any_element(),
            Case::PercentOfAutoParent => div()
                .w_full()
                .child(SizedMeasured {
                    width: gpux::relative(1.).into(),
                    height: gpux::relative(1.).into(),
                    fixed: false,
                })
                .into_any_element(),
            Case::DefiniteFlexChild | Case::ParentBecomesFlex => {
                SizedMeasured::definite().into_any_element()
            }
            Case::ParentLosesItsHeight => uniform_list("inferred", ROWS, |range, _, _| {
                range
                    .map(|row| {
                        div()
                            .w_full()
                            .h(px(ROW))
                            .bg(hsla((row % 97) as f32 / 97., 0.5, 0.5, 1.))
                    })
                    .collect::<Vec<_>>()
            })
            .with_sizing_behavior(ListSizingBehavior::Infer)
            .size_full()
            .into_any_element(),
        }
    }
}

struct Root {
    case: Case,
    parent: ScrollHandle,
    child: Entity<Child>,
    width: Rc<Cell<f32>>,
    /// Toggled to change the root without changing the child.
    beside: bool,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let parent = div()
            .id("parent")
            .w(px(self.width.get()))
            .h(px(H))
            .flex_none()
            .overflow_y_scroll()
            .track_scroll(&self.parent);
        let parent = match self.case {
            Case::MeasuredFlexChild | Case::DefiniteFlexChild => parent.flex().flex_col(),
            Case::ParentBecomesFlex if self.beside => parent.flex().flex_col(),
            _ => parent,
        };
        div()
            .size_full()
            .bg(gpux::white())
            .flex()
            .child(parent.child(match self.case {
                // The root changes the parent the leaf resolves against; the child is unchanged.
                Case::ParentLosesItsHeight => {
                    let wrapper = div().w_full();
                    let wrapper = if self.beside {
                        wrapper
                    } else {
                        wrapper.h(px(ROWS as f32 * ROW))
                    };
                    wrapper.child(self.child.clone()).into_any_element()
                }
                _ => self.child.clone().into_any_element(),
            }))
            .child(
                div()
                    .w(px(30.))
                    .h(px(if self.beside { 40. } else { 20. }))
                    .bg(gpux::black()),
            )
    }
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Draw,
    Wheel(f32),
    /// The root renders again; the child does not change.
    Beside,
    /// The parent becomes this wide.
    Width(f32),
}

const SCRIPT: &[Step] = &[
    Step::Draw,
    Step::Draw,
    Step::Wheel(-60.),
    Step::Beside,
    Step::Wheel(-400.),
    Step::Draw,
    Step::Beside,
    Step::Width(240.),
    Step::Wheel(-200.),
    Step::Beside,
    Step::Width(300.),
    Step::Wheel(500.),
    Step::Draw,
];

#[derive(Debug, PartialEq)]
struct Frame {
    max_offset: f32,
    offset: f32,
    primitives: Vec<String>,
}

fn with<R>(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    cx.update_window(window, |_, window, cx| f(window, cx))
        .unwrap()
}

fn draw(cx: &mut TestAppContext, window: AnyWindowHandle) {
    with(cx, window, |window, cx| window.draw(cx).clear(cx));
}

/// Runs the script in a fresh application with `mode`, or with fast mode off when `reference`,
/// returning its frames and its counters.
fn run(case: Case, mode: FastMode, reference: bool) -> (Vec<Frame>, gpux::FastStats) {
    let mut cx = TestAppContext::build(TestDispatcher::new(0), None);
    let cx = &mut cx;
    let parent = ScrollHandle::new();
    let width = Rc::new(Cell::new(W));
    let root: Rc<Cell<Option<Entity<Root>>>> = Rc::default();
    let window: AnyWindowHandle = cx
        .add_window({
            let (parent, root) = (parent.clone(), root.clone());
            move |_, cx| {
                root.set(Some(cx.entity()));
                Root {
                    case,
                    parent,
                    child: cx.new(|_| Child { case }),
                    width,
                    beside: false,
                }
            }
        })
        .into();
    let root = root.take().unwrap();
    with(cx, window, |window, _| {
        window.set_fast_mode(if reference { FastMode::OFF } else { mode });
        // Count only the frames drawn in `mode`, not those the window drew as it opened.
        window.reset_fast_stats();
    });
    draw(cx, window);
    let mut frames = Vec::new();
    for step in SCRIPT {
        match *step {
            Step::Draw => {}
            Step::Wheel(dy) => with(cx, window, |window, cx| {
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: point(px(50.), px(H / 2.)),
                        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                        modifiers: Default::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            }),
            Step::Beside => root.update(cx, |root, cx| {
                root.beside = !root.beside;
                cx.notify();
            }),
            Step::Width(new) => root.update(cx, |root, cx| {
                root.width.set(new);
                cx.notify();
            }),
        }
        cx.run_until_parked();
        draw(cx, window);
        frames.push(Frame {
            max_offset: parent.max_offset().y.as_f32(),
            offset: parent.offset().y.as_f32(),
            primitives: with(cx, window, |window, _| window.painted_primitives()),
        });
    }
    let stats = with(cx, window, |window, _| window.fast_stats());
    cx.quit();
    (frames, stats)
}

/// Every combination of the switches, each requiring the ones before it.
const MODES: [(&str, FastMode); 5] = [
    ("every switch", FastMode::ALL),
    (
        "layers without splice",
        FastMode {
            splice: false,
            ..FastMode::ALL
        },
    ),
    (
        "retention and splice",
        FastMode {
            scroll_layers: false,
            ..FastMode::ALL
        },
    ),
    (
        "view retention",
        FastMode {
            splice: false,
            scroll_layers: false,
            ..FastMode::ALL
        },
    ),
    (
        "layout retention",
        FastMode {
            view_retention: false,
            splice: false,
            scroll_layers: false,
            ..FastMode::ALL
        },
    ),
];

/// Checks `case` against fast mode off in every mode, returning each mode's counters.
fn check(case: Case) -> Vec<(&'static str, FastMode, gpux::FastStats)> {
    let (reference, _) = run(case, FastMode::OFF, true);
    assert!(
        reference.iter().all(|frame| frame.max_offset > 1000.),
        "{case:?}: the reference's parent scrolls over the leaf: {:?}",
        reference
            .iter()
            .map(|frame| frame.max_offset)
            .collect::<Vec<_>>()
    );
    let mut counted = Vec::new();
    for (name, mode) in MODES {
        let (frames, stats) = run(case, mode, false);
        counted.push((name, mode, stats));
        for (index, (frame, expected)) in frames.iter().zip(&reference).enumerate() {
            let step = SCRIPT[index];
            assert!(
                frame.max_offset == expected.max_offset,
                "{case:?}, {name}, step {index} ({step:?}): measured leaf collapsed: the \
                 parent's scroll extent is {} px, not {} px as with fast mode off",
                frame.max_offset,
                expected.max_offset
            );
            assert_eq!(
                frame.offset, expected.offset,
                "{case:?}, {name}, step {index} ({step:?}): the parent's offset"
            );
            if let Some(difference) =
                gpux::first_frame_difference(&frame.primitives, &expected.primitives)
            {
                panic!(
                    "{case:?}, {name}, step {index} ({step:?}): the frame differs from fast \
                     mode off: {difference}"
                );
            }
        }
    }
    counted
}

/// With view retention on, the frames that kept the child view built it instead, and counted
/// it: the child is the only view holding a measured leaf, so it is counted at most once a
/// frame, and on every frame that changed only the root.
fn built_for_the_leaf(case: Case) {
    let only_the_root = SCRIPT
        .iter()
        .filter(|step| matches!(step, Step::Beside))
        .count() as u64;
    for (name, mode, stats) in check(case) {
        let built = stats.views_built_for_measured_leaves;
        if mode.view_retention {
            assert!(
                (only_the_root..=stats.frames).contains(&built),
                "{case:?}, {name}: built {built} views for its measured leaf, not between \
                 {only_the_root} and {}: {stats:?}",
                stats.frames
            );
        } else {
            assert_eq!(
                built, 0,
                "{case:?}, {name}: no view is kept without view retention"
            );
        }
    }
}

#[test]
fn an_inferred_uniform_list_keeps_its_extent() {
    built_for_the_leaf(Case::InferredList);
}

#[test]
fn a_measured_element_keeps_its_size_as_scroll_content() {
    built_for_the_leaf(Case::Measured);
}

#[test]
fn a_measured_element_keeps_its_size_as_a_flex_child() {
    built_for_the_leaf(Case::MeasuredFlexChild);
}

#[test]
fn a_kept_leaf_is_measured_afresh_when_its_room_changes() {
    built_for_the_leaf(Case::WidthDependent);
}

/// A view whose measured leaves all have definite sizes is still kept without building.
#[test]
fn a_view_with_definite_measured_leaves_is_still_kept() {
    for (name, mode, stats) in check(Case::DefiniteSize) {
        assert_eq!(
            stats.views_built_for_measured_leaves, 0,
            "{name}: built for a measured leaf of definite size"
        );
        if mode.view_retention {
            assert!(
                stats.views_reused > 0,
                "{name}: the child view was kept: {stats:?}"
            );
        }
    }
}

#[test]
fn a_leaf_sized_by_a_percentage_of_an_auto_parent_keeps_its_size() {
    built_for_the_leaf(Case::PercentOfAutoParent);
}

#[test]
fn a_definite_flex_child_keeps_its_automatic_minimum() {
    built_for_the_leaf(Case::DefiniteFlexChild);
}

#[test]
fn a_kept_leaf_keeps_its_size_when_its_parent_loses_its_height() {
    built_for_the_leaf(Case::ParentLosesItsHeight);
}

#[test]
fn a_kept_leaf_keeps_its_size_when_its_parent_becomes_flex() {
    built_for_the_leaf(Case::ParentBecomesFlex);
}
