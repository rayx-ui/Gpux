//! A `ParentViewport` `VirtualList` in a view that is not rebuilt keeps its full virtual extent
//! under view retention, with each fast-mode switch combination.
//!
//! The list sits in a child view that registers no dependent view, inside a scrolling `div`
//! of the root view. Every frame, the parent's scroll extent must equal the list's content
//! extent, the wheel must scroll the parent, the list must render the rows the parent's
//! viewport shows, and the frame must equal the one a window with fast mode off draws.
//!
//! The cause: the list asked for a measured leaf whose closure returned its content extent.
//! Layout retention takes every measured leaf's closure when a frame is cleared and dirties
//! the leaf; a view kept by view retention is not built, so it requests nothing, and only text
//! leaves are given a closure back (`fast::layout::keep_retained_nodes`). The kept list was
//! measured with no closure, sized empty, and the parent's scroll extent fell to 0. A
//! `ParentViewport` list now asks for a plain node whose size along its axis is its content
//! extent, which a kept node holds in its style.
//!
//! `cargo test -p gpux --features test-support --test virtual_list_parent_viewport`

use std::{cell::RefCell, ops::Range, rc::Rc};

use gpux::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FastMode, InteractiveElement as _,
    IntoElement, ParentElement as _, PlatformInput, Render, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _, TestAppContext, TestDispatcher,
    TouchPhase, VirtualListExternalScrollBehavior, VirtualListScrollHandle, Window, div, hsla,
    point, px, size, v_virtual_list,
};

const ROWS: usize = 1000;
const ROW: f32 = 20.;
const W: f32 = 300.;
const H: f32 = 200.;
const EXTENT: f32 = ROWS as f32 * ROW;

/// The padding of the padded list.
const PAD: f32 = 10.;

type Ranges = Rc<RefCell<Vec<Range<usize>>>>;

/// How the list and its parent are laid out.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// The parent is a block.
    Block,
    /// The parent is a flex column, whose items may shrink to their minimum size.
    FlexColumn,
    /// The list has [`PAD`] of padding on every side.
    Padded,
}

impl Shape {
    fn pad(self) -> f32 {
        match self {
            Shape::Padded => PAD,
            Shape::Block | Shape::FlexColumn => 0.,
        }
    }
}

/// Holds the list; it reads the parent's handle only through the list.
struct Holder {
    handle: VirtualListScrollHandle,
    ranges: Ranges,
    shape: Shape,
}

impl Render for Holder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ranges = self.ranges.clone();
        v_virtual_list(
            cx.entity(),
            "parent-viewport-list",
            Rc::new(vec![size(px(W), px(ROW)); ROWS]),
            |_, range, _, _| {
                range
                    .map(|ix| {
                        div()
                            .w_full()
                            .h(px(ROW))
                            .bg(hsla((ix % 97) as f32 / 97., 0.5, 0.5, 1.))
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_external_scroll(
            &self.handle,
            VirtualListExternalScrollBehavior::ParentViewport,
        )
        .on_rendered_range(move |range, _, _| ranges.borrow_mut().push(range))
        .p(px(self.shape.pad()))
    }
}

/// Owns the parent `div` and its scroll; a counter beside it can be notified alone.
struct Root {
    parent: ScrollHandle,
    holder: Entity<Holder>,
    beside: Entity<Beside>,
    shape: Shape,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let parent = div()
            .id("parent")
            .w(px(W))
            .h(px(H))
            .flex_none()
            .overflow_y_scroll()
            .track_scroll(&self.parent);
        let parent = match self.shape {
            Shape::FlexColumn => parent.flex().flex_col(),
            Shape::Block | Shape::Padded => parent,
        };
        div()
            .size_full()
            .bg(gpux::white())
            .flex()
            .child(parent.child(self.holder.clone()))
            .child(self.beside.clone())
    }
}

struct Beside {
    count: usize,
}

impl Render for Beside {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(40.))
            .h(px(20. + (self.count % 2) as f32 * 10.))
            .bg(gpux::black())
    }
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Draw,
    Wheel(f32),
    NotifyBeside,
}

const SCRIPT: &[Step] = &[
    Step::Draw,
    Step::Draw,
    Step::Wheel(-60.),
    Step::Wheel(-60.),
    Step::Wheel(-400.),
    Step::NotifyBeside,
    Step::Draw,
    Step::Wheel(-15.),
    Step::Wheel(-1000.),
    Step::NotifyBeside,
    Step::Wheel(300.),
    Step::Draw,
];

