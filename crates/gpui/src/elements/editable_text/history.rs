use std::ops::Range;

use super::EditableSelection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditKind {
    Typing,
    Composition,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistoryState {
    pub text: String,
    pub selection: EditableSelection,
    pub marked_range: Option<Range<usize>>,
}

#[derive(Clone, Debug)]
struct HistoryEntry {
    before: HistoryState,
    after: HistoryState,
    kind: EditKind,
}

#[derive(Default)]
pub(crate) struct EditHistory {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl EditHistory {
    pub fn record(&mut self, before: HistoryState, after: HistoryState, kind: EditKind) {
        let can_group = matches!(kind, EditKind::Typing | EditKind::Composition)
            && self.undo.last().is_some_and(|entry| {
                entry.kind == kind
                    && entry.after == before
                    && (!matches!(kind, EditKind::Typing)
                        || (entry.after.selection.is_empty() && entry.after.marked_range.is_none()))
            });

        if can_group {
            self.undo.last_mut().unwrap().after = after;
        } else {
            self.undo.push(HistoryEntry {
                before,
                after,
                kind,
            });
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> Option<HistoryState> {
        let entry = self.undo.pop()?;
        let state = entry.before.clone();
        self.redo.push(entry);
        Some(state)
    }

    pub fn redo(&mut self) -> Option<HistoryState> {
        let entry = self.redo.pop()?;
        let state = entry.after.clone();
        self.undo.push(entry);
        Some(state)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn rewrite_current_after(
        &mut self,
        current: &HistoryState,
        rewritten: HistoryState,
    ) -> bool {
        let Some(entry) = self.undo.last_mut() else {
            return false;
        };
        if &entry.after != current {
            return false;
        }
        entry.after = rewritten;
        true
    }
}
