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
    render(snapshot, view, root, bounds, None)
}

/// [`render_snapshot`], at most `depth` levels below the root when given.
pub(crate) fn render(
    snapshot: &SemanticSnapshot,
    view: QueryView,
    root: Option<NodeId>,
    bounds: bool,
    depth: Option<usize>,
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
    write(snapshot, &children, start, 0, bounds, depth, &mut out);
    out
}

fn write(
    snapshot: &SemanticSnapshot,
    children: &[Vec<usize>],
    index: usize,
    depth: usize,
    bounds: bool,
    limit: Option<usize>,
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
    let has_children = !children[index].is_empty() && limit.is_none_or(|limit| depth < limit);
    match (content, has_children) {
        (Some(content), _) if is_text => {
            out.push_str(": ");
            out.push_str(&scalar(&content));
        }
        (Some(content), _) => {
            out.push_str(": ");
            out.push_str(&quoted(&content));
        }
        (None, true) => out.push(':'),
        (None, false) => {}
    }
    out.push('\n');
    if has_children {
        for child in &children[index] {
            write(snapshot, children, *child, depth + 1, bounds, limit, out);
        }
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

/// Text as a YAML scalar: plain where YAML reads it back as written, else quoted (line breaks,
/// a `: ` or ` #`, leading or trailing spaces, an indicator first, or text YAML takes for
/// another type), as Playwright's aria snapshots quote it.
fn scalar(text: &str) -> String {
    let first = text.chars().next();
    let special = text.is_empty()
        || text.contains(['\n', '\r', '\t'])
        || text.contains(": ")
        || text.ends_with(':')
        || text.contains(" #")
        || text.starts_with(' ')
        || text.ends_with(' ')
        || first.is_some_and(|c| "-?:,[]{}#&*!|>'\"%@`".contains(c))
        || matches!(
            text.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "~" | "yes" | "no"
        )
        || text.parse::<f64>().is_ok();
    if special {
        quoted(text)
    } else {
        text.to_string()
    }
}

/// `text` in double quotes, escaped as YAML reads it.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| format!("\"{text}\""))
}
