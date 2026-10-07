//! Semantic element queries over a window's [`DevtoolsFrameSnapshot`].
//!
//! A [`Query`] finds rendered elements by what they mean (role, accessible name, text, value,
//! element id, element type, enclosing element) instead of by coordinates. It reads only the
//! public snapshot, so it behaves the same in debug and release builds and never depends on
//! debug selectors.
//!
//! Queries have a small text form so automation outside Rust (in-app conformance hosts, browser
//! bridges) can use the same vocabulary:
//!
//! ```text
//! id=toolbar >> role=button name="Save as" nth=1
//! ```
//!
//! A scope is a list of `key=value` terms, all of which must match. `>>` scopes the query on
//! its right to the descendants of the elements its left side matches. Keys are `role`, `name`,
//! `text`, `value` (exact match, or substring with `~=`), `id` (a suffix of the global element
//! id on a `.` boundary), `type` (the element type's name, such as `Div`), and `nth` (the
//! zero-based match to pick). Values containing spaces, quotes, `=` or `>` are double-quoted with
//! `\"` and `\\` escapes.

use crate::{DevtoolsBounds, DevtoolsElementSnapshot, DevtoolsFrameSnapshot};
use std::{collections::HashSet, fmt, str::FromStr};

#[cfg(any(test, feature = "test-support"))]
mod locator;
#[cfg(any(test, feature = "test-support"))]
pub use locator::*;

/// How a query field matches an element's string property.
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

/// A semantic element query. Build one with the constructors and builder methods, or parse
/// its text form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    /// The accessibility role, such as `button`, `textbox` or `text`.
    pub role: Option<String>,
    /// The accessible name.
    pub name: Option<TextMatch>,
    /// The rendered text content.
    pub text: Option<TextMatch>,
    /// The editable value.
    pub value: Option<TextMatch>,
    /// A suffix of the global element id, matched on a `.` boundary.
    pub id: Option<String>,
    /// The element type's name, such as `Div` or `StyledText`.
    pub type_name: Option<String>,
    /// Restricts matches to descendants of the elements this query matches.
    pub within: Option<Box<Query>>,
    /// Picks the zero-based match among all matches, in prepaint order.
    pub nth: Option<usize>,
}

impl Query {
    /// Matches elements with this accessibility role.
    pub fn role(role: impl Into<String>) -> Self {
        Self::default().with_role(role)
    }

    /// Matches elements whose global element id ends with `id`.
    pub fn id(id: impl Into<String>) -> Self {
        Self::default().with_id(id)
    }

    /// Matches elements whose rendered text equals `text`.
    pub fn text(text: impl Into<String>) -> Self {
        Self::default().with_text(text)
    }

    /// Matches elements whose accessible name equals `name`.
    pub fn name(name: impl Into<String>) -> Self {
        Self::default().with_name(name)
    }

    /// Matches elements of this type, such as `Div`.
    pub fn type_name(type_name: impl Into<String>) -> Self {
        Self::default().with_type_name(type_name)
    }

