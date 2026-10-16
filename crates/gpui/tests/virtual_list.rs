//! `VirtualList` driven through a test window with fast mode off: both axes, variable sizes with
//! `gap` and padding, overscan, the cross-axis size, sticky rows, scroll-to-item, the
//! rendered-range listener, and the `ParentViewport` and `SynchronizedOffset` external modes.
//!
//! Every list here tracks a handle its view keeps.
//!
//! `cargo test -p gpux --features test-support --test virtual_list`

use std::{cell::RefCell, collections::BTreeMap, ops::Range, rc::Rc};

use gpux::{
    Along as _, AnyElement, AnyWindowHandle, App, AppContext as _, Axis, Bounds, Context, Div,
    FastMode, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, PlatformInput,
    Point, Render, ScrollDelta, ScrollHandle, ScrollStrategy, ScrollWheelEvent,
    StatefulInteractiveElement as _, StickyAxisState, Styled as _, TestAppContext, TouchPhase,
    VirtualList, VirtualListExternalScrollBehavior, VirtualListScrollHandle, VirtualStickyRow,
    Window, canvas, div, h_virtual_list, point, px, rgb, size, v_virtual_list, virtual_list,
};

/// The list's size along both axes.
const W: f32 = 300.;
const H: f32 = 200.;

/// What a list's rows and listeners saw on the last frame.
#[derive(Default)]
struct Probe {
    /// The ranges the row closure was called with, in order.
    calls: Vec<Range<usize>>,
    /// The ranges `on_rendered_range` received, in order.
    ranges: Vec<Range<usize>>,
    /// Each prepainted row's bounds.
    bounds: BTreeMap<usize, Bounds<Pixels>>,
    /// The sticky state changes, by row.
    sticky: Vec<(usize, StickyAxisState)>,
}

impl Probe {
    fn clear(&mut self) {
        self.calls.clear();
        self.ranges.clear();
        self.bounds.clear();
    }
}

type Shared = Rc<RefCell<Probe>>;

type Build = Rc<dyn Fn(&mut Window, &mut Context<Root>) -> AnyElement>;

struct Root {
    build: Build,
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let build = self.build.clone();
        div().size_full().bg(gpux::white()).child(build(window, cx))
    }
}

/// A row of `extent` along `axis` that records its bounds when it prepaints.
fn row(probe: &Shared, ix: usize, axis: Axis, extent: f32) -> Div {
    let probe = probe.clone();
    let row = match axis {
        Axis::Vertical => div().w_full().h(px(extent)),
        Axis::Horizontal => div().h_full().w(px(extent)),
    };
    row.flex_none().child(
        canvas(
            move |bounds, _, _| {
                probe.borrow_mut().bounds.insert(ix, bounds);
            },
            |_, _, _, _| {},
        )
        .size_full(),
    )
}

/// Builds a list over `extents` whose rows record into `probe`.
fn list(cx: &mut Context<Root>, axis: Axis, extents: &Rc<Vec<f32>>, probe: &Shared) -> VirtualList {
    let sizes = Rc::new(
        extents
            .iter()
            .map(|extent| match axis {
                Axis::Vertical => size(px(W), px(*extent)),
                Axis::Horizontal => size(px(*extent), px(H)),
            })
            .collect::<Vec<_>>(),
    );
    let render = {
        let probe = probe.clone();
        let extents = extents.clone();
        move |_: &mut Root, range: Range<usize>, _: &mut Window, _: &mut Context<Root>| {
            probe.borrow_mut().calls.push(range.clone());
            range
                .map(|ix| row(&probe, ix, axis, extents[ix]))
                .collect::<Vec<_>>()
        }
    };
    let list = match axis {
        Axis::Vertical => v_virtual_list(cx.entity(), "list", sizes, render),
        Axis::Horizontal => h_virtual_list(cx.entity(), "list", sizes, render),
    };
    list.on_rendered_range({
        let probe = probe.clone();
        move |range, _, _| probe.borrow_mut().ranges.push(range)
    })
}

fn open(
    cx: &mut TestAppContext,
    build: impl Fn(&mut Window, &mut Context<Root>) -> AnyElement + 'static,
) -> AnyWindowHandle {
    let build: Build = Rc::new(build);
    let window: AnyWindowHandle = cx.add_window(move |_, _| Root { build }).into();
    with(cx, window, |window, _| window.set_fast_mode(FastMode::OFF));
    window
}

