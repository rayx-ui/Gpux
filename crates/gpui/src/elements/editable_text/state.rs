use std::{ops::Range, time::Duration};

use crate::{
    Bounds, ClipboardItem, Context, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    Pixels, Point, ShapedLine, Subscription, UTF16Selection, Window,
};

use super::{
    EditableSelection, EditableTextEvent, EditableTextGeometry, EditableTextMode, TextBuffer,
    TextBufferSnapshot, TextEdit,
    history::{EditHistory, EditKind, HistoryState},
    movement::{next_grapheme_boundary, normalize_single_line, previous_grapheme_boundary},
};

const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);

/// Entity state for neutral single-line editing mechanics.
pub struct EditableTextState {
    buffer: TextBuffer,
    selection: EditableSelection,
    marked_range: Option<Range<usize>>,
    mode: EditableTextMode,
    placeholder: String,
    focus_handle: FocusHandle,
    history: EditHistory,
    geometry: Option<EditableTextGeometry>,
    is_selecting: bool,
    is_focused: bool,
    caret_visible: bool,
    blink_epoch: u64,
    focus_subscriptions: Vec<Subscription>,
}

#[allow(missing_docs)]
impl EditableTextState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::with_text("", cx)
    }

    pub fn with_text(text: impl AsRef<str>, cx: &mut Context<Self>) -> Self {
        let text = normalize_single_line(text.as_ref());
        Self {
            selection: EditableSelection::caret(text.len()),
            buffer: TextBuffer::new(text),
            marked_range: None,
            mode: EditableTextMode::Editable,
            placeholder: String::new(),
            focus_handle: cx.focus_handle(),
            history: EditHistory::default(),
            geometry: None,
            is_selecting: false,
            is_focused: false,
            caret_visible: false,
            blink_epoch: 0,
            focus_subscriptions: Vec::new(),
        }
    }

    pub fn snapshot(&self) -> TextBufferSnapshot {
        self.buffer.snapshot()
    }

    pub fn text(&self) -> String {
        self.snapshot().text()
    }

    pub fn revision(&self) -> u64 {
        self.buffer.revision()
    }

    pub fn selection(&self) -> &EditableSelection {
        &self.selection
    }

    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    pub fn mode(&self) -> EditableTextMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: EditableTextMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if mode == EditableTextMode::Disabled {
            self.is_selecting = false;
            self.marked_range = None;
            self.stop_blink();
        }
        cx.notify();
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<String>, cx: &mut Context<Self>) {
        self.placeholder = normalize_single_line(&placeholder.into());
        cx.notify();
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn is_focused(&self) -> bool {
        self.is_focused
    }

    pub fn caret_visible(&self) -> bool {
        self.caret_visible
    }

    pub fn geometry(&self) -> Option<&EditableTextGeometry> {
        self.geometry.as_ref()
    }

    pub fn install_geometry(
        &mut self,
        revision: u64,
        bounds: Bounds<Pixels>,
        line: ShapedLine,
    ) -> bool {
        if revision != self.revision() {
            return false;
        }
        self.geometry = Some(EditableTextGeometry::new(revision, bounds, line));
        true
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) -> bool {
        if self.mode == EditableTextMode::Disabled {
            return false;
        }
        let range = anchor.min(head)..anchor.max(head);
        if self.snapshot().slice(range).is_err() {
            return false;
        }
        self.selection = EditableSelection::new(anchor, head);
        self.marked_range = None;
        self.restart_blink(cx);
        cx.notify();
        true
    }

    pub fn move_to_start(&mut self, cx: &mut Context<Self>) {
        self.move_to(0, false, cx);
    }

    pub fn move_to_start_extending(&mut self, cx: &mut Context<Self>) {
        self.move_to(0, true, cx);
    }

    pub fn move_to_end(&mut self, cx: &mut Context<Self>) {
        self.move_to(self.buffer.len(), false, cx);
    }

    pub fn move_to_end_extending(&mut self, cx: &mut Context<Self>) {
        self.move_to(self.buffer.len(), true, cx);
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        self.selection = EditableSelection::new(0, self.buffer.len());
        self.marked_range = None;
        self.restart_blink(cx);
        cx.notify();
    }

    pub fn move_left(&mut self, extend: bool, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        let offset = if !extend && !self.selection.is_empty() {
            self.selection.range().start
        } else {
            previous_grapheme_boundary(&self.text(), self.selection.head())
        };
        self.move_to(offset, extend, cx);
    }

    pub fn move_right(&mut self, extend: bool, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        let offset = if !extend && !self.selection.is_empty() {
            self.selection.range().end
        } else {
            next_grapheme_boundary(&self.text(), self.selection.head())
        };
        self.move_to(offset, extend, cx);
    }

    pub fn delete_backward(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let range = if self.selection.is_empty() {
            previous_grapheme_boundary(&self.text(), self.selection.head())..self.selection.head()
        } else {
            self.selection.range()
        };
        !range.is_empty() && self.replace_byte_range(range, "", EditKind::Other, cx)
    }

    pub fn delete_forward(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let range = if self.selection.is_empty() {
            self.selection.head()..next_grapheme_boundary(&self.text(), self.selection.head())
        } else {
            self.selection.range()
        };
        !range.is_empty() && self.replace_byte_range(range, "", EditKind::Other, cx)
    }

    pub fn replace_selection(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let kind = if self.selection.is_empty() && !text.is_empty() {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.replace_byte_range(self.selection.range(), text, kind, cx)
    }

    pub fn replace_utf16_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let range = match range_utf16 {
            Some(range) => match self.strict_range_from_utf16(range) {
                Some(range) => range,
                None => return false,
            },
            None => self
                .marked_range
                .clone()
                .unwrap_or_else(|| self.selection.range()),
        };
        let kind = if range.is_empty() && !text.is_empty() {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.replace_byte_range(range, text, kind, cx)
    }

    pub fn replace_and_mark(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_range_utf16: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let range = match range_utf16 {
            Some(range) => match self.strict_range_from_utf16(range) {
                Some(range) => range,
                None => return false,
            },
            None => self
                .marked_range
                .clone()
                .unwrap_or_else(|| self.selection.range()),
        };
        let replacement = normalize_single_line(text);
        let replacement_snapshot = TextBuffer::new(&replacement).snapshot();
        let relative_selection = match selected_range_utf16 {
            Some(relative) => {
                let start = match replacement_snapshot.utf16_to_byte(relative.start) {
                    Ok(start) => start,
                    Err(_) => return false,
                };
                let end = match replacement_snapshot.utf16_to_byte(relative.end) {
                    Ok(end) => end,
                    Err(_) => return false,
                };
                if start > end {
                    return false;
                }
                start..end
            }
            None => replacement.len()..replacement.len(),
        };
        let selection = EditableSelection::new(
            range.start + relative_selection.start,
            range.start + relative_selection.end,
        );
        let marked =
            (!replacement.is_empty()).then_some(range.start..range.start + replacement.len());
        self.apply_edit(
            range,
            replacement,
            selection,
            marked,
            EditKind::Composition,
            cx,
        )
    }

    pub fn unmark(&mut self, cx: &mut Context<Self>) {
        if self.marked_range.take().is_some() {
            cx.notify();
        }
    }

    pub fn copy(&self, cx: &mut Context<Self>) -> bool {
        if self.mode == EditableTextMode::Disabled || self.selection.is_empty() {
            return false;
        }
        let Ok(text) = self.snapshot().slice(self.selection.range()) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    pub fn cut(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() || self.selection.is_empty() {
            return false;
        }
        if !self.copy(cx) {
            return false;
        }
        self.replace_byte_range(self.selection.range(), "", EditKind::Other, cx)
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return false;
        };
        self.replace_byte_range(self.selection.range(), &text, EditKind::Other, cx)
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let Some(state) = self.history.undo() else {
            return false;
        };
        self.restore_history_state(state, cx)
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let Some(state) = self.history.redo() else {
            return false;
        };
        self.restore_history_state(state, cx)
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        if self.text().is_empty() && self.selection == EditableSelection::default() {
            return;
        }
        if self.replace_byte_range(0..self.buffer.len(), "", EditKind::Other, cx) {
            self.history.clear();
        }
    }

    pub(crate) fn ensure_focus_tracking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus_subscriptions.is_empty() {
            let handle = self.focus_handle.clone();
            let focus = cx.on_focus(&handle, window, |state, window, cx| {
                state.did_focus(window, cx)
            });
            let blur = cx.on_blur(&handle, window, |state, _, cx| state.did_blur(cx));
            self.focus_subscriptions = vec![focus, blur];
        }
        if self.focus_handle.is_focused(window) && !self.mode.permits_focus() {
            window.blur();
            self.did_blur(cx);
        } else if self.focus_handle.is_focused(window) && !self.is_focused {
            self.did_focus(window, cx);
        } else if !self.focus_handle.is_focused(window) && self.is_focused {
            self.did_blur(cx);
        }
    }

    pub(crate) fn pointer_down(
        &mut self,
        position: Point<Pixels>,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        self.is_selecting = true;
        let offset = self.index_for_point(position);
        self.move_to(offset, extend, cx);
    }

    pub(crate) fn pointer_move(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.is_selecting && self.mode != EditableTextMode::Disabled {
            let offset = self.index_for_point(position);
            self.move_to(offset, true, cx);
        }
    }

    pub(crate) fn pointer_up(&mut self) {
        self.is_selecting = false;
    }

    fn move_to(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        self.selection = if extend {
            self.selection.with_head(offset)
        } else {
            EditableSelection::caret(offset)
        };
        self.marked_range = None;
        self.restart_blink(cx);
        cx.notify();
    }

    fn index_for_point(&self, position: Point<Pixels>) -> usize {
        self.geometry
            .as_ref()
            .filter(|geometry| geometry.revision() == self.revision())
            .map(|geometry| geometry.index_for_point(position).min(self.buffer.len()))
            .unwrap_or(0)
    }

    fn replace_byte_range(
        &mut self,
        range: Range<usize>,
        text: &str,
        kind: EditKind,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let replacement = normalize_single_line(text);
        let caret = range.start + replacement.len();
        self.apply_edit(
            range,
            replacement,
            EditableSelection::caret(caret),
            None,
            kind,
            cx,
        )
    }

    fn apply_edit(
        &mut self,
        range: Range<usize>,
        replacement: String,
        selection: EditableSelection,
        marked_range: Option<Range<usize>>,
        kind: EditKind,
        cx: &mut Context<Self>,
    ) -> bool {
        let before = self.history_state();
        let edit = TextEdit::new(self.revision(), range, replacement);
        if self.buffer.apply(edit).is_err() {
            return false;
        }
        self.selection = selection;
        self.marked_range = marked_range;
        self.geometry = None;
        let after = self.history_state();
        self.history.record(before, after, kind);
        self.restart_blink(cx);
        cx.emit(EditableTextEvent::Changed {
            revision: self.revision(),
        });
        cx.notify();
        true
    }

    fn restore_history_state(&mut self, state: HistoryState, cx: &mut Context<Self>) -> bool {
        let edit = TextEdit::new(self.revision(), 0..self.buffer.len(), state.text);
        if self.buffer.apply(edit).is_err() {
            return false;
        }
        self.selection = state.selection;
        self.marked_range = state.marked_range;
        self.geometry = None;
        self.restart_blink(cx);
        cx.emit(EditableTextEvent::Changed {
            revision: self.revision(),
        });
        cx.notify();
        true
    }

    fn history_state(&self) -> HistoryState {
        HistoryState {
            text: self.text(),
            selection: self.selection,
            marked_range: self.marked_range.clone(),
        }
    }

    fn strict_range_from_utf16(&self, range: Range<usize>) -> Option<Range<usize>> {
        if range.start > range.end {
            return None;
        }
        let snapshot = self.snapshot();
        Some(snapshot.utf16_to_byte(range.start).ok()?..snapshot.utf16_to_byte(range.end).ok()?)
    }

    fn range_to_utf16(&self, range: Range<usize>) -> Option<Range<usize>> {
        let snapshot = self.snapshot();
        Some(snapshot.byte_to_utf16(range.start).ok()?..snapshot.byte_to_utf16(range.end).ok()?)
    }

    fn did_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.mode.permits_focus() {
            window.blur();
            return;
        }
        self.is_focused = true;
        self.restart_blink(cx);
        cx.notify();
    }

    fn did_blur(&mut self, cx: &mut Context<Self>) {
        self.is_focused = false;
        self.is_selecting = false;
        self.stop_blink();
        cx.notify();
    }

    fn stop_blink(&mut self) {
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        self.caret_visible = false;
    }

    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        self.caret_visible = self.is_focused;
        if !self.is_focused {
            return;
        }
        let epoch = self.blink_epoch;
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor().timer(CARET_BLINK_INTERVAL).await;
                let keep_running = entity
                    .update(cx, |state, cx| {
                        if !state.is_focused || state.blink_epoch != epoch {
                            return false;
                        }
                        state.caret_visible = !state.caret_visible;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_running {
                    break;
                }
            }
        })
        .detach();
    }
}