    /// Also requires this accessibility role.
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }

    /// Also requires this exact accessible name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(TextMatch::Exact(name.into()));
        self
    }

    /// Also requires an accessible name containing `name`.
    pub fn with_name_containing(mut self, name: impl Into<String>) -> Self {
        self.name = Some(TextMatch::Contains(name.into()));
        self
    }

    /// Also requires this exact rendered text.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(TextMatch::Exact(text.into()));
        self
    }

    /// Also requires rendered text containing `text`.
    pub fn with_text_containing(mut self, text: impl Into<String>) -> Self {
        self.text = Some(TextMatch::Contains(text.into()));
        self
    }

    /// Also requires this exact editable value.
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(TextMatch::Exact(value.into()));
        self
    }

    /// Also requires an editable value containing `value`.
    pub fn with_value_containing(mut self, value: impl Into<String>) -> Self {
        self.value = Some(TextMatch::Contains(value.into()));
        self
    }

    /// Also requires a global element id ending with `id`.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Also requires this element type.
    pub fn with_type_name(mut self, type_name: impl Into<String>) -> Self {
        self.type_name = Some(type_name.into());
        self
    }

    /// Restricts matches to descendants of the elements `parent` matches.
    pub fn within(mut self, parent: impl Into<Query>) -> Self {
        self.within = Some(Box::new(parent.into()));
        self
    }

    /// Picks the zero-based match among all matches.
    pub fn nth(mut self, index: usize) -> Self {
        self.nth = Some(index);
        self
    }

    /// Every element this query matches, in prepaint order, ignoring [`Self::nth`].
    pub fn matches<'a>(
        &self,
        snapshot: &'a DevtoolsFrameSnapshot,
    ) -> Vec<&'a DevtoolsElementSnapshot> {
        let scopes = match &self.within {
            Some(within) => {
                let parents = if within.nth.is_some() {
                    within
                        .resolve(snapshot)
                        .map(|parent| vec![parent])
                        .unwrap_or_default()
                } else {
                    within.matches(snapshot)
                };
                Some(
                    parents
                        .into_iter()
                        .map(|parent| parent.devtools_node_id)
                        .collect::<HashSet<_>>(),
                )
            }
            None => None,
        };
        snapshot
            .elements
            .iter()
            .filter(|element| self.matches_element(element))
            .filter(|element| {
                scopes
                    .as_ref()
                    .is_none_or(|scopes| has_ancestor_in(snapshot, element, scopes))
            })
            .collect()
    }

    /// The one element this query identifies: its only match, or its [`Self::nth`] match.
    ///
    /// Fails when nothing matches, or when several elements match and no `nth` picks one. The
    /// error lists the candidates.
    pub fn resolve<'a>(
        &self,
        snapshot: &'a DevtoolsFrameSnapshot,
    ) -> Result<&'a DevtoolsElementSnapshot, QueryError> {
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
            (None, [element]) => Ok(element),
            (None, []) => Err(error(QueryErrorKind::NotFound)),
            (None, _) => Err(error(QueryErrorKind::Ambiguous {
                count: matches.len(),
            })),
        }
    }

    fn matches_element(&self, element: &DevtoolsElementSnapshot) -> bool {
        let semantics = &element.semantics;
        self.role
            .as_ref()
            .is_none_or(|role| semantics.role.as_deref() == Some(role.as_str()))
            && self
                .name
                .as_ref()
                .is_none_or(|name| name.matches(semantics.name.as_deref()))
            && self
                .text
                .as_ref()
                .is_none_or(|text| text.matches(semantics.text.as_deref()))
            && self
                .value
                .as_ref()
                .is_none_or(|value| value.matches(semantics.value.as_deref()))
            && self.id.as_ref().is_none_or(|id| {
                element
                    .global_id
                    .as_deref()
                    .is_some_and(|global_id| id_has_suffix(global_id, id))
            })
            && self
                .type_name
                .as_ref()
                .is_none_or(|type_name| short_type_name(&element.type_name) == type_name)
    }
}

/// Whether `element` or one of its descendants owns focus in this frame.
pub fn focus_is_within(
    snapshot: &DevtoolsFrameSnapshot,
    element: &DevtoolsElementSnapshot,
) -> bool {
    let scope = HashSet::from([element.devtools_node_id]);
    snapshot.elements.iter().any(|candidate| {
        candidate.focused
            && (candidate.devtools_node_id == element.devtools_node_id
                || has_ancestor_in(snapshot, candidate, &scope))
    })
}

fn has_ancestor_in(
    snapshot: &DevtoolsFrameSnapshot,
    element: &DevtoolsElementSnapshot,
    ancestors: &HashSet<usize>,
) -> bool {
    let mut parent = element.parent_devtools_node_id;
    while let Some(id) = parent {
        if ancestors.contains(&id) {
            return true;
        }
        parent = snapshot
            .elements
            .iter()
            .find(|element| element.devtools_node_id == id)
            .and_then(|element| element.parent_devtools_node_id);
    }
    false
}

