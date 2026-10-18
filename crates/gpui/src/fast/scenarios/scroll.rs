//! Pointer and scrolling: a hover sweep across rows, and real wheel events
//! over every kind of scroll container gpux has.

use std::{cell::Cell, rc::Rc};

use crate::{
    AnyView, App, Axis, Context, Entity, FastStats, InputEvent as _, IntoElement, ListAlignment,
    ListState, Modifiers, MouseMoveEvent, Pixels, Point, Render, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, SharedString, Size, TouchPhase, UniformListScrollHandle,
    VirtualListExternalScrollBehavior, VirtualListScrollHandle, VirtualStickyRow, Window,
    backdrop_blur, div, effect_frame, hsla, list, point, prelude::*, px, size, uniform_list,
    virtual_list,
};

use super::{
    Fixture, Mode, Scenario, check, drawn_per_frame, expect_frames, place_pointer,
    views_built_at_least,
};

/// Where every scroll container of these scenarios sits: its top left and size.
const VIEWPORT: (f32, f32, f32, f32) = (0., 0., 400., 300.);
const ROW_HEIGHT: f32 = 20.;
const ROWS: usize = 400;
/// Pixels a wheel event scrolls.
const WHEEL_STEP: f32 = 24.;
/// Frames a wheel scrolls one way before it turns back.
const WHEEL_TURN: usize = 30;
/// The strip a scrolling `div` leaves beside its rows. A scroll layer shows a
/// frame from its tiles only where no hover its content read can have
/// changed (a list's layer renders the rows whose hover did instead), so the
/// scenarios scroll a `div` both ways: `scroll-child-view` with the pointer
/// resting here, over no row, and `scroll-same-view` with it over the rows,
/// whose hover changes with every wheel step.
const GUTTER: f32 = 40.;
/// The rows of the `virtual_list` scenarios, of varying size.
const VIRTUAL_ROWS: usize = 10_000;
/// How wide the driver beside a `SynchronizedOffset` list is.
const DRIVER: f32 = 120.;

/// The extent along its axis of virtual row `index`.
fn virtual_extent(index: usize, axis: Axis) -> f32 {
    match axis {
        Axis::Vertical => ROW_HEIGHT - 4. + (index % 5) as f32 * 4.,
        Axis::Horizontal => 40. + (index % 5) as f32 * 12.,
    }
}

pub(super) fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "hover-sweep",
            description: "40 rows with hover styles in one view; the pointer moves to the next \
                          row per frame",
            build: |_, cx| Box::new(Pointer::new(cx)),
            warm_up_frames: 3,
        },
        scroll_scenario(
            "scroll-child-view",
            "a scroll area whose 400 rows are a child view; a wheel event per frame, the pointer \
             beside the rows",
            Content::ChildView,
        ),
        scroll_scenario(
            "scroll-same-view",
            "a scroll area whose 400 rows are in its own view; a wheel event per frame, the \
             pointer over the rows, whose hover styles follow it",
            Content::SameView,
        ),
        scroll_scenario(
            "scroll-uniform-list",
            "a uniform list of 400 rows; a wheel event per frame",
            Content::UniformList,
        ),
        scroll_scenario(
            "scroll-list",
            "a list of 400 rows of varying height; a wheel event per frame",
            Content::List,
        ),
        scroll_scenario(
            "scroll-virtual-list",
            "a virtual list of 10,000 rows of varying height; a wheel event per frame",
            Content::VirtualList,
        ),
        scroll_scenario(
            "scroll-virtual-list-horizontal",
            "a horizontal virtual list of 10,000 columns of varying width; a wheel event per \
             frame",
            Content::VirtualListHorizontal,
        ),
        scroll_scenario(
            "scroll-virtual-list-sticky",
            "a virtual list of 10,000 rows with a sticky row every 50; a wheel event per frame",
            Content::VirtualListSticky,
        ),
        scroll_scenario(
            "scroll-virtual-list-synchronized",
            "a virtual list of 10,000 rows following a scrolling div beside it \
             (`SynchronizedOffset`); a wheel event per frame over the div",
            Content::VirtualListSynchronized,
        ),
        scroll_scenario(
            "scroll-virtual-list-parent-viewport",
            "a virtual list of 10,000 rows of varying height in the flow of a scrolling div \
             (`ParentViewport`); a wheel event per frame",
            Content::VirtualListParentViewport,
        ),
        scroll_scenario(
            "chat-scroll",
            "a bottom-aligned list of 400 chat messages of varying length; a wheel event per \
             frame",
            Content::Chat,
        ),
        scroll_scenario(
            "sticky-scroll",
            "a scroll area of sections with sticky headers; a wheel event per frame",
            Content::Sticky,
        ),
        scroll_scenario(
            "effect-scroll",
            "a scroll area holding an effect frame with a backdrop blur, which a scroll layer \
             cannot composite; a wheel event per frame",
            Content::Effect,
        ),
    ]
}

