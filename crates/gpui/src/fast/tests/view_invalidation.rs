//! What builds a view again with view retention on: a change to something it
//! read builds exactly the views that read it, and the views around them,
//! and no others. Each dependency kind is checked against a board of probes
//! that each read one thing. Nothing is drawn from last frame while the
//! window refreshes, during a drag, while the inspector picks, while
//! AccessKit is active, or for a view whose id is drawn twice, and each of
//! those is counted; with view retention off, nothing is retained.

use super::view_support::{VIEWS, ViewPair};
use crate::{
    AppContext as _, Context, Entity, FastMode, Global, InputEvent as _, IntoElement, ListState,
    Modifiers, Render, ScrollHandle, Window, div, list, point, prelude::*, px,
};

/// A model a probe reads.
struct Value(usize);

struct Setting(usize);

impl Global for Setting {}

struct Marker;

impl Global for Marker {}

/// What a probe reads.
#[derive(Clone)]
enum Reads {
    Nothing,
    Model(Entity<Value>),
    Setting,
    Marker,
    Scroll(ScrollHandle),
    List(ListState),
    Modifiers,
    Hover,
    Clicks,
    /// Writes the model while it renders, when asked to.
    Writes(Entity<Value>),
}

struct Probe {
    reads: Reads,
    clicks: usize,
    write: bool,
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match &self.reads {
            Reads::Nothing => "nothing".to_string(),
            Reads::Model(value) => format!("model {}", value.read(cx).0),
            Reads::Setting => format!("setting {}", cx.try_global::<Setting>().map_or(0, |s| s.0)),
            Reads::Marker => format!("marker {}", cx.has_global::<Marker>()),
            Reads::Scroll(_) | Reads::List(_) | Reads::Hover => String::new(),
            Reads::Modifiers => format!("shift {}", window.modifiers().shift),
            Reads::Clicks => format!("clicks {}", self.clicks),
            Reads::Writes(value) => {
                if self.write {
                    self.write = false;
                    value.update(cx, |value, _| value.0 += 1);
                }
                "writer".to_string()
            }
        };
        let probe = div().w(px(60.)).h(px(40.)).child(label);
        match &self.reads {
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
            Reads::Hover => probe.child(
                div()
                    .id("hover")
                    .size(px(20.))
                    .bg(crate::red())
                    .hover(|style| style.bg(crate::blue())),
            ),
            Reads::Clicks => probe.child(div().id("button").size(px(20.)).on_click(cx.listener(
                |this, _, _, cx| {
                    this.clicks += 1;
                    cx.notify();
                },
            ))),
            _ => probe,
        }
    }
}

/// A row of probes, the first reading nothing.
struct Board {
    probes: Vec<Entity<Probe>>,
}

impl Render for Board {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .flex_wrap()
            .children(self.probes.iter().cloned())
    }
}

/// Opens a board of a probe reading nothing and probes reading what `reads`
/// makes, and draws until every probe is drawn from last frame.
fn board(
    mode: FastMode,
    reads: impl Fn(&mut Context<Board>) -> Vec<Reads> + Clone + 'static,
) -> ViewPair<Board> {
    let mut pair = ViewPair::open_in(mode, move |_, cx| {
        let mut all = vec![Reads::Nothing];
        all.extend(reads(cx));
        Board {
            probes: all
                .into_iter()
                .map(|reads| {
                    cx.new(|_| Probe {
                        reads,
                        clicks: 0,
                        write: false,
                    })
                })
                .collect(),
        }
    });
    pair.pointer_away();
    pair.draw("second frame");
    pair.draw("third frame");
    pair
}

/// Draws, and requires `built` views built: the board and the probes that
/// changed, every other probe drawn from last frame.
fn expect_built(pair: &mut ViewPair<Board>, label: &str, built: u64) {
    let probes = pair.retained(|board, _, cx| board.read(cx).probes.len() as u64);
    let stats = pair.draw(label);
    assert_eq!(stats.views_built, built, "{label}: {stats:?}");
    let reused_probes = if built == 0 { 1 } else { probes - (built - 1) };
    assert_eq!(stats.views_reused, reused_probes, "{label}: {stats:?}");
}

