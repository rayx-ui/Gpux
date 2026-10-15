//! Scrolling: virtual lists of uniform and varying rows, a plain scrolling div, a data table.
//!
//! Every frame is one wheel step with the pointer over the content, dispatched as the platform
//! would. The rows have no hover styles, so these frames only scroll; the direction reverses
//! every few thousand pixels.

use std::rc::Rc;

use gpux::{
    App, AppContext as _, Context, IntoElement, ListAlignment, ListState, ParentElement as _,
    Render, SharedString, Styled as _, Window, div, list, point, prelude::*, px, rgb, uniform_list,
};

use crate::{
    Built,
    util::{Rng, color, scroll_step, words},
};

/// The middle of the window, over the content.
fn center(window: &Window) -> gpux::Point<gpux::Pixels> {
    let viewport = window.viewport_size();
    point(viewport.width / 2., viewport.height / 2.)
}

struct UniformRows {
    names: Rc<Vec<SharedString>>,
}

const UNIFORM_ROWS: usize = 100_000;

impl Render for UniformRows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let names = self.names.clone();
        uniform_list("uniform-rows", UNIFORM_ROWS, move |range, _, _| {
            range
                .map(|index| {
                    let name = names[index % names.len()].clone();
                    div()
                        .h(px(28.))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_sm()
                        .when(index % 2 == 1, |row| row.bg(rgb(0x23262c)))
                        .child(
                            div()
                                .w(px(64.))
                                .text_color(rgb(0x6e7681))
                                .child(format!("#{index}")),
                        )
                        .child(div().w(px(260.)).child(name))
                        .child(
                            div()
                                .w(px(120.))
                                .child(format!("{} KB", (index * 37) % 9999)),
                        )
                        .child(div().w(px(160.)).child(format!(
                            "2026-{:02}-{:02}",
                            index % 12 + 1,
                            index % 28 + 1
                        )))
                        .child(
                            div()
                                .w(px(120.))
                                .child(["owner", "editor", "viewer"][index % 3]),
                        )
                        .child(
                            div()
                                .px_2()
                                .rounded_sm()
                                .bg(color(index as u64 % 9))
                                .text_xs()
                                .child(["new", "open", "done", "late"][index % 4]),
                        )
                })
                .collect()
        })
        .size_full()
    }
}

pub fn uniform(_: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(19);
    let names = Rc::new((0..1024).map(|_| words(&mut rng, 3).into()).collect());
    let root = cx.new(|_| UniformRows { names });
    Built {
        root: root.into(),
        step: Box::new(|frame, window, cx| {
            scroll_step(frame, window, cx, center(window), 20_000., 36.)
        }),
    }
}

struct Messages {
    state: ListState,
}

impl Render for Messages {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_2().child(
            list(self.state.clone(), {
                let mut rng_texts: Option<Rc<Vec<(SharedString, SharedString)>>> = None;
                move |index, _, _| {
                    let texts = rng_texts.get_or_insert_with(message_texts).clone();
                    let (author, body) = texts[index % texts.len()].clone();
                    div()
                        .w_full()
                        .py_2()
                        .flex()
                        .gap_3()
                        .child(
                            div()
                                .size(px(32.))
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(color(index as u64)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div().flex().gap_2().text_sm().child(author).child(
                                        div()
                                            .text_color(rgb(0x6e7681))
                                            .child(format!("message {index}")),
                                    ),
                                )
                                .child(div().text_sm().line_height(px(19.)).child(body)),
                        )
                        .into_any_element()
                }
            })
            .size_full(),
        )
    }
}

thread_local! {
    static MESSAGE_TEXTS: Rc<Vec<(SharedString, SharedString)>> = {
        let mut rng = Rng::new(23);
        Rc::new(
            (0..2048)
                .map(|_| {
                    let author = words(&mut rng, 2).into();
                    let length = 4 + rng.below(90) as usize;
                    (author, words(&mut rng, length).into())
                })
                .collect(),
        )
    };
}

fn message_texts() -> Rc<Vec<(SharedString, SharedString)>> {
    MESSAGE_TEXTS.with(Rc::clone)
}

pub fn variable(_: &mut Window, cx: &mut App) -> Built {
    let state = ListState::new(20_000, ListAlignment::Top, px(200.));
    let root = cx.new(|_| Messages { state });
    Built {
        root: root.into(),
        step: Box::new(|frame, window, cx| {
            scroll_step(frame, window, cx, center(window), 20_000., 36.)
        }),
    }
}

struct Card {
    title: SharedString,
    body: SharedString,
    tags: [SharedString; 4],
}

struct Cards {
    cards: Vec<Card>,
}

impl Render for Cards {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("cards")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .justify_center()
            .items_start()
            .child(div().w(px(760.)).py_3().flex().flex_col().gap_2().children(
                self.cards.iter().enumerate().map(|(index, card)| {
                    div()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x3a3d45))
                        .bg(rgb(0x272a31))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .child(div().text_base().child(card.title.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x6e7681))
                                        .child(format!("#{index}")),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xb1b8c0))
                                .child(card.body.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .children(card.tags.iter().enumerate().map(|(tag_index, tag)| {
                                    div()
                                        .px_2()
                                        .rounded_sm()
                                        .text_xs()
                                        .bg(color((index + tag_index) as u64))
                                        .child(tag.clone())
                                })),
                        )
                }),
            ))
    }
}

pub fn div_cards(_: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(29);
    let cards = (0..400)
        .map(|_| Card {
            title: words(&mut rng, 3).into(),
            body: words(&mut rng, 24).into(),
            tags: [
                words(&mut rng, 1).into(),
                words(&mut rng, 1).into(),
                words(&mut rng, 1).into(),
                words(&mut rng, 1).into(),
            ],
        })
        .collect();
    let root = cx.new(|_| Cards { cards });
    Built {
        root: root.into(),
        step: Box::new(|frame, window, cx| {
            scroll_step(frame, window, cx, center(window), 8_000., 36.)
        }),
    }
}

const TABLE_ROWS: usize = 50_000;
const TABLE_COLUMNS: usize = 16;

struct DataTable;

impl Render for DataTable {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .bg(rgb(0x2b2e35))
                    .text_xs()
                    .children(
                        (0..TABLE_COLUMNS)
                            .map(|column| div().w(px(78.)).px_1().child(format!("Field {column}"))),
                    ),
            )
            .child(
                uniform_list("table-rows", TABLE_ROWS, |range, _, _| {
                    range
                        .map(|row| {
                            div()
                                .h(px(22.))
                                .flex()
                                .items_center()
                                .text_xs()
                                .border_b_1()
                                .border_color(rgb(0x2b2e35))
                                .children((0..TABLE_COLUMNS).map(move |column| {
                                    let value = (row * 7919 + column * 104_729) % 1_000_000;
                                    div().w(px(78.)).px_1().child(format!(
                                        "{}.{:02}",
                                        value / 100,
                                        value % 100
                                    ))
                                }))
                        })
                        .collect()
                })
                .flex_1(),
            )
    }
}

pub fn data_table(_: &mut Window, cx: &mut App) -> Built {
    let root = cx.new(|_| DataTable);
    Built {
        root: root.into(),
        step: Box::new(|frame, window, cx| {
            scroll_step(frame, window, cx, center(window), 30_000., 48.)
        }),
    }
}
