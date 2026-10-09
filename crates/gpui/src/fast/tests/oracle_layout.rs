//! Layout retention composes with gpux's layout features: the randomized
//! oracle in layout-only mode matches the frame drawn from scratch over every
//! change kind, with nodes really kept clean, and targeted cases follow
//! sticky and fixed positioning, container queries, and the style handoff
//! and conditional prepaint recompute, against the pool frame by frame.

use super::oracle_support::{PALETTE, oracle_app, run_lockstep, seeds_and_steps};
use crate::{
    AppContext as _, Context, FastMode, InputEvent as _, Point, Render, ScrollHandle, Window,
    WindowHandle, container_query, div, point, prelude::*, px,
};

const LAYOUT: FastMode = FastMode {
    layout_retention: true,
    ..FastMode::OFF
};

/// The randomized oracle with only layout retention on: every frame equals
/// the one drawn from scratch, across every change kind, and the run did
/// keep nodes, write fewer styles than it claimed nodes, and carry text.
#[test]
fn layout_retention_matches_frames_drawn_from_scratch() {
    let (seeds, steps) = seeds_and_steps(16, 60);
    for seed in seeds {
        let run = run_lockstep(seed, steps, LAYOUT);
        let stats = run.incremental;
        assert!(stats.nodes_claimed > 0, "seed {seed}: {stats:?}");
        assert!(
            stats.style_writes < stats.nodes_claimed / 10,
            "seed {seed}: kept nodes were mostly written: {stats:?}"
        );
        assert!(stats.leaves_carried > 0, "seed {seed}: {stats:?}");
    }
}

/// A view laid out with layout retention beside the same view laid out by
/// the pool, given the same changes and pointer.
struct Pair<V: 'static> {
    windows: [(crate::TestAppContext, WindowHandle<V>); 2],
}