fn with<R>(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    cx.update_window(window, |_, window, cx| f(window, cx))
        .unwrap()
}

/// Clears the probe and draws one frame.
fn draw(cx: &mut TestAppContext, window: AnyWindowHandle, probe: &Shared) {
    probe.borrow_mut().clear();
    with(cx, window, |window, cx| window.draw(cx).clear(cx));
}

/// Turns the wheel at `position` by `delta`, then draws one frame.
fn wheel(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    probe: &Shared,
    position: Point<Pixels>,
    delta: Point<Pixels>,
) {
    with(cx, window, |window, cx| {
        window.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(delta),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx, window, probe);
}

fn centre() -> Point<Pixels> {
    point(px(W / 2.), px(H / 2.))
}

/// The start of each item: the sum of the extents and gaps before it.
fn origins(extents: &[f32], gap: f32) -> Vec<f32> {
    let mut origin = 0.;
    extents
        .iter()
        .map(|extent| {
            let start = origin;
            origin += extent + gap;
            start
        })
        .collect()
}

/// Ten to forty pixels, cycling.
fn varied(count: usize) -> Rc<Vec<f32>> {
    Rc::new((0..count).map(|ix| 10. + (ix % 4) as f32 * 10.).collect())
}

fn assert_rows_placed(
    probe: &Shared,
    axis: Axis,
    extents: &[f32],
    gap: f32,
    start: Point<Pixels>,
    offset: f32,
) {
    let origins = origins(extents, gap);
    let probe = probe.borrow();
    assert!(!probe.bounds.is_empty(), "no row was prepainted");
    for (ix, bounds) in &probe.bounds {
        let along = start.along(axis) + px(origins[*ix] + offset);
        let (origin, extent) = match axis {
            Axis::Vertical => (bounds.origin.y, bounds.size.height),
            Axis::Horizontal => (bounds.origin.x, bounds.size.width),
        };
        assert_eq!(
            origin, along,
            "row {ix} starts at {origin:?}, not {along:?}"
        );
        assert_eq!(extent, px(extents[*ix]), "row {ix} has extent {extent:?}");
        let across = match axis {
            Axis::Vertical => bounds.origin.x,
            Axis::Horizontal => bounds.origin.y,
        };
        assert_eq!(across, start.along(axis.invert()), "row {ix} cross start");
    }
}

/// The rows that intersect `start..end` along the list, with the extent index's two rows of
/// lookahead past the end.
fn expected_range(extents: &[f32], gap: f32, start: f32, end: f32) -> Range<usize> {
    let origins = origins(extents, gap);
    let ends = origins
        .iter()
        .zip(extents.iter().enumerate())
        .map(|(origin, (ix, extent))| {
            origin + extent + if ix + 1 == extents.len() { 0. } else { gap }
        })
        .collect::<Vec<_>>();
    let first = ends.iter().take_while(|e| **e <= start.max(0.)).count();
    let boundary = ends.iter().take_while(|e| **e <= end).count();
    let last = if boundary == extents.len() {
        boundary
    } else {
        (boundary + 2).min(extents.len())
    };
    first..last
}

#[gpux::test]
fn vertical_rows_follow_variable_sizes_gap_and_padding(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let extents = varied(500);
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, extents, handle) = (probe.clone(), extents.clone(), handle.clone());
        move |_, cx| {
            list(cx, Axis::Vertical, &extents, &probe)
                .track_scroll(&handle)
                .gap(px(5.))
                .pt(px(8.))
                .pl(px(3.))
                .w(px(W))
                .h(px(H))
                .into_any_element()
        }
    });
    draw(cx, window, &probe);

    let content = point(px(3.), px(8.));
    assert_rows_placed(&probe, Axis::Vertical, &extents, 5., content, 0.);
    assert_eq!(
        probe.borrow().ranges,
        vec![expected_range(&extents, 5., -8., H - 8.)]
    );

    wheel(cx, window, &probe, centre(), point(px(0.), px(-100.)));
    assert_eq!(handle.offset().y, px(-100.));
    assert_rows_placed(&probe, Axis::Vertical, &extents, 5., content, -100.);
    assert_eq!(
        probe.borrow().ranges,
        vec![expected_range(&extents, 5., 100. - 8., 100. + H - 8.)]
    );
}

