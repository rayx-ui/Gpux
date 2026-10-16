use crate::{
    App, Bounds, Color, ContentMask, CursorStyle, DecorationRun, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, GlobalElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, RenderOnce, ScrollWheelEvent,
    ShapedLine, SharedString, Style, TextAlign, TextRun, UnderlineStyle, WeakEntity, Window,
    WrappedLine, div, fill, point, prelude::*, px, relative, size,
};
use std::ops::Range;

use super::{
    EditableTextDisplayTransform, EditableTextLineMode, EditableTextMode, EditableTextState,
    EditableTextStyleRun, TextDisplayPoint, TextLayoutStyle,
};

/// A reusable editable-text element backed by [`EditableTextState`].
#[derive(IntoElement)]
pub struct EditableText {
    state: Entity<EditableTextState>,
    placeholder_color: Option<Color>,
    selection_color: Option<Color>,
    caret_color: Option<Color>,
    display_transform: EditableTextDisplayTransform,
    aria_label: Option<SharedString>,
}

#[allow(missing_docs)]
impl EditableText {
    pub fn new(state: &Entity<EditableTextState>) -> Self {
        Self {
            state: state.clone(),
            placeholder_color: None,
            selection_color: None,
            caret_color: None,
            display_transform: EditableTextDisplayTransform::Identity,
            aria_label: None,
        }
    }

    /// The accessible name of the text field, which screen readers announce and queries match
    /// by `name`.
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn placeholder_color(mut self, color: Color) -> Self {
        self.placeholder_color = Some(color);
        self
    }

    pub fn selection_color(mut self, color: Color) -> Self {
        self.selection_color = Some(color);
        self
    }

    pub fn caret_color(mut self, color: Color) -> Self {
        self.caret_color = Some(color);
        self
    }

    pub fn display_transform(mut self, transform: EditableTextDisplayTransform) -> Self {
        self.display_transform = transform;
        self
    }
}

impl RenderOnce for EditableText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state
            .update(cx, |state, cx| state.ensure_focus_tracking(window, cx));
        let (focus_handle, mode, line_mode) = {
            let state = self.state.read(cx);
            (state.focus_handle(), state.mode(), state.line_mode())
        };
        // The text field's accessibility node: its text is copied only while the tree is built.
        let a11y = window.is_a11y_active().then(|| {
            let state = self.state.read(cx);
            (state.text(), state.placeholder().to_string())
        });
        let entity_id = self.state.entity_id();

        let key_state = self.state.clone();
        let down_state = self.state.clone();
        let down_focus_handle = focus_handle.clone();
        let up_state = self.state.clone();
        let scroll_state = self.state.clone();

        div()
            .id(("editable-text", entity_id.as_u64() as usize))
            .role(match line_mode {
                EditableTextLineMode::SingleLine => accesskit::Role::TextInput,
                EditableTextLineMode::MultiLine => accesskit::Role::MultilineTextInput,
            })
            .when_some(self.aria_label, |element, label| element.aria_label(label))
            .when_some(a11y, |element, (text, placeholder)| {
                element
                    .aria_value(text)
                    .when(!placeholder.is_empty(), |element| {
                        element.aria_placeholder(placeholder)
                    })
            })
            .aria_read_only(mode == super::EditableTextMode::ReadOnly)
            .aria_disabled(mode == super::EditableTextMode::Disabled)
            .flex()
            .w_full()
            .overflow_hidden()
            .when(line_mode == EditableTextLineMode::MultiLine, |element| {
                element.h_full()
            })
            .key_context("EditableText")
            .cursor(CursorStyle::IBeam)
            .on_key_down(move |event, window, cx| {
                if handle_key_down(&key_state, event, window, cx) {
                    cx.stop_propagation();
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    if mode.permits_focus() {
                        window.focus(&down_focus_handle, cx);
                    }
                    down_state.update(cx, |state, cx| {
                        state.pointer_down(
                            event.position,
                            event.modifiers.shift,
                            event.click_count,
                            cx,
                        )
                    });
                },
            )
            .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _window, cx| {
                up_state.update(cx, |state, _| state.pointer_up());
            })
            .on_scroll_wheel(move |event: &ScrollWheelEvent, window, cx| {
                let delta = event.delta.pixel_delta(window.line_height());
                let changed = scroll_state.update(cx, |state, cx| state.scroll_by(delta, cx));
                if changed {
                    cx.stop_propagation();
                }
            })
            .when(mode.permits_focus(), |element| {
                element.track_focus(&focus_handle)
            })
            .child(EditableTextPaintElement {
                state: self.state,
                placeholder_color: self.placeholder_color,
                selection_color: self.selection_color,
                caret_color: self.caret_color,
                display_transform: self.display_transform,
            })
    }
}

