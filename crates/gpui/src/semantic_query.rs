//! Semantic queries over a window's AccessKit tree, the tree screen readers see.
//!
//! A [`Query`] finds nodes by what they mean (role, accessible name, text, value, element id,
//! state, enclosing node) instead of by coordinates. It reads the window's accessibility tree
//! ([`Window::a11y_tree`]) through `accesskit_consumer`, so names are the names screen readers
//! announce (a button named by its text, for example), and the inspection id map
//! ([`Window::a11y_element_id`]) for element ids. A window builds the tree while an activation
//! source asks for it; tests and the devtools start inspection
//! ([`Window::begin_a11y_inspection`]), under which elements with an id and no role are generic
//! containers, so every element with an id can be found.
//!
//! Queries have a small text form so automation outside Rust (the conformance hosts, the
//! devtools) uses the same vocabulary:
//!
//! ```text
//! id=toolbar >> role=button name="Save as" nth=1
//! role=checkbox checked=true view=screen-reader
//! ```
//!
//! A scope is a list of `key=value` terms, all of which must match. `>>` scopes the query on
//! its right to the descendants of the nodes its left side matches. Keys are `role` (an ARIA
//! role name, [`crate::query_role_name`]: `text` for text, `generic` for a generic container),
//! `name` (the computed name), `text` (a text node's text, or a text field's), `value` (the value
//! or numeric value) with an exact match or a substring with `~=`, `id` (a suffix of the global
//! element id on a `.` boundary, or an `accessibility_id`), the states `checked`
//! (`true`/`false`/`mixed`), `expanded`, `selected`, `disabled` and `focused`
//! (`true`/`false`), `view` (`inspection`, the default, or `screen-reader`: only the nodes
//! AccessKit's filter shows screen readers, nested as they see them) and `nth` (the zero-based
//! match to pick). Values containing spaces, quotes, `=` or `>` are double-quoted with `\"` and
//! `\\` escapes. The `type` key of the devtools queries is gone.
//!
//! The module is compiled with gpux's `test-support` and `semantic-query` features.

use crate::{Bounds, Pixels, Window, point, px, size};
use accesskit::{Action, NodeId, Toggled};
use accesskit_consumer::{FilterResult, NodeRef, Tree, common_filter};
use std::{collections::HashSet, fmt, str::FromStr};

#[cfg(any(test, feature = "test-support"))]
mod locator;
#[cfg(any(test, feature = "test-support"))]
pub use locator::*;

/// How a query field matches a node's string property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextMatch {
    /// The property equals the string.
    Exact(String),
    /// The property contains the string.
    Contains(String),
}

impl TextMatch {
    fn matches(&self, value: Option<&str>) -> bool {
        match (self, value) {
            (TextMatch::Exact(expected), Some(value)) => value == expected,
            (TextMatch::Contains(expected), Some(value)) => value.contains(expected.as_str()),
            (_, None) => false,
        }
    }
}

/// A check box's, switch's or toggle button's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    /// Checked.
    True,
    /// Not checked.
    False,
    /// Partly checked, as a check box over a mixed selection.
    Mixed,
}

impl Checked {
    fn name(self) -> &'static str {
        match self {
            Checked::True => "true",
            Checked::False => "false",
            Checked::Mixed => "mixed",
        }
    }
}

/// Which tree a query matches over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QueryView {
    /// The whole tree the window built, generic containers included.
    #[default]
    Inspection,
    /// The nodes AccessKit's filter shows screen readers, nested as they see them.
    ScreenReader,
}

/// A node's bounds in window pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SemanticBounds {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub width: f32,
    /// The height.
    pub height: f32,
}

impl SemanticBounds {
    /// The bounds as gpux bounds.
    pub fn to_bounds(self) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.x), px(self.y)),
            size(px(self.width), px(self.height)),
        )
    }
}