/// What one frame showed.
#[derive(Debug, PartialEq)]
struct Frame {
    max_offset: f32,
    offset: f32,
    range: Option<Range<usize>>,
    description: Vec<String>,
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

/// Runs the script in a fresh application with `mode` and returns its frames. The reference
/// (`reference: true`) draws with fast mode off and the snapping `mode`'s layers use.
fn run(mode: FastMode, shape: Shape, reference: bool) -> Vec<Frame> {
    let mut cx = TestAppContext::build(TestDispatcher::new(0), None);
    let cx = &mut cx;
    let parent = ScrollHandle::new();
    let ranges = Ranges::default();
    let (window, beside) = {
        let beside = Rc::new(RefCell::new(None));
        let window: AnyWindowHandle = cx
            .add_window({
                let (parent, ranges, beside) = (parent.clone(), ranges.clone(), beside.clone());
                move |_, cx| {
                    let handle = VirtualListScrollHandle::from(parent.clone());
                    let entity = cx.new(|_| Beside { count: 0 });
                    *beside.borrow_mut() = Some(entity.clone());
                    Root {
                        parent,
                        holder: cx.new(|_| Holder {
                            handle,
                            ranges,
                            shape,
                        }),
                        beside: entity,
                        shape,
                    }
                }
            })
            .into();
        let beside = beside.borrow().clone().unwrap();
        (window, beside)
    };
    with(cx, window, |window, _| {
        if reference {
            window.set_fast_mode(FastMode::OFF);
            window.force_layer_snapping(mode.scroll_layers);
        } else {
            window.set_fast_mode(mode);
        }
    });
    draw(cx, window);
    let mut frames = Vec::new();
    for step in SCRIPT {
        ranges.borrow_mut().clear();
        match *step {
            Step::Draw => {}
            Step::Wheel(dy) => with(cx, window, |window, cx| {
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: point(px(W / 2.), px(H / 2.)),
                        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                        modifiers: Default::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            }),
            Step::NotifyBeside => beside.update(cx, |beside, cx| {
                beside.count += 1;
                cx.notify();
            }),
        }
        cx.run_until_parked();
        draw(cx, window);
        frames.push(Frame {
            max_offset: parent.max_offset().y.as_f32(),
            offset: parent.offset().y.as_f32(),
            range: ranges.borrow().last().cloned(),
            description: with(cx, window, |window, _| window.describe_rendered_frame()),
        });
    }
    cx.quit();
    frames
}

/// The rows the parent's viewport shows when it is scrolled by `offset` over a list padded by
/// `pad`, with the extent index's two rows of lookahead.
fn shown(offset: f32, pad: f32) -> Range<usize> {
    let start = (offset - pad).max(0.);
    let end = (offset - pad + H).min(EXTENT);
    let first = (start / ROW).floor() as usize;
    let boundary = (end / ROW).floor() as usize;
    first..if boundary == ROWS {
        ROWS
    } else {
        (boundary + 2).min(ROWS)
    }
}

fn check(name: &str, mode: FastMode, shape: Shape) {
    let frames = run(mode, shape, false);
    let reference = run(mode, shape, true);
    let pad = shape.pad();
    let max_offset = EXTENT + 2. * pad - H;
    let mut offset = 0f32;
    for (index, (frame, expected)) in frames.iter().zip(&reference).enumerate() {
        let step = SCRIPT[index];
        let name = format!("{name} ({shape:?})");
        if let Step::Wheel(dy) = step {
            offset = (offset - dy).clamp(0., max_offset);
        }
        assert!(
            frame.max_offset == max_offset,
            "{name}, step {index} ({step:?}): parent viewport collapsed: the parent's scroll \
             extent is {} px, not the list's {} px less the viewport",
            frame.max_offset,
            EXTENT + 2. * pad
        );
        assert_eq!(
            -frame.offset, offset,
            "{name}, step {index} ({step:?}): the wheel scrolls the parent"
        );
        if let Some(range) = &frame.range {
            assert_eq!(
                *range,
                shown(offset, pad),
                "{name}, step {index} ({step:?}): the list renders the rows the parent shows"
            );
        }
        assert!(
            frame.range.is_some() || !matches!(step, Step::Wheel(_)),
            "{name}, step {index} ({step:?}): a scrolled frame renders the rows it reaches"
        );
        assert_eq!(
            (frame.max_offset, frame.offset),
            (expected.max_offset, expected.offset),
            "{name}, step {index} ({step:?}): the reference with fast mode off differs"
        );
        // A frame that keeps the holder's view does not draw the list, so its range listener
        // runs only when the list is drawn; the reference draws it every frame.
        if frame.range.is_some() {
            assert_eq!(
                frame.range, expected.range,
                "{name}, step {index} ({step:?}): the reference renders other rows"
            );
        }
        if let Some(difference) =
            gpux::first_frame_difference(&frame.description, &expected.description)
        {
            panic!(
                "{name}, step {index} ({step:?}): the frame differs from fast mode off: \
                 {difference}"
            );
        }
    }
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

#[test]
fn every_switch_keeps_the_extent() {
    let (name, mode) = MODES[0];
    check(name, mode, Shape::Block);
}

#[test]
fn scroll_layers_without_splice_keep_the_extent() {
    let (name, mode) = MODES[1];
    check(name, mode, Shape::Block);
}

#[test]
fn view_retention_with_splice_keeps_the_extent() {
    let (name, mode) = MODES[2];
    check(name, mode, Shape::Block);
}

#[test]
fn view_retention_alone_keeps_the_extent() {
    let (name, mode) = MODES[3];
    check(name, mode, Shape::Block);
}

#[test]
fn layout_retention_alone_keeps_the_extent() {
    let (name, mode) = MODES[4];
    check(name, mode, Shape::Block);
}

/// A flex item shrinks to its minimum size; the list's is its whole extent.
#[test]
fn a_flex_column_parent_keeps_the_extent() {
    for (name, mode) in MODES {
        check(name, mode, Shape::FlexColumn);
    }
}

/// Padding lies outside the content extent.
#[test]
fn a_padded_list_keeps_its_extent_and_padding() {
    for (name, mode) in MODES {
        check(name, mode, Shape::Padded);
    }
}
