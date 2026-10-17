//! The scenarios: the shapes of work a complex application's frames have.
//!
//! Each builds its window content once and changes it every frame the way an application does:
//! a status bar's clock, data arriving for one panel or for all of them, typing, a log, a wheel
//! over virtual and plain lists, the pointer over hoverable cells, an animation, a tree folding.

mod dashboard;
mod effects;
mod interaction;
mod lists;
mod text;

use crate::Scenario;

pub fn all() -> &'static [Scenario] {
    &[
        Scenario {
            id: "dashboard-clock",
            group: "retention",
            description: "48 panel views (~2.6k elements); only the status bar's clock changes",
            build: dashboard::clock,
        },
        Scenario {
            id: "dashboard-one-panel",
            group: "retention",
            description: "48 panel views; one panel's data changes every frame",
            build: dashboard::one_panel,
        },
        Scenario {
            id: "dashboard-all-panels",
            group: "retention",
            description: "48 panel views; every panel's data changes every frame (worst case)",
            build: dashboard::all_panels,
        },
        Scenario {
            id: "table-stream",
            group: "retention",
            description: "60 row views x 12 cells; 6 random rows get new values every frame",
            build: dashboard::table_stream,
        },
        Scenario {
            id: "text-typing",
            group: "text",
            description: "150 wrapped paragraph views; one character typed per frame",
            build: text::typing,
        },
        Scenario {
            id: "text-log-stream",
            group: "text",
            description: "monospace log of 80 visible lines; one line appended per frame",
            build: text::log_stream,
        },
        Scenario {
            id: "text-relabel",
            group: "text",
            description: "1,500 numeric labels, all of them changing every frame",
            build: text::relabel,
        },
        Scenario {
            id: "scroll-uniform-list",
            group: "scroll",
            description: "uniform_list of 100k rows x 6 columns; a 36 px wheel step per frame",
            build: lists::uniform,
        },
        Scenario {
            id: "scroll-variable-list",
            group: "scroll",
            description: "list of 20k chat messages of varying height; a 36 px wheel step per frame",
            build: lists::variable,
        },
        Scenario {
            id: "scroll-div-cards",
            group: "scroll",
            description: "plain scrolling div of 400 cards (~4k elements); a 36 px wheel step per frame",
            build: lists::div_cards,
        },
        Scenario {
            id: "scroll-data-table",
            group: "scroll",
            description: "uniform_list of 50k rows x 16 text cells; a 48 px wheel step per frame",
            build: lists::data_table,
        },
        Scenario {
            id: "hover-grid",
            group: "input",
            description: "1,600 hoverable cells; the pointer moves to the next cell every frame",
            build: interaction::hover_grid,
        },
        Scenario {
            id: "tree-toggle",
            group: "input",
            description: "file tree of 2.3k nodes (~800 rows shown); one folder toggles every frame",
            build: interaction::tree_toggle,
        },
        Scenario {
            id: "animated-dots",
            group: "animation",
            description: "1,000 absolutely positioned dots, all moving every frame",
            build: effects::animated_dots,
        },
        Scenario {
            id: "effects-cards",
            group: "animation",
            description: "240 cards with shadows, gradients, borders and opacity; a progress bar animates",
            build: effects::effects_cards,
        },
    ]
}
