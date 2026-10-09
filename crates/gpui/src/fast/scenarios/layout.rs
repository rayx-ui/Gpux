//! Layout changes: a settings page whose sections open and close, and a tree
//! whose nodes expand and collapse.

use crate::{
    AnyView, App, Context, Entity, FastStats, IntoElement, Render, SharedString, Window, div, hsla,
    prelude::*, px,
};

use super::{
    Fixture, Mode, Scenario, at_least_per_frame, drawn_per_frame, exactly_per_frame, expect_frames,
};

const SECTIONS: usize = 12;
const SETTINGS_PER_SECTION: usize = 10;
const TREE_NODES: usize = 24;
const TREE_CHILDREN: usize = 6;

pub(super) fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "settings-toggle",
            description: "a settings page of 12 section views; one section opens or closes per \
                          frame, moving the sections below it",
            build: |_, cx| Box::new(Settings::new(cx)),
            warm_up_frames: 3,
        },
        Scenario {
            name: "tree-expand",
            description: "a tree of 24 node views; one node expands or collapses per frame",
            build: |_, cx| Box::new(Tree::new(cx)),
            warm_up_frames: 3,
        },
    ]
}

pub(crate) struct Section {
    index: usize,
    open: bool,
}

impl Render for Section {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(360.))
            .p_1()
            .border_1()
            .border_color(hsla(0., 0., 0.3, 1.))
            .child(SharedString::from(format!("Section {}", self.index)))
            .when(self.open, |this| {
                this.children((0..SETTINGS_PER_SECTION).map(|setting| {
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .h(px(20.))
                        .child(SharedString::from(format!("Setting {setting}")))
                        .child(
                            div()
                                .w(px(30.))
                                .h(px(14.))
                                .rounded_md()
                                .bg(hsla(0.55, 0.5, 0.4, 1.)),
                        )
                }))
            })
    }
}

pub(crate) struct SettingsPage {
    sections: Vec<Entity<Section>>,
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(self.sections.iter().cloned())
    }
}

struct Settings {
    page: Entity<SettingsPage>,
    sections: Vec<Entity<Section>>,
}

impl Settings {
    fn new(cx: &mut App) -> Self {
        let sections: Vec<Entity<Section>> = (0..SECTIONS)
            .map(|index| {
                cx.new(|_| Section {
                    index,
                    open: index.is_multiple_of(3),
                })
            })
            .collect();
        let page = cx.new(|_| SettingsPage {
            sections: sections.clone(),
        });
        Self { page, sections }
    }
}

impl Fixture for Settings {
    fn root(&self) -> AnyView {
        self.page.clone().into()
    }

    fn step(&mut self, frame: usize, _: &mut Window, cx: &mut App) {
        self.sections[(frame * 5) % SECTIONS].update(cx, |section, cx| {
            section.open = !section.open;
            cx.notify();
        });
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        exactly_per_frame(stats, "views_built", SECTIONS as u64 + 2)?;
        // A section opening or closing writes its children.
        at_least_per_frame(stats, "children_writes", 1)?;
        // The glyphs of every section's title.
        drawn_per_frame(stats, (SECTIONS * 8) as u64)
    }
}

pub(crate) struct Node {
    index: usize,
    expanded: bool,
}

impl Render for Node {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .pl(px(12.))
            .child(
                div()
                    .h(px(18.))
                    .bg(hsla(0.6, 0.2, 0.2, 1.))
                    .child(SharedString::from(format!(
                        "{} node {}",
                        if self.expanded { "v" } else { ">" },
                        self.index
                    ))),
            )
            .when(self.expanded, |this| {
                this.children((0..TREE_CHILDREN).map(|child| {
                    div()
                        .pl(px(12.))
                        .h(px(18.))
                        .child(SharedString::from(format!("leaf {}.{child}", self.index)))
                }))
            })
    }
}

pub(crate) struct TreeView {
    nodes: Vec<Entity<Node>>,
}

impl Render for TreeView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().children(self.nodes.iter().cloned())
    }
}

struct Tree {
    view: Entity<TreeView>,
    nodes: Vec<Entity<Node>>,
}

impl Tree {
    fn new(cx: &mut App) -> Self {
        let nodes: Vec<Entity<Node>> = (0..TREE_NODES)
            .map(|index| {
                cx.new(|_| Node {
                    index,
                    expanded: index.is_multiple_of(4),
                })
            })
            .collect();
        let view = cx.new(|_| TreeView {
            nodes: nodes.clone(),
        });
        Self { view, nodes }
    }
}

impl Fixture for Tree {
    fn root(&self) -> AnyView {
        self.view.clone().into()
    }

    fn step(&mut self, frame: usize, _: &mut Window, cx: &mut App) {
        self.nodes[(frame * 7) % TREE_NODES].update(cx, |node, cx| {
            node.expanded = !node.expanded;
            cx.notify();
        });
    }

    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String> {
        expect_frames(mode, stats)?;
        exactly_per_frame(stats, "views_built", TREE_NODES as u64 + 2)?;
        at_least_per_frame(stats, "children_writes", 1)?;
        // A background and the glyphs of every node's header.
        drawn_per_frame(stats, (TREE_NODES * 7) as u64)
    }
}
