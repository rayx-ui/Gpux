use std::{ops::Range, time::Duration};

use crate::{
    Bounds, ClipboardItem, Context, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    Pixels, Point, Size, Subscription, UTF16Selection, Window, WindowTextSystem, point, px,
};

use super::{
    EditableSelection, EditableTextEvent, EditableTextGeometry, EditableTextLineMode,
    EditableTextMode, EditableTextStyleRuns, TextBuffer, TextBufferError, TextBufferSnapshot,
    TextEdit, TextLayoutError, TextLayoutMap, TextLayoutStyle, TextLayoutUpdate,
    history::{EditHistory, EditKind, HistoryState},
    movement::{
        grapheme_boundary_at_or_before, next_grapheme_boundary, next_logical_line_end,
        next_word_boundary, normalize_multiline, normalize_single_line, previous_grapheme_boundary,
        previous_logical_line_start, previous_word_boundary,
    },
};

const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const DRAG_SCROLL_INTERVAL: Duration = Duration::from_millis(16);

/// Entity state for neutral editable-text mechanics.
pub struct EditableTextState {
    buffer: TextBuffer,
    selection: EditableSelection,
    marked_range: Option<Range<usize>>,
    mode: EditableTextMode,
    line_mode: EditableTextLineMode,
    soft_wrap: bool,
    placeholder: String,
    style_runs: EditableTextStyleRuns,
    focus_handle: FocusHandle,
    history: EditHistory,
    geometry: Option<EditableTextGeometry>,
    layout_map: TextLayoutMap,
    scroll_offset: Point<Pixels>,
    caret_reveal_pending: bool,
    preferred_vertical_x: Option<Pixels>,
    drag_position: Option<Point<Pixels>>,
    drag_scroll_epoch: u64,
    drag_scroll_active: bool,
    is_selecting: bool,
    is_focused: bool,
    caret_visible: bool,
    blink_epoch: u64,
    focus_subscriptions: Vec<Subscription>,
    text_export_enabled: bool,
}

