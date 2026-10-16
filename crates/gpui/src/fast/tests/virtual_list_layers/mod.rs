//! Scroll layers over `virtual_list`: a vertical list that is its own scroll
//! container and has no sticky rows composites from a list layer as
//! `uniform_list` does, and every other kind draws as without layers.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::virtual_list_layers::
//! ```

mod changes;
mod horizontal;
mod horizontal_oracle;
mod ineligible;
mod oracle;
mod parent_viewport;
mod range;
mod rows;

use crate::fast::tests::layer_support::{add_window, draw, with_window};
use crate::{
    AnyWindowHandle, AppContext as _, Axis, Context, FastStats, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, ScrollDelta, ScrollHandle, ScrollWheelEvent,
    StatefulInteractiveElement as _, Styled as _, TestAppContext, TouchPhase,
    VirtualListExternalScrollBehavior, VirtualListScrollHandle, VirtualStickyRow, Window, div,
    point, px, size,
};
use std::{cell::RefCell, collections::BTreeMap, ops::Range, rc::Rc};

/// How wide and tall the list is, in pixels.
pub(super) const W: f32 = 200.;
pub(super) const H: f32 = 100.;

/// The kind of list a [`Page`] shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// A vertical `virtual_list` scrolling itself: the kind with a layer.
    Virtual,
    /// A `uniform_list` of the same rows, which must all be as tall.
    Uniform,
    /// A horizontal `virtual_list` scrolling itself.
    Horizontal,
    /// A vertical `virtual_list` with sticky rows.
    Sticky,
    /// A `virtual_list` in the flow of a scrolling `div`.
    ParentViewport,
    /// A `virtual_list` beside a scrolling `div` whose offset it applies.
    Synchronized,
}

/// What the rows of a page's list were rendered with, and what its range
/// listener received.
#[derive(Default)]
pub(super) struct Log {
    /// The ranges the row closure was called with.
    pub(super) calls: Vec<Range<usize>>,
    /// The ranges `on_rendered_range` received.
    pub(super) ranges: Vec<Range<usize>>,
}

impl Log {
    /// How many rows the closure rendered.
    pub(super) fn rows_rendered(&self) -> usize {
        self.calls.iter().map(|range| range.len()).sum()
    }
}

pub(super) type SharedLog = Rc<RefCell<Log>>;

/// A white page holding one list of rows `extents` tall (wide, for a
/// horizontal list), each a coloured block.
pub(super) struct Page {
    pub(super) kind: Kind,
    pub(super) extents: Rc<Vec<f32>>,
    pub(super) gap: f32,
    pub(super) padding: f32,
    pub(super) overscan: usize,
    /// Whether each row has an id, a hover style and a click listener that
    /// tints the rows.
    pub(super) interactive: bool,
    /// Whether each row is tinted, which a test toggles to change the rows.
    pub(super) tint: bool,
    pub(super) handle: VirtualListScrollHandle,
    pub(super) parent: ScrollHandle,
    /// The list's handle over `parent`, for a list scrolled by it.
    pub(super) parent_list: VirtualListScrollHandle,
    pub(super) log: SharedLog,
    /// A view beside the list, notified alone to draw the page from last
    /// frame.
    pub(super) beside: Option<crate::Entity<Beside>>,
}

/// A small block beside the list whose height a notification toggles.
pub(super) struct Beside {
    pub(super) count: usize,
}

impl Render for Beside {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(20.))
            .h(px(10. + (self.count % 2) as f32 * 5.))
            .bg(crate::black())
    }
}

impl Page {
    pub(super) fn new(kind: Kind, extents: Vec<f32>) -> Self {
        let parent = ScrollHandle::new();
        Page {
            parent_list: VirtualListScrollHandle::from(parent.clone()),
            kind,
            extents: Rc::new(extents),
            gap: 0.,
            padding: 0.,
            overscan: 0,
            interactive: false,
            tint: false,
            handle: VirtualListScrollHandle::new(),
            parent,
            log: SharedLog::default(),
            beside: None,
        }
    }
}

