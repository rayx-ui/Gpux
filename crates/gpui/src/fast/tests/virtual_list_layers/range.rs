//! The `on_rendered_range` listener of a layered `virtual_list` runs on
//! every frame the list is drawn, composited frames included, with the
//! mounted, clamped, overscanned range the frame drawn without layers
//! publishes.

use super::{Kind, Page, compare_with_layers_off, varied_heights, wheel_history};
use crate::TestAppContext;

/// The list composites on most frames of a long history; on each, it
/// publishes once, the range a window without layers publishes.
#[crate::test]
fn a_composited_frame_publishes_the_range_drawn_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_with_layers_off(
        cx,
        || Page::new(Kind::Virtual, varied_heights()),
        &wheel_history(1, 80),
        "varied heights",
    );
    assert!(
        counters["layers_composited"] >= 60,
        "the history composites: {counters:?}"
    );
    assert!(
        counters["published_from_layer"] >= 60,
        "the listener ran on the frames shown from the layer: {counters:?}"
    );
}

/// With overscan, padding and a gap the range stays the one drawn without
/// layers, frame after frame.
#[crate::test]
fn overscan_padding_and_gap_publish_the_range_drawn_without_layers(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let counters = compare_with_layers_off(
        cx,
        || {
            let mut page = Page::new(Kind::Virtual, varied_heights());
            page.gap = 3.;
            page.padding = 6.;
            page.overscan = 2;
            page
        },
        &wheel_history(2, 60),
        "overscan, padding and gap",
    );
    assert!(counters["layers_composited"] >= 40, "{counters:?}");
    assert!(counters["published_from_layer"] >= 40, "{counters:?}");
}

/// Sticky rows keep a list off its layer, and the rows stuck at the top,
/// rendered outside the mounted range, never widen it.
#[crate::test]
fn stuck_rows_never_widen_the_range(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let page = || Page::new(Kind::Sticky, vec![20.; 2_000]);
    {
        let page = page();
        let log = page.log.clone();
        let window = super::open(cx, page, false);
        for _ in 0..8 {
            log.borrow_mut().ranges.clear();
            super::wheel(cx, window, -130.);
            let ranges = log.borrow().ranges.clone();
            assert_eq!(ranges.len(), 1, "one range a frame: {ranges:?}");
            assert!(
                !ranges[0].contains(&0),
                "the stuck row 0 is not part of {ranges:?}"
            );
        }
    }
    compare_with_layers_off(cx, page, &wheel_history(3, 40), "sticky rows");
}

/// A holder view recording the range its list publishes in itself, and an
/// outer view that reads the holder as it renders, as the view holding a
/// wrapper around a list does; with `shows`, the outer view also shows the
/// first row of the range, which it takes from an event the holder emits.
mod recorded {
    use crate::fast::tests::layer_support::{add_window, draw, with_window};
    use crate::{
        AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Render,
        Styled as _, Subscription, TestAppContext, VirtualListScrollHandle, Window, div, px, size,
        v_virtual_list,
    };
    use std::{cell::Cell, ops::Range, rc::Rc};

    pub(super) struct Holder {
        handle: VirtualListScrollHandle,
        pub(super) range: Range<usize>,
    }

    /// The range the holder's list published changed.
    pub(super) struct RangeChanged(Range<usize>);

    impl EventEmitter<RangeChanged> for Holder {}

    impl Render for Holder {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let holder = cx.entity();
            v_virtual_list(
                cx.entity(),
                "recorded",
                Rc::new(vec![size(px(200.), px(20.)); 2_000]),
                |_, range, _, _| {
                    range
                        .map(|row| {
                            div()
                                .w_full()
                                .h(px(20.))
                                .bg(super::super::row_color(row, false))
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.handle)
            .on_rendered_range(move |range, _, cx| {
                holder.update(cx, |holder, cx| {
                    if holder.range != range {
                        holder.range = range.clone();
                        cx.emit(RangeChanged(range));
                    }
                });
            })
            .w(px(200.))
            .h(px(100.))
        }
    }

    pub(super) struct Outer {
        pub(super) holder: Entity<Holder>,
        pub(super) renders: Rc<Cell<usize>>,
        /// The first row the holder's last event told of, shown as a bar.
        shown: Option<usize>,
        _subscription: Option<Subscription>,
    }

    impl Render for Outer {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            // Reads the holder, as a wrapper around a list reads its state.
            let _ = self.holder.read(cx).range.len();
            div()
                .size_full()
                .bg(crate::white())
                .flex()
                .child(self.holder.clone())
                .children(
                    self.shown
                        .map(|first| div().w(px(1. + first as f32)).h(px(4.)).bg(crate::black())),
                )
        }
    }

