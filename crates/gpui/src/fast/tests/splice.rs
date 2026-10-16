//! Splice: a view dirty only because views nested in it changed is drawn
//! from last frame around them, with each rebuilt in its gap, and every
//! frame equals the frame drawn from scratch.

use super::view_support::ViewPair;
use crate::{
    AppContext as _, Context, Entity, FastMode, IntoElement, Render, Window, div, prelude::*, px,
};

pub(super) const SPLICE: FastMode = FastMode {
    layout_retention: true,
    view_retention: true,
    splice: true,
    ..FastMode::OFF
};

/// A panel showing its tick.
struct Panel {
    index: usize,
    tick: usize,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(80.))
            .h(px(30.))
            .bg(crate::red())
            .child(format!("panel {} {}", self.index, self.tick))
    }
}

struct Board {
    panels: Vec<Entity<Panel>>,
}

impl Render for Board {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .children(self.panels.iter().cloned())
    }
}

fn board(panels: usize) -> ViewPair<Board> {
    let mut pair = ViewPair::open_in(SPLICE, move |_, cx| Board {
        panels: (0..panels)
            .map(|index| cx.new(|_| Panel { index, tick: 0 }))
            .collect(),
    });
    pair.pointer_away();
    pair.draw("second frame");
    pair.draw("third frame");
    pair
}

fn tick_panels(pair: &mut ViewPair<Board>, panels: &[usize]) {
    let panels = panels.to_vec();
    pair.update_app(move |board, cx| {
        for &index in &panels {
            let panel = board.read(cx).panels[index].clone();
            panel.update(cx, |panel, cx| {
                panel.tick += 1;
                cx.notify();
            });
        }
    });
}

/// One panel changes: only it is built, the board around it is spliced and
/// the other panels are copied with it.
#[test]
fn a_single_nested_rebuild_splices_the_view_around_it() {
    let mut pair = board(4);
    for step in 0..3 {
        tick_panels(&mut pair, &[1]);
        let stats = pair.draw(&format!("step {step}"));
        assert_eq!(stats.views_built, 1, "step {step}: {stats:?}");
        assert_eq!(stats.views_spliced, 1, "step {step}: {stats:?}");
    }
}

/// Two panels change: both are built in their gaps, once each.
#[test]
fn several_gaps_are_built_in_one_splice() {
    let mut pair = board(5);
    tick_panels(&mut pair, &[0, 3]);
    let stats = pair.draw("two panels");
    assert_eq!(stats.views_built, 2, "{stats:?}");
    assert_eq!(stats.views_spliced, 1, "{stats:?}");
    tick_panels(&mut pair, &[1, 2, 4]);
    let stats = pair.draw("three panels");
    assert_eq!(stats.views_built, 3, "{stats:?}");
    assert_eq!(stats.views_spliced, 1, "{stats:?}");
}

/// A panel whose content is laid out at another node.
struct Shifting {
    generation: usize,
}

impl Render for Shifting {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("shifting", self.generation))
            .w(px(80.))
            .h(px(30.))
            .child(format!("generation {}", self.generation))
    }
}

struct ShiftingBoard {
    shifting: Entity<Shifting>,
    other: Entity<Panel>,
}

impl Render for ShiftingBoard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(self.shifting.clone())
            .child(self.other.clone())
    }
}

/// A gap laid out at another node changes the view around it, which is
/// built after all, taking over the nested view already built rather than
/// building it twice.
#[test]
fn a_gap_laid_out_elsewhere_hands_its_view_to_the_view_built_around_it() {
    let mut pair = ViewPair::open_in(SPLICE, |_, cx| ShiftingBoard {
        shifting: cx.new(|_| Shifting { generation: 0 }),
        other: cx.new(|_| Panel { index: 0, tick: 0 }),
    });
    pair.pointer_away();
    pair.draw("second frame");
    pair.update_app(|board, cx| {
        let shifting = board.read(cx).shifting.clone();
        shifting.update(cx, |shifting, cx| {
            shifting.generation += 1;
            cx.notify();
        });
    });
    let stats = pair.draw("shifted");
    assert_eq!(stats.views_spliced, 0, "{stats:?}");
    assert_eq!(stats.views_prebuilt, 1, "{stats:?}");
    assert_eq!(
        stats.views_built, 2,
        "the board, and the shifted view once: {stats:?}"
    );
}