/// One node of a window's accessibility tree, as queries see it.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticNode {
    /// The node's AccessKit id: a devtools ref, and the target of an action request.
    pub node_id: NodeId,
    /// The AccessKit role.
    pub ak_role: accesskit::Role,
    /// The query name of the role ([`crate::query_role_name`]).
    pub role: String,
    /// The computed name, as screen readers announce it.
    pub name: Option<String>,
    /// A text node's text, or a text field's.
    pub text: Option<String>,
    /// The value or numeric value.
    pub value: Option<String>,
    /// The description.
    pub description: Option<String>,
    /// The placeholder of an empty text field.
    pub placeholder: Option<String>,
    /// The global element id the node belongs to, while inspecting.
    pub element_id: Option<String>,
    /// Whether the element id is its parent's: a text leaf or a synthetic child of the element.
    /// An `id` term matches the element's own node only.
    pub inherits_element_id: bool,
    /// The `accessibility_id` its element set.
    pub accessibility_id: Option<String>,
    /// Bounds in window pixels.
    pub bounds: SemanticBounds,
    /// The checked state of a check box, switch or toggle.
    pub checked: Option<Checked>,
    /// Whether an expandable node is expanded.
    pub expanded: Option<bool>,
    /// Whether a selectable node is selected.
    pub selected: Option<bool>,
    /// Whether the node is disabled.
    pub disabled: bool,
    /// Whether a form control needs a value.
    pub required: bool,
    /// Whether a control's value can be read but not changed.
    pub read_only: bool,
    /// Whether the node holds the window's accessibility focus.
    pub focused: bool,
    /// The heading or tree level.
    pub level: Option<usize>,
    /// The numeric value of a slider, spin button or progress indicator.
    pub numeric_value: Option<f64>,
    /// The least numeric value.
    pub min_numeric_value: Option<f64>,
    /// The greatest numeric value.
    pub max_numeric_value: Option<f64>,
    /// The actions the node advertises.
    pub actions: Vec<Action>,
    /// The node's custom actions: id and description.
    pub custom_actions: Vec<(i32, String)>,
    /// The parent in the whole tree, an index into [`SemanticSnapshot::nodes`].
    pub parent: Option<usize>,
    /// Whether screen readers see the node.
    pub screen_reader_visible: bool,
    /// The nearest ancestor screen readers see.
    pub screen_reader_parent: Option<usize>,
    /// The depth in the whole tree, the root at 0.
    pub depth: usize,
}

/// A window's accessibility tree as queries see it: its nodes in tree order (depth first, the
/// root first).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticSnapshot {
    /// The nodes, depth first.
    pub nodes: Vec<SemanticNode>,
    /// The focused node, an index into [`Self::nodes`].
    pub focus: Option<usize>,
}

