//! A dashboard of panel views: nothing changed, one, six, or every panel
//! notified per frame.

use crate::{
    AnyView, App, Context, Entity, FastStats, IntoElement, Render, SharedString, Window, div, hsla,
    prelude::*, px,
};

use super::{Fixture, Mode, Scenario, check, drawn_per_frame, exactly_per_frame, expect_frames};

const PANELS: usize = 60;
const LABELS: usize = 64;

pub(super) fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "still-frame",
            description: "60 panel views of 64 labels; the root is notified and nothing changes",
            build: |_, cx| Box::new(Dashboard::new(cx, Changing::None)),
            warm_up_frames: 3,
        },
        Scenario {
            name: "one-panel",
            description: "60 panel views; one panel's labels tick per frame",
            build: |_, cx| Box::new(Dashboard::new(cx, Changing::Panels(1))),
            warm_up_frames: 3,
        },
        Scenario {
            name: "six-panels",
            description: "60 panel views; six panels' labels tick per frame",
            build: |_, cx| Box::new(Dashboard::new(cx, Changing::Panels(6))),
            warm_up_frames: 3,
        },
        Scenario {
            name: "all-panels",
            description: "60 panel views; every panel's labels tick per frame",
            build: |_, cx| Box::new(Dashboard::new(cx, Changing::Panels(PANELS))),
            warm_up_frames: 3,
        },
    ]
}

/// What a frame changes.
#[derive(Clone, Copy)]
enum Changing {
    /// Nothing: the root is notified.
    None,
    /// This many panels tick.
    Panels(usize),
}

/// A panel of labels showing its tick.
pub(crate) struct Panel {
    index: usize,
    tick: usize,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tick = self.tick;
        div()
            .flex()
            .flex_col()
            .w(px(220.))
            .p_1()
            .bg(hsla(0.6, 0.2, 0.18, 1.))
            .child(SharedString::from(format!("panel {} {tick}", self.index)))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .children((0..LABELS).map(|cell| {
                        div()
                            .w(px(26.))
                            .h(px(14.))
                            .hover(|style| style.bg(hsla(0.1, 0.6, 0.4, 1.)))
                            .child(SharedString::from(((cell + tick) % 97).to_string()))
                    })),
            )
    }
}

/// The panels, wrapping.
pub(crate) struct Panels {
    panels: Vec<Entity<Panel>>,
}

impl Render for Panels {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .flex_wrap()
            .children(self.panels.iter().cloned())
    }
}

struct Dashboard {
    root: Entity<Panels>,
    panels: Vec<Entity<Panel>>,
    changing: Changing,
}

impl Dashboard {
    fn new(cx: &mut App, changing: Changing) -> Self {
        let panels: Vec<Entity<Panel>> = (0..PANELS)
            .map(|index| cx.new(|_| Panel { index, tick: index }))
            .collect();
        let root = cx.new(|_| Panels {
            panels: panels.clone(),
        });
        Self {
            root,
            panels,
            changing,
        }
    }
}

impl Fixture for Dashboard {
    fn root(&self) -> AnyView {
        self.root.clone().into()
    }

    fn step(&mut self, frame: usize, _: &mut Window, cx: &mut App) {
        match self.changing {
            Changing::None => cx.notify(self.root.entity_id()),
            Changing::Panels(count) => {
                for offset in 0..count {
                    let panel = &self.panels[(frame * count + offset) % PANELS];
                    panel.update(cx, |panel, cx| {
                        panel.tick += 1;
                        cx.notify();
                    });
                }
            }
        }
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        let changing = match self.changing {
            Changing::None => 0,
            Changing::Panels(count) => count,
        } as u64;
        if mode.fast_mode().view_retention {
            // The scenario root and the dashboard around the changed panels
            // are built with them; every other panel is drawn from last
            // frame where it was.
            exactly_per_frame(stats, "views_built", changing + 2)?;
            exactly_per_frame(stats, "views_reused", PANELS as u64 - changing)?;
            exactly_per_frame(stats, "views_built_at_retained_layout", 0)?;
        } else {
            // Every frame builds the scenario root, the dashboard and its
            // panels.
            exactly_per_frame(stats, "views_built", PANELS as u64 + 2)?;
        }
        if mode.fast_mode().layout_retention {
            // Panel text changes, but no element comes or goes: every node
            // is claimed again and none is written or released.
            exactly_per_frame(stats, "style_writes", 0)?;
            exactly_per_frame(stats, "children_writes", 0)?;
            exactly_per_frame(stats, "nodes_released", 0)?;
            // A panel's title and labels: the panels that did not change
            // carry all of them, unless they are drawn from last frame and
            // lay nothing out, and the ones that did measure theirs again,
            // replayed where the new text has the old one's size.
            let texts = (LABELS + 1) as u64;
            let carrying = if mode.fast_mode().view_retention {
                0
            } else {
                PANELS as u64 - changing
            };
            exactly_per_frame(stats, "leaves_carried", carrying * texts)?;
            check(
                stats.leaves_replayed + stats.leaves_remeasured == changing * texts * stats.frames,
                || format!("the changed panels did not measure their text again: {stats:?}"),
            )?;
            if changing == 0 {
                exactly_per_frame(stats, "leaves_measured", 0)?;
            }
        }
        // A background per panel and a glyph or more per label.
        drawn_per_frame(stats, (PANELS * (LABELS + 1)) as u64)
    }
}