/// A board that defers an overlay of its own, outside its panels.
struct Overlaid {
    panels: Vec<Entity<Panel>>,
}

impl Render for Overlaid {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .children(self.panels.iter().cloned())
            .child(crate::deferred(
                crate::anchored().child(div().size(px(10.)).bg(crate::blue())),
            ))
    }
}

/// A panel that defers an overlay inside itself.
struct Popping {
    tick: usize,
}

impl Render for Popping {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(60.))
            .h(px(20.))
            .child(format!("tick {}", self.tick))
            .child(crate::deferred(crate::anchored().child(
                div().size(px(8.) + px(self.tick as f32)).bg(crate::red()),
            )))
    }
}

struct PoppingBoard {
    popping: Entity<Popping>,
    other: Entity<Panel>,
}

impl Render for PoppingBoard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.popping.clone())
            .child(self.other.clone())
    }
}

/// A deferred draw outside every gap would be copied however it reads, so
/// its view is built; one inside a gap is built with the gap.
#[test]
fn deferred_draws_splice_only_inside_gaps() {
    let mut pair = ViewPair::open_in(SPLICE, |_, cx| Overlaid {
        panels: (0..3)
            .map(|index| cx.new(|_| Panel { index, tick: 0 }))
            .collect(),
    });
    pair.pointer_away();
    pair.draw("second frame");
    pair.update_app(|board, cx| {
        let panel = board.read(cx).panels[1].clone();
        panel.update(cx, |panel, cx| {
            panel.tick += 1;
            cx.notify();
        });
    });
    let stats = pair.draw("deferred outside");
    assert_eq!(stats.views_spliced, 0, "{stats:?}");

    let mut pair = ViewPair::open_in(SPLICE, |_, cx| PoppingBoard {
        popping: cx.new(|_| Popping { tick: 0 }),
        other: cx.new(|_| Panel { index: 0, tick: 0 }),
    });
    pair.pointer_away();
    pair.draw("second frame");
    for step in 0..3 {
        pair.update_app(|board, cx| {
            let popping = board.read(cx).popping.clone();
            popping.update(cx, |popping, cx| {
                popping.tick += 1;
                cx.notify();
            });
        });
        let stats = pair.draw(&format!("deferred inside {step}"));
        assert_eq!(stats.views_spliced, 1, "{stats:?}");
        assert_eq!(stats.views_built, 1, "{stats:?}");
    }
}

/// A board that draws a panel under every inherited stack gpux has.
struct Inheriting {
    inner: Entity<Panel>,
    other: Entity<Panel>,
}

impl Render for Inheriting {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.other.clone())
            .child(
                div()
                    .id("scroller")
                    .h(px(60.))
                    .w(px(200.))
                    .overflow_y_scroll()
                    .text_color(crate::green())
                    .opacity(0.5)
                    .child(
                        div()
                            .w(px(100.))
                            .h(px(20.))
                            .overflow_hidden()
                            .child(self.inner.clone()),
                    ),
            )
    }
}

/// A nested view built in its gap inherits the content masks, text style,
/// opacity, offset and scroll context it had where it was drawn.
#[test]
fn a_gap_inherits_what_it_had_where_it_was_drawn() {
    let mut pair = ViewPair::open_in(SPLICE, |_, cx| Inheriting {
        inner: cx.new(|_| Panel { index: 0, tick: 0 }),
        other: cx.new(|_| Panel { index: 1, tick: 0 }),
    });
    pair.pointer_away();
    pair.draw("second frame");
    for step in 0..3 {
        pair.update_app(|board, cx| {
            let inner = board.read(cx).inner.clone();
            inner.update(cx, |inner, cx| {
                inner.tick += 1;
                cx.notify();
            });
        });
        let stats = pair.draw(&format!("step {step}"));
        assert_eq!(stats.views_spliced, 1, "step {step}: {stats:?}");
        assert_eq!(stats.views_built, 1, "step {step}: {stats:?}");
    }
}

