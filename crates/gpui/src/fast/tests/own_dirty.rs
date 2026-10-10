//! Own-dependency dirtying: at frame start, a view whose own reads changed
//! is marked dirty as if notified, so the views around it are spliced around
//! it rather than built again: an entity updated, with or without a notify,
//! a global written, a scroll handle or list scrolled.

use super::{splice::SPLICE, view_support::ViewPair};
use crate::{
    AppContext as _, Context, Entity, Global, IntoElement, ListState, Render, ScrollHandle, Window,
    div, list, prelude::*, px,
};

struct Value(usize);

struct Setting(usize);

impl Global for Setting {}

#[derive(Clone)]
enum Reads {
    Nothing,
    Model(Entity<Value>),
    Setting,
    Scroll(ScrollHandle),
    List(ListState),
}

struct Probe {
    reads: Reads,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let probe = div().w(px(60.)).h(px(40.));
        match &self.reads {
            Reads::Nothing => probe.child("nothing"),
            Reads::Model(value) => probe.child(format!("model {}", value.read(cx).0)),
            Reads::Setting => probe.child(format!(
                "setting {}",
                cx.try_global::<Setting>().map_or(0, |s| s.0)
            )),
            Reads::Scroll(scroll) => probe.child(
                div()
                    .id("scroll")
                    .h(px(30.))
                    .overflow_y_scroll()
                    .track_scroll(scroll)
                    .children(
                        (0..8usize).map(|row| div().id(("row", row)).h(px(10.)).child("row")),
                    ),
            ),
            Reads::List(state) => probe.child(
                list(state.clone(), |ix, _, _| {
                    div()
                        .h(px(10.))
                        .child(format!("item {ix}"))
                        .into_any_element()
                })
                .h(px(30.)),
            ),
        }
    }
}

struct Board {
    probes: Vec<Entity<Probe>>,
}

impl Render for Board {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .children(self.probes.iter().cloned())
    }
}

/// A board of a probe reading nothing beside one reading what `reads` makes.
fn board(reads: impl Fn(&mut Context<Board>) -> Reads + Clone + 'static) -> ViewPair<Board> {
    let mut pair = ViewPair::open_in(SPLICE, move |_, cx| {
        let reads = reads(cx);
        Board {
            probes: vec![
                cx.new(|_| Probe {
                    reads: Reads::Nothing,
                }),
                cx.new(|_| Probe { reads }),
            ],
        }
    });
    pair.pointer_away();
    pair.draw("second frame");
    pair.draw("third frame");
    pair
}

/// The reader alone is built; the board is spliced around it.
fn expect_reader_built(pair: &mut ViewPair<Board>, label: &str) {
    let stats = pair.draw(label);
    assert_eq!(stats.views_built, 1, "{label}: {stats:?}");
    assert_eq!(stats.views_spliced, 1, "{label}: {stats:?}");
}

#[test]
fn an_entity_updated_and_notified_dirties_its_reader() {
    let mut pair = board(|cx| Reads::Model(cx.new(|_| Value(0))));
    pair.update_app(|board, cx| {
        let Reads::Model(value) = board.read(cx).probes[1].read(cx).reads.clone() else {
            unreachable!()
        };
        value.update(cx, |value, cx| {
            value.0 += 1;
            cx.notify();
        });
    });
    expect_reader_built(&mut pair, "updated and notified");
}

#[test]
fn an_entity_updated_without_notify_dirties_its_reader() {
    let mut pair = board(|cx| Reads::Model(cx.new(|_| Value(0))));
    pair.update_app(|board, cx| {
        let Reads::Model(value) = board.read(cx).probes[1].read(cx).reads.clone() else {
            unreachable!()
        };
        value.update(cx, |value, _| value.0 += 1);
    });
    pair.each(|cx, window| {
        cx.update_window(window, |_, window, _| window.invalidator.set_dirty(true))
            .unwrap();
    });
    expect_reader_built(&mut pair, "updated silently");
}

#[test]
fn a_global_written_dirties_its_reader() {
    let mut pair = board(|_| Reads::Setting);
    pair.update_app(|_, cx| cx.set_global(Setting(4)));
    expect_reader_built(&mut pair, "global written");
}

#[test]
fn a_scroll_handle_scrolled_dirties_its_view() {
    let mut pair = board(|_| Reads::Scroll(ScrollHandle::new()));
    pair.update_app(|board, cx| {
        let Reads::Scroll(scroll) = board.read(cx).probes[1].read(cx).reads.clone() else {
            unreachable!()
        };
        scroll.scroll_to_item(5);
    });
    pair.each(|cx, window| {
        cx.update_window(window, |_, window, _| window.invalidator.set_dirty(true))
            .unwrap();
    });
    expect_reader_built(&mut pair, "scrolled");
}

#[test]
fn a_list_scrolled_dirties_its_view() {
    let mut pair = board(|_| Reads::List(ListState::new(20, crate::ListAlignment::Top, px(10.))));
    pair.update_app(|board, cx| {
        let Reads::List(state) = board.read(cx).probes[1].read(cx).reads.clone() else {
            unreachable!()
        };
        state.scroll_to(crate::ListOffset {
            item_ix: 6,
            offset_in_item: px(0.),
        });
    });
    pair.each(|cx, window| {
        cx.update_window(window, |_, window, _| window.invalidator.set_dirty(true))
            .unwrap();
    });
    expect_reader_built(&mut pair, "list scrolled");
}