fn scroll_scenario(name: &'static str, description: &'static str, content: Content) -> Scenario {
    Scenario {
        name,
        description,
        build: match content {
            Content::ChildView => |_, cx| Box::new(Scrolling::new(cx, Content::ChildView)),
            Content::SameView => |_, cx| Box::new(Scrolling::new(cx, Content::SameView)),
            Content::UniformList => |_, cx| Box::new(Scrolling::new(cx, Content::UniformList)),
            Content::List => |_, cx| Box::new(Scrolling::new(cx, Content::List)),
            Content::Chat => |_, cx| Box::new(Scrolling::new(cx, Content::Chat)),
            Content::Sticky => |_, cx| Box::new(Scrolling::new(cx, Content::Sticky)),
            Content::Effect => |_, cx| Box::new(Scrolling::new(cx, Content::Effect)),
            Content::VirtualList => |_, cx| Box::new(Scrolling::new(cx, Content::VirtualList)),
            Content::VirtualListHorizontal => {
                |_, cx| Box::new(Scrolling::new(cx, Content::VirtualListHorizontal))
            }
            Content::VirtualListSticky => {
                |_, cx| Box::new(Scrolling::new(cx, Content::VirtualListSticky))
            }
            Content::VirtualListSynchronized => {
                |_, cx| Box::new(Scrolling::new(cx, Content::VirtualListSynchronized))
            }
            Content::VirtualListParentViewport => {
                |_, cx| Box::new(Scrolling::new(cx, Content::VirtualListParentViewport))
            }
        },
        warm_up_frames: 3,
    }
}

/// The centre of the scroll containers, where the pointer is over a list.
fn viewport_center() -> Point<Pixels> {
    point(
        px(VIEWPORT.0 + VIEWPORT.2 / 2.),
        px(VIEWPORT.1 + VIEWPORT.3 / 2.),
    )
}

/// Where the pointer is over a scrolling `div`: in its [`GUTTER`].
fn gutter_center() -> Point<Pixels> {
    point(
        px(VIEWPORT.0 + VIEWPORT.2 - GUTTER / 2.),
        px(VIEWPORT.1 + VIEWPORT.3 / 2.),
    )
}

fn row(index: usize, height: f32) -> impl IntoElement {
    div()
        .id(("row", index))
        .flex()
        .flex_row()
        .gap_2()
        .h(px(height))
        .bg(if index.is_multiple_of(2) {
            hsla(0., 0., 0.12, 1.)
        } else {
            hsla(0., 0., 0.16, 1.)
        })
        .hover(|style| style.bg(hsla(0.6, 0.4, 0.3, 1.)))
        .child(SharedString::from(format!("row {index}")))
        .child(div().w(px(8. + (index % 5) as f32 * 6.)).h(px(8.)).bg(hsla(
            index as f32 / 40. % 1.,
            0.6,
            0.5,
            1.,
        )))
}