impl SemanticSnapshot {
    /// The snapshot of the tree `window` built last, or `None` while no source asks for one.
    pub fn of(window: &Window) -> Option<SemanticSnapshot> {
        let update = window.a11y_tree()?.clone();
        let scale = window.scale_factor().max(f32::EPSILON) as f64;
        let tree = Tree::new(update, true);
        let state = tree.state();
        // The tree's focus, or the inspection container holding the window's focus.
        let focus_id = window
            .a11y_inspection_focus()
            .or_else(|| state.focus().map(|node| node.locate().0));
        let mut snapshot = SemanticSnapshot::default();
        // Depth first, recording each node's parent and nearest screen-reader ancestor.
        let mut stack: Vec<(NodeRef, Option<usize>, Option<usize>, usize)> =
            vec![(state.root(), None, None, 0)];
        while let Some((node, parent, sr_parent, depth)) = stack.pop() {
            let index = snapshot.nodes.len();
            let visible = depth == 0 || common_filter(&node) == FilterResult::Include;
            let data = node.data();
            let id = node.locate().0;
            let label_value = |value: Option<&str>| value.map(str::to_string);
            let text = match node.role() {
                accesskit::Role::Label
                | accesskit::Role::TextInput
                | accesskit::Role::MultilineTextInput
                | accesskit::Role::SearchInput
                | accesskit::Role::PasswordInput => node.value(),
                _ => None,
            };
            let value = node
                .value()
                .or_else(|| data.numeric_value().map(format_number));
            let bounds = data
                .bounds()
                .map(|rect| SemanticBounds {
                    x: (rect.x0 / scale) as f32,
                    y: (rect.y0 / scale) as f32,
                    width: ((rect.x1 - rect.x0) / scale) as f32,
                    height: ((rect.y1 - rect.y0) / scale) as f32,
                })
                .unwrap_or_default();
            let mut next_action = 0u8;
            let actions = std::iter::from_fn(|| {
                let action = Action::n(next_action)?;
                next_action += 1;
                Some(action)
            })
            .filter(|action| data.supports_action(*action))
            .collect();
            let element_id = window.a11y_element_id(id).map(|id| id.to_string());
            let inherits_element_id = element_id.is_some()
                && parent
                    .is_some_and(|parent: usize| snapshot.nodes[parent].element_id == element_id);
            snapshot.nodes.push(SemanticNode {
                node_id: id,
                ak_role: node.role(),
                role: crate::query_role_name(node.role()),
                name: node.label(),
                text,
                value,
                description: node.description(),
                placeholder: label_value(data.placeholder()),
                element_id,
                inherits_element_id,
                accessibility_id: label_value(data.author_id()),
                bounds,
                checked: data.toggled().map(|toggled| match toggled {
                    Toggled::True => Checked::True,
                    Toggled::False => Checked::False,
                    Toggled::Mixed => Checked::Mixed,
                }),
                expanded: data.is_expanded(),
                selected: data.is_selected(),
                disabled: data.is_disabled(),
                required: data.is_required(),
                read_only: data.is_read_only(),
                focused: focus_id == Some(id),
                level: data.level(),
                numeric_value: data.numeric_value(),
                min_numeric_value: data.min_numeric_value(),
                max_numeric_value: data.max_numeric_value(),
                actions,
                custom_actions: data
                    .custom_actions()
                    .iter()
                    .map(|action| (action.id, action.description.clone()))
                    .collect(),
                parent,
                screen_reader_visible: visible,
                screen_reader_parent: sr_parent,
                depth,
            });
            if focus_id == Some(id) {
                snapshot.focus = Some(index);
            }
            let child_sr_parent = if visible { Some(index) } else { sr_parent };
            let children: Vec<NodeRef> = node.children().collect();
            for child in children.into_iter().rev() {
                stack.push((child, Some(index), child_sr_parent, depth + 1));
            }
        }
        Some(snapshot)
    }

    /// The node with AccessKit id `id`.
    pub fn node(&self, id: NodeId) -> Option<&SemanticNode> {
        self.nodes.iter().find(|node| node.node_id == id)
    }

    fn parent_in(&self, node: &SemanticNode, view: QueryView) -> Option<usize> {
        match view {
            QueryView::Inspection => node.parent,
            QueryView::ScreenReader => node.screen_reader_parent,
        }
    }

    /// Whether `node` lies under one of `ancestors` (indices) in `view`.
    fn has_ancestor_in(
        &self,
        node: &SemanticNode,
        ancestors: &HashSet<usize>,
        view: QueryView,
    ) -> bool {
        let mut parent = self.parent_in(node, view);
        while let Some(index) = parent {
            if ancestors.contains(&index) {
                return true;
            }
            parent = self.parent_in(&self.nodes[index], view);
        }
        false
    }

    fn index_of(&self, node: &SemanticNode) -> Option<usize> {
        self.nodes
            .iter()
            .position(|candidate| candidate.node_id == node.node_id)
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// A semantic query. Build one with the constructors and builder methods, or parse its text
/// form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// The role's query name, such as `button`, `textbox` or `text`.
    pub role: Option<String>,
    /// The computed name.
    pub name: Option<TextMatch>,
    /// A text node's text, or a text field's.
    pub text: Option<TextMatch>,
    /// The value.
    pub value: Option<TextMatch>,
    /// A suffix of the global element id, matched on a `.` boundary, or an `accessibility_id`.
    pub id: Option<String>,
    /// The checked state.
    pub checked: Option<Checked>,
    /// The expanded state.
    pub expanded: Option<bool>,
    /// The selected state (an unselectable node is not selected).
    pub selected: Option<bool>,
    /// The disabled state.
    pub disabled: Option<bool>,
    /// Whether the node holds focus.
    pub focused: Option<bool>,
    /// The tree the query matches over.
    pub view: QueryView,
    /// Restricts matches to descendants of the nodes this query matches.
    pub within: Option<Box<Query>>,
    /// Picks the zero-based match among all matches, in tree order.
    pub nth: Option<usize>,
}

impl Query {
    /// Matches nodes with this role.
    pub fn role(role: impl Into<String>) -> Self {
        Self::default().with_role(role)
    }

