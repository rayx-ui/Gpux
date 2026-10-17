//! Retained records carry every per-frame output of a view drawn from last
//! frame: a child view is drawn from its record while its host is built
//! around it, the frame equals the frame drawn from scratch, and what the
//! output does (a click, a focus, a state, a tooltip, a window control)
//! still works. A rollback drops the records the discarded prepaint made.

use super::view_support::ViewPair;
use crate::{
    AnyElement, App, AppContext as _, Bounds, Context, Element, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Render, Style, UTF16Selection, Window, WindowControlArea, anchored, canvas, deferred,
    div, point, prelude::*, px, size,
};

/// A root view that renders a tick and the child view `C`, so that a step
/// that changes only the tick builds the root and draws the child from last
/// frame.
struct Host<C: 'static> {
    child: Entity<C>,
    tick: usize,
}

impl<C: Render> Render for Host<C> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.child.clone())
            .child(format!("tick {}", self.tick))
    }
}

fn host<C: Render>(
    child: impl Fn(&mut Window, &mut Context<C>) -> C + Clone + 'static,
) -> ViewPair<Host<C>> {
    let mut pair = ViewPair::open(move |window, cx| Host {
        child: cx.new(|cx| child(window, cx)),
        tick: 0,
    });
    pair.pointer_away();
    pair.draw("second frame");
    pair
}

/// Builds the host alone: the child is drawn from last frame, and the frame
/// equals the frame drawn from scratch.
fn tick<C: Render>(pair: &mut ViewPair<Host<C>>, label: &str) {
    pair.update(|host, _, cx| {
        host.tick += 1;
        cx.notify();
    });
    let stats = pair.draw(label);
    assert_eq!(
        stats.views_built, 1,
        "{label}: only the host is built: {stats:?}"
    );
    assert!(
        stats.views_reused >= 1,
        "{label}: the child is reused: {stats:?}"
    );
}

/// A target that counts its clicks.
struct Clicks {
    clicks: usize,
}

impl Render for Clicks {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("target")
            .size(px(40.))
            .bg(crate::red())
            .on_mouse_move(|_, _, _| {})
            .on_click(cx.listener(|this, _, _, _| this.clicks += 1))
    }
}

/// Hitboxes, re-pointed at the dispatch nodes copied with them, and mouse
/// listeners: a click on a child drawn from last frame reaches it.
#[test]
fn hitboxes_and_mouse_listeners_reach_a_reused_child() {
    let mut pair = host(|_, _| Clicks { clicks: 0 });
    tick(&mut pair, "reused");
    tick(&mut pair, "reused again");
    pair.click(point(px(10.), px(10.)));
    pair.pointer_away();
    let clicks = pair.retained(|host, _, cx| host.read(cx).child.read(cx).clicks);
    assert_eq!(clicks, 1);
}

struct Tip;

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("tip")
    }
}

struct Tooltipped;

impl Render for Tooltipped {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("tooltipped")
            .size(px(40.))
            .tooltip(|_, cx| cx.new(|_| Tip).into())
    }
}

/// Tooltip requests are carried with the child: its tooltip, shown with
/// the pointer over it, stays shown while it is drawn from last frame.
#[test]
fn tooltip_requests_are_carried() {
    let mut pair = host(|_, _| Tooltipped);
    pair.move_to(point(px(10.), px(10.)));
    pair.advance_clock(std::time::Duration::from_secs(2));
    pair.draw("tooltip shown");
    tick(&mut pair, "reused");
    let requests = pair.retained(|_, window, _| {
        window
            .rendered_frame
            .tooltip_requests
            .iter()
            .filter(|request| request.is_some())
            .count()
    });
    assert_eq!(requests, 1);
}

/// A model only something deferred from the child reads.
struct Width(f32);

struct Deferring {
    width: Entity<Width>,
}

impl Render for Deferring {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.clone();
        div().size(px(40.)).child(deferred(
            anchored().child(
                canvas(
                    move |_, _, cx| width.read(cx).0,
                    |bounds, width, window, _| {
                        window.paint_quad(crate::fill(
                            Bounds::new(bounds.origin, size(px(width), px(10.))),
                            crate::blue(),
                        ))
                    },
                )
                .size(px(30.)),
            ),
        ))
    }
}

/// Deferred draws are carried, and what drawing them reads is the child's:
/// a change to it builds the child again.
#[test]
fn deferred_draws_are_carried_and_attributed_to_their_view() {
    let mut pair = host(|_, cx| Deferring {
        width: cx.new(|_| Width(5.)),
    });
    tick(&mut pair, "reused");
    pair.update_app(|host, cx| {
        let width = host.read(cx).child.read(cx).width.clone();
        width.update(cx, |width, cx| {
            width.0 = 25.;
            cx.notify();
        });
    });
    let stats = pair.draw("deferred read changed");
    assert!(stats.views_built >= 1, "{stats:?}");
    tick(&mut pair, "reused after");
}