    pub(super) fn open(
        cx: &mut TestAppContext,
        shows: bool,
        reference: bool,
    ) -> (crate::AnyWindowHandle, Rc<Cell<usize>>, Entity<Holder>) {
        let renders = Rc::new(Cell::new(0));
        let holder = cx.new(|_| Holder {
            handle: VirtualListScrollHandle::new(),
            range: 0..0,
        });
        let window = add_window(cx, {
            let (renders, holder) = (renders.clone(), holder.clone());
            move |_, cx| Outer {
                _subscription: shows.then(|| {
                    cx.subscribe(&holder, |outer: &mut Outer, _, event: &RangeChanged, cx| {
                        outer.shown = Some(event.0.start);
                        cx.notify();
                    })
                }),
                holder,
                renders,
                shown: shows.then_some(0),
            }
        })
        .into();
        with_window(cx, window, |window, _| {
            if reference {
                crate::fast::tests::layer_support::make_reference(window);
            } else {
                // With splice on, as applications run: a view around a
                // rebuilt one is copied around it, so only what the outer
                // view read builds it again.
                window.set_fast_mode(crate::FastMode::ALL);
            }
        });
        draw(cx, window);
        draw(cx, window);
        (window, renders, holder)
    }
}

/// The listener runs as part of drawing the view holding the list: what it
/// records in that view does not build its readers again on the frames it
/// scrolls to other ranges, the holder has the range of every frame, and the
/// frames equal those drawn without fast mode.
#[crate::test]
fn a_range_recorded_in_the_holding_view_builds_no_reader_again(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let (window, renders, holder) = recorded::open(cx, false, false);
    let (reference, _, _) = recorded::open(cx, false, true);
    let wheel_both = |cx: &mut TestAppContext| {
        super::wheel(cx, window, -15.);
        super::wheel(cx, reference, -15.);
        let primitives = |cx: &mut TestAppContext, window| {
            crate::fast::tests::layer_support::with_window(cx, window, |window, _| {
                window.painted_primitives()
            })
        };
        assert_eq!(primitives(cx, window), primitives(cx, reference));
    };
    for _ in 0..4 {
        wheel_both(cx);
    }
    let before = renders.get();
    for step in 1..=30 {
        wheel_both(cx);
        let offset = 60. + 15. * step as f32;
        let first = (offset / 20.).floor() as usize;
        assert_eq!(
            holder.read_with(cx, |holder, _| holder.range.start),
            first,
            "step {step}: the holder recorded this frame's range"
        );
    }
    assert_eq!(
        renders.get(),
        before,
        "the view reading the holder was built again for the ranges it recorded"
    );
}

/// A view that shows the range, taking it from an event the listener emits,
/// is built again when the range changes, and its frames equal those drawn
/// without fast mode.
#[crate::test]
fn a_view_told_of_the_range_by_an_event_shows_it(cx: &mut TestAppContext) {
    if !crate::fast::layers::COMPILED {
        return;
    }
    let (window, renders, _) = recorded::open(cx, true, false);
    let (reference, _, _) = recorded::open(cx, true, true);
    let before = renders.get();
    for step in 0..24 {
        super::wheel(cx, window, -15.);
        super::wheel(cx, reference, -15.);
        // The event comes as the frame draws; the next frame shows it.
        crate::fast::tests::layer_support::draw(cx, window);
        crate::fast::tests::layer_support::draw(cx, reference);
        let primitives = |cx: &mut TestAppContext, window| {
            crate::fast::tests::layer_support::with_window(cx, window, |window, _| {
                window.painted_primitives()
            })
        };
        assert_eq!(
            primitives(cx, window),
            primitives(cx, reference),
            "step {step}: the bar shows the range drawn without fast mode"
        );
    }
    assert!(
        renders.get() > before + 10,
        "the view told of each new range was built again: {} builds",
        renders.get() - before
    );
}