fn handle_key_down(
    state: &Entity<EditableTextState>,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let key = event.keystroke.key.as_str();
    let modifiers = event.keystroke.modifiers;
    let secondary = modifiers.secondary();
    state.update(cx, |state, cx| match (secondary, modifiers.shift, key) {
        (false, false, "backspace") => state.delete_backward(cx),
        (false, false, "delete") => state.delete_forward(cx),
        (true, false, "backspace") => state.delete_word_backward(cx),
        (true, false, "delete") => state.delete_word_forward(cx),
        (false, shift, "left") => {
            state.move_left(shift, cx);
            true
        }
        (false, shift, "right") => {
            state.move_right(shift, cx);
            true
        }
        (true, shift, "left") => {
            state.move_word_left(shift, cx);
            true
        }
        (true, shift, "right") => {
            state.move_word_right(shift, cx);
            true
        }
        (false, shift, "up") => {
            state.move_up(shift, cx);
            true
        }
        (false, shift, "down") => {
            state.move_down(shift, cx);
            true
        }
        (true, true, "up") => {
            state.extend_to_previous_logical_line_start(cx);
            true
        }
        (true, true, "down") => {
            state.extend_to_next_logical_line_end(cx);
            true
        }
        (false, shift, "pageup") => {
            state.move_page_up(shift, cx);
            true
        }
        (false, shift, "pagedown") => {
            state.move_page_down(shift, cx);
            true
        }
        (false, _, "enter") => {
            state.insert_newline(cx);
            true
        }
        (false, false, "home") => {
            state.move_to_visual_line_start(false, cx);
            true
        }
        (false, true, "home") => {
            state.move_to_visual_line_start(true, cx);
            true
        }
        (true, false, "home") => {
            state.move_to_document_start(false, cx);
            true
        }
        (true, true, "home") => {
            state.move_to_document_start(true, cx);
            true
        }
        (false, false, "end") => {
            state.move_to_visual_line_end(false, cx);
            true
        }
        (false, true, "end") => {
            state.move_to_visual_line_end(true, cx);
            true
        }
        (true, false, "end") => {
            state.move_to_document_end(false, cx);
            true
        }
        (true, true, "end") => {
            state.move_to_document_end(true, cx);
            true
        }
        (true, false, "a") => {
            state.select_all(cx);
            true
        }
        (true, false, "c") => state.copy(cx),
        (true, false, "x") => state.cut(cx),
        (true, false, "v") => state.paste(cx),
        (true, false, "z") => state.undo(cx),
        (true, true, "z") => state.redo(cx),
        (true, false, "y") => state.redo(cx),
        (true, false, "space") if modifiers.control => {
            window.show_character_palette();
            true
        }
        _ => false,
    })
}

struct EditableTextPaintElement {
    state: Entity<EditableTextState>,
    placeholder_color: Option<Color>,
    selection_color: Option<Color>,
    caret_color: Option<Color>,
    display_transform: EditableTextDisplayTransform,
}