/// Column `index` of `scroll-virtual-list-horizontal`: a strip as wide as its extent, with its
/// number, drawing nothing outside its own room.
fn column(index: usize) -> impl IntoElement {
    div()
        .id(("column", index))
        .w(px(virtual_extent(index, Axis::Horizontal)))
        .h_full()
        .overflow_hidden()
        .bg(if index.is_multiple_of(2) {
            hsla(0., 0., 0.12, 1.)
        } else {
            hsla(0., 0., 0.16, 1.)
        })
        .hover(|style| style.bg(hsla(0.6, 0.4, 0.3, 1.)))
        .child(SharedString::from(format!("{}", index % 1000)))
}

/// What a scroll scenario scrolls.
#[derive(Clone, Copy, PartialEq)]
enum Content {
    ChildView,
    SameView,
    UniformList,
    List,
    Chat,
    Sticky,
    Effect,
    VirtualList,
    VirtualListHorizontal,
    VirtualListSticky,
    VirtualListSynchronized,
    VirtualListParentViewport,
}

impl Content {
    fn is_virtual(self) -> bool {
        matches!(
            self,
            Content::VirtualList
                | Content::VirtualListHorizontal
                | Content::VirtualListSticky
                | Content::VirtualListSynchronized
                | Content::VirtualListParentViewport
        )
    }
}

/// The rows of `scroll-child-view`, a view of their own.
pub(crate) struct Rows;

impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .children((0..ROWS).map(|index| row(index, ROW_HEIGHT)))
    }
}

pub(crate) struct Scroller {
    content: Content,
    scroll: ScrollHandle,
    uniform: UniformListScrollHandle,
    list: ListState,
    rows: Entity<Rows>,
    /// The virtual list's own handle, and the one it shares with its driver.
    virtual_handle: VirtualListScrollHandle,
    synchronized: VirtualListScrollHandle,
    /// The scroll area's handle, as the handle of a virtual list in its flow.
    parent_viewport: VirtualListScrollHandle,
    /// The rows a virtual list's row renderer rendered.
    virtual_rows: Rc<Cell<u64>>,
}

impl Scroller {
    /// A `virtual_list` of [`VIRTUAL_ROWS`] rows along `axis`, counting the rows it renders.
    fn virtual_list(&self, axis: Axis, cx: &mut Context<Self>) -> crate::VirtualList {
        let sizes: Rc<Vec<Size<Pixels>>> = Rc::new(
            (0..VIRTUAL_ROWS)
                .map(|index| match axis {
                    Axis::Vertical => size(px(VIEWPORT.2), px(virtual_extent(index, axis))),
                    Axis::Horizontal => size(px(virtual_extent(index, axis)), px(VIEWPORT.3)),
                })
                .collect(),
        );
        let rendered = self.virtual_rows.clone();
        virtual_list(
            cx.entity(),
            "virtual rows",
            axis,
            sizes,
            move |_, range, _, _| {
                rendered.set(rendered.get() + range.len() as u64);
                range
                    .map(|index| match axis {
                        Axis::Vertical => {
                            row(index, virtual_extent(index, axis)).into_any_element()
                        }
                        Axis::Horizontal => column(index).into_any_element(),
                    })
                    .collect::<Vec<_>>()
            },
        )
    }
}