impl EventEmitter<EditableTextEvent> for EditableTextState {}

impl Focusable for EditableTextState {
    fn focus_handle(&self, _: &crate::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for EditableTextState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let snapshot = self.snapshot();
        let utf16_len = snapshot.byte_to_utf16(snapshot.len()).ok()?;
        let adjusted = range_utf16.start.min(utf16_len)..range_utf16.end.min(utf16_len);
        if adjusted.start > adjusted.end {
            return None;
        }
        let range = snapshot.utf16_to_byte(adjusted.start).ok()?
            ..snapshot.utf16_to_byte(adjusted.end).ok()?;
        actual_range.replace(adjusted);
        snapshot.slice(range).ok()
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.mode == EditableTextMode::Disabled && !ignore_disabled_input {
            return None;
        }
        Some(UTF16Selection {
            range: self.range_to_utf16(self.selection.range())?,
            reversed: self.selection.is_reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.range_to_utf16(self.marked_range.clone()?)
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.unmark(cx);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_utf16_range(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_and_mark(range, new_text, new_selected_range, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.strict_range_from_utf16(range_utf16)?;
        let geometry = self
            .geometry
            .as_ref()
            .filter(|geometry| geometry.revision() == self.revision())?;
        Some(geometry.bounds_for_range(range))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.snapshot()
            .byte_to_utf16(self.index_for_point(point))
            .ok()
    }

    fn accepts_text_input(&self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        self.mode.permits_mutation()
    }
}