    /// Matches nodes whose element's global id ends with `id`, or whose `accessibility_id` is
    /// `id`.
    pub fn id(id: impl Into<String>) -> Self {
        Self::default().with_id(id)
    }

    /// Matches text nodes whose text equals `text`.
    pub fn text(text: impl Into<String>) -> Self {
        Self::default().with_text(text)
    }

    /// Matches nodes whose computed name equals `name`.
    pub fn name(name: impl Into<String>) -> Self {
        Self::default().with_name(name)
    }

    /// Also requires this role.
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }

    /// Also requires this exact computed name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(TextMatch::Exact(name.into()));
        self
    }

    /// Also requires a computed name containing `name`.
    pub fn with_name_containing(mut self, name: impl Into<String>) -> Self {
        self.name = Some(TextMatch::Contains(name.into()));
        self
    }

    /// Also requires this exact text.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(TextMatch::Exact(text.into()));
        self
    }

    /// Also requires text containing `text`.
    pub fn with_text_containing(mut self, text: impl Into<String>) -> Self {
        self.text = Some(TextMatch::Contains(text.into()));
        self
    }

    /// Also requires this exact value.
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(TextMatch::Exact(value.into()));
        self
    }

    /// Also requires a value containing `value`.
    pub fn with_value_containing(mut self, value: impl Into<String>) -> Self {
        self.value = Some(TextMatch::Contains(value.into()));
        self
    }

    /// Also requires this element id (see [`Self::id`]).
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Also requires this checked state.
    pub fn checked(mut self, checked: Checked) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Also requires this expanded state.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// Also requires this selected state.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// Also requires this disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = Some(disabled);
        self
    }

    /// Also requires the node to hold focus, or not.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    /// Matches over the tree screen readers see.
    pub fn for_screen_readers(mut self) -> Self {
        self.view = QueryView::ScreenReader;
        self
    }

    /// Restricts matches to descendants of the nodes `parent` matches.
    pub fn within(mut self, parent: impl Into<Query>) -> Self {
        self.within = Some(Box::new(parent.into()));
        self
    }

    /// Picks the zero-based match among all matches.
    pub fn nth(mut self, index: usize) -> Self {
        self.nth = Some(index);
        self
    }

    /// Every node this query matches, in tree order, ignoring [`Self::nth`].
    pub fn matches<'a>(&self, snapshot: &'a SemanticSnapshot) -> Vec<&'a SemanticNode> {
        let scopes = self.within.as_ref().map(|within| {
            let parents = if within.nth.is_some() {
                within
                    .resolve(snapshot)
                    .map(|parent| vec![parent])
                    .unwrap_or_default()
            } else {
                within.matches(snapshot)
            };
            parents
                .into_iter()
                .filter_map(|parent| snapshot.index_of(parent))
                .collect::<HashSet<_>>()
        });
        snapshot
            .nodes
            .iter()
            .filter(|node| self.matches_node(node))
            .filter(|node| {
                scopes
                    .as_ref()
                    .is_none_or(|scopes| snapshot.has_ancestor_in(node, scopes, self.view))
            })
            .collect()
    }

    /// The one node this query identifies: its only match, or its [`Self::nth`] match.
    ///
    /// Fails when nothing matches, or when several nodes match and no `nth` picks one. The
    /// error lists the candidates.
    pub fn resolve<'a>(
        &self,
        snapshot: &'a SemanticSnapshot,
    ) -> Result<&'a SemanticNode, QueryError> {
        let matches = self.matches(snapshot);
        let error = |kind| QueryError {
            query: self.to_string(),
            kind,
            candidates: candidates(snapshot, &matches),
        };
        match (self.nth, matches.as_slice()) {
            (Some(index), _) => matches.get(index).copied().ok_or_else(|| {
                error(QueryErrorKind::NthOutOfRange {
                    index,
                    count: matches.len(),
                })
            }),
            (None, [node]) => Ok(node),
            (None, []) => Err(error(QueryErrorKind::NotFound)),
            (None, _) => Err(error(QueryErrorKind::Ambiguous {
                count: matches.len(),
            })),
        }
    }

    fn matches_node(&self, node: &SemanticNode) -> bool {
        (self.view == QueryView::Inspection || node.screen_reader_visible)
            && self.role.as_ref().is_none_or(|role| node.role == *role)
            && self
                .name
                .as_ref()
                .is_none_or(|name| name.matches(node.name.as_deref()))
            && self
                .text
                .as_ref()
                .is_none_or(|text| text.matches(node.text.as_deref()))
            && self
                .value
                .as_ref()
                .is_none_or(|value| value.matches(node.value.as_deref()))
            && self.id.as_ref().is_none_or(|id| {
                (!node.inherits_element_id
                    && node
                        .element_id
                        .as_deref()
                        .is_some_and(|element_id| id_has_suffix(element_id, id)))
                    || node.accessibility_id.as_deref() == Some(id.as_str())
            })
            && self
                .checked
                .is_none_or(|checked| node.checked == Some(checked))
            && self
                .expanded
                .is_none_or(|expanded| node.expanded == Some(expanded))
            && self
                .selected
                .is_none_or(|selected| node.selected.unwrap_or(false) == selected)
            && self
                .disabled
                .is_none_or(|disabled| node.disabled == disabled)
            && self.focused.is_none_or(|focused| node.focused == focused)
    }
}