impl<V: Render + 'static> Pair<V> {
    fn open(build: impl Fn() -> V) -> Self {
        let open = |mode: FastMode, view: V| {
            let mut cx = oracle_app(0);
            cx.set_draw_on_flush(false);
            let window = cx.add_window(move |_, _| view);
            cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
                .unwrap();
            (cx, window)
        };
        let mut pair = Self {
            windows: [open(LAYOUT, build()), open(FastMode::OFF, build())],
        };
        pair.step("first frame", |_| {});
        pair.step("second frame", |_| {});
        pair
    }

    /// Applies `change` to both views, draws them and compares the frames.
    fn step(&mut self, label: &str, change: impl Fn(&mut V)) {
        let frames: Vec<Vec<String>> = self
            .windows
            .iter_mut()
            .map(|(cx, window)| {
                cx.update_window((*window).into(), |root, window, cx| {
                    root.downcast::<V>().unwrap().update(cx, |view, cx| {
                        change(view);
                        cx.notify();
                    });
                    window.draw(cx).clear(cx);
                    window.describe_rendered_frame()
                })
                .unwrap()
            })
            .collect();
        if let Some(difference) =
            crate::fast::verify::first_frame_difference(&frames[0], &frames[1])
        {
            panic!("{label}: the kept layout differs from the pool's: {difference}");
        }
    }

    /// Moves the pointer to `position` in both windows, draws and compares.
    fn hover(&mut self, label: &str, position: Point<crate::Pixels>) {
        for (cx, window) in &mut self.windows {
            cx.update_window((*window).into(), |_, window, cx| {
                window.dispatch_event(
                    crate::MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
        }
        self.step(label, |_| {});
    }
}

/// A scroll area of rows under sticky section headers, with a fixed box.
struct Sticky {
    rows: usize,
    scroll: ScrollHandle,
}

impl Render for Sticky {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("scroller")
                .w(px(200.))
                .h(px(160.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(
                    div()
                        .id("fixed")
                        .fixed()
                        .top(px(4.))
                        .left(px(150.))
                        .size(px(20.))
                        .bg(PALETTE[2]),
                )
                .children((0..4).map(|section| {
                    div()
                        .id(("section", section))
                        .child(
                            div()
                                .id(("header", section))
                                .sticky()
                                .top(px(0.))
                                .h(px(18.))
                                .bg(PALETTE[section % PALETTE.len()])
                                .child(format!("section {section}")),
                        )
                        .children((0..self.rows).map(move |row| {
                            div()
                                .id(("row", section * 100 + row))
                                .h(px(14. + (row % 3) as f32 * 4.))
                                .child(format!("row {row}"))
                        }))
                })),
        )
    }
}

/// Sticky headers are found and pinned, and the fixed box placed against
/// the window, as rows come and go and the area scrolls.
#[test]
fn sticky_and_fixed_positioning_follow_a_kept_layout() {
    let scroll = ScrollHandle::new();
    let mut pair = Pair::open(|| Sticky {
        rows: 6,
        scroll: scroll.clone(),
    });
    for (step, (offset, rows)) in [(30., 6), (75., 6), (75., 9), (140., 9), (10., 4), (200., 4)]
        .into_iter()
        .enumerate()
    {
        scroll.request_offset(
            point(px(0.), px(-offset)),
            crate::ScrollMutationSource::Programmatic,
        );
        pair.step(&format!("step {step}"), |view| view.rows = rows);
    }
}

/// A container query whose content is laid out while it is prepainted, and
/// changes with the width it is given.
struct Query {
    width: f32,
}

impl Render for Query {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .h(px(80.))
            .child(container_query(|size, _, _| {
                if size.width < px(150.) {
                    div()
                        .flex()
                        .flex_col()
                        .child(div().h(px(10.)).bg(PALETTE[0]).child("narrow"))
                        .child(div().h(px(10.)).bg(PALETTE[1]))
                } else {
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(div().w(px(40.)).h(px(20.)).bg(PALETTE[2]).child("wide"))
                        .child(div().flex_1().h(px(20.)).bg(PALETTE[3]))
                }
            }))
    }
}

/// A container query lays its content out under a kept layout as the pool
/// does, through widths that switch its content back and forth.
#[test]
fn container_queries_lay_their_content_out_under_a_kept_layout() {
    let mut pair = Pair::open(|| Query { width: 100. });
    for width in [120., 200., 260., 140., 140., 90., 300.] {
        pair.step(&format!("width {width}"), |view| view.width = width);
    }
}

/// Elements whose styles go through the request-to-prepaint handoff or are
/// held from it (hover, group hover, active), and sticky headers whose
/// styles prepaint computes again when they stick.
struct Styles {
    scroll: ScrollHandle,
}

impl Render for Styles {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("group")
                    .group("row")
                    .flex()
                    .flex_row()
                    .children((0..4).map(|ix| {
                        div()
                            .id(("cell", ix))
                            .w(px(30. + ix as f32 * 5.))
                            .h(px(20.))
                            .bg(PALETTE[ix % PALETTE.len()])
                            .hover(|style| style.w(px(50.)))
                            .group_hover("row", |style| style.h(px(26.)))
                            .active(|style| style.bg(PALETTE[4]))
                    })),
            )
            .child(
                div()
                    .id("stuck area")
                    .w(px(160.))
                    .h(px(80.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .id("stuck header")
                            .sticky()
                            .top(px(0.))
                            .h(px(16.))
                            .when_stuck(|style| style.h(px(24.)).bg(PALETTE[1]))
                            .child("header"),
                    )
                    .children(
                        (0..12)
                            .map(|row| div().id(("line", row as usize)).h(px(16.)).child("line")),
                    ),
            )
    }
}

/// Hover and group-hover styles that change layout, and a sticky header
/// restyled when it sticks, under a kept layout as under the pool.
#[test]
fn the_style_handoff_and_prepaint_recompute_hold_under_a_kept_layout() {
    let scroll = ScrollHandle::new();
    let mut pair = Pair::open(|| Styles {
        scroll: scroll.clone(),
    });
    for (step, x) in [10., 40., 80., 300., 120.].into_iter().enumerate() {
        pair.hover(&format!("hover {step}"), point(px(x), px(10.)));
    }
    for (step, offset) in [0., 20., 60., 5., 0.].into_iter().enumerate() {
        scroll.request_offset(
            point(px(0.), px(-offset)),
            crate::ScrollMutationSource::Programmatic,
        );
        pair.step(&format!("scroll {step}"), |_| {});
    }
}