#[gpux::test]
fn horizontal_rows_follow_variable_widths(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let extents = varied(500);
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, extents, handle) = (probe.clone(), extents.clone(), handle.clone());
        move |_, cx| {
            list(cx, Axis::Horizontal, &extents, &probe)
                .track_scroll(&handle)
                .gap(px(2.))
                .w(px(W))
                .h(px(H))
                .into_any_element()
        }
    });
    draw(cx, window, &probe);
    assert_rows_placed(&probe, Axis::Horizontal, &extents, 2., Point::default(), 0.);
    assert_eq!(
        probe.borrow().ranges,
        vec![expected_range(&extents, 2., 0., W)]
    );
    for bounds in probe.borrow().bounds.values() {
        assert_eq!(
            bounds.size.height,
            px(H),
            "a column spans the list's height"
        );
    }

    wheel(cx, window, &probe, centre(), point(px(-150.), px(0.)));
    assert_eq!(handle.offset(), point(px(-150.), px(0.)));
    assert_rows_placed(
        &probe,
        Axis::Horizontal,
        &extents,
        2.,
        Point::default(),
        -150.,
    );
    assert_eq!(
        probe.borrow().ranges,
        vec![expected_range(&extents, 2., 150., 150. + W)]
    );
}

#[gpux::test]
fn virtual_list_takes_its_axis(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, handle) = (probe.clone(), handle.clone());
        move |_, cx| {
            let probe = probe.clone();
            virtual_list(
                cx.entity(),
                "axis",
                Axis::Horizontal,
                Rc::new(vec![size(px(50.), px(H)); 100]),
                move |_, range, _, _| {
                    range
                        .map(|ix| row(&probe, ix, Axis::Horizontal, 50.))
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&handle)
            .w(px(W))
            .h(px(H))
            .into_any_element()
        }
    });
    draw(cx, window, &probe);
    let bounds = probe.borrow().bounds.clone();
    assert_eq!(bounds[&0].origin, point(px(0.), px(0.)));
    assert_eq!(bounds[&3].origin, point(px(150.), px(0.)));
    assert_eq!(handle.max_offset().x, px(50. * 100. - W));
}

#[gpux::test]
fn overscan_mounts_extra_rows_on_both_sides(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let extents = Rc::new(vec![20.; 200]);
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, extents, handle) = (probe.clone(), extents, handle);
        move |_, cx| {
            list(cx, Axis::Vertical, &extents, &probe)
                .track_scroll(&handle)
                .overscan(3)
                .w(px(W))
                .h(px(H))
                .into_any_element()
        }
    });
    draw(cx, window, &probe);
    assert_eq!(probe.borrow().ranges, vec![0..15]);

    wheel(cx, window, &probe, centre(), point(px(0.), px(-400.)));
    // Rows 20..30 fill the viewport; two rows of lookahead and three of overscan on each side.
    assert_eq!(probe.borrow().ranges, vec![17..35]);
    let probe = probe.borrow();
    assert_eq!(
        probe.bounds.keys().copied().collect::<Vec<_>>(),
        (17..35).collect::<Vec<_>>()
    );
    assert_eq!(probe.bounds[&17].origin.y, px(-60.));
}

#[gpux::test]
fn the_first_row_measures_the_cross_axis(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, handle) = (probe.clone(), handle.clone());
        move |_, cx| {
            let probe = probe.clone();
            v_virtual_list(
                cx.entity(),
                "measured",
                Rc::new(vec![size(px(W), px(20.)); 200]),
                move |_, range, _, _| {
                    probe.borrow_mut().calls.push(range.clone());
                    // Row 0 is wider than the list.
                    range
                        .map(|ix| div().h(px(20.)).w(px(if ix == 0 { 450. } else { 100. })))
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&handle)
            .w(px(W))
            .h(px(H))
            .into_any_element()
        }
    });
    draw(cx, window, &probe);
    wheel(cx, window, &probe, centre(), point(px(0.), px(-400.)));
    // The layout renders row 0 alone to measure it, then prepaint renders the rows it shows.
    assert_eq!(probe.borrow().calls, vec![0..1, 20..32]);
    assert_eq!(handle.max_offset().x, px(450. - W));
}