fn value_of(cx: &mut Context<Board>) -> Entity<Value> {
    cx.new(|_| Value(0))
}

/// An entity updated and notified builds the views that read it.
#[test]
fn an_update_with_notify_builds_exactly_its_readers() {
    let mut pair = board(VIEWS, |cx| {
        let value = value_of(cx);
        vec![Reads::Model(value), Reads::Model(value_of(cx))]
    });
    pair.update_app(|board, cx| {
        let Reads::Model(value) = board.read(cx).probes[1].read(cx).reads.clone() else {
            unreachable!()
        };
        value.update(cx, |value, cx| {
            value.0 += 1;
            cx.notify();
        });
    });
    expect_built(&mut pair, "updated and notified", 2);
}

/// An entity notified without being updated holds what it held: no view
/// that read it is built.
#[test]
fn a_notify_without_an_update_builds_no_reader() {
    let mut pair = board(VIEWS, |cx| vec![Reads::Model(value_of(cx))]);
    pair.update_app(|board, cx| {
        let Reads::Model(value) = board.read(cx).probes[1].read(cx).reads.clone() else {
            unreachable!()
        };
        cx.notify(value.entity_id());
    });
    expect_built(&mut pair, "notified only", 0);
}

/// An entity updated without being notified changed what its readers show:
/// they are built on the next frame drawn, as a frame drawn from scratch
/// would show it.
#[test]
fn an_update_without_notify_builds_its_readers_on_the_next_frame() {
    let mut pair = board(VIEWS, |cx| vec![Reads::Model(value_of(cx))]);
    pair.update_app(|board, cx| {
        let Reads::Model(value) = board.read(cx).probes[1].read(cx).reads.clone() else {
            unreachable!()
        };
        value.update(cx, |value, _| value.0 += 1);
        // Something else draws the next frame.
        let other = board.read(cx).probes[0].clone();
        other.update(cx, |_, cx| cx.notify());
    });
    expect_built(&mut pair, "updated silently", 3);
}

/// A model written while the window draws, by a view drawn after one that
/// read it, builds the reader on the next frame.
#[test]
fn a_write_while_drawing_builds_the_views_that_read_it_before() {
    let mut pair = board(VIEWS, |cx| {
        let value = value_of(cx);
        vec![Reads::Model(value.clone()), Reads::Writes(value)]
    });
    pair.update_app(|board, cx| {
        let writer = board.read(cx).probes[2].clone();
        writer.update(cx, |writer, cx| {
            writer.write = true;
            cx.notify();
        });
    });
    expect_built(&mut pair, "writer built", 2);
    expect_built(&mut pair, "reader built", 2);
    expect_built(&mut pair, "settled", 0);
}

/// A global written builds the views that read it.
#[test]
fn a_global_write_builds_its_readers() {
    let mut pair = board(VIEWS, |_| vec![Reads::Setting, Reads::Marker]);
    pair.update_app(|_, cx| cx.set_global(Setting(3)));
    expect_built(&mut pair, "setting written", 2);
}

/// A global inserted or removed builds the views that asked whether it is
/// there.
#[test]
fn a_global_presence_change_builds_the_views_that_asked() {
    let mut pair = board(VIEWS, |_| vec![Reads::Setting, Reads::Marker]);
    pair.update_app(|_, cx| cx.set_global(Marker));
    expect_built(&mut pair, "marker inserted", 2);
    pair.update_app(|_, cx| {
        cx.remove_global::<Marker>();
    });
    expect_built(&mut pair, "marker removed", 2);
}

/// A scroll handle scrolled builds the view that laid out its container.
#[test]
fn a_scroll_handle_scrolled_builds_its_view() {
    let mut pair = board(VIEWS, |_| vec![Reads::Scroll(ScrollHandle::new())]);
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
    expect_built(&mut pair, "scrolled", 2);
}