/// A row's colour.
pub(super) fn row_color(row: usize, tint: bool) -> Hsla {
    let hue = (row % 97) as f32 / 97.;
    crate::hsla(hue, if tint { 0.9 } else { 0.5 }, 0.5, 1.)
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let extents = self.extents.clone();
        let tint = self.tint;
        let horizontal = self.kind == Kind::Horizontal;
        let interactive = self.interactive;
        let log = self.log.clone();
        let page = cx.entity().downgrade();
        let rows = move |range: Range<usize>| {
            log.borrow_mut().calls.push(range.clone());
            range
                .map(|row| {
                    let block = if horizontal {
                        div().h_full().w(px(extents[row]))
                    } else {
                        div().w_full().h(px(extents[row]))
                    };
                    let block = block.flex_none().bg(row_color(row, tint));
                    if !interactive {
                        return block.into_any_element();
                    }
                    let page = page.clone();
                    block
                        .id(("row", row))
                        .hover(|style| style.opacity(0.5))
                        .on_click(move |_, _, cx| {
                            page.update(cx, |page, cx| {
                                page.tint = !page.tint;
                                cx.notify();
                            })
                            .ok();
                        })
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        };
        let sizes = Rc::new(
            self.extents
                .iter()
                .map(|extent| {
                    if horizontal {
                        size(px(*extent), px(H))
                    } else {
                        size(px(W), px(*extent))
                    }
                })
                .collect::<Vec<_>>(),
        );
        let ranges = self.log.clone();
        let virtual_list = |cx: &mut Context<Self>, axis: Axis| {
            crate::virtual_list(cx.entity(), "list", axis, sizes.clone(), {
                let rows = rows.clone();
                move |_, range, _, _| rows(range)
            })
            .on_rendered_range(move |range, _, _| ranges.borrow_mut().ranges.push(range))
            .overscan(self.overscan)
            .gap(px(self.gap))
            .p(px(self.padding))
        };
        let list = match self.kind {
            Kind::Uniform => crate::uniform_list("list", self.extents.len(), {
                let rows = rows.clone();
                move |range, _, _| rows(range)
            })
            .p(px(self.padding))
            .w(px(W))
            .h(px(H))
            .into_any_element(),
            Kind::Virtual => virtual_list(cx, Axis::Vertical)
                .track_scroll(&self.handle)
                .w(px(W))
                .h(px(H))
                .into_any_element(),
            Kind::Horizontal => virtual_list(cx, Axis::Horizontal)
                .track_scroll(&self.handle)
                .w(px(W))
                .h(px(H))
                .into_any_element(),
            Kind::Sticky => virtual_list(cx, Axis::Vertical)
                .track_scroll(&self.handle)
                .sticky_rows(
                    (0..self.extents.len())
                        .step_by(25)
                        .map(|row| (row, VirtualStickyRow::top())),
                )
                .w(px(W))
                .h(px(H))
                .into_any_element(),
            Kind::ParentViewport => div()
                .id("parent")
                .w(px(W))
                .h(px(H))
                .overflow_y_scroll()
                .track_scroll(&self.parent)
                .child(
                    virtual_list(cx, Axis::Vertical)
                        .track_external_scroll(
                            &self.parent_list,
                            VirtualListExternalScrollBehavior::ParentViewport,
                        )
                        .w(px(W)),
                )
                .into_any_element(),
            Kind::Synchronized => {
                let extent: f32 = self.extents.iter().sum::<f32>()
                    + self.gap * self.extents.len().saturating_sub(1) as f32
                    + 2. * self.padding;
                div()
                    .flex()
                    .child(
                        div()
                            .id("driver")
                            .w(px(W / 2.))
                            .h(px(H))
                            .overflow_y_scroll()
                            .track_scroll(&self.parent)
                            .child(div().w(px(W / 2.)).h(px(extent)).bg(crate::black())),
                    )
                    .child(
                        virtual_list(cx, Axis::Vertical)
                            .track_external_scroll(
                                &self.parent_list,
                                VirtualListExternalScrollBehavior::SynchronizedOffset,
                            )
                            .w(px(W / 2.))
                            .h(px(H)),
                    )
                    .into_any_element()
            }
        };
        let beside = self
            .beside
            .get_or_insert_with(|| cx.new(|_| Beside { count: 0 }))
            .clone();
        div()
            .size_full()
            .bg(crate::white())
            .flex()
            .child(list)
            .child(beside)
    }
}

/// Opens `page` in layers mode and draws it twice; `reference` opens it as
/// what a window with layers is compared with instead.
pub(super) fn open(cx: &mut TestAppContext, page: Page, reference: bool) -> AnyWindowHandle {
    open_at(cx, page, reference, 1.)
}

/// [`open`] at `scale_factor`.
pub(super) fn open_at(
    cx: &mut TestAppContext,
    page: Page,
    reference: bool,
    scale_factor: f32,
) -> AnyWindowHandle {
    let window: AnyWindowHandle = add_window(cx, move |_, _| page).into();
    cx.test_window(window)
        .simulate_scale_factor_change(scale_factor);
    if reference {
        with_window(cx, window, |window, _| {
            crate::fast::tests::layer_support::make_reference(window)
        });
    }
    draw(cx, window);
    draw(cx, window);
    window
}

/// Turns the wheel by `delta` over the list's top left, and draws the frame
/// that follows, unless the wheel drew it.
pub(super) fn wheel_by(cx: &mut TestAppContext, window: AnyWindowHandle, dx: f32, dy: f32) {
    let frame = with_window(cx, window, |window, _| window.fast_layers.frame);
    with_window(cx, window, |window, cx| {
        window.dispatch_event(
            crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(20.), px(20.)),
                delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    });
    if with_window(cx, window, |window, _| window.fast_layers.frame) == frame {
        draw(cx, window);
    }
}

pub(super) fn wheel(cx: &mut TestAppContext, window: AnyWindowHandle, dy: f32) {
    wheel_by(cx, window, 0., dy);
}

pub(super) fn stats(cx: &mut TestAppContext, window: AnyWindowHandle) -> FastStats {
    with_window(cx, window, |window, _| window.fast_stats())
}

/// What one frame cost: the counters it added and the rows the closure
/// rendered for it.
pub(super) struct FrameWork {
    pub(super) counters: BTreeMap<String, u64>,
    pub(super) closure_rows: usize,
    pub(super) calls: Vec<Range<usize>>,
}

impl FrameWork {
    pub(super) fn counter(&self, name: &str) -> u64 {
        self.counters.get(name).copied().unwrap_or(0)
    }
}

/// The counters `after` added to `before`.
pub(super) fn difference(after: &FastStats, before: &FastStats) -> BTreeMap<String, u64> {
    let before: BTreeMap<String, u64> = before.counters().into_iter().collect();
    after
        .counters()
        .into_iter()
        .map(|(name, value)| {
            let was = before.get(&name).copied().unwrap_or(0);
            (name, value - was)
        })
        .collect()
}

/// Turns the wheel by `dy` on each of `frames` frames after four turns to
/// promote the list, returning each frame's work.
pub(super) fn scroll_frames(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    log: &SharedLog,
    frames: usize,
    dy: f32,
) -> Vec<FrameWork> {
    for _ in 0..4 {
        wheel(cx, window, dy);
    }
    (0..frames)
        .map(|_| {
            let before = stats(cx, window);
            log.borrow_mut().calls.clear();
            wheel(cx, window, dy);
            let after = stats(cx, window);
            let calls = log.borrow().calls.clone();
            FrameWork {
                counters: difference(&after, &before),
                closure_rows: log.borrow().rows_rendered(),
                calls,
            }
        })
        .collect()
}

/// The mean of `counter` over `work`.
pub(super) fn mean(work: &[FrameWork], counter: &str) -> f64 {
    work.iter().map(|frame| frame.counter(counter)).sum::<u64>() as f64 / work.len() as f64
}

/// Ten thousand rows of varying, whole-pixel heights from 16 to 28 px.
pub(super) fn varied_heights() -> Vec<f32> {
    (0..10_000).map(|row| 16. + (row % 5) as f32 * 3.).collect()
}

/// What a frame drew, as [`Window::painted_primitives`] tells it (a
/// layer's tiles expanded into their content), and the ranges the list's
/// range listener received while it was drawn.
#[derive(Debug, PartialEq)]
pub(super) struct Shown {
    pub(super) primitives: Vec<String>,
    pub(super) ranges: Vec<Range<usize>>,
    /// What a list in its parent's flow published of the parent's viewport:
    /// its size, the list-local offset and the largest one.
    pub(super) metrics: (Option<Pixels>, Option<Pixels>, Option<Pixels>),
}

/// Opens two windows showing `page()`, one with layers and the reference
/// without, turns the wheel by each of `deltas` in both, and checks after
/// each that they drew the same primitives and published the same ranges.
/// Returns the layered window's counters over the turns.
pub(super) fn compare_with_layers_off(
    cx: &mut TestAppContext,
    page: impl Fn() -> Page,
    deltas: &[(f32, f32)],
    label: &str,
) -> BTreeMap<String, u64> {
    compare(cx, page, deltas, label, true)
}

/// [`compare_with_layers_off`], comparing the published ranges only when
/// `ranges`.
pub(super) fn compare(
    cx: &mut TestAppContext,
    page: impl Fn() -> Page,
    deltas: &[(f32, f32)],
    label: &str,
    ranges: bool,
) -> BTreeMap<String, u64> {
    let steps: Vec<Step> = deltas
        .iter()
        .map(|(dx, dy)| Step::Wheel(*dx, *dy))
        .collect();
    compare_steps(cx, page, &steps, 1., label, ranges)
}

/// One step of a script both windows of a comparison run.
#[derive(Clone, Copy)]
pub(super) enum Step {
    /// A wheel turn over the list.
    Wheel(f32, f32),
    /// A change of the page, which notifies its view.
    Edit(fn(&mut Page)),
    /// A programmatic scroll to an item.
    ScrollTo(usize, crate::ScrollStrategy),
    /// A notification of the view beside the list alone.
    NotifyBeside,
}

impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Step::Wheel(dx, dy) => write!(f, "wheel ({dx}, {dy})"),
            Step::Edit(_) => write!(f, "edit"),
            Step::ScrollTo(ix, strategy) => write!(f, "scroll to {ix} ({strategy:?})"),
            Step::NotifyBeside => write!(f, "notify beside"),
        }
    }
}

