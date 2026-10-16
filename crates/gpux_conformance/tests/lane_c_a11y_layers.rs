//! Scroll and list layers with the accessibility tree built: a window that scrolls its content
//! from GPU tiles carries the content's nodes (their bounds moved by the scroll, the rows a
//! list's layer holds off screen left out) and builds, frame for frame, the tree a window
//! without scroll layers builds.
//!
//! Both windows of each test build their trees for inspection. Their node ids differ (each
//! window's view is its own entity), so the trees are compared as what they show: every node in
//! tree order with its role, name, value and bounds.
//!
//! ```text
//! cargo test -p gpux-conformance --test lane_c_a11y_layers
//! ```

mod scroll_layers_support;

use gpux::{A11yInspection, AppContext as _};
use scroll_layers_support::*;

/// The tree a window built last, as lines that do not depend on node ids.
fn tree(run: &Run, cx: &mut gpux::TestAppContext) -> Vec<String> {
    let update = cx
        .update_window(run.window(), |_, window, _| window.a11y_tree().cloned())
        .expect("the window is open")
        .expect("the window builds its tree");
    let nodes: std::collections::HashMap<_, _> =
        update.nodes.iter().map(|(id, node)| (*id, node)).collect();
    let mut lines = Vec::new();
    let mut stack = vec![(update.tree.as_ref().unwrap().root, 0)];
    while let Some((id, depth)) = stack.pop() {
        let node = nodes[&id];
        let bounds = node.bounds().map(|b| {
            format!(
                "{:.1},{:.1} {:.1}x{:.1}",
                b.x0,
                b.y0,
                b.x1 - b.x0,
                b.y1 - b.y0
            )
        });
        lines.push(format!(
            "{}{:?} {:?} {:?} {}",
            "  ".repeat(depth),
            node.role(),
            node.label(),
            node.value(),
            bounds.unwrap_or_default()
        ));
        stack.extend(
            node.children()
                .iter()
                .rev()
                .map(|child| (*child, depth + 1)),
        );
    }
    lines
}

fn inspect(run: &Run, cx: &mut gpux::TestAppContext) -> A11yInspection {
    cx.update_window(run.window(), |_, window, _| window.begin_a11y_inspection())
        .expect("the window is open")
}

/// Scrolls `content` in a layered window and a reference, comparing their trees after every
/// step, and returns how many frames the layered window composited.
fn compare_trees(content: Content, scale: f32, script: &[Step]) -> u64 {
    let mut cx = app();
    let layered = Run::open(&mut cx, content, scale, true);
    let reference = Run::open(&mut cx, content, scale, false);
    let _guards = (inspect(&layered, &mut cx), inspect(&reference, &mut cx));
    let mut composited = 0;
    for (index, step) in std::iter::once(None)
        .chain(script.iter().map(Some))
        .enumerate()
    {
        if let Some(step) = step {
            for run in [&layered, &reference] {
                match *step {
                    Step::Wheel(dx, dy) => run.wheel(&cx, dx, dy),
                    Step::Tick => run.tick(&mut cx),
                }
            }
        }
        let (_, stats) = layered.frame(&mut cx);
        reference.frame(&mut cx);
        composited += stats.layers_composited;
        let (actual, expected) = (tree(&layered, &mut cx), tree(&reference, &mut cx));
        if actual != expected {
            let at = actual
                .iter()
                .zip(&expected)
                .position(|(a, e)| a != e)
                .unwrap_or(actual.len().min(expected.len()));
            panic!(
                "{content:?} at {scale}x, step {index}: the layered window's tree differs at line \
                 {at}:\n  layered:   {:?}\n  reference: {:?}\n({} and {} lines)",
                actual.get(at),
                expected.get(at),
                actual.len(),
                expected.len()
            );
        }
        assert!(actual.len() > 3, "the tree holds the content: {actual:#?}");
    }
    composited
}

#[test]
fn a_scrolled_div_carries_its_nodes_from_tiles() {
    for scale in [1., 2.] {
        let composited = compare_trees(Content::Text, scale, &scrolls());
        assert!(composited > 0, "the layered window composited its layer");
    }
}

#[test]
fn a_uniform_list_carries_its_shown_rows_from_tiles() {
    let composited = compare_trees(Content::UniformList, 1., &scrolls());
    assert!(composited > 0, "the layered window composited its layer");
}

#[test]
fn a_list_carries_its_shown_rows_from_tiles() {
    let composited = compare_trees(Content::List, 1., &scrolls());
    assert!(composited > 0, "the layered window composited its layer");
}

#[test]
fn rows_changed_while_scrolled_are_built_where_they_lie() {
    let mut script = scrolls();
    script.insert(3, Step::Tick);
    script.push(Step::Tick);
    for content in [Content::Text, Content::UniformList] {
        compare_trees(content, 1., &script);
    }
}