/// Whether `node` or one of its descendants holds focus.
pub fn focus_is_within(snapshot: &SemanticSnapshot, node: &SemanticNode) -> bool {
    let Some(index) = snapshot.index_of(node) else {
        return false;
    };
    let scope = HashSet::from([index]);
    snapshot.nodes.iter().any(|candidate| {
        candidate.focused
            && (candidate.node_id == node.node_id
                || snapshot.has_ancestor_in(candidate, &scope, QueryView::Inspection))
    })
}

fn id_has_suffix(element_id: &str, suffix: &str) -> bool {
    element_id == suffix
        || element_id
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

const MAX_LISTED_CANDIDATES: usize = 20;

/// The matches, or when there are none, the nodes a test is likely to look for: those with a
/// name, text, value or element id.
fn candidates(snapshot: &SemanticSnapshot, matches: &[&SemanticNode]) -> Vec<String> {
    let listed: Vec<&SemanticNode> = if matches.is_empty() {
        snapshot
            .nodes
            .iter()
            .filter(|node| {
                node.depth > 0
                    && (node.element_id.is_some()
                        || node.name.is_some()
                        || node.text.is_some()
                        || node.value.is_some())
            })
            .collect()
    } else {
        matches.to_vec()
    };
    listed
        .iter()
        .take(MAX_LISTED_CANDIDATES)
        .map(|node| describe_node(node))
        .collect()
}

/// A one-line description of a node: its role, id, name, text, value, states and bounds.
pub fn describe_node(node: &SemanticNode) -> String {
    let mut description = format!("role={}", quote(&node.role));
    for (key, value) in [
        ("id", node.element_id.as_deref()),
        ("name", node.name.as_deref()),
        ("text", node.text.as_deref()),
        ("value", node.value.as_deref()),
    ] {
        if let Some(value) = value {
            description.push_str(&format!(" {key}={}", quote(value)));
        }
    }
    if let Some(checked) = node.checked {
        description.push_str(&format!(" checked={}", checked.name()));
    }
    if node.focused {
        description.push_str(" focused=true");
    }
    let SemanticBounds {
        x,
        y,
        width,
        height,
    } = node.bounds;
    description.push_str(&format!(" at ({x}, {y}) {width}x{height}"));
    description
}

/// Why a query did not identify exactly one node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryErrorKind {
    /// No node matched.
    NotFound,
    /// Several nodes matched and no `nth` picked one.
    Ambiguous {
        /// How many nodes matched.
        count: usize,
    },
    /// `nth` asked for a match past the last one.
    NthOutOfRange {
        /// The requested zero-based index.
        index: usize,
        /// How many nodes matched.
        count: usize,
    },
    /// The window builds no accessibility tree (accessibility is disabled).
    NoTree,
}