impl Render for Scroller {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let area = div()
            .absolute()
            .left(px(VIEWPORT.0))
            .top(px(VIEWPORT.1))
            .w(px(VIEWPORT.2))
            .h(px(VIEWPORT.3));
        let scroll_area = || {
            div()
                .id("scroll area")
                .size_full()
                .pr(px(GUTTER))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
        };
        let content = match self.content {
            Content::ChildView => scroll_area().child(self.rows.clone()).into_any_element(),
            Content::SameView => scroll_area()
                .children((0..ROWS).map(|index| row(index, ROW_HEIGHT)))
                .into_any_element(),
            Content::UniformList => uniform_list("rows", ROWS, |range, _, _| {
                range.map(|index| row(index, ROW_HEIGHT)).collect()
            })
            .track_scroll(&self.uniform)
            .size_full()
            .into_any_element(),
            Content::List => list(self.list.clone(), |index, _, _| {
                row(index, ROW_HEIGHT + (index % 4) as f32 * 8.).into_any_element()
            })
            .size_full()
            .into_any_element(),
            Content::Chat => list(self.list.clone(), |index, _, _| {
                let words = 3 + (index * 7) % 23;
                div()
                    .id(("message", index))
                    .p_1()
                    .bg(if index.is_multiple_of(2) {
                        hsla(0.6, 0.3, 0.2, 1.)
                    } else {
                        hsla(0.3, 0.3, 0.2, 1.)
                    })
                    .child(SharedString::from(
                        (0..words)
                            .map(|word| ["hello", "fast", "mode", "chat", "gpux"][word % 5])
                            .collect::<Vec<_>>()
                            .join(" "),
                    ))
                    .into_any_element()
            })
            .size_full()
            .into_any_element(),
            Content::Sticky => scroll_area()
                .children((0..20).map(|section| {
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id(("header", section))
                                .sticky()
                                .top(px(0.))
                                .h(px(24.))
                                .bg(hsla(0.3, 0.5, 0.3, 1.))
                                .child(SharedString::from(format!("Section {section}"))),
                        )
                        .children((0..20).map(move |index| row(section * 20 + index, ROW_HEIGHT)))
                }))
                .into_any_element(),
            Content::Effect => scroll_area()
                .children((0..ROWS).map(|index| {
                    if index % 40 == 5 {
                        effect_frame()
                            .effect(backdrop_blur(px(6.)))
                            .child(row(index, ROW_HEIGHT))
                            .into_any_element()
                    } else {
                        row(index, ROW_HEIGHT).into_any_element()
                    }
                }))
                .into_any_element(),
            Content::VirtualList => self
                .virtual_list(Axis::Vertical, cx)
                .track_scroll(&self.virtual_handle)
                .size_full()
                .into_any_element(),
            Content::VirtualListHorizontal => self
                .virtual_list(Axis::Horizontal, cx)
                .track_scroll(&self.virtual_handle)
                .size_full()
                .into_any_element(),
            Content::VirtualListSticky => self
                .virtual_list(Axis::Vertical, cx)
                .track_scroll(&self.virtual_handle)
                .sticky_rows(
                    (0..VIRTUAL_ROWS)
                        .step_by(50)
                        .map(|index| (index, VirtualStickyRow::top())),
                )
                .size_full()
                .into_any_element(),
            Content::VirtualListSynchronized => {
                let extent: f32 = (0..VIRTUAL_ROWS)
                    .map(|index| virtual_extent(index, Axis::Vertical))
                    .sum();
                div()
                    .size_full()
                    .flex()
                    .child(
                        div()
                            .id("driver")
                            .w(px(DRIVER))
                            .h_full()
                            .overflow_y_scroll()
                            .track_scroll(self.synchronized.base_handle())
                            .child(div().w(px(DRIVER)).h(px(extent)).bg(hsla(0., 0., 0.2, 1.))),
                    )
                    .child(
                        // Clipped to the list's bounds, as a table's body is, so that the list
                        // has a layer of its own.
                        div().flex_1().h_full().overflow_hidden().child(
                            self.virtual_list(Axis::Vertical, cx)
                                .track_external_scroll(
                                    &self.synchronized,
                                    VirtualListExternalScrollBehavior::SynchronizedOffset,
                                )
                                .size_full(),
                        ),
                    )
                    .into_any_element()
            }
            // The list lies in the scroll area's flow, between two strips, and is part of the
            // area's layer.
            Content::VirtualListParentViewport => scroll_area()
                .flex()
                .flex_col()
                .child(div().flex_none().h(px(60.)).w_full())
                .child(
                    self.virtual_list(Axis::Vertical, cx)
                        .track_external_scroll(
                            &self.parent_viewport,
                            VirtualListExternalScrollBehavior::ParentViewport,
                        )
                        .w_full(),
                )
                .child(div().flex_none().h(px(60.)).w_full())
                .into_any_element(),
        };
        // An opaque panel, as an application's window has: what a scroll
        // layer's tiles are cleared with.
        div()
            .size_full()
            .relative()
            .bg(hsla(0., 0., 0.08, 1.))
            .child(area.child(content))
    }
}