#[gpux::test]
fn a_fixed_cross_axis_size_renders_no_row_to_measure(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let extents = Rc::new(vec![20.; 200]);
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, extents, handle) = (probe.clone(), extents, handle.clone());
        move |_, cx| {
            list(cx, Axis::Vertical, &extents, &probe)
                .track_scroll(&handle)
                .fixed_cross_axis_size(px(500.))
                .w(px(W))
                .h(px(H))
                .into_any_element()
        }
    });
    draw(cx, window, &probe);
    wheel(cx, window, &probe, centre(), point(px(0.), px(-400.)));
    assert_eq!(probe.borrow().calls, vec![20..32]);
    assert_eq!(handle.max_offset().x, px(500. - W));

    wheel(cx, window, &probe, centre(), point(px(-80.), px(0.)));
    assert_eq!(handle.offset(), point(px(-80.), px(-400.)));
    assert_eq!(probe.borrow().bounds[&20].origin, point(px(-80.), px(0.)));
}

/// Rows 0 and 10 are level-0 headers and row 3 a level-1 header below row 0.
fn sticky_list(
    cx: &mut Context<Root>,
    probe: &Shared,
    handle: &VirtualListScrollHandle,
) -> VirtualList {
    let extents = Rc::new(vec![20.; 200]);
    let changes = |ix: usize| {
        let probe = probe.clone();
        move |event: &gpux::StickyStateChangedEvent, _: &mut Window, _: &mut App| {
            probe.borrow_mut().sticky.push((ix, event.current.y));
        }
    };
    list(cx, Axis::Vertical, &extents, probe)
        .track_scroll(handle)
        .sticky_rows([
            (
                0,
                VirtualStickyRow::top()
                    .level(0)
                    .when_stuck(|style| style.bg(rgb(0xff0000)))
                    .on_sticky_state_changed(changes(0)),
            ),
            (3, VirtualStickyRow::top().level(1)),
            (10, VirtualStickyRow::top().level(0)),
        ])
        .w(px(W))
        .h(px(H))
}

#[gpux::test]
fn sticky_rows_stack_by_level_and_push_out(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, handle) = (probe.clone(), handle);
        move |_, cx| sticky_list(cx, &probe, &handle).into_any_element()
    });
    draw(cx, window, &probe);
    assert_eq!(probe.borrow().bounds[&0].origin.y, px(0.));
    assert_eq!(probe.borrow().bounds[&3].origin.y, px(60.));

    // Row 5 is at the top: row 0 sticks at the top and row 3 below it.
    wheel(cx, window, &probe, centre(), point(px(0.), px(-100.)));
    {
        let probe = probe.borrow();
        assert_eq!(probe.bounds[&0].origin.y, px(0.));
        assert_eq!(probe.bounds[&3].origin.y, px(20.));
        assert_eq!(probe.bounds[&5].origin.y, px(0.));
        assert_eq!(
            probe.ranges,
            vec![5..17],
            "sticky rows do not widen the range"
        );
    }

    // Row 10 starts 10 px below the top and pushes the whole level-0 chain up by 30 px, so
    // that row 3's bottom meets row 10's top.
    wheel(cx, window, &probe, centre(), point(px(0.), px(-90.)));
    {
        let probe = probe.borrow();
        assert_eq!(probe.bounds[&10].origin.y, px(10.));
        assert_eq!(probe.bounds[&0].origin.y, px(-30.));
        assert_eq!(probe.bounds[&3].origin.y, px(-10.));
    }

    // Past row 10, it replaces row 0 and row 3 leaves the stack.
    wheel(cx, window, &probe, centre(), point(px(0.), px(-100.)));
    {
        let probe = probe.borrow();
        assert_eq!(probe.bounds[&10].origin.y, px(0.));
        assert!(!probe.bounds.contains_key(&0));
        assert!(!probe.bounds.contains_key(&3));
    }
}