struct Focusable {
    focus: FocusHandle,
}

impl Render for Focusable {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("focusable")
            .size(px(40.))
            .track_focus(&self.focus)
            .key_context("Focusable")
    }
}

/// The dispatch subtree is carried with the focus it holds.
#[test]
fn the_dispatch_subtree_keeps_its_focus() {
    let mut pair = host(|_, cx| Focusable {
        focus: cx.focus_handle(),
    });
    pair.update(|host, window, cx| {
        let focus = host.child.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    pair.draw("focused");
    tick(&mut pair, "reused");
    tick(&mut pair, "reused again");
    let focused = pair.retained(|host, window, cx| {
        let focus = host.read(cx).child.read(cx).focus.clone();
        window.rendered_frame.focus == Some(focus.id) && focus.is_focused(window)
    });
    assert!(focused);
}

/// Keeps a state in element state and notes the state's entity each time it
/// is built.
struct Stateful {
    seen: Vec<crate::EntityId>,
}

impl Render for Stateful {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = window.use_keyed_state("kept", cx, |_, _| 7usize);
        self.seen.push(state.entity_id());
        div().size(px(40.)).child(format!("{}", state.read(cx)))
    }
}

/// Element states, and their transfer to the next frame, are carried: after
/// frames drawn from last frame, the child built again finds its state.
#[test]
fn element_states_survive_frames_drawn_from_last_frame() {
    let mut pair = host(|_, _| Stateful { seen: Vec::new() });
    tick(&mut pair, "reused");
    tick(&mut pair, "reused again");
    pair.update_app(|host, cx| {
        let child = host.read(cx).child.clone();
        child.update(cx, |_, cx| cx.notify());
    });
    pair.draw("child built");
    let seen = pair.retained(|host, _, cx| host.read(cx).child.read(cx).seen.clone());
    assert!(seen.len() >= 2, "{seen:?}");
    assert!(
        seen.iter().all(|id| *id == seen[0]),
        "the state was made again: {seen:?}"
    );
}

struct Text;

impl Render for Text {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(120.)).child("some words to lay out")
    }
}

/// Line layouts are carried: a child drawn from last frame shapes nothing.
#[test]
fn line_layouts_are_carried() {
    let mut pair = host(|_, _| Text);
    pair.update(|host, _, cx| {
        host.tick += 1;
        cx.notify();
    });
    pair.draw("warm");
    tick(&mut pair, "reused");
    let stats = pair.draw("still");
    assert_eq!(stats.lines_shaped, 0, "{stats:?}");
}

/// A text input that answers nothing.
struct Field {
    focus: FocusHandle,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let focus = self.focus.clone();
        div().size(px(40.)).track_focus(&self.focus).child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx)
                },
            )
            .size_full(),
        )
    }
}

impl EntityInputHandler for Field {
    fn text_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        None
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _: crate::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// Input handlers are carried: the focused input drawn from last frame is
/// still the window's input handler.
#[test]
fn input_handlers_are_carried() {
    let mut pair = host(|_, cx| Field {
        focus: cx.focus_handle(),
    });
    pair.update(|host, window, cx| {
        let focus = host.child.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    pair.draw("focused");
    tick(&mut pair, "reused");
    tick(&mut pair, "reused again");
    let has_handler = pair.retained(|_, window, _| {
        let handler = window.platform_window.take_input_handler();
        let has = handler.is_some();
        if let Some(handler) = handler {
            window.platform_window.set_input_handler(handler);
        }
        has
    });
    assert!(has_handler);
}

struct Cursors;

impl Render for Cursors {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id("pointer").size(px(40.)).cursor_pointer()
    }
}

/// Cursor styles are carried, against the hitboxes carried with them.
#[test]
fn cursor_styles_are_carried() {
    let mut pair = host(|_, _| Cursors);
    tick(&mut pair, "reused");
    let cursors = pair.retained(|_, window, _| window.rendered_frame.cursor_styles.len());
    assert_eq!(cursors, 1);
}

struct TabStops {
    handles: [FocusHandle; 2],
}

impl Render for TabStops {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size(px(40.))
            .child(div().size(px(10.)).track_focus(&self.handles[0]))
            .child(div().size(px(10.)).track_focus(&self.handles[1]))
    }
}

/// Tab stops are carried: tabbing moves through a child drawn from last
/// frame.
#[test]
fn tab_stops_are_carried() {
    let mut pair = host(|_, cx| TabStops {
        handles: [
            cx.focus_handle().tab_index(1).tab_stop(true),
            cx.focus_handle().tab_index(2).tab_stop(true),
        ],
    });
    tick(&mut pair, "reused");
    pair.update(|_, window, cx| window.focus_next(cx));
    let focused_first = pair
        .retained(|host, window, cx| host.read(cx).child.read(cx).handles[0].is_focused(window));
    assert!(focused_first);
}

struct TitleBar;

impl Render for TitleBar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("drag area")
            .size(px(40.))
            .window_control_area(WindowControlArea::Drag)
    }
}