struct Scrolling {
    scroller: Entity<Scroller>,
    content: Content,
    /// Whether the scenario's window runs scroll layers: a mode with them
    /// leaves them off in a window whose renderer rasterizes no tile.
    layers: bool,
    /// The rows a virtual list's row renderer rendered.
    virtual_rows: Rc<Cell<u64>>,
}

impl Scrolling {
    fn new(cx: &mut App, content: Content) -> Self {
        let rows = cx.new(|_| Rows);
        let list = match content {
            Content::Chat => ListState::new(ROWS, ListAlignment::Bottom, px(200.)),
            _ => ListState::new(ROWS, ListAlignment::Top, px(200.)),
        };
        let virtual_rows = Rc::new(Cell::new(0));
        let scroll = ScrollHandle::new();
        let scroller = cx.new(|_| Scroller {
            content,
            scroll: scroll.clone(),
            uniform: UniformListScrollHandle::new(),
            list,
            rows,
            virtual_handle: VirtualListScrollHandle::new(),
            synchronized: VirtualListScrollHandle::new(),
            parent_viewport: scroll.into(),
            virtual_rows: virtual_rows.clone(),
        });
        Self {
            scroller,
            content,
            layers: false,
            virtual_rows,
        }
    }
}

impl Fixture for Scrolling {
    fn root(&self) -> AnyView {
        self.scroller.clone().into()
    }

    fn step(&mut self, frame: usize, window: &mut Window, cx: &mut App) {
        // Scroll one way, then back, so the wheel never runs out of content.
        let down = (frame / WHEEL_TURN).is_multiple_of(2);
        self.layers = window.fast_mode().scroll_layers;
        let content = self.content;
        let chat = content == Content::Chat;
        // A chat starts at its bottom, so it scrolls up first.
        let delta = if down != chat {
            -WHEEL_STEP
        } else {
            WHEEL_STEP
        };
        let position = match content {
            Content::SameView
            | Content::UniformList
            | Content::List
            | Content::Chat
            | Content::VirtualList
            | Content::VirtualListHorizontal
            | Content::VirtualListSticky
            | Content::VirtualListParentViewport => viewport_center(),
            Content::ChildView | Content::Sticky | Content::Effect => gutter_center(),
            // Over the div driving the list.
            Content::VirtualListSynchronized => point(
                px(VIEWPORT.0 + DRIVER / 2.),
                px(VIEWPORT.1 + VIEWPORT.3 / 2.),
            ),
        };
        let delta = match content {
            Content::VirtualListHorizontal => point(px(delta), px(0.)),
            _ => point(px(0.), px(delta)),
        };
        place_pointer(window, position);
        window.dispatch_event(
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(delta),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        );
    }

    fn rows_rendered(&self) -> Option<u64> {
        self.content.is_virtual().then(|| self.virtual_rows.get())
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        if self.layers {
            return self.expect_layers(stats);
        }
        // Every scrolled frame builds the scroller.
        views_built_at_least(mode, stats, 2)?;
        if self.content.is_virtual() {
            // The rows in view with their glyphs, or the six or so columns in view with their
            // numbers.
            let primitives = if self.content == Content::VirtualListHorizontal {
                12
            } else {
                30
            };
            return drawn_per_frame(stats, primitives);
        }
        // The backgrounds and glyphs of the rows (or messages) in view, well
        // above the 31 backgrounds alone.
        drawn_per_frame(stats, 60)
    }
}