/// Runs `step` in `window`, then draws the frame that follows.
fn run_step(cx: &mut TestAppContext, window: AnyWindowHandle, step: Step) {
    match step {
        Step::Wheel(dx, dy) => wheel_by(cx, window, dx, dy),
        Step::Edit(edit) => {
            cx.update_window(window, |root, _, cx| {
                root.downcast::<Page>().unwrap().update(cx, |page, cx| {
                    edit(page);
                    cx.notify();
                })
            })
            .unwrap();
            draw(cx, window);
        }
        Step::NotifyBeside => {
            notify_beside(cx, window);
            draw(cx, window);
        }
        Step::ScrollTo(ix, strategy) => {
            let handle = cx
                .update_window(window, |root, _, cx| {
                    root.downcast::<Page>().unwrap().read(cx).handle.clone()
                })
                .unwrap();
            handle.scroll_to_item(ix, strategy);
            with_window(cx, window, |window, _| window.refresh());
            draw(cx, window);
            draw(cx, window);
        }
    }
}

/// Notifies the view beside the list, which leaves the page as it was.
pub(super) fn notify_beside(cx: &mut TestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |root, _, cx| {
        let beside = root.downcast::<Page>().unwrap().read(cx).beside.clone();
        if let Some(beside) = beside {
            beside.update(cx, |beside, cx| {
                beside.count += 1;
                cx.notify();
            });
        }
    })
    .unwrap();
}

