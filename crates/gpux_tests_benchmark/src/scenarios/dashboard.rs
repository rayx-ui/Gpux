//! Many views, a few of which change: what view retention is for, and its worst case.

use gpux::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div, prelude::*, px, rgb,
};

use crate::{
    Built,
    util::{Rng, color, words},
};

const PANELS: usize = 48;
const BARS: usize = 16;
const ROWS: usize = 8;

struct Panel {
    index: u64,
    title: SharedString,
    bars: [f32; BARS],
    rows: Vec<(SharedString, SharedString)>,
    total: f64,
    rng: Rng,
}

impl Panel {
    fn new(index: u64) -> Self {
        let mut rng = Rng::new(index + 1);
        let title = words(&mut rng, 2).into();
        let rows = (0..ROWS)
            .map(|_| {
                (
                    words(&mut rng, 1).into(),
                    format!("{:.2}", rng.unit() * 1000.).into(),
                )
            })
            .collect();
        let mut panel = Panel {
            index,
            title,
            bars: [0.; BARS],
            rows,
            total: 0.,
            rng,
        };
        panel.refresh_data();
        panel
    }

    /// New data arrives: every bar and value changes.
    fn refresh_data(&mut self) {
        for bar in &mut self.bars {
            *bar = 0.1 + self.rng.unit() * 0.9;
        }
        for row in &mut self.rows {
            row.1 = format!("{:.2}", self.rng.unit() * 1000.).into();
        }
        self.total += self.rng.unit() as f64;
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let accent = color(self.index);
        div()
            .w(px(196.))
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x3a3d45))
            .bg(rgb(0x272a31))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_sm().child(self.title.clone()))
                    .child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .bg(accent)
                            .text_xs()
                            .child(format!("{:.1}", self.total)),
                    ),
            )
            .child(
                div().h(px(48.)).flex().items_end().gap(px(2.)).children(
                    self.bars
                        .iter()
                        .map(|bar| div().w(px(9.)).h(px(48. * bar)).bg(accent).rounded_sm()),
                ),
            )
            .children(self.rows.iter().map(|(key, value)| {
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .child(div().text_color(rgb(0x9aa0a6)).child(key.clone()))
                    .child(value.clone())
            }))
    }
}

struct StatusBar {
    frame: u64,
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let millis = self.frame * 16;
        div()
            .h(px(24.))
            .px_2()
            .flex()
            .items_center()
            .justify_between()
            .bg(rgb(0x2f6feb))
            .text_xs()
            .child("connected · 48 panels")
            .child(format!(
                "frame {} · {:02}:{:02}.{:03}",
                self.frame,
                millis / 60_000,
                millis / 1000 % 60,
                millis % 1000
            ))
    }
}

struct Dashboard {
    panels: Vec<Entity<Panel>>,
    status: Entity<StatusBar>,
}

impl Render for Dashboard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .px_3()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(0x3a3d45))
                    .child("Operations dashboard"),
            )
            .child(
                div()
                    .id("panels")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_2()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(self.panels.iter().cloned()),
            )
            .child(self.status.clone())
    }
}

struct Handles {
    root: AnyView,
    panels: Vec<Entity<Panel>>,
    status: Entity<StatusBar>,
}

fn build(cx: &mut App) -> Handles {
    let panels: Vec<_> = (0..PANELS as u64)
        .map(|index| cx.new(|_| Panel::new(index)))
        .collect();
    let status = cx.new(|_| StatusBar { frame: 0 });
    let root = cx.new(|_| Dashboard {
        panels: panels.clone(),
        status: status.clone(),
    });
    Handles {
        root: root.into(),
        panels,
        status,
    }
}

fn tick_status(status: &Entity<StatusBar>, frame: u64, cx: &mut App) {
    status.update(cx, |status, cx| {
        status.frame = frame;
        cx.notify();
    });
}

fn refresh_panel(panel: &Entity<Panel>, cx: &mut App) {
    panel.update(cx, |panel, cx| {
        panel.refresh_data();
        cx.notify();
    });
}

pub fn clock(_: &mut Window, cx: &mut App) -> Built {
    let Handles { root, status, .. } = build(cx);
    Built {
        root,
        step: Box::new(move |frame, _, cx| tick_status(&status, frame, cx)),
    }
}

pub fn one_panel(_: &mut Window, cx: &mut App) -> Built {
    let Handles {
        root,
        panels,
        status,
    } = build(cx);
    Built {
        root,
        step: Box::new(move |frame, _, cx| {
            // The panels on screen (the first two rows) take turns.
            refresh_panel(&panels[(frame % 12) as usize], cx);
            tick_status(&status, frame, cx);
        }),
    }
}

pub fn all_panels(_: &mut Window, cx: &mut App) -> Built {
    let Handles {
        root,
        panels,
        status,
    } = build(cx);
    Built {
        root,
        step: Box::new(move |frame, _, cx| {
            for panel in &panels {
                refresh_panel(panel, cx);
            }
            tick_status(&status, frame, cx);
        }),
    }
}

const TABLE_ROWS: usize = 60;
const TABLE_COLUMNS: usize = 12;

struct Row {
    index: usize,
    symbol: SharedString,
    cells: [f64; TABLE_COLUMNS],
    up: bool,
}

impl Render for Row {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let change = if self.up {
            rgb(0x3fb950)
        } else {
            rgb(0xf85149)
        };
        div()
            .h(px(22.))
            .flex()
            .items_center()
            .text_xs()
            .when(self.index % 2 == 1, |row| row.bg(rgb(0x23262c)))
            .child(div().w(px(96.)).px_2().child(self.symbol.clone()))
            .children(self.cells.iter().enumerate().map(|(column, value)| {
                div()
                    .w(px(90.))
                    .px_2()
                    .flex()
                    .justify_end()
                    .when(column == 0, |cell| cell.text_color(change))
                    .child(format!("{value:.3}"))
            }))
    }
}

struct Table {
    rows: Vec<Entity<Row>>,
}

impl Render for Table {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .text_xs()
                    .bg(rgb(0x2b2e35))
                    .child(div().w(px(96.)).px_2().child("Symbol"))
                    .children((0..TABLE_COLUMNS).map(|column| {
                        div()
                            .w(px(90.))
                            .px_2()
                            .flex()
                            .justify_end()
                            .child(format!("Col {column}"))
                    })),
            )
            .child(
                div()
                    .id("rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(self.rows.iter().cloned()),
            )
    }
}

pub fn table_stream(_: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(7);
    let rows: Vec<_> = (0..TABLE_ROWS)
        .map(|index| {
            let symbol = format!("SYM{index:03}.{}", (b'A' + (index % 26) as u8) as char).into();
            let mut cells = [0.; TABLE_COLUMNS];
            for cell in &mut cells {
                *cell = rng.unit() as f64 * 500.;
            }
            cx.new(|_| Row {
                index,
                symbol,
                cells,
                up: true,
            })
        })
        .collect();
    let root = cx.new(|_| Table { rows: rows.clone() });
    Built {
        root: root.into(),
        step: Box::new(move |_, _, cx| {
            for _ in 0..6 {
                let row = &rows[rng.below(TABLE_ROWS as u64) as usize];
                let delta = (rng.unit() as f64 - 0.5) * 4.;
                row.update(cx, |row, cx| {
                    for cell in &mut row.cells {
                        *cell += delta;
                    }
                    row.up = delta >= 0.;
                    cx.notify();
                });
            }
        }),
    }
}