impl Scrolling {
    /// What scroll layers do with each kind of content.
    fn expect_layers(&self, stats: &FastStats) -> Result<(), String> {
        use crate::fast::stats::LayerBypassReason;
        let frames = stats.frames;
        let bypassed = |reason: LayerBypassReason| stats.layer_bypasses[reason as usize];
        let all_bypasses: u64 = stats.layer_bypasses.iter().sum();
        match self.content {
            // A scrolling `div` whose content only scrolls: promoted while
            // the scenario warms up, every frame shows the layer's tiles,
            // painted again only where the scroll runs past what the layer
            // holds (twice its viewport on either side).
            Content::ChildView => {
                check(
                    stats.layers_composited + stats.layers_repainted == frames,
                    || format!("a frame went without its layer: {stats:?}"),
                )?;
                check(all_bypasses == 0, || {
                    format!("a frame was drawn on today's path: {stats:?}")
                })?;
                check(stats.layers_repainted <= 2, || {
                    format!("the layer was painted more than the scroll exposes: {stats:?}")
                })?;
                check(stats.tiles_composited >= frames, || {
                    format!("a frame composited no tile: {stats:?}")
                })?;
                // The view holding the container is built for its scroll, and
                // the rows' view only on the frames that paint the layer.
                check(stats.views_built == frames + stats.layers_repainted, || {
                    format!("a composited frame rendered the content's view: {stats:?}")
                })
            }
            // The wheel turns over rows with hover styles: every step moves
            // another row under the pointer, so no frame only scrolls. The
            // layer is painted again until the policy gives it up, and the
            // container is drawn as without one from then on.
            Content::SameView => {
                check(stats.layers_composited == 0, || {
                    format!("a frame showed tiles painted for other hovers: {stats:?}")
                })?;
                check(stats.layers_repainted + all_bypasses == frames, || {
                    format!("a frame was not counted: {stats:?}")
                })?;
                check(bypassed(LayerBypassReason::Demoted) + 2 >= frames, || {
                    format!("the layer kept being painted for nothing: {stats:?}")
                })?;
                check(stats.views_built == frames, || {
                    format!("other views were built than the scroller: {stats:?}")
                })?;
                // The frames that painted the layer's content hold it in tiles,
                // which the window's own primitives do not count; the others
                // draw it as without a layer.
                let direct = frames - stats.layers_repainted;
                let drawn = stats.primitives_painted + stats.primitives_copied;
                check(drawn >= 60 * direct, || {
                    format!(
                        "{drawn} primitives over {direct} frames drawn without the layer, \
                         below 60 a frame: the text drew no glyphs? {stats:?}"
                    )
                })
            }
            // Sticky headers are painted where they stick, above the rows
            // and not with them, and a backdrop blur samples what is under
            // it: no tile holds either, and every frame is drawn as without
            // layers.
            Content::Sticky | Content::Effect => {
                check(
                    stats.layers_composited + stats.layers_repainted == 0,
                    || format!("a layer held content it cannot composite: {stats:?}"),
                )?;
                check(bypassed(LayerBypassReason::Ineligible) == frames, || {
                    format!("a frame was not counted as ineligible: {stats:?}")
                })?;
                views_built_at_least(Mode::Layers, stats, 2)?;
                drawn_per_frame(stats, 60)
            }
            // A virtual list's layer holds its rows one by one: promoted while
            // the scenario warms up, every frame shows the layer's tiles,
            // keeping the rows it held and rendering only those the scroll
            // uncovers and the two whose hover the pointer changes as the
            // rows move under it.
            // A virtual list's layer, on either axis, with its sticky rows
            // drawn above it, or driven by a sibling (whose own layer
            // composites too), holds its rows as a uniform list's does.
            Content::VirtualList
            | Content::VirtualListHorizontal
            | Content::VirtualListSticky
            | Content::VirtualListSynchronized => {
                let layers = if self.content == Content::VirtualListSynchronized {
                    2
                } else {
                    1
                };
                check(
                    stats.layers_composited + stats.layers_repainted == layers * frames,
                    || format!("a frame went without its layers: {stats:?}"),
                )?;
                check(all_bypasses == 0, || {
                    format!("a frame was drawn on today's path: {stats:?}")
                })?;
                check(stats.layers_repainted <= 2 * layers, || {
                    format!("the rows were painted afresh for a scroll: {stats:?}")
                })?;
                check(stats.tiles_composited >= frames, || {
                    format!("a frame composited no tile: {stats:?}")
                })?;
                check(stats.list_rows_rendered <= 4 * frames, || {
                    format!("frames rendered rows their layer held: {stats:?}")
                })?;
                check(stats.list_rows_kept >= 5 * frames, || {
                    format!("the layer kept too few rows to scroll over: {stats:?}")
                })?;
                views_built_at_least(Mode::Layers, stats, 2)
            }
            // A list in a scrolling div's flow is part of the div's layer, which stays
            // promoted: the list renders the rows of the region the layer paints when it paints,
            // and none on the frames that composite it.
            Content::VirtualListParentViewport => {
                check(
                    stats.layers_composited + stats.layers_repainted == frames,
                    || format!("a frame went without the div's layer: {stats:?}"),
                )?;
                check(all_bypasses == 0, || {
                    format!("a frame was drawn on today's path: {stats:?}")
                })?;
                check(stats.layers_repainted <= 2, || {
                    format!("the layer was painted more than the scroll exposes: {stats:?}")
                })?;
                check(stats.tiles_composited >= frames, || {
                    format!("a frame composited no tile: {stats:?}")
                })?;
                views_built_at_least(Mode::Layers, stats, 2)
            }
            Content::UniformList | Content::List | Content::Chat => {
                check(
                    stats.layers_composited + stats.layers_repainted == frames,
                    || format!("a frame went without its layer: {stats:?}"),
                )?;
                check(all_bypasses == 0, || {
                    format!("a frame was drawn on today's path: {stats:?}")
                })?;
                check(stats.layers_repainted <= 2, || {
                    format!("the rows were painted afresh for a scroll: {stats:?}")
                })?;
                check(stats.tiles_composited >= frames, || {
                    format!("a frame composited no tile: {stats:?}")
                })?;
                check(stats.list_rows_rendered <= 4 * frames, || {
                    format!("frames rendered rows their layer held: {stats:?}")
                })?;
                // A layer holds the rows it shows and a fifth of a viewport of overscan on each
                // side (`paint::LIST_OVERSCAN_VIEWPORTS`), which these small lists make a few rows.
                check(stats.list_rows_kept >= 5 * frames, || {
                    format!("the layer kept too few rows to scroll over: {stats:?}")
                })?;
                // The view holding the list is built for its scroll.
                views_built_at_least(Mode::Layers, stats, 2)
            }
        }
    }
}

/// Rows with hover styles, swept by the pointer.
pub(crate) struct HoverRows;

impl Render for HoverRows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .children((0..40).map(|index| row(index, ROW_HEIGHT)))
    }
}

struct Pointer {
    rows: Entity<HoverRows>,
}

impl Pointer {
    fn new(cx: &mut App) -> Self {
        Self {
            rows: cx.new(|_| HoverRows),
        }
    }
}

impl Fixture for Pointer {
    fn root(&self) -> AnyView {
        self.rows.clone().into()
    }

    fn step(&mut self, frame: usize, window: &mut Window, cx: &mut App) {
        let position = point(
            px(60.),
            px(ROW_HEIGHT / 2. + (frame % 40) as f32 * ROW_HEIGHT),
        );
        place_pointer(window, position);
        window.dispatch_event(
            MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        views_built_at_least(mode, stats, 2)?;
        // A background and the glyphs of every row.
        drawn_per_frame(stats, 40 * 5)
    }
}
