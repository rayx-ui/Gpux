//! How a view is drawn with view retention on: from last frame when nothing
//! it depends on changed and it is drawn where it was (reused); built at the
//! layout it kept when it moved, or its content mask or painted context
//! changed; built otherwise, a changed text style included.
//! A cached view depends on what it reads as other views do, and a stateless
//! component is never retained. Each frame equals the frame drawn from
//! scratch.

use super::view_support::ViewPair;
use crate::{
    App, AppContext as _, Context, Entity, IntoElement, Render, RenderOnce, StyleRefinement,
    Window, div, prelude::*, px,
};

/// A view with text and a few boxes, whose content does not change.
struct Leaf;

impl Render for Leaf {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(120.))
            .child("a leaf of text")
            .child(div().h(px(8.)).bg(crate::red()))
            .child(div().h(px(8.)).bg(crate::blue()))
    }
}

/// What the host changes around its leaves.
#[derive(Clone, Copy, Default)]
struct Around {
    /// The height of a spacer above the moving leaf.
    spacer: f32,
    /// The height of the clip around the clipped leaf.
    clip: f32,
    /// Whether the text around the styled leaf is green.
    green: bool,
    /// The opacity of the faded leaf's container.
    opacity: f32,
    tick: usize,
}

struct Host {
    leaves: [Entity<Leaf>; 4],
    around: Around,
}

impl Host {
    fn new(cx: &mut Context<Self>) -> Self {
        Host {
            leaves: [
                cx.new(|_| Leaf),
                cx.new(|_| Leaf),
                cx.new(|_| Leaf),
                cx.new(|_| Leaf),
            ],
            around: Around {
                spacer: 10.,
                clip: 60.,
                green: false,
                opacity: 1.,
                tick: 0,
            },
        }
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let around = self.around;
        let [moving, clipped, styled, faded] = self.leaves.clone();
        div()
            .size_full()
            .flex()
            .flex_row()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(120.))
                    .child(div().h(px(around.spacer)))
                    .child(moving),
            )
            .child(
                div()
                    .w(px(120.))
                    .h(px(around.clip))
                    .overflow_hidden()
                    .child(clipped),
            )
            .child(
                div()
                    .w(px(120.))
                    .when(around.green, |this| this.text_color(crate::green()))
                    .child(styled),
            )
            .child(div().w(px(120.)).opacity(around.opacity).child(faded))
            .child(format!("tick {}", around.tick))
    }
}

fn open() -> ViewPair<Host> {
    let mut pair = ViewPair::open(|_, cx| Host::new(cx));
    pair.pointer_away();
    pair.draw("second frame");
    pair
}

/// Changes what the host draws around its leaves, and draws: the host is
/// built, and `at_retained_layout` leaves are built at the layout they kept
/// while the others are drawn from last frame.
fn change(pair: &mut ViewPair<Host>, label: &str, at_retained_layout: u64, f: fn(&mut Around)) {
    pair.update(|host, _, cx| {
        f(&mut host.around);
        cx.notify();
    });
    let stats = pair.draw(label);
    assert_eq!(
        stats.views_built_at_retained_layout, at_retained_layout,
        "{label}: {stats:?}"
    );
    assert_eq!(
        stats.views_built,
        1 + at_retained_layout,
        "{label}: {stats:?}"
    );
    assert_eq!(
        stats.views_reused,
        4 - at_retained_layout,
        "{label}: the leaves left where they were are reused: {stats:?}"
    );
}

/// A window nothing changed in renders no view and writes no layout node:
/// every view is drawn from last frame.
#[test]
fn a_still_window_renders_no_view_and_writes_no_layout_node() {
    let mut pair = open();
    for step in 0..3 {
        let stats = pair.draw(&format!("still {step}"));
        assert_eq!(stats.views_built, 0, "{stats:?}");
        assert_eq!(stats.style_writes, 0, "{stats:?}");
        assert_eq!(stats.children_writes, 0, "{stats:?}");
        assert_eq!(stats.leaves_measured, 0, "{stats:?}");
        assert!(stats.views_reused > 0, "{stats:?}");
    }
}

/// A host built around its leaves draws each of them from last frame.
#[test]
fn leaves_of_a_built_host_are_reused() {
    let mut pair = open();
    change(&mut pair, "tick", 0, |around| around.tick += 1);
}

/// A leaf that moved is built at the layout it kept.
#[test]
fn a_moved_view_is_built_at_its_retained_layout() {
    let mut pair = open();
    change(&mut pair, "moved", 1, |around| around.spacer = 30.);
    change(&mut pair, "moved back", 1, |around| around.spacer = 10.);
    change(&mut pair, "still", 0, |around| around.tick += 1);
}