/// A list scrolled builds the view the list is in.
#[test]
fn a_list_scrolled_builds_its_view() {
    let mut pair = board(VIEWS, |_| {
        vec![Reads::List(ListState::new(
            20,
            crate::ListAlignment::Top,
            px(10.),
        ))]
    });
    pair.update_app(|board, cx| {
        let Reads::List(state) = board.read(cx).probes[1].read(cx).reads.clone() else {
            unreachable!()
        };
        state.scroll_to(crate::ListOffset {
            item_ix: 6,
            offset_in_item: px(0.),
        });
        let _ = cx;
    });
    pair.each(|cx, window| {
        cx.update_window(window, |_, window, _| window.invalidator.set_dirty(true))
            .unwrap();
    });
    expect_built(&mut pair, "list scrolled", 2);
}

/// Modifiers changed build the views that read them.
#[test]
fn ambient_input_builds_the_views_that_read_it() {
    let mut pair = board(VIEWS, |_| vec![Reads::Modifiers, Reads::Hover]);
    pair.dispatch(
        crate::ModifiersChangedEvent {
            modifiers: Modifiers::shift(),
            capslock: Default::default(),
        }
        .to_platform_input(),
    );
    pair.each(|cx, window| {
        cx.update_window(window, |_, window, _| window.invalidator.set_dirty(true))
            .unwrap();
    });
    expect_built(&mut pair, "shift pressed", 2);
}

/// A hover that changes how a view looks builds it while the pointer is
/// over it, and the view is drawn from last frame again once it is not.
#[test]
fn a_hover_builds_the_view_it_changes() {
    let mut pair = board(VIEWS, |_| vec![Reads::Hover]);
    let over = pair.retained(|_, window, _| {
        let hitbox = window
            .rendered_frame
            .hitboxes
            .iter()
            .find(|hitbox| hitbox.bounds.size.width == px(20.))
            .expect("the hover target");
        hitbox.bounds.center()
    });
    pair.move_to(over);
    expect_built(&mut pair, "hovered", 2);
    pair.pointer_away();
    expect_built(&mut pair, "unhovered", 2);
    expect_built(&mut pair, "away", 0);
}

/// An interaction inside a view builds that view and the views around it;
/// the views beside it are drawn from last frame.
#[test]
fn an_interaction_inside_a_view_builds_it_and_not_its_siblings() {
    let mut pair = board(VIEWS, |_| vec![Reads::Clicks, Reads::Hover]);
    let button = pair.retained(|_, window, _| {
        window
            .rendered_frame
            .hitboxes
            .iter()
            .find(|hitbox| hitbox.bounds.size.width == px(20.))
            .expect("the button")
            .bounds
            .center()
    });
    pair.click(button);
    pair.pointer_away();
    expect_built(&mut pair, "clicked", 2);
    let clicks = pair.retained(|board, _, cx| board.read(cx).probes[1].read(cx).clicks);
    assert_eq!(clicks, 1);
}

/// While the window refreshes, every view is built, and counted.
#[test]
fn nothing_is_reused_while_refreshing() {
    let mut pair = board(VIEWS, |_| vec![Reads::Hover]);
    pair.update(|_, window, _| window.refresh());
    let stats = pair.draw("refreshing");
    assert_eq!(stats.views_reused, 0, "{stats:?}");
    assert_eq!(stats.views_bypassed_refreshing, 3, "{stats:?}");
}

/// A preview dragged by the pointer.
struct Preview;

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(10.)).bg(crate::blue())
    }
}

struct Dragging {
    probes: Vec<Entity<Probe>>,
}

impl Render for Dragging {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                div()
                    .id("handle")
                    .size(px(30.))
                    .on_drag(7usize, |_, _, _, cx| cx.new(|_| Preview)),
            )
            .children(self.probes.iter().cloned())
    }
}

