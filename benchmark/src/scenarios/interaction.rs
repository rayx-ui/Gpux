//! Input-driven frames: the pointer over hoverable cells, and a tree folding.

use gpux::{
    App, AppContext as _, Context, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div, point, prelude::*, px, rgb,
};

use crate::{
    Built,
    util::{Rng, mouse_move, words},
};

const GRID_COLUMNS: usize = 40;
const GRID_ROWS: usize = 40;
const CELL: f32 = 30.;
const GRID_LEFT: f32 = 20.;
const GRID_TOP: f32 = 0.;

struct HoverGrid;

impl Render for HoverGrid {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .pl(px(GRID_LEFT))
            .pt(px(GRID_TOP))
            .flex()
            .flex_col()
            .children((0..GRID_ROWS).map(|row| {
                div().flex().children((0..GRID_COLUMNS).map(move |column| {
                    let index = row * GRID_COLUMNS + column;
                    div()
                        .id(index)
                        .w(px(CELL))
                        .h(px(CELL * 0.62))
                        .border_1()
                        .border_color(rgb(0x2b2e35))
                        .text_xs()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgb(0x23262c))
                        .hover(|cell| cell.bg(rgb(0x2f6feb)).text_color(rgb(0xffffff)))
                        .child(SharedString::from(format!("{}", index % 100)))
                }))
            }))
    }
}

pub fn hover_grid(_: &mut Window, cx: &mut App) -> Built {
    let root = cx.new(|_| HoverGrid);
    Built {
        root: root.into(),
        step: Box::new(|frame, window, cx| {
            // A serpentine path over the cells on screen, one cell per frame.
            let viewport = window.viewport_size();
            let columns =
                (((f32::from(viewport.width) - GRID_LEFT) / CELL) as usize).clamp(1, GRID_COLUMNS);
            let rows = (((f32::from(viewport.height) - GRID_TOP) / (CELL * 0.62)) as usize)
                .saturating_sub(1)
                .clamp(1, GRID_ROWS);
            let index = frame as usize % (columns * rows);
            let row = index / columns;
            let mut column = index % columns;
            if row % 2 == 1 {
                column = columns - 1 - column;
            }
            let position = point(
                px(GRID_LEFT + column as f32 * CELL + CELL / 2.),
                px(GRID_TOP + row as f32 * CELL * 0.62 + CELL * 0.31),
            );
            mouse_move(window, cx, position);
        }),
    }
}

struct Node {
    name: SharedString,
    children: Vec<Node>,
    expanded: bool,
}

/// A tree of folders six wide: four folders and two files each, files only at depth 4.
/// Eight roots make 2,300 nodes, of which about 800 rows are expanded.
fn tree(rng: &mut Rng, depth: usize) -> Vec<Node> {
    let breadth = if depth == 0 {
        8
    } else if depth == 4 {
        4
    } else {
        6
    };
    (0..breadth)
        .map(|index| {
            let folder = depth < 4 && index < breadth - 2;
            let name = if folder {
                words(rng, 1)
            } else {
                format!("{}.rs", words(rng, 1))
            };
            Node {
                name: name.into(),
                children: if folder {
                    tree(rng, depth + 1)
                } else {
                    Vec::new()
                },
                expanded: depth < 2 || rng.below(2) == 0,
            }
        })
        .collect()
}

fn render_nodes(nodes: &[Node], depth: usize, rows: &mut Vec<gpux::AnyElement>) {
    for node in nodes {
        let folder = !node.children.is_empty();
        rows.push(
            div()
                .h(px(20.))
                .pl(px(12. + depth as f32 * 14.))
                .flex()
                .items_center()
                .gap_1()
                .text_sm()
                .child(
                    div()
                        .w(px(12.))
                        .text_color(rgb(0x6e7681))
                        .child(if !folder {
                            " "
                        } else if node.expanded {
                            "▾"
                        } else {
                            "▸"
                        }),
                )
                .child(
                    div()
                        .when(folder, |name| name.text_color(rgb(0x79c0ff)))
                        .child(node.name.clone()),
                )
                .into_any_element(),
        );
        if folder && node.expanded {
            render_nodes(&node.children, depth + 1, rows);
        }
    }
}

struct FileTree {
    roots: Vec<Node>,
}

impl Render for FileTree {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut rows = Vec::new();
        render_nodes(&self.roots, 0, &mut rows);
        div()
            .id("tree")
            .size_full()
            .overflow_y_scroll()
            .py_1()
            .flex()
            .flex_col()
            .children(rows)
    }
}

/// The folders in the order the tree shows them, as paths of child indices.
fn folders(nodes: &[Node], prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (index, node) in nodes.iter().enumerate() {
        if !node.children.is_empty() {
            prefix.push(index);
            out.push(prefix.clone());
            folders(&node.children, prefix, out);
            prefix.pop();
        }
    }
}

pub fn tree_toggle(_: &mut Window, cx: &mut App) -> Built {
    let mut rng = Rng::new(31);
    let roots = tree(&mut rng, 0);
    let mut paths = Vec::new();
    folders(&roots, &mut Vec::new(), &mut paths);
    // Folders near the top, which are on screen.
    paths.truncate(24);
    let root = cx.new(|_| FileTree { roots });
    let entity = root.clone();
    Built {
        root: root.into(),
        step: Box::new(move |frame, _, cx| {
            let path = &paths[frame as usize % paths.len()];
            entity.update(cx, |tree, cx| {
                let mut nodes = &mut tree.roots;
                for (depth, index) in path.iter().enumerate() {
                    if depth + 1 == path.len() {
                        nodes[*index].expanded = !nodes[*index].expanded;
                    } else {
                        nodes = &mut nodes[*index].children;
                    }
                }
                cx.notify();
            });
        }),
    }
}