#[gpux::test]
fn a_stuck_row_takes_its_stuck_style_and_reports_its_state(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, handle) = (probe.clone(), handle);
        move |_, cx| sticky_list(cx, &probe, &handle).into_any_element()
    });
    // Rows paint nothing of their own, so the frame shows a quad besides the window's
    // background only while row 0's stuck style gives it one.
    let shows_red = |cx: &mut TestAppContext| {
        with(cx, window, |window, _| {
            window
                .describe_rendered_frame()
                .iter()
                .filter(|line| line.starts_with("primitive") && line.contains("Quad {"))
                .count()
                == 2
        })
    };
    draw(cx, window, &probe);
    assert!(!shows_red(cx), "row 0 is not stuck at the top");

    wheel(cx, window, &probe, centre(), point(px(0.), px(-100.)));
    assert!(shows_red(cx), "row 0 is stuck");
    assert_eq!(
        probe.borrow().sticky.last(),
        Some(&(0, StickyAxisState::StuckToStart))
    );

    wheel(cx, window, &probe, centre(), point(px(0.), px(100.)));
    assert!(!shows_red(cx), "row 0 is back in place");
    assert_ne!(
        probe.borrow().sticky.last(),
        Some(&(0, StickyAxisState::StuckToStart))
    );
}

#[gpux::test]
fn scroll_to_item_applies_each_strategy(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let extents = Rc::new(vec![20.; 100]);
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, extents, handle) = (probe.clone(), extents, handle.clone());
        move |_, cx| {
            list(cx, Axis::Vertical, &extents, &probe)
                .track_scroll(&handle)
                .w(px(W))
                .h(px(H))
                .into_any_element()
        }
    });
    draw(cx, window, &probe);

    let scroll_to = |cx: &mut TestAppContext, ix: usize, strategy: ScrollStrategy| {
        handle.scroll_to_item(ix, strategy);
        draw(cx, window, &probe);
        draw(cx, window, &probe);
        handle.offset().y
    };
    assert_eq!(scroll_to(cx, 50, ScrollStrategy::Top), px(-1000.));
    assert_eq!(
        scroll_to(cx, 50, ScrollStrategy::Center),
        px(-(1010. - H / 2.))
    );
    assert_eq!(scroll_to(cx, 50, ScrollStrategy::Bottom), px(-(1020. - H)));
    // Row 48 is shown, so the nearest strategy leaves the offset alone.
    assert_eq!(scroll_to(cx, 48, ScrollStrategy::Nearest), px(-(1020. - H)));
    assert_eq!(scroll_to(cx, 10, ScrollStrategy::Nearest), px(-200.));
    assert_eq!(scroll_to(cx, 40, ScrollStrategy::Nearest), px(-(820. - H)));
    // The last row: the offset stops at the end of the content.
    handle.scroll_to_bottom();
    draw(cx, window, &probe);
    assert_eq!(handle.offset().y, px(-(2000. - H)));
    assert!(probe.borrow().bounds.contains_key(&99));
}

#[gpux::test]
fn the_rendered_range_listener_runs_on_every_prepaint(cx: &mut TestAppContext) {
    let probe = Shared::default();
    let count = Rc::new(RefCell::new(100usize));
    let handle = VirtualListScrollHandle::new();
    let window = open(cx, {
        let (probe, count, handle) = (probe.clone(), count.clone(), handle);
        move |_, cx| {
            let extents = Rc::new(vec![20.; *count.borrow()]);
            list(cx, Axis::Vertical, &extents, &probe)
                .track_scroll(&handle)
                .w(px(W))
                .h(px(H))
                .into_any_element()
        }
    });
    draw(cx, window, &probe);
    assert_eq!(probe.borrow().ranges, vec![0..12]);
    draw(cx, window, &probe);
    assert_eq!(
        probe.borrow().ranges,
        vec![0..12],
        "an unchanged frame reports again"
    );
    wheel(cx, window, &probe, centre(), point(px(0.), px(-30.)));
    assert_eq!(probe.borrow().ranges, vec![1..13]);

    *count.borrow_mut() = 0;
    with(cx, window, |window, _| window.refresh());
    draw(cx, window, &probe);
    assert_eq!(
        probe.borrow().ranges,
        vec![0..0],
        "an empty list reports an empty range"
    );
    assert!(probe.borrow().calls.is_empty());
}

struct ParentRoot {
    parent: ScrollHandle,
    list: VirtualListScrollHandle,
    probe: Shared,
    behavior: VirtualListExternalScrollBehavior,
}