/// Window-control hitboxes are carried.
#[test]
fn window_control_hitboxes_are_carried() {
    let mut pair = host(|_, _| TitleBar);
    tick(&mut pair, "reused");
    let controls =
        pair.retained(|_, window, _| window.rendered_frame.window_control_hitboxes.len());
    assert_eq!(controls, 1);
}

struct Positioned;

impl Render for Positioned {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("positioned host")
            .relative()
            .size(px(60.))
            .child(
                div()
                    .id("fixed")
                    .fixed()
                    .top(px(100.))
                    .left(px(100.))
                    .size(px(20.))
                    .bg(crate::red()),
            )
            .child(
                div()
                    .id("absolute")
                    .absolute()
                    .top(px(5.))
                    .left(px(5.))
                    .size(px(20.))
                    .bg(crate::green()),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    |bounds, _, window, _| {
                        window.paint_top_layer(bounds, |window| {
                            window.paint_quad(crate::fill(bounds, crate::blue()))
                        })
                    },
                )
                .size(px(10.)),
            )
    }
}

/// Fixed, absolute and top-layer content keeps its draw-order bands and its
/// visual bounds when its view is drawn from last frame.
#[test]
fn positioned_and_top_layer_bands_and_visual_bounds_are_carried() {
    let mut pair = host(|_, _| Positioned);
    tick(&mut pair, "reused");
    tick(&mut pair, "reused again");
}

struct Effects;

impl Render for Effects {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(60.)).child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, _| {
                    window.paint_screen_effect_region("reused region", bounds, Default::default())
                },
            )
            .size(px(30.)),
        )
    }
}

/// Screen-effect requests a child made are neither lost nor doubled when it
/// is drawn from last frame.
#[test]
fn window_owned_effect_requests_are_neither_lost_nor_doubled() {
    let mut pair = host(|_, _| Effects);
    tick(&mut pair, "reused");
    tick(&mut pair, "reused again");
}

struct Inspectable;

impl Render for Inspectable {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("outer")
            .size(px(60.))
            .child(div().id("inner a").size(px(10.)))
            .child(div().id("inner b").size(px(10.)).child("label"))
    }
}

/// The accessibility nodes of a view drawn from last frame are carried under
/// their parents: the tree equals the one drawn from scratch, and it holds
/// the view's elements.
#[test]
fn accessibility_nodes_are_carried() {
    let mut pair = host(|_, _| Inspectable);
    let inspections = std::cell::RefCell::new(Vec::new());
    pair.each(|cx, window| {
        let inspection = cx
            .update_window(window, |_, window, _| window.begin_a11y_inspection())
            .unwrap();
        inspections.borrow_mut().push(inspection);
    });
    pair.draw("inspected");
    tick(&mut pair, "reused");
    let nodes = pair.retained(|_, window, _| window.a11y_tree().map_or(0, |tree| tree.nodes.len()));
    assert!(nodes >= 4, "{nodes}");
    drop(inspections);
}

/// Lays its child view out and prepaints it, discards that prepaint through
/// a rollback, and lays out and prepaints it again, as a list retrying an
/// autoscroll does.
struct RollingBack {
    child: Entity<Clicks>,
    kept: Option<AnyElement>,
}

impl IntoElement for RollingBack {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for RollingBack {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<crate::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size = size(px(80.).into(), px(80.).into());
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let records = window.next_frame.fast_retained.len();
        let available = size(px(80.), px(80.)).into();
        let _ = window.transact(|window| {
            let mut child = self.child.clone().into_any_element();
            child.layout_as_root(available, window, cx);
            child.prepaint_at(bounds.origin, window, cx);
            Err::<(), ()>(())
        });
        assert_eq!(window.next_frame.fast_retained.len(), records);
        let mut child = self.child.clone().into_any_element();
        child.layout_as_root(available, window, cx);
        child.prepaint_at(bounds.origin, window, cx);
        self.kept = Some(child);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(child) = self.kept.as_mut() {
            child.paint(window, cx);
        }
    }
}

struct RollbackHost {
    child: Entity<Clicks>,
    tick: usize,
}

impl Render for RollbackHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(RollingBack {
                child: self.child.clone(),
                kept: None,
            })
            .child(format!("tick {}", self.tick))
    }
}

