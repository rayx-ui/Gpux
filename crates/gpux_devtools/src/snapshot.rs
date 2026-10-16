//! A window's tree as YAML, in the shape of Playwright's aria snapshot: one line per node with
//! its role, name, states and `[ref=<node id>]`, its value or text after a colon, its children
//! nested under it.
//!
//! ```yaml
//! - window "Settings" [ref=0]:
//!   - button "Save" [ref=1824]
//!   - checkbox "Wrap lines" [checked=false] [ref=1825]
//!   - slider "Volume" [ref=1826]: "5"
//!   - text [ref=1827]: Ready
//! ```

use gpux::{
    accesskit::NodeId,
    semantic_query::{Checked, QueryView, SemanticNode, SemanticSnapshot},
};

/// Renders `snapshot` in `view`, from the node `root` (the window when `None`), with each
/// node's bounds in window pixels when `bounds`.
pub fn render_snapshot(
    snapshot: &SemanticSnapshot,
    view: QueryView,
    root: Option<NodeId>,
    bounds: bool,
) -> String {
    let start = root
        .and_then(|id| snapshot.nodes.iter().position(|node| node.node_id == id))
        .unwrap_or(0);
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); snapshot.nodes.len()];
    for (index, node) in snapshot.nodes.iter().enumerate() {
        let parent = match view {
            QueryView::Inspection => node.parent,
            QueryView::ScreenReader if node.screen_reader_visible => node.screen_reader_parent,
            QueryView::ScreenReader => None,
        };
        if let Some(parent) = parent {
            children[parent].push(index);
        }
    }
    let mut out = String::new();
    write(snapshot, &children, start, 0, bounds, &mut out);
    out
}

fn write(
    snapshot: &SemanticSnapshot,
    children: &[Vec<usize>],
    index: usize,
    depth: usize,
    bounds: bool,
    out: &mut String,
) {
    let node = &snapshot.nodes[index];
    out.push_str(&"  ".repeat(depth));
    out.push_str("- ");
    out.push_str(&node.role);
    let is_text = node.role == "text";
    if !is_text && let Some(name) = node.name.as_deref().filter(|name| !name.is_empty()) {
        out.push(' ');
        out.push_str(&quoted(name));
    }
    for state in states(node) {
        out.push_str(" [");
        out.push_str(&state);
        out.push(']');
    }
    out.push_str(&format!(" [ref={}]", node.node_id.0));
    if bounds {
        let b = &node.bounds;
        out.push_str(&format!(
            " [bounds={},{},{},{}]",
            b.x, b.y, b.width, b.height
        ));
    }
    let content = if is_text {
        node.text.clone().or_else(|| node.name.clone())
    } else {
        node.value.clone()
    };
    let has_children = !children[index].is_empty();
    match (content, has_children) {
        (Some(content), _) if is_text => {
            out.push_str(": ");
            out.push_str(&content);
        }
        (Some(content), _) => {
            out.push_str(": ");
            out.push_str(&quoted(&content));
        }
        (None, true) => out.push(':'),
        (None, false) => {}
    }
    out.push('\n');
    for child in &children[index] {
        write(snapshot, children, *child, depth + 1, bounds, out);
    }
}

fn states(node: &SemanticNode) -> Vec<String> {
    let mut states = Vec::new();
    match node.checked {
        Some(Checked::True) => states.push("checked".into()),
        Some(Checked::False) => states.push("checked=false".into()),
        Some(Checked::Mixed) => states.push("checked=mixed".into()),
        None => {}
    }
    match node.expanded {
        Some(true) => states.push("expanded".into()),
        Some(false) => states.push("expanded=false".into()),
        None => {}
    }
    if node.selected == Some(true) {
        states.push("selected".into());
    }
    if node.disabled {
        states.push("disabled".into());
    }
    if node.required {
        states.push("required".into());
    }
    if node.read_only {
        states.push("readonly".into());
    }
    if node.focused {
        states.push("focused".into());
    }
    if let Some(level) = node.level {
        states.push(format!("level={level}"));
    }
    states
}

/// `text` in double quotes, escaped as YAML reads it.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| format!("\"{text}\""))
}
