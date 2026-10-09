//! Pointer and scrolling: a hover sweep across rows, and real wheel events
//! over every kind of scroll container gpux has.

use crate::{
    AnyView, App, Context, Entity, FastStats, InputEvent as _, IntoElement, ListAlignment,
    ListState, Modifiers, MouseMoveEvent, Pixels, Point, Render, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, SharedString, TouchPhase, UniformListScrollHandle, Window, backdrop_blur,
    div, effect_frame, hsla, list, point, prelude::*, px, uniform_list,
};

use super::{Fixture, Mode, Scenario, at_least_per_frame, drawn_per_frame, expect_frames};

/// Where every scroll container of these scenarios sits: its top left and size.
const VIEWPORT: (f32, f32, f32, f32) = (0., 0., 400., 300.);
const ROW_HEIGHT: f32 = 20.;
const ROWS: usize = 400;
/// Pixels a wheel event scrolls.
const WHEEL_STEP: f32 = 24.;
/// Frames a wheel scrolls one way before it turns back.
const WHEEL_TURN: usize = 30;

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
            "a scroll area whose 400 rows are a child view; a wheel event per frame",
            Content::ChildView,
        ),
        scroll_scenario(
            "scroll-same-view",
            "a scroll area whose 400 rows are in its own view; a wheel event per frame",
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
        },
        warm_up_frames: 3,
    }
}

/// The centre of the scroll containers, where the pointer is.
fn viewport_center() -> Point<Pixels> {
    point(
        px(VIEWPORT.0 + VIEWPORT.2 / 2.),
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
}

impl Render for Scroller {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
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
        };
        div().size_full().relative().child(area.child(content))
    }
}

struct Scrolling {
    scroller: Entity<Scroller>,
}

impl Scrolling {
    fn new(cx: &mut App, content: Content) -> Self {
        let rows = cx.new(|_| Rows);
        let list = match content {
            Content::Chat => ListState::new(ROWS, ListAlignment::Bottom, px(200.)),
            _ => ListState::new(ROWS, ListAlignment::Top, px(200.)),
        };
        let scroller = cx.new(|_| Scroller {
            content,
            scroll: ScrollHandle::new(),
            uniform: UniformListScrollHandle::new(),
            list,
            rows,
        });
        Self { scroller }
    }
}

impl Fixture for Scrolling {
    fn root(&self) -> AnyView {
        self.scroller.clone().into()
    }

    fn step(&mut self, frame: usize, window: &mut Window, cx: &mut App) {
        // Scroll one way, then back, so the wheel never runs out of content.
        let down = (frame / WHEEL_TURN).is_multiple_of(2);
        let chat = self.scroller.read(cx).content == Content::Chat;
        // A chat starts at its bottom, so it scrolls up first.
        let delta = if down != chat {
            -WHEEL_STEP
        } else {
            WHEEL_STEP
        };
        window.dispatch_event(
            ScrollWheelEvent {
                position: viewport_center(),
                delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        );
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        // Until scroll layers land, every scrolled frame builds the scroller.
        at_least_per_frame(stats, "views_built", 2)?;
        // The glyphs of the rows (or messages) in view.
        drawn_per_frame(stats, 30)
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
        window.dispatch_event(
            MouseMoveEvent {
                position: point(
                    px(60.),
                    px(ROW_HEIGHT / 2. + (frame % 40) as f32 * ROW_HEIGHT),
                ),
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        at_least_per_frame(stats, "views_built", 2)?;
        // A background and the glyphs of every row.
        drawn_per_frame(stats, 40 * 5)
    }
}