/// A rollback drops the records the discarded prepaint made: the child
/// prepainted twice is recorded once, findable
/// to be drawn again, and the frame equals the frame drawn from scratch.
#[test]
fn a_rollback_truncates_the_records() {
    let mut pair = ViewPair::open(|_, cx| RollbackHost {
        child: cx.new(|_| Clicks { clicks: 0 }),
        tick: 0,
    });
    pair.pointer_away();
    pair.draw("second frame");
    for step in 0..2 {
        pair.update(|host, _, cx| {
            host.tick += 1;
            cx.notify();
        });
        pair.draw(&format!("step {step}"));
        let (records, findable) = pair.retained(|host, window, cx| {
            let child = crate::ElementId::View(host.read(cx).child.entity_id());
            let retained = &window.rendered_frame.fast_retained;
            let records = retained
                .records
                .iter()
                .filter(|record| record.id.last() == Some(&child))
                .collect::<Vec<_>>();
            let findable = records
                .first()
                .is_some_and(|record| retained.find(&record.id).is_some());
            (records.len(), findable)
        });
        assert_eq!(records, 1, "step {step}");
        assert!(findable, "step {step}");
    }
    pair.click(point(px(10.), px(10.)));
    let clicks = pair.retained(|host, _, cx| host.read(cx).child.read(cx).clicks);
    assert_eq!(clicks, 1);
}

/// A section view whose sticky header pins against a scroll container
/// outside it.
struct Section {
    index: usize,
}

impl Render for Section {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("section", self.index))
            .child(
                div()
                    .id("header")
                    .sticky()
                    .top(px(0.))
                    .h(px(16.))
                    .bg(crate::red())
                    .child(format!("section {}", self.index)),
            )
            .children((0..6usize).map(|row| div().id(("row", row)).h(px(14.)).child("row")))
    }
}

struct Sections {
    sections: Vec<Entity<Section>>,
    scroll: crate::ScrollHandle,
    tick: usize,
}

impl Render for Sections {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(format!("tick {}", self.tick))
            .child(
                div()
                    .id("scroller")
                    .h(px(120.))
                    .w(px(200.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(self.sections.iter().cloned()),
            )
    }
}

/// Sticky headers pinned by a scroll container outside their view stay
/// pinned as the container scrolls and as the views around them are built:
/// a view whose content registers with an outer scroll container is laid
/// out by building it, never from its record, so its candidates are always
/// there for the container to pin.
#[test]
fn sticky_candidates_of_an_outer_scroll_container_stay_pinned() {
    let scroll = crate::ScrollHandle::new();
    let mut pair = ViewPair::open(move |_, cx| Sections {
        sections: (0..3).map(|index| cx.new(|_| Section { index })).collect(),
        scroll: scroll.clone(),
        tick: 0,
    });
    pair.pointer_away();
    pair.draw("second frame");
    for (step, offset) in [0., 30., 30., 75., 140., 140.].into_iter().enumerate() {
        pair.update(|sections, _, cx| {
            sections.scroll.request_offset(
                point(px(0.), px(-offset)),
                crate::ScrollMutationSource::Programmatic,
            );
            sections.tick += 1;
            cx.notify();
        });
        pair.draw(&format!("step {step}"));
    }
}

/// A view whose content defers a hover-styled overlay, as a menu does.
struct Popover;

impl Render for Popover {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(40.)).child(deferred(
            anchored().child(
                div()
                    .id("pop")
                    .size(px(20.))
                    .bg(crate::red())
                    .hover(|style| style.bg(crate::blue())),
            ),
        ))
    }
}

struct Around {
    popover: Entity<Popover>,
    tick: usize,
}

impl Render for Around {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size(px(100.))
            .child(self.popover.clone())
            .child(format!("tick {}", self.tick))
    }
}

/// Found by review: a deferred draw copied with the view that deferred it
/// lost the records around it, so their hovers named hitboxes outside them
/// and they were never drawn from last frame again. Copied, it is still a
/// part of them: after the view around the popover is built once, still
/// frames build nothing.
#[test]
fn a_copied_deferred_draw_stays_a_part_of_the_views_around_it() {
    let mut pair = host(|_, cx| Around {
        popover: cx.new(|_| Popover),
        tick: 0,
    });
    let stats = pair.draw("still");
    assert_eq!(stats.views_built, 0, "{stats:?}");
    pair.update_app(|host, cx| {
        let around = host.read(cx).child.clone();
        around.update(cx, |around, cx| {
            around.tick += 1;
            cx.notify();
        });
    });
    let stats = pair.draw("around built");
    assert_eq!(
        stats.views_built, 2,
        "the host and the view around: {stats:?}"
    );
    for step in 0..3 {
        let stats = pair.draw(&format!("still {step}"));
        assert_eq!(stats.views_built, 0, "still {step}: {stats:?}");
    }
}