/// During a drag, nothing is drawn from last frame, and the views built are
/// counted.
#[test]
fn nothing_is_reused_during_a_drag() {
    let mut pair = ViewPair::open(|_, cx| Dragging {
        probes: vec![cx.new(|_| Probe {
            reads: Reads::Nothing,
            clicks: 0,
            write: false,
        })],
    });
    pair.draw("second frame");
    let start = point(px(10.), px(10.));
    pair.move_to(start);
    pair.dispatch(
        crate::MouseDownEvent {
            button: crate::MouseButton::Left,
            position: start,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
    );
    pair.dispatch(
        crate::MouseMoveEvent {
            position: point(px(60.), px(60.)),
            pressed_button: Some(crate::MouseButton::Left),
            modifiers: Modifiers::default(),
        }
        .to_platform_input(),
    );
    pair.draw("drag started");
    pair.dispatch(
        crate::MouseMoveEvent {
            position: point(px(80.), px(70.)),
            pressed_button: Some(crate::MouseButton::Left),
            modifiers: Modifiers::default(),
        }
        .to_platform_input(),
    );
    pair.update(|_, _, cx| cx.notify());
    let stats = pair.draw("dragging");
    assert_eq!(stats.views_reused, 0, "{stats:?}");
    assert!(stats.views_bypassed_drag > 0, "{stats:?}");
}

/// While the inspector picks, nothing is drawn from last frame.
#[test]
fn nothing_is_reused_while_the_inspector_picks() {
    let mut pair = board(VIEWS, |_| vec![Reads::Hover]);
    // An inspector opens picking.
    pair.update(|_, window, cx| window.toggle_inspector(cx));
    pair.draw("inspector opened");
    pair.update(|_, _, cx| cx.notify());
    let stats = pair.draw("picking");
    assert_eq!(stats.views_reused, 0, "{stats:?}");
    assert!(stats.views_bypassed_inspector > 0, "{stats:?}");
}

/// While AccessKit is active, nothing is drawn from last frame.
#[test]
fn nothing_is_reused_while_accessibility_is_active() {
    let mut pair = board(VIEWS, |_| vec![Reads::Hover]);
    pair.each(|cx, window| {
        cx.activate_accessibility(window);
    });
    pair.update(|_, _, cx| cx.notify());
    pair.draw("activated");
    pair.update(|_, _, cx| cx.notify());
    let stats = pair.draw("active");
    assert_eq!(stats.views_reused, 0, "{stats:?}");
    assert!(stats.views_bypassed_accessibility > 0, "{stats:?}");
}

struct Twice {
    probe: Entity<Probe>,
    tick: usize,
}

impl Render for Twice {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.probe.clone())
            .child(self.probe.clone())
            .child(format!("tick {}", self.tick))
    }
}

/// A view drawn twice under one id is never drawn from last frame.
#[test]
fn a_duplicated_id_is_never_reused() {
    let mut pair = ViewPair::open(|_, cx| Twice {
        probe: cx.new(|_| Probe {
            reads: Reads::Nothing,
            clicks: 0,
            write: false,
        }),
        tick: 0,
    });
    pair.pointer_away();
    pair.draw("second frame");
    pair.update(|twice, _, cx| {
        twice.tick += 1;
        cx.notify();
    });
    let stats = pair.draw("duplicated");
    assert_eq!(stats.views_reused, 0, "{stats:?}");
    assert_eq!(stats.views_bypassed_duplicate, 2, "{stats:?}");
}

/// With view retention off, nothing is recorded or drawn from last frame.
#[test]
fn nothing_is_retained_with_view_retention_off() {
    let layout = FastMode {
        layout_retention: true,
        ..FastMode::OFF
    };
    let mut pair = board(layout, |cx| vec![Reads::Model(value_of(cx))]);
    pair.update(|_, _, cx| cx.notify());
    let stats = pair.draw("tick");
    assert_eq!(stats.views_reused, 0, "{stats:?}");
    assert_eq!(stats.view_retention_total(), 0, "{stats:?}");
    let records = pair.retained(|_, window, _| window.rendered_frame.fast_retained.records.len());
    assert_eq!(records, 0);
}