/// A query that did not identify exactly one node, with the candidates it saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    /// The query, in text form.
    pub query: String,
    /// What went wrong.
    pub kind: QueryErrorKind,
    /// The matching nodes, or when nothing matched, the nodes with a name, text, value or id,
    /// at most twenty, one line each.
    pub candidates: Vec<String>,
}

impl QueryError {
    /// The error for a query on a window that builds no tree.
    pub fn no_tree(query: &Query) -> Self {
        QueryError {
            query: query.to_string(),
            kind: QueryErrorKind::NoTree,
            candidates: Vec::new(),
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            QueryErrorKind::NotFound => write!(f, "no node matches `{}`", self.query)?,
            QueryErrorKind::Ambiguous { count } => write!(
                f,
                "{count} nodes match `{}`; narrow the query or pick one with nth",
                self.query
            )?,
            QueryErrorKind::NthOutOfRange { index, count } => write!(
                f,
                "`{}` asks for match {index}, but only {count} nodes match",
                self.query
            )?,
            QueryErrorKind::NoTree => {
                return write!(
                    f,
                    "`{}` cannot be resolved: the window builds no accessibility tree \
                     (accessibility is disabled)",
                    self.query
                );
            }
        }
        if self.candidates.is_empty() {
            write!(f, "\nthe tree has no nodes with a name, text, value or id")?;
        } else {
            write!(f, "\ncandidates:")?;
            for candidate in &self.candidates {
                write!(f, "\n  {candidate}")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for QueryError {}

fn needs_quotes(value: &str) -> bool {
    value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\\' | '=' | '>' | '~'))
}

fn quote(value: &str) -> String {
    if !needs_quotes(value) {
        return value.to_string();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for c in value.chars() {
        if matches!(c, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(within) = &self.within {
            write!(f, "{within} >> ")?;
        }
        let mut terms = Vec::new();
        if let Some(role) = &self.role {
            terms.push(format!("role={}", quote(role)));
        }
        for (key, value) in [
            ("name", &self.name),
            ("text", &self.text),
            ("value", &self.value),
        ] {
            match value {
                Some(TextMatch::Exact(value)) => terms.push(format!("{key}={}", quote(value))),
                Some(TextMatch::Contains(value)) => terms.push(format!("{key}~={}", quote(value))),
                None => {}
            }
        }
        if let Some(id) = &self.id {
            terms.push(format!("id={}", quote(id)));
        }
        if let Some(checked) = self.checked {
            terms.push(format!("checked={}", checked.name()));
        }
        for (key, value) in [
            ("expanded", self.expanded),
            ("selected", self.selected),
            ("disabled", self.disabled),
            ("focused", self.focused),
        ] {
            if let Some(value) = value {
                terms.push(format!("{key}={value}"));
            }
        }
        if self.view == QueryView::ScreenReader {
            terms.push("view=screen-reader".into());
        }
        if let Some(nth) = self.nth {
            terms.push(format!("nth={nth}"));
        }
        if terms.is_empty() {
            write!(f, "*")
        } else {
            write!(f, "{}", terms.join(" "))
        }
    }
}

/// A query text that could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryParseError(pub String);

impl fmt::Display for QueryParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid query: {}", self.0)
    }
}

impl std::error::Error for QueryParseError {}

impl FromStr for Query {
    type Err = QueryParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut query: Option<Query> = None;
        for scope in split_scopes(text)? {
            let mut next = parse_scope(&scope)?;
            if let Some(parent) = query.take() {
                next.within = Some(Box::new(parent));
            }
            query = Some(next);
        }
        query.ok_or_else(|| QueryParseError("the query is empty".into()))
    }
}

impl From<&str> for Query {
    /// Parses the text form; panics on invalid text. Use [`str::parse`] to handle errors.
    fn from(text: &str) -> Self {
        text.parse().unwrap_or_else(|error| panic!("{error}"))
    }
}

/// Splits on `>>` outside quotes.
fn split_scopes(text: &str) -> Result<Vec<String>, QueryParseError> {
    let mut scopes = vec![String::new()];
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_quotes => {
                scopes.last_mut().unwrap().push(c);
                if let Some(escaped) = chars.next() {
                    scopes.last_mut().unwrap().push(escaped);
                }
            }
            '"' => {
                in_quotes = !in_quotes;
                scopes.last_mut().unwrap().push(c);
            }
            '>' if !in_quotes && chars.peek() == Some(&'>') => {
                chars.next();
                scopes.push(String::new());
            }
            _ => scopes.last_mut().unwrap().push(c),
        }
    }
    if in_quotes {
        return Err(QueryParseError("unterminated quoted value".into()));
    }
    if scopes.iter().any(|scope| scope.trim().is_empty()) && scopes.len() > 1 {
        return Err(QueryParseError("`>>` needs a scope on both sides".into()));
    }
    Ok(scopes)
}

fn parse_bool(key: &str, value: &str) -> Result<bool, QueryParseError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(QueryParseError(format!(
            "`{key}` is `true` or `false`, got `{value}`"
        ))),
    }
}

