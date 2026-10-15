//! Text: typing into a long document, a streaming log, and labels that all change.

use std::collections::VecDeque;

use gpux::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div, prelude::*, px, rgb,
};

use crate::{
    Built,
    util::{Rng, words},
};

const PARAGRAPHS: usize = 150;

struct Paragraph {
    text: String,
    original_len: usize,
    heading: bool,
}

impl Render for Paragraph {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .when(self.heading, |paragraph| paragraph.text_xl().mt_3())
            .when(!self.heading, |paragraph| {
                paragraph.text_sm().line_height(px(20.))
            })
            .child(SharedString::from(self.text.clone()))
    }
}

struct Document {
    paragraphs: Vec<Entity<Paragraph>>,
}

impl Render for Document {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("document")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .justify_center()
            .child(
                div()
                    .w(px(860.))
                    .py_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(self.paragraphs.iter().cloned()),
            )
    }
}

pub fn typing(_: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(11);
    let paragraphs: Vec<_> = (0..PARAGRAPHS)
        .map(|index| {
            let heading = index % 10 == 0;
            let count = if heading {
                4
            } else {
                40 + rng.below(80) as usize
            };
            let text = words(&mut rng, count);
            let original_len = text.len();
            cx.new(|_| Paragraph {
                text,
                original_len,
                heading,
            })
        })
        .collect();
    let root = cx.new(|_| Document {
        paragraphs: paragraphs.clone(),
    });
    let typed = b"the quick brown fox jumps over the lazy dog ";
    Built {
        root: root.into(),
        step: Box::new(move |frame, _, cx| {
            // The caret sits in the third paragraph, on screen.
            paragraphs[2].update(cx, |paragraph, cx| {
                if paragraph.text.len() - paragraph.original_len >= 120 {
                    paragraph.text.truncate(paragraph.original_len);
                }
                paragraph
                    .text
                    .push(typed[(frame % typed.len() as u64) as usize] as char);
                cx.notify();
            });
        }),
    }
}

const LOG_LINES: usize = 80;

struct Log {
    lines: VecDeque<(u64, SharedString)>,
}

impl Render for Log {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_2()
            .bg(rgb(0x111214))
            .font_family("Lilex")
            .text_xs()
            .flex()
            .flex_col()
            .overflow_hidden()
            .children(self.lines.iter().map(|(serial, line)| {
                div()
                    .flex()
                    .gap_2()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .text_color(rgb(0x6e7681))
                            .child(format!("{serial:>8}")),
                    )
                    .child(
                        div()
                            .text_color(match serial % 7 {
                                0 => rgb(0xf85149),
                                3 => rgb(0xd29922),
                                _ => rgb(0xc9d1d9),
                            })
                            .child(line.clone()),
                    )
            }))
    }
}

pub fn log_stream(_: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(13);
    let mut line = move |serial: u64| -> (u64, SharedString) {
        let level = ["INFO", "DEBUG", "WARN", "TRACE"][rng.below(4) as usize];
        let count = 6 + rng.below(10) as usize;
        (
            serial,
            format!("{level} gpux::frame {}", words(&mut rng, count)).into(),
        )
    };
    let lines = (0..LOG_LINES as u64).map(&mut line).collect();
    let log = cx.new(|_| Log { lines });
    let mut serial = LOG_LINES as u64;
    Built {
        root: log.clone().into(),
        step: Box::new(move |_, _, cx| {
            let next = line(serial);
            serial += 1;
            log.update(cx, |log, cx| {
                log.lines.pop_front();
                log.lines.push_back(next);
                cx.notify();
            });
        }),
    }
}

const LABEL_COLUMNS: usize = 30;
const LABEL_ROWS: usize = 50;

struct Labels {
    values: Vec<f32>,
}

impl Render for Labels {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_1()
            .flex()
            .flex_col()
            .text_xs()
            .children(self.values.chunks(LABEL_COLUMNS).map(|row| {
                div().h(px(15.)).flex().children(
                    row.iter()
                        .map(|value| div().w(px(42.)).child(format!("{value:.2}"))),
                )
            }))
    }
}

pub fn relabel(_: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(17);
    let values = (0..LABEL_COLUMNS * LABEL_ROWS)
        .map(|_| rng.unit() * 99.)
        .collect();
    let labels = cx.new(|_| Labels { values });
    Built {
        root: labels.clone().into(),
        step: Box::new(move |_, _, cx| {
            labels.update(cx, |labels, cx| {
                for value in &mut labels.values {
                    *value = (*value + rng.unit()) % 99.;
                }
                cx.notify();
            });
        }),
    }
}