fn id_has_suffix(global_id: &str, suffix: &str) -> bool {
    global_id == suffix
        || global_id
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

/// `gpux::elements::div::Div` is `Div`; generic parameters are dropped.
fn short_type_name(type_name: &str) -> &str {
    let base = type_name.split('<').next().unwrap_or(type_name);
    base.rsplit("::").next().unwrap_or(base)
}

const MAX_LISTED_CANDIDATES: usize = 20;

/// The matches, or when there are none, the elements a test is likely to look for: those with
/// semantics or an element id.
fn candidates(
    snapshot: &DevtoolsFrameSnapshot,
    matches: &[&DevtoolsElementSnapshot],
) -> Vec<String> {
    let listed: Vec<&DevtoolsElementSnapshot> = if matches.is_empty() {
        snapshot
            .elements
            .iter()
            .filter(|element| {
                let semantics = &element.semantics;
                element.global_id.is_some()
                    || semantics.role.is_some()
                    || semantics.name.is_some()
                    || semantics.text.is_some()
                    || semantics.value.is_some()
            })
            .collect()
    } else {
        matches.to_vec()
    };
    listed
        .iter()
        .take(MAX_LISTED_CANDIDATES)
        .map(|element| describe_element(element))
        .collect()
}

/// A one-line description of an element: its type, id, semantics and bounds.
pub fn describe_element(element: &DevtoolsElementSnapshot) -> String {
    let mut description = short_type_name(&element.type_name).to_string();
    let semantics = &element.semantics;
    for (key, value) in [
        ("id", element.global_id.as_deref()),
        ("role", semantics.role.as_deref()),
        ("name", semantics.name.as_deref()),
        ("text", semantics.text.as_deref()),
        ("value", semantics.value.as_deref()),
    ] {
        if let Some(value) = value {
            description.push_str(&format!(" {key}={}", quote(value)));
        }
    }
    let DevtoolsBounds {
        x,
        y,
        width,
        height,
    } = element.bounds;
    description.push_str(&format!(" at ({x}, {y}) {width}x{height}"));
    description
}

/// Why a query did not identify exactly one element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryErrorKind {
    /// No element matched.
    NotFound,
    /// Several elements matched and no `nth` picked one.
    Ambiguous {
        /// How many elements matched.
        count: usize,
    },
    /// `nth` asked for a match past the last one.
    NthOutOfRange {
        /// The requested zero-based index.
        index: usize,
        /// How many elements matched.
        count: usize,
    },
}

/// A query that did not identify exactly one element, with the candidates it saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    /// The query, in text form.
    pub query: String,
    /// What went wrong.
    pub kind: QueryErrorKind,
    /// The matching elements, or when nothing matched, the elements with semantics or an id,
    /// at most twenty, one line each.
    pub candidates: Vec<String>,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            QueryErrorKind::NotFound => write!(f, "no element matches `{}`", self.query)?,
            QueryErrorKind::Ambiguous { count } => write!(
                f,
                "{count} elements match `{}`; narrow the query or pick one with nth",
                self.query
            )?,
            QueryErrorKind::NthOutOfRange { index, count } => write!(
                f,
                "`{}` asks for match {index}, but only {count} elements match",
                self.query
            )?,
        }
        if self.candidates.is_empty() {
            write!(f, "\nthe frame has no elements with semantics or ids")?;
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
        if let Some(type_name) = &self.type_name {
            terms.push(format!("type={}", quote(type_name)));
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
            "role" | "id" | "type" | "nth" if contains => {
                return Err(QueryParseError(format!("`{key}` does not support `~=`")));
            }
            "role" => query.role = Some(value),
            "id" => query.id = Some(value),
            "type" => query.type_name = Some(value),
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