/// [`compare`] over a script of `steps`, at `scale_factor`.
pub(super) fn compare_steps(
    cx: &mut TestAppContext,
    page: impl Fn() -> Page,
    steps: &[Step],
    scale_factor: f32,
    label: &str,
    ranges: bool,
) -> BTreeMap<String, u64> {
    let layered_page = page();
    let layered_log = layered_page.log.clone();
    let reference_page = page();
    let reference_log = reference_page.log.clone();
    let layered = open_at(cx, layered_page, false, scale_factor);
    let reference = open_at(cx, reference_page, true, scale_factor);
    let before = stats(cx, layered);
    let mut published_frames = 0;
    let shown = |cx: &mut TestAppContext, window: AnyWindowHandle, log: &SharedLog| {
        let handle = cx
            .update_window(window, |root, _, cx| {
                root.downcast::<Page>()
                    .unwrap()
                    .read(cx)
                    .parent_list
                    .clone()
            })
            .unwrap();
        Shown {
            primitives: with_window(cx, window, |window, _| window.painted_primitives()),
            ranges: std::mem::take(&mut log.borrow_mut().ranges),
            metrics: (
                handle.parent_viewport_primary_size(),
                handle.parent_viewport_scroll_offset(),
                handle.parent_viewport_max_scroll_offset(),
            ),
        }
    };
    // A frame that keeps the view holding the list does not draw the list,
    // and its listener does not run: what it published last stands. The
    // reference draws every frame.
    let mut published = layered_log.borrow().ranges.last().cloned();
    for (frame, step) in steps.iter().copied().enumerate() {
        layered_log.borrow_mut().ranges.clear();
        reference_log.borrow_mut().ranges.clear();
        let step_before = stats(cx, layered);
        run_step(cx, layered, step);
        run_step(cx, reference, step);
        let step_after = stats(cx, layered);
        let actual = shown(cx, layered, &layered_log);
        let expected = shown(cx, reference, &reference_log);
        // A programmatic scroll draws two frames, one to take the request.
        let draws = if matches!(step, Step::ScrollTo(..)) {
            2
        } else {
            1
        };
        assert!(
            actual.ranges.len() <= draws,
            "{label}: frame {frame} published {:?}",
            actual.ranges
        );
        // A frame shown from the list's layer draws the list: its listener
        // runs once, composited or painted afresh.
        let from_layer = step_after.layers_composited + step_after.layers_repainted
            > step_before.layers_composited + step_before.layers_repainted;
        if from_layer && matches!(step, Step::Wheel(..)) && ranges {
            assert_eq!(
                actual.ranges.len(),
                1,
                "{label}: frame {frame}, after {step:?}, shown from the layer, published {:?}",
                actual.ranges
            );
            published_frames += 1;
        }
        if let Some(range) = actual.ranges.last() {
            published = Some(range.clone());
        }
        assert!(
            !ranges || published.as_ref() == expected.ranges.last(),
            "{label}: frame {frame}, after {step:?}: the range last published, {published:?}, \
             is not {:?}",
            expected.ranges.last()
        );
        assert_eq!(
            published.is_some(),
            expected.ranges.last().is_some(),
            "{label}: frame {frame}, after {step:?}: the range last published"
        );
        assert_eq!(
            actual.metrics, expected.metrics,
            "{label}: frame {frame}, after {step:?}: the parent viewport's metrics"
        );
        if actual.primitives != expected.primitives {
            let difference =
                crate::first_frame_difference(&actual.primitives, &expected.primitives)
                    .unwrap_or_default();
            panic!("{label}: frame {frame}, after {step:?}: {difference}");
        }
    }
    let mut counters = difference(&stats(cx, layered), &before);
    counters.insert("published_from_layer".into(), published_frames);
    counters
}

/// A pseudo-random history of `count` vertical wheel turns from `seed`,
/// whole and fractional, mostly down.
pub(super) fn wheel_history(seed: u32, count: usize) -> Vec<(f32, f32)> {
    const DELTAS: [f32; 8] = [-15., -15., -40., 25., -0.5, -7.25, -90., 12.5];
    let mut state: u32 = 0x2545_f491 ^ seed.wrapping_mul(0x9e37_79b9);
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (0., DELTAS[((state >> 16) as usize) % DELTAS.len()])
        })
        .collect()
}