/// The dispatch tree does not grow when the same panel is spliced in again
/// and again: each gap hangs off its element's copied node.
#[test]
fn dispatch_depth_holds_over_a_thousand_spliced_frames() {
    let mut pair = board(3);
    let shape = |pair: &mut ViewPair<Board>| {
        pair.retained(|_, window, _| {
            let tree = &window.rendered_frame.dispatch_tree;
            let depth = (0..tree.len())
                .map(|index| {
                    let mut depth = 0;
                    let mut node = Some(crate::key_dispatch::DispatchNodeId::from_usize(index));
                    while let Some(id) = node {
                        depth += 1;
                        node = tree.parent_node_id(id);
                    }
                    depth
                })
                .max()
                .unwrap_or(0);
            (tree.len(), depth)
        })
    };
    let before = shape(&mut pair);
    for step in 0..1000 {
        tick_panels(&mut pair, &[step % 3]);
        let stats = pair.draw(&format!("step {step}"));
        assert_eq!(stats.views_spliced, 1, "step {step}: {stats:?}");
    }
    assert_eq!(shape(&mut pair), before);
}

/// A panel that counts clicks.
struct Clicks {
    clicks: usize,
}

impl Render for Clicks {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("clicks")
            .w(px(80.))
            .h(px(30.))
            .on_click(cx.listener(|this, _, _, _| this.clicks += 1))
            .child(format!("clicks {}", self.clicks))
    }
}

struct ClickBoard {
    clicks: Entity<Clicks>,
    panel: Entity<Panel>,
}

impl Render for ClickBoard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(self.clicks.clone())
            .child(self.panel.clone())
    }
}

/// A view copied around a gap keeps its listeners and hitboxes, re-pointed
/// at the copied dispatch nodes.
#[test]
fn views_copied_around_a_gap_keep_their_listeners() {
    let mut pair = ViewPair::open_in(SPLICE, |_, cx| ClickBoard {
        clicks: cx.new(|_| Clicks { clicks: 0 }),
        panel: cx.new(|_| Panel { index: 0, tick: 0 }),
    });
    pair.pointer_away();
    pair.draw("second frame");
    pair.update_app(|board, cx| {
        let panel = board.read(cx).panel.clone();
        panel.update(cx, |panel, cx| {
            panel.tick += 1;
            cx.notify();
        });
    });
    let stats = pair.draw("spliced");
    assert_eq!(stats.views_spliced, 1, "{stats:?}");
    pair.click(crate::point(px(10.), px(10.)));
    pair.pointer_away();
    let clicks = pair.retained(|board, _, cx| board.read(cx).clicks.read(cx).clicks);
    assert_eq!(clicks, 1);
    pair.draw("after click");
}

/// A nested view whose element gains a listener, and so a hitbox, without
/// changing its layout.
struct Toggling {
    clickable: bool,
}

impl Render for Toggling {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("clickable")
            .w(px(40.))
            .h(px(20.))
            .when(self.clickable, |this| {
                this.on_click(cx.listener(|_, _, _, _| {}))
            })
    }
}

struct Wrapping {
    toggling: Entity<Toggling>,
}

impl Render for Wrapping {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().p_1().child(self.toggling.clone()))
    }
}

/// A gap that gains or loses a hitbox is spliced, and the hitboxes copied
/// around it and its new ones equal the frame drawn from scratch.
#[test]
fn a_gap_that_gains_or_loses_a_hitbox_is_spliced() {
    let mut pair = ViewPair::open_in(SPLICE, |_, cx| Wrapping {
        toggling: cx.new(|_| Toggling { clickable: false }),
    });
    pair.pointer_away();
    pair.draw("second frame");
    for (step, clickable) in [true, false, true].into_iter().enumerate() {
        pair.update_app(move |wrapping, cx| {
            let toggling = wrapping.read(cx).toggling.clone();
            toggling.update(cx, |toggling, cx| {
                toggling.clickable = clickable;
                cx.notify();
            });
        });
        let stats = pair.draw(&format!("step {step}"));
        assert_eq!(stats.views_spliced, 1, "step {step}: {stats:?}");
    }
}
