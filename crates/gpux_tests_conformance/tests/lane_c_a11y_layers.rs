//! Scroll and list layers with the accessibility tree built: a window that scrolls its content
//! from GPU tiles carries the content's nodes (their bounds moved by the scroll, the rows a
//! list's layer holds off screen left out) and builds, frame for frame, the tree a window
//! without scroll layers builds.
//!
//! Both windows of each test build their trees for inspection. Their node ids differ (each
//! window's view is its own entity), so the trees are compared as what they show: every node in
//! tree order with its role, name, value and bounds, and for rows that are controls, the whole
//! accessibility part of the frame description (focusable nodes, action handlers, the bounds
//! and scroll containers actions use) with the ids numbered in tree order.
//!
//! ```text
//! cargo test -p gpux-tests-conformance --test lane_c_a11y_layers
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

/// The accessibility part of the window's frame description, with node ids replaced by their
/// position in the tree and the element id map left out (its paths name each window's view).
fn description(run: &Run, cx: &mut gpux::TestAppContext) -> Vec<String> {
    let lines = cx
        .update_window(run.window(), |_, window, _| {
            window.describe_rendered_frame()
        })
        .expect("the window is open");
    let mut numbers = std::collections::HashMap::new();
    let mut tree = Vec::new();
    let mut rest = Vec::new();
    for line in lines
        .into_iter()
        .filter(|line| line.starts_with("a11y ") && !line.starts_with("a11y element"))
    {
        let mut out = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '#' && chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                let mut digits = String::new();
                while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                    digits.push(*d);
                    chars.next();
                }
                let next = numbers.len();
                out.push_str(&format!("@{}", numbers.entry(digits).or_insert(next)));
            } else {
                out.push(c);
            }
        }
        // The tree lines come first and number the ids; the sorted lists after them follow
        // raw ids, so they are sorted again by the numbers.
        if line.starts_with("a11y focusable")
            || line.starts_with("a11y listener")
            || line.starts_with("a11y bounds")
        {
            rest.push(out);
        } else {
            tree.push(out);
        }
    }
    rest.sort();
    tree.extend(rest);
    tree
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

/// Rows that are controls keep, frame for frame, what assistive technology acts on: their
/// focusable entries, action handlers and the bounds a click or ScrollIntoView reaches, also
/// for the rows the layer held off screen and shows again.
#[test]
fn a_list_of_controls_carries_where_actions_reach_its_rows() {
    let mut cx = app();
    let layered = Run::open(&mut cx, Content::Controls, 1., true);
    let reference = Run::open(&mut cx, Content::Controls, 1., false);
    let _guards = (inspect(&layered, &mut cx), inspect(&reference, &mut cx));
    let mut composited = 0;
    let mut script = scrolls();
    script.extend(scrolls().into_iter().rev().map(|step| match step {
        Step::Wheel(dx, dy) => Step::Wheel(-dx, -dy),
        step => step,
    }));
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
        let (actual, expected) = (
            description(&layered, &mut cx),
            description(&reference, &mut cx),
        );
        if actual != expected {
            let at = actual
                .iter()
                .zip(&expected)
                .position(|(a, e)| a != e)
                .unwrap_or(actual.len().min(expected.len()));
            panic!(
                "step {index}: the layered window's description differs at line {at}:\n  \
                 layered:   {:?}\n  reference: {:?}",
                actual.get(at),
                expected.get(at),
            );
        }
        assert!(
            actual.iter().any(|line| line.starts_with("a11y bounds")),
            "{actual:#?}"
        );
    }
    assert!(composited > 0, "the layered window composited its layer");
}