/// A leaf whose content mask changed is built at the layout it kept.
#[test]
fn a_view_whose_content_mask_changed_is_built_at_its_retained_layout() {
    let mut pair = open();
    change(&mut pair, "clipped", 1, |around| around.clip = 20.);
    change(&mut pair, "unclipped", 1, |around| around.clip = 60.);
}

/// A leaf whose inherited text style changed is built, its layout too: a
/// text style can measure text otherwise, so the layout it kept does not
/// stand. Its kept nodes are claimed again as it is laid out.
#[test]
fn a_view_whose_text_style_changed_is_built() {
    let mut pair = open();
    for (label, green) in [("green", true), ("not green", false)] {
        pair.update(|host, _, cx| {
            host.around.green = green;
            cx.notify();
        });
        let stats = pair.draw(label);
        assert_eq!(stats.views_built, 2, "{label}: {stats:?}");
        assert_eq!(
            stats.views_built_at_retained_layout, 0,
            "{label}: {stats:?}"
        );
        assert_eq!(stats.views_reused, 3, "{label}: {stats:?}");
        assert_eq!(stats.style_writes, 0, "{label}: {stats:?}");
    }
}

/// A leaf painted at another opacity is built at the layout it kept: the
/// painted context is part of where a view is drawn.
#[test]
fn a_view_whose_painted_context_changed_is_built_at_its_retained_layout() {
    let mut pair = open();
    change(&mut pair, "faded", 1, |around| around.opacity = 0.5);
    change(&mut pair, "opaque", 1, |around| around.opacity = 1.);
}

/// A model a cached view reads.
struct Count(usize);

struct Counter {
    count: Entity<Count>,
}

impl Render for Counter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(format!("count {}", self.count.read(cx).0))
    }
}

struct CachedHost {
    counter: Entity<Counter>,
    other: Entity<Count>,
}

impl Render for CachedHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            self.counter
                .clone()
                .cached(StyleRefinement::default().w(px(120.)).h(px(30.))),
        )
    }
}

/// A cached view depends on what it reads: a model it reads changing builds
/// it, though nothing observes the model, and one it does not read changing
/// builds nothing. Cached views keep their layout by the style they are
/// cached with.
#[test]
fn a_cached_view_depends_on_what_it_reads() {
    let mut pair = ViewPair::open(|_, cx| {
        let count = cx.new(|_| Count(0));
        CachedHost {
            counter: cx.new(|_| Counter { count }),
            other: cx.new(|_| Count(0)),
        }
    });
    pair.pointer_away();
    pair.draw("second frame");
    let stats = pair.draw("still");
    assert_eq!(stats.views_built, 0, "{stats:?}");

    pair.update_app(|host, cx| {
        let count = host.read(cx).counter.read(cx).count.clone();
        count.update(cx, |count, cx| {
            count.0 += 1;
            cx.notify();
        });
    });
    let stats = pair.draw("read model changed");
    assert_eq!(
        stats.views_built, 2,
        "the host and the cached view: {stats:?}"
    );

    pair.update_app(|host, cx| {
        let other = host.read(cx).other.clone();
        other.update(cx, |other, cx| {
            other.0 += 1;
            cx.notify();
        });
    });
    let stats = pair.draw("unread model changed");
    assert_eq!(stats.views_built, 0, "{stats:?}");
}

/// A stateless component, which counts its renders.
#[derive(IntoElement)]
struct Badge {
    renders: std::rc::Rc<std::cell::Cell<usize>>,
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div().w(px(40.)).h(px(10.)).bg(crate::red())
    }
}

struct BadgeHost {
    renders: std::rc::Rc<std::cell::Cell<usize>>,
    tick: usize,
}

impl Render for BadgeHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(Badge {
                renders: self.renders.clone(),
            })
            .child(format!("tick {}", self.tick))
    }
}

/// A stateless `RenderOnce` component is never retained: it is rendered
/// whenever the view it is in is built, and leaves no record.
#[test]
fn a_stateless_component_is_never_retained() {
    let renders = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut pair = ViewPair::open({
        let renders = renders.clone();
        move |_, _| BadgeHost {
            renders: renders.clone(),
            tick: 0,
        }
    });
    pair.pointer_away();
    pair.draw("second frame");
    let before = renders.get();
    pair.update(|host, _, cx| {
        host.tick += 1;
        cx.notify();
    });
    let stats = pair.draw("host built");
    assert_eq!(stats.views_built, 1, "{stats:?}");
    // Rendered once in each window.
    assert_eq!(renders.get(), before + 2);
    let records = pair.retained(|_, window, _| window.rendered_frame.fast_retained.records.len());
    assert_eq!(records, 1, "only the host has a record");
}
