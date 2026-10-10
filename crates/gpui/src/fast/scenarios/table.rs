//! A data table whose cells tick: some to text of another width, some to
//! text of the same width.

use crate::{
    AnyView, App, Context, Entity, FastStats, IntoElement, Render, SharedString, Window, div, hsla,
    prelude::*, px,
};

use super::{Fixture, Mode, Scenario, drawn_per_frame, exactly_per_frame, expect_frames};

const ROWS: usize = 40;
const COLUMNS: usize = 8;
/// Rows ticked per frame.
const TICKING: usize = 4;

pub(super) fn scenarios() -> Vec<Scenario> {
    vec![Scenario {
        name: "table-stream",
        description: "a 40x8 table of row views; four rows tick a cell of changing and a cell of \
                      equal width per frame",
        build: |_, cx| Box::new(Stream::new(cx)),
        warm_up_frames: 3,
    }]
}

/// A row of cells.
pub(crate) struct Row {
    index: usize,
    /// Ticks for the cell whose width changes.
    widening: usize,
    /// Ticks for the cell whose width stays.
    steady: usize,
}

impl Render for Row {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let widening = 10_usize.pow((self.widening % 4) as u32) + self.widening;
        // The same glyphs in another order, so the cell is as wide whatever
        // the font makes of each digit.
        let steady = if self.steady.is_multiple_of(2) {
            "1221"
        } else {
            "2112"
        };
        div()
            .flex()
            .flex_row()
            .h(px(18.))
            .bg(if self.index.is_multiple_of(2) {
                hsla(0., 0., 0.12, 1.)
            } else {
                hsla(0., 0., 0.16, 1.)
            })
            .child(
                div()
                    .w(px(80.))
                    .child(SharedString::from(widening.to_string())),
            )
            .child(div().w(px(80.)).child(SharedString::from(steady)))
            .children((2..COLUMNS).map(|column| {
                div()
                    .w(px(80.))
                    .child(SharedString::from(format!("r{}c{column}", self.index)))
            }))
    }
}

/// The table.
pub(crate) struct Table {
    rows: Vec<Entity<Row>>,
}

impl Render for Table {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().children(self.rows.iter().cloned())
    }
}

struct Stream {
    table: Entity<Table>,
    rows: Vec<Entity<Row>>,
}

impl Stream {
    fn new(cx: &mut App) -> Self {
        let rows: Vec<Entity<Row>> = (0..ROWS)
            .map(|index| {
                cx.new(|_| Row {
                    index,
                    widening: 0,
                    steady: 0,
                })
            })
            .collect();
        let table = cx.new(|_| Table { rows: rows.clone() });
        Self { table, rows }
    }
}

impl Fixture for Stream {
    fn root(&self) -> AnyView {
        self.table.clone().into()
    }

    fn step(&mut self, frame: usize, _: &mut Window, cx: &mut App) {
        for offset in 0..TICKING {
            self.rows[(frame * TICKING + offset) % ROWS].update(cx, |row, cx| {
                row.widening += 1;
                row.steady += 1;
                cx.notify();
            });
        }
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        // Rows drawn from last frame lay nothing out.
        let laid_out_rows = if mode.fast_mode().splice {
            // Only the ticking rows are built; the table and the scenario
            // root are spliced around them.
            exactly_per_frame(stats, "views_built", TICKING as u64)?;
            exactly_per_frame(stats, "views_spliced", 2)?;
            TICKING
        } else if mode.fast_mode().view_retention {
            // The scenario root and the table are built around the ticking
            // rows; every other row is drawn from last frame.
            exactly_per_frame(stats, "views_built", TICKING as u64 + 2)?;
            exactly_per_frame(stats, "views_reused", (ROWS - TICKING) as u64)?;
            TICKING
        } else {
            // Every frame builds the scenario root, the table and its rows.
            exactly_per_frame(stats, "views_built", ROWS as u64 + 2)?;
            ROWS
        };
        if mode.fast_mode().layout_retention {
            // Each ticking row's equal-width cell replays and its widening
            // cell is measured afresh; every other cell laid out carries; no
            // node is written, so every row but the ticking cells stays
            // clean.
            exactly_per_frame(stats, "leaves_replayed", TICKING as u64)?;
            exactly_per_frame(stats, "leaves_remeasured", TICKING as u64)?;
            exactly_per_frame(
                stats,
                "leaves_carried",
                (laid_out_rows * COLUMNS - 2 * TICKING) as u64,
            )?;
            exactly_per_frame(stats, "style_writes", 0)?;
            exactly_per_frame(stats, "children_writes", 0)?;
            exactly_per_frame(stats, "nodes_released", 0)?;
        }
        // Two glyphs or more per cell.
        drawn_per_frame(stats, (ROWS * COLUMNS * 2) as u64)
    }
}