struct EditableTextPrepaintState {
    lines: Vec<EditableTextPaintLine>,
    placeholder: Option<ShapedLine>,
    text_align: TextAlign,
    scroll_offset: crate::Point<Pixels>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

struct EditableTextPaintLine {
    line: WrappedLine,
    visual_start: usize,
}

impl IntoElement for EditableTextPaintElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditableTextPaintElement {
    type RequestLayoutState = ();
    type PrepaintState = EditableTextPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&crate::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = match self.state.read(cx).line_mode() {
            EditableTextLineMode::SingleLine => window.line_height().into(),
            EditableTextLineMode::MultiLine => relative(1.).into(),
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&crate::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // RenderOnce text still needs view ownership so state notifications
        // invalidate the cached ancestors that rendered it.
        window.set_view_id(self.state.entity_id());
        let style = window.text_style();
        let placeholder_color = self
            .placeholder_color
            .clone()
            .unwrap_or_else(|| style.color.opacity(0.5));
        let selection_color = self
            .selection_color
            .clone()
            .unwrap_or_else(|| style.color.opacity(0.2));
        let caret_color = self
            .caret_color
            .clone()
            .unwrap_or_else(|| style.color.clone());
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let layout_style = TextLayoutStyle {
            font: style.font(),
            font_size,
            line_height,
            letter_spacing: style.letter_spacing.to_pixels(window.rem_size()),
            wrap_width: None,
            device_scale_factor: window.scale_factor(),
            tab_size: 4,
        };
        let layout_ready = self.state.update(cx, |state, _| {
            state
                .update_layout(
                    bounds,
                    layout_style,
                    style.text_align,
                    window.text_system(),
                    &self.display_transform,
                )
                .is_ok()
        });
        if let Some(generation) = self
            .state
            .update(cx, |state, _| state.take_pending_programmatic_generation())
        {
            // The scheduled callback redraws the field; a refresh while drawing would do
            // nothing.
            schedule_editable_programmatic_scroll(self.state.downgrade(), generation, window);
        }

        let state = self.state.read(cx);
        let snapshot = state.snapshot();
        let content = snapshot.text();
        let selection = *state.selection();
        let marked_range = state
            .marked_range()
            .map(|range| state.layout_map().buffer_range_to_display(range));
        let scroll_offset = state.scroll_offset();
        let alignment_offset = state
            .geometry()
            .map_or(px(0.), |geometry| geometry.alignment_offset());
        let content_origin = bounds.origin + scroll_offset + point(alignment_offset, px(0.));

        let mut lines = Vec::new();
        let paint_rows = if layout_ready && line_height > px(0.) {
            let first_visible_row =
                ((-f32::from(scroll_offset.y) / f32::from(line_height)).max(0.)) as usize;
            let visible_row_end = ((f32::from(bounds.size.height - scroll_offset.y)
                / f32::from(line_height))
            .ceil()) as usize;
            let visible_rows = first_visible_row.saturating_sub(1)
                ..visible_row_end
                    .saturating_add(1)
                    .min(state.layout_map().visual_line_count());
            state
                .layout_map()
                .logical_rows_for_visual_range(visible_rows)
        } else {
            0..0
        };
        for row_index in paint_rows {
            let Some((display_start, visual_start, text, layout)) =
                state.layout_map().paint_row(row_index)
            else {
                continue;
            };
            let decorations = decoration_runs(
                display_start,
                text.len(),
                state.layout_map().projected_style_runs(),
                marked_range.clone(),
                style.color.clone(),
            );
            lines.push(EditableTextPaintLine {
                line: WrappedLine {
                    layout,
                    text,
                    decoration_runs: decorations,
                },
                visual_start,
            });
        }

        let placeholder = (content.is_empty() && !state.placeholder().is_empty()).then(|| {
            let text: SharedString = state.placeholder().to_owned().into();
            let run = TextRun {
                len: text.len(),
                font: style.font(),
                color: placeholder_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(text, font_size, &[run], None)
        });

        let selection_quads = layout_ready
            .then(|| {
                editable_text_selection_quads(
                    state,
                    selection.range(),
                    content_origin,
                    bounds,
                    selection_color,
                )
            })
            .unwrap_or_default();
        let cursor = (layout_ready && state.is_focused() && state.caret_visible()).then(|| {
            let position = state
                .layout_map()
                .position_for_index(selection.head())
                .unwrap_or_default();
            fill(
                Bounds::new(
                    content_origin + position,
                    size(px(2.), state.layout_map().line_height()),
                ),
                caret_color,
            )
        });

        EditableTextPrepaintState {
            lines,
            placeholder,
            text_align: style.text_align,
            scroll_offset,
            cursor,
            selection: selection_quads,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&crate::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, mode) = {
            let state = self.state.read(cx);
            (state.focus_handle(), state.mode())
        };
        window.with_content_mask(Some(ContentMask::rectangular(bounds)), |window| {
            if mode != EditableTextMode::Disabled {
                window.handle_input(
                    &focus_handle,
                    ElementInputHandler::new(bounds, self.state.clone()),
                    cx,
                );
            }
            let line_height = window.line_height();
            for paint_line in &prepaint.lines {
                paint_line
                    .line
                    .paint_background(
                        bounds.origin
                            + prepaint.scroll_offset
                            + point(px(0.), line_height * paint_line.visual_start),
                        line_height,
                        prepaint.text_align,
                        Some(bounds),
                        window,
                        cx,
                    )
                    .expect("editable text line background should paint");
            }
            for selection in prepaint.selection.drain(..) {
                window.paint_quad(selection);
            }
            for paint_line in &prepaint.lines {
                paint_line
                    .line
                    .paint(
                        bounds.origin
                            + prepaint.scroll_offset
                            + point(px(0.), line_height * paint_line.visual_start),
                        line_height,
                        prepaint.text_align,
                        Some(bounds),
                        window,
                        cx,
                    )
                    .expect("editable text line should paint");
            }
            if let Some(placeholder) = &prepaint.placeholder {
                placeholder
                    .paint(
                        bounds.origin,
                        line_height,
                        prepaint.text_align,
                        Some(bounds.size.width),
                        window,
                        cx,
                    )
                    .expect("editable text placeholder should paint");
            }
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });

        let drag_state = self.state.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble {
                drag_state.update(cx, |state, cx| state.pointer_move(event.position, cx));
            }
        });
        let release_state = self.state.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble {
                release_state.update(cx, |state, _| state.pointer_up());
            }
        });
    }
}