#[allow(missing_docs)]
impl EditableTextState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::with_text("", cx)
    }

    pub fn with_text(text: impl AsRef<str>, cx: &mut Context<Self>) -> Self {
        Self::with_line_mode(text, EditableTextLineMode::SingleLine, cx)
    }

    pub fn with_line_mode(
        text: impl AsRef<str>,
        line_mode: EditableTextLineMode,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = match line_mode {
            EditableTextLineMode::SingleLine => normalize_single_line(text.as_ref()),
            EditableTextLineMode::MultiLine => normalize_multiline(text.as_ref()),
        };
        Self {
            selection: EditableSelection::caret(text.len()),
            buffer: TextBuffer::new(text),
            marked_range: None,
            mode: EditableTextMode::Editable,
            line_mode,
            soft_wrap: true,
            placeholder: String::new(),
            style_runs: EditableTextStyleRuns::empty(0),
            focus_handle: cx.focus_handle(),
            history: EditHistory::default(),
            geometry: None,
            layout_map: TextLayoutMap::new(),
            scroll_offset: point(px(0.), px(0.)),
            caret_reveal_pending: false,
            preferred_vertical_x: None,
            drag_position: None,
            drag_scroll_epoch: 0,
            drag_scroll_active: false,
            is_selecting: false,
            is_focused: false,
            caret_visible: false,
            blink_epoch: 0,
            focus_subscriptions: Vec::new(),
            text_export_enabled: true,
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

    /// Whether platform selection extraction and clipboard export may expose text.
    pub fn text_export_enabled(&self) -> bool {
        self.text_export_enabled
    }

    /// Enables or disables platform selection extraction and clipboard export.
    pub fn set_text_export_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.text_export_enabled == enabled {
            return;
        }
        self.text_export_enabled = enabled;
        cx.notify();
    }

    pub fn line_mode(&self) -> EditableTextLineMode {
        self.line_mode
    }

    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    pub fn set_soft_wrap(&mut self, soft_wrap: bool, cx: &mut Context<Self>) {
        if self.soft_wrap == soft_wrap {
            return;
        }
        self.soft_wrap = soft_wrap;
        self.geometry = None;
        self.scroll_offset.x = px(0.);
        cx.notify();
    }

    pub fn layout_map(&self) -> &TextLayoutMap {
        &self.layout_map
    }

    pub fn viewport_bounds(&self) -> Option<Bounds<Pixels>> {
        self.geometry.as_ref().map(EditableTextGeometry::bounds)
    }

    pub fn scroll_offset(&self) -> Point<Pixels> {
        self.scroll_offset
    }

    /// Returns the full laid-out content size used by external scroll owners.
    pub fn content_size(&self) -> Size<Pixels> {
        self.layout_map.size()
    }

    /// Returns the most-negative scroll offset allowed by the current layout and viewport.
    pub fn max_scroll_offset(&self) -> Point<Pixels> {
        let Some(bounds) = self.viewport_bounds() else {
            return point(px(0.), px(0.));
        };
        point(
            (bounds.size.width - self.layout_map.size().width).min(px(0.)),
            (bounds.size.height - self.layout_map.size().height).min(px(0.)),
        )
    }

    pub fn preferred_vertical_x(&self) -> Option<Pixels> {
        self.preferred_vertical_x
    }

    pub fn set_mode(&mut self, mode: EditableTextMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if mode == EditableTextMode::Disabled {
            self.is_selecting = false;
            self.drag_position = None;
            self.stop_drag_auto_scroll();
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

    /// Returns the revision-bound inline paint styles currently installed.
    pub fn style_runs(&self) -> &EditableTextStyleRuns {
        &self.style_runs
    }

    /// Installs inline paint styles only when they match the current buffer revision.
    pub fn set_style_runs(
        &mut self,
        style_runs: EditableTextStyleRuns,
        cx: &mut Context<Self>,
    ) -> Result<(), TextBufferError> {
        let snapshot = self.snapshot();
        style_runs.clipped_to(&snapshot, 0..snapshot.len())?;
        if self.style_runs != style_runs {
            self.style_runs = style_runs;
            self.geometry = None;
            cx.notify();
        }
        Ok(())
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

    pub(crate) fn update_layout(
        &mut self,
        bounds: Bounds<Pixels>,
        mut style: TextLayoutStyle,
        text_system: &WindowTextSystem,
        display_transform: &super::EditableTextDisplayTransform,
    ) -> Result<TextLayoutUpdate, TextLayoutError> {
        let reveal_caret = self.caret_reveal_pending
            || self
                .geometry
                .as_ref()
                .is_none_or(|geometry| geometry.bounds() != bounds);
        style.wrap_width = match self.line_mode {
            EditableTextLineMode::SingleLine => None,
            EditableTextLineMode::MultiLine if self.soft_wrap && bounds.size.width > px(0.) => {
                Some(bounds.size.width)
            }
            EditableTextLineMode::MultiLine if self.soft_wrap => Some(px(0.)),
            EditableTextLineMode::MultiLine => None,
        };
        let snapshot = self.snapshot();
        let projection = display_transform.project(&snapshot.text());
        let update = match self.layout_map.update_with_projection(
            &snapshot,
            projection.as_ref(),
            &self.style_runs,
            &style,
            text_system,
        ) {
            Ok(update) => update,
            Err(error) => {
                self.geometry = None;
                return Err(error);
            }
        };
        if reveal_caret {
            self.ensure_caret_visible(bounds);
            self.caret_reveal_pending = false;
        } else {
            self.clamp_scroll(bounds);
        }
        self.geometry = Some(EditableTextGeometry::new(
            snapshot.revision(),
            bounds,
            self.scroll_offset,
        ));
        Ok(update)
    }

    pub fn scroll_by(&mut self, delta: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(bounds) = self.viewport_bounds() else {
            return false;
        };
        let previous = self.scroll_offset;
        if self.line_mode == EditableTextLineMode::MultiLine {
            self.scroll_offset.y += delta.y;
            if !self.soft_wrap {
                self.scroll_offset.x += delta.x;
            }
        } else {
            self.scroll_offset.x += delta.x;
        }
        self.clamp_scroll(bounds);
        if self.scroll_offset == previous {
            return false;
        }
        self.geometry = Some(EditableTextGeometry::new(
            self.revision(),
            bounds,
            self.scroll_offset,
        ));
        cx.notify();
        true
    }

    /// Sets the internal nonpositive scroll offset and clamps it to current geometry.
    pub fn set_scroll_offset(&mut self, offset: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(bounds) = self.viewport_bounds() else {
            return false;
        };
        let previous = self.scroll_offset;
        self.scroll_offset = offset;
        if self.line_mode == EditableTextLineMode::SingleLine {
            self.scroll_offset.y = px(0.);
        } else if self.soft_wrap {
            self.scroll_offset.x = px(0.);
        }
        self.clamp_scroll(bounds);
        if self.scroll_offset == previous {
            return false;
        }
        self.geometry = Some(EditableTextGeometry::new(
            self.revision(),
            bounds,
            self.scroll_offset,
        ));
        cx.notify();
        true
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) -> bool {
        if self.mode == EditableTextMode::Disabled {
            return false;
        }
        if anchor > self.buffer.len() || head > self.buffer.len() {
            return false;
        }
        let text = self.text();
        let anchor = grapheme_boundary_at_or_before(&text, anchor);
        let head = grapheme_boundary_at_or_before(&text, head);
        self.selection = EditableSelection::new(anchor, head);
        self.marked_range = None;
        self.caret_reveal_pending = true;
        self.restart_blink(cx);
        cx.notify();
        true
    }

    pub fn move_to_document_start(&mut self, extend: bool, cx: &mut Context<Self>) {
        self.move_to(0, extend, cx);
    }

    pub fn move_to_document_end(&mut self, extend: bool, cx: &mut Context<Self>) {
        self.move_to(self.buffer.len(), extend, cx);
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
        self.caret_reveal_pending = true;
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

    pub fn move_word_left(&mut self, extend: bool, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        let offset = if !extend && !self.selection.is_empty() {
            self.selection.range().start
        } else {
            previous_word_boundary(&self.text(), self.selection.head())
        };
        self.move_to(offset, extend, cx);
    }

    pub fn move_word_right(&mut self, extend: bool, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        let offset = if !extend && !self.selection.is_empty() {
            self.selection.range().end
        } else {
            next_word_boundary(&self.text(), self.selection.head())
        };
        self.move_to(offset, extend, cx);
    }

    pub fn move_to_line_start(&mut self, extend: bool, cx: &mut Context<Self>) {
        let text = self.text();
        let head = self.selection.head().min(text.len());
        let offset = text[..head].rfind('\n').map_or(0, |index| index + 1);
        self.move_to(offset, extend, cx);
    }

    pub fn move_to_line_end(&mut self, extend: bool, cx: &mut Context<Self>) {
        let text = self.text();
        let head = self.selection.head().min(text.len());
        let offset = text[head..]
            .find('\n')
            .map_or(text.len(), |index| head + index);
        self.move_to(offset, extend, cx);
    }

    pub fn move_to_visual_line_start(&mut self, extend: bool, cx: &mut Context<Self>) {
        let Some(display) = self.layout_map.buffer_to_display(self.selection.head()) else {
            return;
        };
        let Some(range) = self.layout_map.visual_row_range(display.row) else {
            return;
        };
        self.move_to(range.start, extend, cx);
    }

    pub fn move_to_visual_line_end(&mut self, extend: bool, cx: &mut Context<Self>) {
        let Some(display) = self.layout_map.buffer_to_display(self.selection.head()) else {
            return;
        };
        let Some(range) = self.layout_map.visual_row_range(display.row) else {
            return;
        };
        self.move_to(range.end, extend, cx);
    }

    pub fn move_up(&mut self, extend: bool, cx: &mut Context<Self>) {
        self.move_vertical(-1, extend, cx);
    }

    pub fn move_down(&mut self, extend: bool, cx: &mut Context<Self>) {
        self.move_vertical(1, extend, cx);
    }

    pub fn extend_to_previous_logical_line_start(&mut self, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        let text = self.text();
        let offset = previous_logical_line_start(&text, self.selection.head());
        self.move_to(offset, true, cx);
    }

    pub fn extend_to_next_logical_line_end(&mut self, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        let text = self.text();
        let offset = next_logical_line_end(&text, self.selection.head());
        self.move_to(offset, true, cx);
    }

    pub fn move_page_up(&mut self, extend: bool, cx: &mut Context<Self>) {
        self.move_vertical(-self.page_row_count(), extend, cx);
    }

    pub fn move_page_down(&mut self, extend: bool, cx: &mut Context<Self>) {
        self.move_vertical(self.page_row_count(), extend, cx);
    }

    pub fn insert_newline(&mut self, cx: &mut Context<Self>) -> bool {
        self.line_mode == EditableTextLineMode::MultiLine && self.replace_selection("\n", cx)
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

    pub fn delete_word_backward(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let range = if self.selection.is_empty() {
            previous_word_boundary(&self.text(), self.selection.head())..self.selection.head()
        } else {
            self.selection.range()
        };
        !range.is_empty() && self.replace_byte_range(range, "", EditKind::Other, cx)
    }

    pub fn delete_word_forward(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let range = if self.selection.is_empty() {
            self.selection.head()..next_word_boundary(&self.text(), self.selection.head())
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

    /// Rewrite the result of the current edit without adding another undo step.
    ///
    /// This is intended for synchronous, deterministic normalization performed
    /// by a subscriber to [`EditableTextEvent::Changed`]. The supplied text and
    /// selection replace the current history entry's `after` state, so undo and
    /// redo continue to operate on whole normalized values.
    pub fn rewrite_current_edit(
        &mut self,
        text: &str,
        selection: EditableSelection,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let current = self.history_state();
        let replacement = self.normalize_input(text);
        if selection.anchor() > replacement.len()
            || selection.head() > replacement.len()
            || !replacement.is_char_boundary(selection.anchor())
            || !replacement.is_char_boundary(selection.head())
        {
            return false;
        }
        let edit = TextEdit::new(self.revision(), 0..self.buffer.len(), replacement);
        if self.buffer.apply(edit).is_err() {
            return false;
        }
        self.invalidate_style_runs();
        self.selection = selection;
        self.marked_range = None;
        self.geometry = None;
        let rewritten = self.history_state();
        if !self.history.rewrite_current_after(&current, rewritten) {
            let restore = TextEdit::new(self.revision(), 0..self.buffer.len(), current.text);
            let _ = self.buffer.apply(restore);
            self.invalidate_style_runs();
            self.selection = current.selection;
            self.marked_range = current.marked_range;
            return false;
        }
        self.restart_blink(cx);
        cx.emit(EditableTextEvent::Changed {
            revision: self.revision(),
        });
        cx.notify();
        true
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
        let replacement = self.normalize_input(text);
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
        if !self.text_export_enabled
            || self.mode == EditableTextMode::Disabled
            || self.selection.is_empty()
        {
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
        self.drag_position = Some(position);
        let offset = self.index_for_point(position);
        self.move_to(offset, extend, cx);
    }

    pub(crate) fn pointer_move(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.is_selecting && self.mode != EditableTextMode::Disabled {
            self.drag_position = Some(position);
            self.auto_scroll_for_pointer(position);
            let offset = self.index_for_point(position);
            self.move_to(offset, true, cx);
            if self.pointer_is_outside_viewport(position) {
                self.start_drag_auto_scroll(cx);
            } else {
                self.stop_drag_auto_scroll();
            }
        }
    }

    pub(crate) fn pointer_up(&mut self) {
        self.is_selecting = false;
        self.drag_position = None;
        self.stop_drag_auto_scroll();
    }

    fn move_to(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        let offset = grapheme_boundary_at_or_before(&self.text(), offset);
        self.selection = if extend {
            self.selection.with_head(offset)
        } else {
            EditableSelection::caret(offset)
        };
        self.marked_range = None;
        self.preferred_vertical_x = None;
        self.caret_reveal_pending = true;
        self.restart_blink(cx);
        cx.notify();
    }

    fn move_vertical(&mut self, row_delta: isize, extend: bool, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled
            || self.line_mode != EditableTextLineMode::MultiLine
        {
            return;
        }
        let Some(display) = self.layout_map.buffer_to_display(self.selection.head()) else {
            return;
        };
        let Some(position) = self.layout_map.position_for_index(self.selection.head()) else {
            return;
        };
        let preferred_x = self.preferred_vertical_x.unwrap_or(position.x);
        let last_row = self.layout_map.visual_line_count().saturating_sub(1) as isize;
        let target_row = (display.row as isize + row_delta).clamp(0, last_row) as usize;
        let target_position = point(
            preferred_x,
            self.layout_map.line_height() * target_row + self.layout_map.line_height() / 2.0,
        );
        let Some(offset) = self.layout_map.index_for_position(target_position) else {
            return;
        };
        self.move_to(offset.min(self.buffer.len()), extend, cx);
        self.preferred_vertical_x = Some(preferred_x);
    }

    fn page_row_count(&self) -> isize {
        let line_height = self.layout_map.line_height();
        if line_height <= px(0.) {
            return 1;
        }
        self.geometry
            .as_ref()
            .map(|geometry| {
                (f32::from(geometry.bounds().size.height) / f32::from(line_height))
                    .floor()
                    .max(1.) as isize
            })
            .unwrap_or(1)
    }

    fn normalize_input(&self, text: &str) -> String {
        match self.line_mode {
            EditableTextLineMode::SingleLine => normalize_single_line(text),
            EditableTextLineMode::MultiLine => normalize_multiline(text),
        }
    }

    fn ensure_caret_visible(&mut self, bounds: Bounds<Pixels>) {
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            self.scroll_offset = point(px(0.), px(0.));
            return;
        }
        let Some(caret) = self.layout_map.position_for_index(self.selection.head()) else {
            self.scroll_offset = point(px(0.), px(0.));
            return;
        };
        let horizontal_scroll = self.line_mode == EditableTextLineMode::SingleLine
            || (self.line_mode == EditableTextLineMode::MultiLine && !self.soft_wrap);
        if horizontal_scroll {
            let visible_x = caret.x + self.scroll_offset.x;
            if visible_x < px(0.) {
                self.scroll_offset.x = -caret.x;
            } else if visible_x + px(2.) > bounds.size.width {
                self.scroll_offset.x = bounds.size.width - caret.x - px(2.);
            }
        } else {
            self.scroll_offset.x = px(0.);
        }
        if self.line_mode == EditableTextLineMode::MultiLine {
            let visible_y = caret.y + self.scroll_offset.y;
            let line_height = self.layout_map.line_height();
            if visible_y < px(0.) {
                self.scroll_offset.y = -caret.y;
            } else if visible_y + line_height > bounds.size.height {
                self.scroll_offset.y = bounds.size.height - caret.y - line_height;
            }
        } else {
            self.scroll_offset.y = px(0.);
        }
        self.clamp_scroll(bounds);
    }

    fn auto_scroll_for_pointer(&mut self, position: Point<Pixels>) {
        let Some(geometry) = self.geometry.as_ref() else {
            return;
        };
        let bounds = geometry.bounds();
        let line_height = self.layout_map.line_height().max(px(1.));
        if position.y < bounds.top() {
            self.scroll_offset.y += line_height;
        } else if position.y > bounds.bottom() {
            self.scroll_offset.y -= line_height;
        }
        if !self.soft_wrap && position.x < bounds.left() {
            self.scroll_offset.x += line_height;
        } else if !self.soft_wrap && position.x > bounds.right() {
            self.scroll_offset.x -= line_height;
        }
        self.clamp_scroll(bounds);
        self.geometry = Some(EditableTextGeometry::new(
            self.revision(),
            bounds,
            self.scroll_offset,
        ));
    }

    fn pointer_is_outside_viewport(&self, position: Point<Pixels>) -> bool {
        self.geometry.as_ref().is_some_and(|geometry| {
            let bounds = geometry.bounds();
            position.x < bounds.left()
                || position.x > bounds.right()
                || position.y < bounds.top()
                || position.y > bounds.bottom()
        })
    }

    fn start_drag_auto_scroll(&mut self, cx: &mut Context<Self>) {
        if self.drag_scroll_active {
            return;
        }
        self.drag_scroll_active = true;
        self.drag_scroll_epoch = self.drag_scroll_epoch.wrapping_add(1);
        let epoch = self.drag_scroll_epoch;
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor().timer(DRAG_SCROLL_INTERVAL).await;
                let keep_running = entity
                    .update(cx, |state, cx| {
                        if !state.is_selecting
                            || !state.drag_scroll_active
                            || state.drag_scroll_epoch != epoch
                        {
                            return false;
                        }
                        let Some(position) = state.drag_position else {
                            state.drag_scroll_active = false;
                            return false;
                        };
                        if !state.pointer_is_outside_viewport(position) {
                            state.drag_scroll_active = false;
                            return false;
                        }
                        state.auto_scroll_for_pointer(position);
                        let offset = state.index_for_point(position);
                        state.selection = state.selection.with_head(offset);
                        state.marked_range = None;
                        state.preferred_vertical_x = None;
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

    fn stop_drag_auto_scroll(&mut self) {
        self.drag_scroll_active = false;
        self.drag_scroll_epoch = self.drag_scroll_epoch.wrapping_add(1);
    }

    fn clamp_scroll(&mut self, bounds: Bounds<Pixels>) {
        let min_x = (bounds.size.width - self.layout_map.size().width).min(px(0.));
        let min_y = (bounds.size.height - self.layout_map.size().height).min(px(0.));
        self.scroll_offset.x = self.scroll_offset.x.clamp(min_x, px(0.));
        self.scroll_offset.y = self.scroll_offset.y.clamp(min_y, px(0.));
    }

    fn index_for_point(&self, position: Point<Pixels>) -> usize {
        self.geometry
            .as_ref()
            .filter(|geometry| geometry.revision() == self.revision())
            .map(|geometry| {
                geometry
                    .index_for_point(&self.layout_map, position)
                    .min(self.buffer.len())
            })
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
        let replacement = self.normalize_input(text);
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
        self.invalidate_style_runs();
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
        self.invalidate_style_runs();
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

    fn invalidate_style_runs(&mut self) {
        self.style_runs = EditableTextStyleRuns::empty(self.revision());
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
        self.drag_position = None;
        self.stop_drag_auto_scroll();
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
        if !self.text_export_enabled {
            return None;
        }
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
        if !self.text_export_enabled {
            return None;
        }
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
        Some(geometry.bounds_for_range(&self.layout_map, range))
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
