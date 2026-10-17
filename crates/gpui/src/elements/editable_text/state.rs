use std::{ops::Range, sync::Arc, time::Duration};

use crate::{
    Bounds, ClipboardItem, Context, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    Instant, Pixels, Point, ScrollBehavior, Size, Subscription, TextAlign, UTF16Selection, Window,
    WindowTextSystem, point, px,
};

use super::super::scroll_activity::ScrollActivity;

use super::{
    AttributeSpan, AttributeState, AttributedText, EditableSelection, EditableTextEvent,
    EditableTextFormatEvent, EditableTextGeometry, EditableTextLineMode, EditableTextMode,
    EditableTextStyleRuns, Formatting, ParagraphFormat, TextAttributes, TextAttributesPatch,
    TextBuffer, TextBufferError, TextBufferSnapshot, TextEdit, TextLayoutEdit, TextLayoutError,
    TextLayoutMap, TextLayoutStyle, TextLayoutUpdate, editable_text_alignment_offset,
    history::{EditHistory, EditKind, HistoryState},
    movement::{
        grapheme_boundary_at_or_before, logical_line_range_at, next_grapheme_boundary,
        next_logical_line_end, next_word_boundary, normalize_multiline, normalize_single_line,
        previous_grapheme_boundary, previous_logical_line_start, previous_word_boundary,
        word_range_at,
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
    pending_layout_edit: Option<TextLayoutEdit>,
    scroll_offset: Point<Pixels>,
    scroll_activity: ScrollActivity,
    pending_programmatic_generation: Option<u64>,
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
    /// Formatting stored with the text; `None` until a field is formatted, so plain fields
    /// allocate nothing.
    formatting: Option<Arc<Formatting>>,
    format_revision: u64,
    /// Attributes the next insertion takes, until the caret moves or focus changes.
    typing_attributes: Option<Arc<TextAttributes>>,
    /// Attributes an IME composition keeps for its whole life.
    composition_attributes: Option<Arc<TextAttributes>>,
    /// The height a single-line field last asked layout for.
    pub(crate) requested_height: Option<Pixels>,
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
            pending_layout_edit: None,
            scroll_offset: point(px(0.), px(0.)),
            scroll_activity: ScrollActivity::default(),
            pending_programmatic_generation: None,
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
            formatting: None,
            format_revision: 0,
            typing_attributes: None,
            composition_attributes: None,
            requested_height: None,
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
        self.cancel_programmatic_scroll();
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

    /// Counts formatting-only changes; edits to the text bump [`Self::revision`] instead.
    pub fn format_revision(&self) -> u64 {
        self.format_revision
    }

    /// The stored character formatting, as sorted, non-overlapping spans.
    pub fn attribute_spans(&self) -> &[AttributeSpan] {
        self.formatting
            .as_ref()
            .map_or(&[], |formatting| formatting.spans.spans())
    }

    /// The stored attributes of the character at `offset` (none stored reads as default).
    pub fn attributes_at(&self, offset: usize) -> TextAttributes {
        self.stored_attributes(offset)
            .map(|attributes| (*attributes).clone())
            .unwrap_or_default()
    }

    /// Whether the characters of `range` all have a field on (`is_on`), none have it, or
    /// some do. An empty range answers for the attributes the next insertion there takes.
    pub fn attribute_state(
        &self,
        range: Range<usize>,
        is_on: impl Fn(&TextAttributes) -> bool,
    ) -> AttributeState {
        if range.is_empty() {
            let attributes = self.insertion_attributes(range).unwrap_or_default();
            return if is_on(&attributes) {
                AttributeState::On
            } else {
                AttributeState::Off
            };
        }
        let range = range.start.min(self.buffer.len())..range.end.min(self.buffer.len());
        let off = TextAttributes::default();
        let mut on_len = 0;
        let mut covered = 0;
        if let Some(formatting) = &self.formatting {
            for span in formatting.spans.clipped(range.clone()) {
                covered += span.range.len();
                if is_on(&span.attributes) {
                    on_len += span.range.len();
                }
            }
        }
        if is_on(&off) {
            on_len += range.len() - covered;
        }
        if on_len == range.len() {
            AttributeState::On
        } else if on_len == 0 {
            AttributeState::Off
        } else {
            AttributeState::Mixed
        }
    }

    /// Sets or clears attribute fields over `range` as one undo step. An empty range sets the
    /// typing attributes the next insertion there takes instead.
    pub fn set_attributes(
        &mut self,
        range: Range<usize>,
        patch: &TextAttributesPatch,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.mode.permits_mutation() || patch.is_empty() {
            return false;
        }
        if range.is_empty() {
            let mut attributes = self
                .insertion_attributes(range)
                .map(|attributes| (*attributes).clone())
                .unwrap_or_default();
            patch.apply(&mut attributes);
            self.typing_attributes = Some(Arc::new(attributes));
            cx.notify();
            return true;
        }
        let snapshot = self.snapshot();
        if snapshot.validate_range(&range).is_err() {
            return false;
        }
        let before = self.history_state();
        let mut formatting = self.formatting.as_deref().cloned().unwrap_or_default();
        formatting.spans.patch(range, patch);
        if self.formatting.as_deref().unwrap_or(&Formatting::default()) == &formatting {
            return false;
        }
        self.formatting = (!formatting.is_empty()).then(|| Arc::new(formatting));
        let after = self.history_state();
        self.history.record(before, after, EditKind::Format);
        self.finish_format_change(cx);
        true
    }

    /// The attributes the next insertion takes, when set explicitly.
    pub fn typing_attributes(&self) -> Option<&TextAttributes> {
        self.typing_attributes.as_deref()
    }

    /// Sets the attributes the next insertion takes; they are dropped when the caret moves or
    /// focus changes.
    pub fn set_typing_attributes(&mut self, attributes: TextAttributes, cx: &mut Context<Self>) {
        self.typing_attributes = Some(Arc::new(attributes));
        cx.notify();
    }

    /// The format of the paragraph containing `offset`.
    pub fn paragraph_format(&self, offset: usize) -> ParagraphFormat {
        let Some(formatting) = &self.formatting else {
            return ParagraphFormat::default();
        };
        let snapshot = self.snapshot();
        let paragraph = snapshot
            .byte_to_point(offset.min(snapshot.len()))
            .map_or(0, |point| point.y);
        formatting
            .paragraph(paragraph)
            .map(|format| (**format).clone())
            .unwrap_or_default()
    }

    /// Sets the format of every paragraph `range` touches, as one undo step.
    pub fn set_paragraph_format(
        &mut self,
        range: Range<usize>,
        format: ParagraphFormat,
        cx: &mut Context<Self>,
    ) -> bool {
        self.update_paragraph_format(range, move |paragraph| *paragraph = format.clone(), cx)
    }

    /// Resets the format of every paragraph `range` touches, as one undo step.
    pub fn clear_paragraph_format(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> bool {
        self.set_paragraph_format(range, ParagraphFormat::default(), cx)
    }

    /// Changes the format of every paragraph `range` touches with `update`, as one undo step.
    pub fn update_paragraph_format(
        &mut self,
        range: Range<usize>,
        update: impl Fn(&mut ParagraphFormat),
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        let snapshot = self.snapshot();
        let (Ok(first), Ok(mut last)) = (
            snapshot.byte_to_point(range.start.min(snapshot.len())),
            snapshot.byte_to_point(range.end.min(snapshot.len())),
        ) else {
            return false;
        };
        // A range ending where a paragraph starts does not reach into it.
        if range.end > range.start && last.x == 0 && last.y > first.y {
            last.y -= 1;
        }
        let before = self.history_state();
        let mut formatting = self.formatting.as_deref().cloned().unwrap_or_default();
        formatting.update_paragraphs(first.y..=last.y, snapshot.line_count(), &update);
        if self.formatting.as_deref().unwrap_or(&Formatting::default()) == &formatting {
            return false;
        }
        self.formatting = (!formatting.is_empty()).then(|| Arc::new(formatting));
        let after = self.history_state();
        self.history.record(before, after, EditKind::Format);
        self.finish_format_change(cx);
        true
    }

    /// The text and everything stored with it.
    pub fn attributed_text(&self) -> AttributedText {
        AttributedText {
            text: self.text(),
            spans: self.attribute_spans().to_vec(),
            paragraphs: self
                .formatting
                .as_ref()
                .map_or_else(Vec::new, |formatting| {
                    formatting
                        .paragraphs
                        .iter()
                        .map(|format| (**format).clone())
                        .collect()
                }),
        }
    }

    /// Replaces the text and everything stored with it, as one undo step.
    pub fn set_attributed_text(&mut self, text: AttributedText, cx: &mut Context<Self>) -> bool {
        if !self.mode.permits_mutation() {
            return false;
        }
        // Spans follow their characters through line-break normalization, and land on
        // character boundaries.
        let (replacement, offsets) = super::movement::normalize_with_offsets(
            &text.text,
            self.line_mode == EditableTextLineMode::SingleLine,
        );
        let map = |offset: usize| offsets[offset.min(text.text.len())];
        let spans = text.spans.into_iter().map(|span| AttributeSpan {
            range: map(span.range.start)..map(span.range.end),
            attributes: span.attributes,
        });
        let spans = super::AttributeSpans::from_spans(spans, replacement.len());
        // Paragraph formats follow their paragraphs; breaks a single-line field drops leave
        // the first paragraph's.
        let paragraph_count = replacement.matches('\n').count() + 1;
        let mut paragraphs: Vec<Arc<ParagraphFormat>> = text
            .paragraphs
            .into_iter()
            .take(paragraph_count)
            .map(Arc::new)
            .collect();
        if !paragraphs.is_empty() {
            paragraphs.resize(paragraph_count, Arc::new(ParagraphFormat::default()));
        }
        let before = self.history_state();
        let edit = TextEdit::new(self.revision(), 0..self.buffer.len(), replacement.clone());
        if !self.apply_buffer_edit(edit, None) {
            return false;
        }
        let mut formatting = Formatting { spans, paragraphs };
        formatting.update_paragraphs(0..=0, paragraph_count, &|_| {});
        self.formatting = (!formatting.is_empty()).then(|| Arc::new(formatting));
        self.invalidate_style_runs();
        self.selection = EditableSelection::caret(replacement.len());
        self.marked_range = None;
        self.typing_attributes = None;
        self.composition_attributes = None;
        self.geometry = None;
        let after = self.history_state();
        self.history.record(before, after, EditKind::Other);
        self.restart_blink(cx);
        cx.emit(EditableTextEvent::Changed {
            revision: self.revision(),
            composition_active: false,
        });
        cx.notify();
        true
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
        text_align: TextAlign,
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
        self.layout_map.set_formatting(self.formatting.clone());
        self.layout_map
            .set_alignment(text_align, Some(bounds.size.width));
        let update_result = match (&projection, self.pending_layout_edit.as_ref()) {
            (None, Some(edit)) => self.layout_map.update_after_layout_edit_with_shaper(
                &snapshot,
                edit,
                &self.style_runs,
                &style,
                text_system,
            ),
            _ => self.layout_map.update_with_projection(
                &snapshot,
                projection.as_ref(),
                &self.style_runs,
                &style,
                text_system,
            ),
        };
        let update = match update_result {
            Ok(update) => update,
            Err(error) => {
                self.geometry = None;
                return Err(error);
            }
        };
        self.pending_layout_edit = None;
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
            editable_text_alignment_offset(
                text_align,
                bounds.size.width,
                self.layout_map.size().width,
            ),
        ));
        Ok(update)
    }

    pub fn scroll_by(&mut self, delta: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        self.cancel_programmatic_scroll();
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
            self.geometry
                .as_ref()
                .map_or(px(0.), EditableTextGeometry::alignment_offset),
        ));
        cx.notify();
        true
    }

    /// Sets the internal nonpositive scroll offset and clamps it to current geometry.
    pub fn set_scroll_offset(&mut self, offset: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        self.cancel_programmatic_scroll();
        self.set_scroll_offset_from_owner(offset, cx)
    }

    /// Requests a physical nonpositive offset through this editable owner's shared scroll driver.
    pub fn request_scroll_offset_with_behavior(
        &mut self,
        offset: Point<Pixels>,
        behavior: ScrollBehavior,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.viewport_bounds().is_none() {
            return false;
        }
        self.scroll_activity.stop();
        self.pending_programmatic_generation = None;
        let minimum = self.max_scroll_offset();
        let mut target = point(
            offset.x.clamp(minimum.x, px(0.)),
            offset.y.clamp(minimum.y, px(0.)),
        );
        if self.line_mode == EditableTextLineMode::SingleLine {
            target.y = px(0.);
        } else if self.soft_wrap {
            target.x = px(0.);
        }
        let maximum = point(-minimum.x, -minimum.y);
        let current = point(-self.scroll_offset.x, -self.scroll_offset.y);
        let target_logical = point(-target.x, -target.y);
        let Some(frame) = self.scroll_activity.start_programmatic_at(
            current,
            target_logical,
            maximum,
            behavior,
            cx.background_executor().now(),
        ) else {
            return false;
        };
        if frame.request_next_frame {
            self.pending_programmatic_generation = Some(frame.generation);
            cx.notify();
            true
        } else {
            self.set_scroll_offset_from_owner(point(-frame.offset.x, -frame.offset.y), cx)
        }
    }

    pub(crate) fn take_pending_programmatic_generation(&mut self) -> Option<u64> {
        self.pending_programmatic_generation.take()
    }

    /// Returns the queued driver generation for deterministic owner tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pending_programmatic_generation_for_test(&self) -> Option<u64> {
        self.pending_programmatic_generation
    }

    /// Advances the shared driver at an explicit timestamp for deterministic owner tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn advance_programmatic_scroll_for_test(
        &mut self,
        generation: u64,
        timestamp: Instant,
        cx: &mut Context<Self>,
    ) -> bool {
        self.advance_programmatic_scroll_at(generation, timestamp, cx)
            .is_some()
    }

    pub(crate) fn advance_programmatic_scroll_at(
        &mut self,
        generation: u64,
        timestamp: Instant,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let minimum = self.max_scroll_offset();
        let maximum = point(-minimum.x, -minimum.y);
        let frame = self
            .scroll_activity
            .next_programmatic_at(generation, timestamp, maximum)?;
        self.set_scroll_offset_from_owner(point(-frame.offset.x, -frame.offset.y), cx);
        Some(frame.request_next_frame)
    }

    fn set_scroll_offset_from_owner(
        &mut self,
        offset: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
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
            self.geometry
                .as_ref()
                .map_or(px(0.), EditableTextGeometry::alignment_offset),
        ));
        cx.notify();
        true
    }

    fn cancel_programmatic_scroll(&mut self) {
        self.scroll_activity.stop();
        self.pending_programmatic_generation = None;
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
        self.typing_attributes = None;
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
        self.typing_attributes = None;
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
        self.move_page(-1., extend, cx);
    }

    pub fn move_page_down(&mut self, extend: bool, cx: &mut Context<Self>) {
        self.move_page(1., extend, cx);
    }

    pub fn insert_newline(&mut self, cx: &mut Context<Self>) -> bool {
        self.line_mode == EditableTextLineMode::MultiLine
            && self.replace_byte_range(self.selection.range(), "\n", EditKind::Other, cx)
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
        // Only the part the rewrite changed is replaced, so formatting outside it stays put.
        let changed = changed_range(&current.text, &replacement);
        let inserted = self.insertion_attributes(changed.old.clone());
        let edit = TextEdit::new(self.revision(), changed.old, &replacement[changed.new]);
        if !self.apply_buffer_edit(edit, inserted) {
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
            self.formatting = current.formatting;
            self.invalidate_style_runs();
            self.selection = current.selection;
            self.marked_range = current.marked_range;
            return false;
        }
        self.restart_blink(cx);
        cx.emit(EditableTextEvent::Changed {
            revision: self.revision(),
            composition_active: false,
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
        self.composition_attributes = None;
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
            self.typing_attributes = None;
            self.composition_attributes = None;
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
            window.blur(cx);
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
        click_count: usize,
        cx: &mut Context<Self>,
    ) {
        if self.mode == EditableTextMode::Disabled {
            return;
        }
        self.is_selecting = true;
        self.drag_position = Some(position);
        let offset = self.index_for_point(position);
        match click_count {
            2 => {
                let range = word_range_at(&self.text(), offset);
                self.set_selection(range.start, range.end, cx);
            }
            count if count >= 3 => {
                let range = logical_line_range_at(&self.text(), offset);
                self.set_selection(range.start, range.end, cx);
            }
            _ => self.move_to(offset, extend, cx),
        }
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
        self.cancel_pointer_selection();
    }

    /// Cancels the transient pointer drag without changing the current selection.
    pub fn cancel_pointer_selection(&mut self) {
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
        self.typing_attributes = None;
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
        let Some(target) = self.layout_map.visual_row(target_row) else {
            return;
        };
        let target_position = point(preferred_x, target.top + target.height / 2.0);
        let Some(offset) = self.layout_map.index_for_position(target_position) else {
            return;
        };
        self.move_to(offset.min(self.buffer.len()), extend, cx);
        self.preferred_vertical_x = Some(preferred_x);
    }

    /// Moves by the viewport's height (in whole rows of the field's line height), keeping the
    /// preferred x, whatever the heights of the rows in between.
    fn move_page(&mut self, direction: f32, extend: bool, cx: &mut Context<Self>) {
        if self.mode == EditableTextMode::Disabled
            || self.line_mode != EditableTextLineMode::MultiLine
        {
            return;
        }
        let line_height = self.layout_map.line_height();
        if line_height <= px(0.) {
            return;
        }
        let page = self.geometry.as_ref().map_or(line_height, |geometry| {
            line_height
                * (f32::from(geometry.bounds().size.height) / f32::from(line_height))
                    .floor()
                    .max(1.)
        });
        let Some(display) = self.layout_map.buffer_to_display(self.selection.head()) else {
            return;
        };
        let (Some(position), Some(row)) = (
            self.layout_map.position_for_index(self.selection.head()),
            self.layout_map.visual_row(display.row),
        ) else {
            return;
        };
        let preferred_x = self.preferred_vertical_x.unwrap_or(position.x);
        let height = self.layout_map.size().height;
        let target_y = (row.top + row.height / 2. + page * direction)
            .clamp(px(0.), (height - px(0.5)).max(px(0.)));
        let Some(offset) = self
            .layout_map
            .index_for_position(point(preferred_x, target_y))
        else {
            return;
        };
        self.move_to(offset.min(self.buffer.len()), extend, cx);
        self.preferred_vertical_x = Some(preferred_x);
    }

    fn normalize_input(&self, text: &str) -> String {
        match self.line_mode {
            EditableTextLineMode::SingleLine => normalize_single_line(text),
            EditableTextLineMode::MultiLine => normalize_multiline(text),
        }
    }

    fn ensure_caret_visible(&mut self, bounds: Bounds<Pixels>) {
        self.cancel_programmatic_scroll();
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
            let line_height = self
                .layout_map
                .buffer_to_display(self.selection.head())
                .and_then(|display| self.layout_map.visual_row(display.row))
                .map_or(self.layout_map.line_height(), |row| row.height);
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
        self.cancel_programmatic_scroll();
        let Some(geometry) = self.geometry.as_ref() else {
            return;
        };
        let bounds = geometry.bounds();
        let line_height = self.layout_map.line_height().max(px(1.));
        // Scroll by the height of the row coming into view at the edge.
        let edge_row_height = |y: Pixels| {
            let row = self.layout_map.visual_row_at_y(y.max(px(0.)));
            self.layout_map
                .visual_row(row.min(self.layout_map.visual_line_count().saturating_sub(1)))
                .map_or(line_height, |row| row.height.max(px(1.)))
        };
        if position.y < bounds.top() {
            self.scroll_offset.y += edge_row_height(-self.scroll_offset.y - px(1.));
        } else if position.y > bounds.bottom() {
            self.scroll_offset.y -= edge_row_height(bounds.size.height - self.scroll_offset.y);
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
            self.geometry
                .as_ref()
                .map_or(px(0.), EditableTextGeometry::alignment_offset),
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
        let composing = self.marked_range.is_some();
        let inserted = if replacement.is_empty() {
            None
        } else if composing {
            // A composition keeps the attributes it started with, through its commit.
            self.composition_attributes.clone()
        } else {
            self.insertion_attributes(range.clone())
        };
        if kind == EditKind::Composition && !composing {
            self.composition_attributes = inserted.clone();
        }
        let deleted = (replacement.is_empty() && !range.is_empty())
            .then(|| self.stored_attributes(range.start))
            .flatten();
        let edit = TextEdit::new(self.revision(), range, replacement);
        if !self.apply_buffer_edit(edit, inserted) {
            return false;
        }
        self.invalidate_style_runs();
        self.selection = selection;
        self.marked_range = marked_range;
        if self.marked_range.is_none() {
            self.composition_attributes = None;
        }
        self.typing_attributes = None;
        // Deleting all of a paragraph leaves its last attributes for the next insertion.
        if let Some(deleted) = deleted
            && self.selection.is_empty()
            && self.paragraph_is_empty_at(self.selection.head())
        {
            self.typing_attributes = Some(deleted);
        }
        self.geometry = None;
        let after = self.history_state();
        self.history.record(before, after, kind);
        self.restart_blink(cx);
        cx.emit(EditableTextEvent::Changed {
            revision: self.revision(),
            composition_active: self.marked_range.is_some(),
        });
        cx.notify();
        true
    }

    fn restore_history_state(&mut self, state: HistoryState, cx: &mut Context<Self>) -> bool {
        if state.text == self.text() {
            // A formatting-only step: the text and its revision stay.
            self.formatting = state.formatting;
            self.selection = state.selection;
            self.marked_range = state.marked_range;
            self.typing_attributes = None;
            self.composition_attributes = None;
            self.finish_format_change(cx);
            return true;
        }
        let edit = TextEdit::new(self.revision(), 0..self.buffer.len(), state.text);
        if !self.apply_buffer_edit(edit, None) {
            return false;
        }
        self.formatting = state.formatting;
        self.typing_attributes = None;
        self.composition_attributes = None;
        self.invalidate_style_runs();
        self.selection = state.selection;
        self.marked_range = state.marked_range;
        self.geometry = None;
        self.restart_blink(cx);
        cx.emit(EditableTextEvent::Changed {
            revision: self.revision(),
            composition_active: self.marked_range.is_some(),
        });
        cx.notify();
        true
    }

    fn invalidate_style_runs(&mut self) {
        self.style_runs = EditableTextStyleRuns::empty(self.revision());
    }

    /// The single path every buffer mutation takes: stored formatting moves with the edit, and
    /// the inserted text takes `inserted`.
    fn apply_buffer_edit(&mut self, edit: TextEdit, inserted: Option<Arc<TextAttributes>>) -> bool {
        let pending = TextLayoutEdit::from(&edit);
        let range = edit.range().clone();
        let replacement_len = edit.replacement().len();
        // Where the edit starts among the paragraphs and how many breaks it removes and adds,
        // for paragraph formats.
        let paragraph_edit = self
            .formatting
            .as_ref()
            .filter(|formatting| !formatting.paragraphs.is_empty())
            .and_then(|_| {
                let snapshot = self.snapshot();
                let paragraph = snapshot.byte_to_point(range.start).ok()?.y;
                let deleted = snapshot.slice(range.clone()).ok()?.matches('\n').count();
                Some((paragraph, deleted, edit.replacement().matches('\n').count()))
            });
        if self.buffer.apply(edit).is_err() {
            return false;
        }
        self.pending_layout_edit = Some(pending);
        let inserted = inserted.filter(|attributes| !attributes.is_empty());
        if self.formatting.is_none() && inserted.is_none() {
            return true;
        }
        let formatting = Arc::make_mut(self.formatting.get_or_insert_with(Default::default));
        if let Some((paragraph, deleted, added)) = paragraph_edit {
            formatting.remap_paragraphs(paragraph, deleted, added);
        }
        formatting.spans.remap(
            range,
            replacement_len,
            &inserted.unwrap_or_else(|| Arc::new(TextAttributes::default())),
        );
        if formatting.is_empty() {
            self.formatting = None;
        }
        true
    }

    fn history_state(&self) -> HistoryState {
        HistoryState {
            text: self.text(),
            selection: self.selection,
            marked_range: self.marked_range.clone(),
            formatting: self.formatting.clone(),
        }
    }

    /// The stored attributes of the character at `offset`.
    fn stored_attributes(&self, offset: usize) -> Option<Arc<TextAttributes>> {
        self.formatting.as_ref()?.spans.at(offset).cloned()
    }

    fn paragraph_is_empty_at(&self, offset: usize) -> bool {
        self.buffer
            .char_before(offset)
            .is_none_or(|(_, ch)| ch == '\n')
            && self.buffer.char_at(offset).is_none_or(|ch| ch == '\n')
    }

    /// The attributes text inserted over `range` takes: explicit typing attributes; else those
    /// of the first replaced character; else of the character before (within its paragraph);
    /// at a paragraph start those of the character after; in an empty paragraph those of the
    /// break before it. Links and tags never extend past their end.
    fn insertion_attributes(&self, range: Range<usize>) -> Option<Arc<TextAttributes>> {
        if let Some(typing) = &self.typing_attributes {
            return Some(typing.clone());
        }
        let formatting = self.formatting.as_ref()?;
        let spans = &formatting.spans;
        if !range.is_empty() {
            return spans.at(range.start).cloned();
        }
        let offset = range.start;
        let exclusive = |attributes: &Arc<TextAttributes>, next: Option<&Arc<TextAttributes>>| {
            if attributes.link.is_none() && attributes.tag.is_none() {
                return attributes.clone();
            }
            let mut kept = (**attributes).clone();
            if next.is_none_or(|next| next.link != kept.link) {
                kept.link = None;
            }
            if next.is_none_or(|next| next.tag != kept.tag) {
                kept.tag = None;
            }
            Arc::new(kept)
        };
        match self.buffer.char_before(offset) {
            Some((start, ch)) if ch != '\n' => spans
                .at(start)
                .map(|before| exclusive(before, spans.at(offset))),
            before => match self.buffer.char_at(offset) {
                Some(ch) if ch != '\n' => spans
                    .at(offset)
                    .map(|after| Arc::new(after.without_exclusive())),
                _ => before
                    .and_then(|(start, _)| spans.at(start))
                    .map(|newline| Arc::new(newline.without_exclusive())),
            },
        }
    }

    fn finish_format_change(&mut self, cx: &mut Context<Self>) {
        self.format_revision = self.format_revision.wrapping_add(1);
        self.geometry = None;
        cx.emit(EditableTextFormatEvent::Changed {
            format_revision: self.format_revision,
        });
        cx.notify();
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
            window.blur(cx);
            return;
        }
        self.is_focused = true;
        self.restart_blink(cx);
        cx.notify();
    }

    fn did_blur(&mut self, cx: &mut Context<Self>) {
        self.is_focused = false;
        self.typing_attributes = None;
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

impl EventEmitter<EditableTextFormatEvent> for EditableTextState {}

/// The byte ranges in which `old` and `new` differ, trimmed of their common prefix and suffix
/// at character boundaries.
struct ChangedRange {
    old: Range<usize>,
    new: Range<usize>,
}

fn changed_range(old: &str, new: &str) -> ChangedRange {
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .find(|((_, a), b)| a != b)
        .map_or(old.len().min(new.len()), |((index, _), _)| index);
    let prefix = (0..=prefix)
        .rev()
        .find(|index| old.is_char_boundary(*index) && new.is_char_boundary(*index))
        .unwrap_or(0);
    let max_suffix = (old.len() - prefix).min(new.len() - prefix);
    let mut suffix = 0;
    for (a, b) in old.chars().rev().zip(new.chars().rev()) {
        if a != b || suffix + a.len_utf8() > max_suffix {
            break;
        }
        suffix += a.len_utf8();
    }
    ChangedRange {
        old: prefix..old.len() - suffix,
        new: prefix..new.len() - suffix,
    }
}

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