fn schedule_editable_programmatic_scroll(
    state: WeakEntity<EditableTextState>,
    generation: u64,
    window: &Window,
) {
    window.on_next_frame(move |window, cx| {
        let Some(request_next_frame) = state
            .update(cx, |state, cx| {
                let now = cx.background_executor().now();
                state.advance_programmatic_scroll_at(generation, now, cx)
            })
            .ok()
            .flatten()
        else {
            return;
        };
        if request_next_frame {
            schedule_editable_programmatic_scroll(state.clone(), generation, window);
        }
        // The field's backing entity is its view id: a step redraws the field, not the
        // whole window.
        cx.notify(state.entity_id());
    });
}

fn decoration_runs(
    display_start: usize,
    len: usize,
    style_runs: &[EditableTextStyleRun],
    marked_range: Option<Range<usize>>,
    color: Color,
) -> Vec<DecorationRun> {
    let row_range = display_start..display_start + len;
    if row_range.is_empty() {
        return Vec::new();
    }

    let mut boundaries = vec![row_range.start, row_range.end];
    for run in style_runs {
        let start = run.range.start.max(row_range.start);
        let end = run.range.end.min(row_range.end);
        if start < end {
            boundaries.extend([start, end]);
        }
    }
    if let Some(marked) = &marked_range {
        let start = marked.start.max(row_range.start);
        let end = marked.end.min(row_range.end);
        if start < end {
            boundaries.extend([start, end]);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    boundaries
        .windows(2)
        .filter_map(|segment| {
            let start = segment[0];
            let end = segment[1];
            (start < end).then(|| {
                let mut run_color = color.clone();
                let mut background_color = None;
                let mut underline = None;
                let mut strikethrough = None;
                for run in style_runs
                    .iter()
                    .filter(|run| run.range.start <= start && run.range.end >= end)
                {
                    if let Some(highlight) = &run.style.color {
                        run_color = run_color.blend(highlight.clone());
                    }
                    if let Some(factor) = run.style.fade_out {
                        run_color.fade_out(factor);
                    }
                    if run.style.background_color.is_some() {
                        background_color = run.style.background_color.clone();
                    }
                    if run.style.underline.is_some() {
                        underline = run.style.underline.clone();
                    }
                    if run.style.strikethrough.is_some() {
                        strikethrough = run.style.strikethrough.clone();
                    }
                }
                if marked_range
                    .as_ref()
                    .is_some_and(|marked| marked.start <= start && marked.end >= end)
                {
                    underline = Some(UnderlineStyle {
                        color: Some(run_color.clone()),
                        thickness: px(1.),
                        wavy: false,
                    });
                }
                DecorationRun {
                    len: (end - start) as u32,
                    color: run_color,
                    background_color,
                    underline,
                    strikethrough,
                }
            })
        })
        .collect()
}

/// Returns the selection quads prepared by the editable-text paint path.
///
/// This is publicly hidden so integration tests can inspect exact render geometry
/// without exposing scene internals or relying on screenshots.
#[doc(hidden)]
pub fn editable_text_selection_quads(
    state: &EditableTextState,
    range: Range<usize>,
    origin: crate::Point<Pixels>,
    viewport: Bounds<Pixels>,
    color: Color,
) -> Vec<PaintQuad> {
    if range.is_empty() {
        return Vec::new();
    }
    let map = state.layout_map();
    let Some(start) = map.buffer_to_display(range.start) else {
        return Vec::new();
    };
    let Some(end) = map.buffer_to_display(range.end) else {
        return Vec::new();
    };
    let line_height = map.line_height();
    if line_height <= px(0.) {
        return Vec::new();
    }
    let first_visible_row = (f32::from(viewport.top() - origin.y) / f32::from(line_height))
        .floor()
        .max(0.) as usize;
    let visible_row_end = (f32::from(viewport.bottom() - origin.y) / f32::from(line_height))
        .ceil()
        .max(0.) as usize;
    let first_row = start.row.max(first_visible_row);
    let row_end = (end.row + 1)
        .min(visible_row_end)
        .min(map.visual_line_count());

    (first_row..row_end)
        .filter_map(|row| {
            let row_range = map.visual_row_range(row)?;
            let segment_start = range.start.max(row_range.start).min(row_range.end);
            let segment_end = range.end.min(row_range.end).max(row_range.start);
            let has_text = segment_start < segment_end;
            let crosses_empty_row = row_range.is_empty()
                && range.start <= row_range.start
                && range.end >= row_range.end;
            let crosses_logical_newline = row.checked_sub(1).is_some_and(|previous_row| {
                map.visual_row_range(previous_row).is_some_and(|previous| {
                    previous.end < row_range.start
                        && range.start <= previous.end
                        && range.end >= row_range.start
                })
            });
            if !has_text && !crosses_empty_row && !crosses_logical_newline {
                return None;
            }
            let start_column = segment_start.saturating_sub(row_range.start);
            let end_column = segment_end.saturating_sub(row_range.start);
            let start_position =
                map.position_for_display(TextDisplayPoint::new(row, start_column))?;
            let end_position = map.position_for_display(TextDisplayPoint::new(row, end_column))?;
            Some(fill(
                Bounds::new(
                    origin + start_position,
                    size((end_position.x - start_position.x).max(px(1.)), line_height),
                ),
                color.clone(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext as _, Context, Render, StyleRefinement, TestApp};
    use std::{cell::Cell, rc::Rc};

    struct CachedEditor {
        state: Entity<EditableTextState>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for CachedEditor {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div().size_full().child(EditableText::new(&self.state))
        }
    }

    struct CachedEditorRoot(Entity<CachedEditor>);

    impl Render for CachedEditorRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().child(
                self.0
                    .clone()
                    .cached(StyleRefinement::default().w(px(320.)).h(px(48.))),
            )
        }
    }

    #[test]
    fn editable_text_invalidates_cached_ancestor_after_platform_input() {
        let mut app = TestApp::new();
        let renders = Rc::new(Cell::new(0));
        let mut window = app.open_window(|_, cx| {
            CachedEditorRoot(cx.new(|cx| CachedEditor {
                state: cx.new(|cx| EditableTextState::with_text("1", cx)),
                renders: renders.clone(),
            }))
        });
        let state = window.read(|root, cx| root.0.read(cx).state.clone());
        window.draw();
        window.update(|_, window, cx| state.read(cx).focus_handle().focus(window, cx));
        window.draw();

        let settled_renders = renders.get();
        window.draw();
        assert_eq!(
            renders.get(),
            settled_renders,
            "idle draw must reuse the cached editor"
        );

        for text in ["86420", "72"] {
            window.simulate_keystroke("secondary-a");
            let previous_renders = renders.get();
            assert!(window.update(|_, window, cx| window.dispatch_text_input(text, cx)));
            assert_eq!(app.read_entity(&state, |state, _| state.text()), text);
            window.draw();
            assert!(
                renders.get() > previous_renders,
                "an editable state notification must invalidate its cached ancestor"
            );
            app.read_entity(&state, |state, _| {
                assert_eq!(state.layout_map().revision(), Some(state.revision()));
                assert_eq!(state.layout_map().visual_row_range(0), Some(0..text.len()));
            });
            let settled_renders = renders.get();
            window.draw();
            assert_eq!(
                renders.get(),
                settled_renders,
                "idle draw must remain cached after editing"
            );
        }
    }
}