fn parse_scope(scope: &str) -> Result<Query, QueryParseError> {
    let mut query = Query::default();
    let mut chars = scope.trim().chars().peekable();
    let mut any = false;
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(&first) = chars.peek() else {
            break;
        };
        if first == '*' {
            chars.next();
            any = true;
            continue;
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c == '~' || c.is_whitespace() {
                break;
            }
            key.push(c);
            chars.next();
        }
        let contains = chars.peek() == Some(&'~');
        if contains {
            chars.next();
        }
        if chars.next() != Some('=') {
            return Err(QueryParseError(format!("expected `=` after `{key}`")));
        }
        let value = parse_value(&mut chars)?;
        let text_match = |value: String| {
            if contains {
                TextMatch::Contains(value)
            } else {
                TextMatch::Exact(value)
            }
        };
        match key.as_str() {
            "name" => query.name = Some(text_match(value)),
            "text" => query.text = Some(text_match(value)),
            "value" => query.value = Some(text_match(value)),
            "type" => {
                return Err(QueryParseError(
                    "`type=` was removed: queries match the accessibility tree, which has no \
                     element types; match by `role`, `name`, `text` or `id`"
                        .into(),
                ));
            }
            _ if contains => {
                return Err(QueryParseError(format!("`{key}` does not support `~=`")));
            }
            "role" => query.role = Some(value),
            "id" => query.id = Some(value),
            "checked" => {
                query.checked = Some(match value.as_str() {
                    "true" => Checked::True,
                    "false" => Checked::False,
                    "mixed" => Checked::Mixed,
                    _ => {
                        return Err(QueryParseError(format!(
                            "`checked` is `true`, `false` or `mixed`, got `{value}`"
                        )));
                    }
                })
            }
            "expanded" => query.expanded = Some(parse_bool(&key, &value)?),
            "selected" => query.selected = Some(parse_bool(&key, &value)?),
            "disabled" => query.disabled = Some(parse_bool(&key, &value)?),
            "focused" => query.focused = Some(parse_bool(&key, &value)?),
            "view" => {
                query.view = match value.as_str() {
                    "inspection" => QueryView::Inspection,
                    "screen-reader" => QueryView::ScreenReader,
                    _ => {
                        return Err(QueryParseError(format!(
                            "`view` is `inspection` or `screen-reader`, got `{value}`"
                        )));
                    }
                }
            }
            "nth" => {
                query.nth = Some(value.parse().map_err(|_| {
                    QueryParseError(format!("`nth` needs a non-negative integer, got `{value}`"))
                })?)
            }
            _ => return Err(QueryParseError(format!("unknown key `{key}`"))),
        }
        any = true;
    }
    if !any {
        return Err(QueryParseError("a scope needs at least one term".into()));
    }
    Ok(query)
}

fn parse_value(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, QueryParseError> {
    let mut value = String::new();
    if chars.peek() == Some(&'"') {
        chars.next();
        loop {
            match chars.next() {
                Some('\\') => match chars.next() {
                    Some(escaped) => value.push(escaped),
                    None => return Err(QueryParseError("unterminated escape".into())),
                },
                Some('"') => return Ok(value),
                Some(c) => value.push(c),
                None => return Err(QueryParseError("unterminated quoted value".into())),
            }
        }
    }
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            break;
        }
        value.push(c);
        chars.next();
    }
    if value.is_empty() {
        return Err(QueryParseError("a value is missing".into()));
    }
    Ok(value)
}