impl Render for ParentRoot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let probe = self.probe.clone();
        let list = v_virtual_list(
            cx.entity(),
            "external",
            Rc::new(vec![size(px(W), px(20.)); 1000]),
            move |_, range, _, _| {
                probe.borrow_mut().calls.push(range.clone());
                range
                    .map(|ix| row(&probe, ix, Axis::Vertical, 20.))
                    .collect::<Vec<_>>()
            },
        )
        .track_external_scroll(&self.list, self.behavior)
        .on_rendered_range({
            let probe = self.probe.clone();
            move |range, _, _| probe.borrow_mut().ranges.push(range)
        });
        let body = match self.behavior {
            VirtualListExternalScrollBehavior::ParentViewport => div()
                .id("parent")
                .w(px(W))
                .h(px(H))
                .overflow_y_scroll()
                .track_scroll(&self.parent)
                .child(list.w(px(W)))
                .into_any_element(),
            VirtualListExternalScrollBehavior::SynchronizedOffset => div()
                .flex()
                .child(
                    div()
                        .id("driver")
                        .w(px(W))
                        .h(px(H))
                        .overflow_y_scroll()
                        .track_scroll(&self.parent)
                        .child(div().w(px(W)).h(px(20. * 1000.))),
                )
                .child(list.w(px(W)).h(px(H)))
                .into_any_element(),
        };
        div().size_full().bg(gpux::white()).child(body)
    }
}

fn open_external(
    cx: &mut TestAppContext,
    behavior: VirtualListExternalScrollBehavior,
) -> (
    AnyWindowHandle,
    ScrollHandle,
    VirtualListScrollHandle,
    Shared,
) {
    let parent = ScrollHandle::new();
    let list = VirtualListScrollHandle::from(parent.clone());
    let probe = Shared::default();
    let window: AnyWindowHandle = cx
        .add_window({
            let (parent, list, probe) = (parent.clone(), list.clone(), probe.clone());
            move |_, _| ParentRoot {
                parent,
                list,
                probe,
                behavior,
            }
        })
        .into();
    with(cx, window, |window, _| window.set_fast_mode(FastMode::OFF));
    draw(cx, window, &probe);
    (window, parent, list, probe)
}

#[gpux::test]
fn a_parent_viewport_list_takes_its_full_extent_and_follows_the_parent(cx: &mut TestAppContext) {
    let (window, parent, list, probe) =
        open_external(cx, VirtualListExternalScrollBehavior::ParentViewport);
    assert_eq!(parent.max_offset().y, px(20. * 1000. - H));
    assert_eq!(probe.borrow().ranges, vec![0..12]);
    assert_eq!(list.parent_viewport_primary_size(), Some(px(H)));
    assert_eq!(list.parent_viewport_scroll_offset(), Some(px(0.)));

    wheel(cx, window, &probe, centre(), point(px(0.), px(-400.)));
    assert_eq!(parent.offset().y, px(-400.));
    assert_eq!(probe.borrow().ranges, vec![20..32]);
    assert_eq!(probe.borrow().bounds[&20].origin.y, px(0.));
    assert_eq!(list.parent_viewport_scroll_offset(), Some(px(400.)));
    assert_eq!(
        list.parent_viewport_max_scroll_offset(),
        Some(px(20. * 1000. - H))
    );

    // A list-local offset request moves the parent.
    list.set_parent_viewport_scroll_offset(px(1000.));
    draw(cx, window, &probe);
    draw(cx, window, &probe);
    assert_eq!(parent.offset().y, px(-1000.));
    assert_eq!(probe.borrow().ranges, vec![50..62]);
}

#[gpux::test]
fn a_synchronized_offset_list_applies_the_driver_offset(cx: &mut TestAppContext) {
    let (window, parent, _list, probe) =
        open_external(cx, VirtualListExternalScrollBehavior::SynchronizedOffset);
    assert_eq!(probe.borrow().ranges, vec![0..12]);
    assert_eq!(probe.borrow().bounds[&0].origin, point(px(W), px(0.)));

    wheel(cx, window, &probe, centre(), point(px(0.), px(-400.)));
    assert_eq!(parent.offset().y, px(-400.));
    assert_eq!(probe.borrow().ranges, vec![20..32]);
    assert_eq!(probe.borrow().bounds[&20].origin, point(px(W), px(0.)));
    assert_eq!(probe.borrow().bounds[&21].origin, point(px(W), px(20.)));
}
