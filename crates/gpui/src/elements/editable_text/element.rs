use crate::{
    App, Bounds, Color, ContentMask, CursorStyle, DecorationRun, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, GlobalElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, RenderOnce, ScrollWheelEvent,
    ShapedLine, SharedString, Style, TextAlign, TextRun, UnderlineStyle, Window, WrappedLine, div,
    fill, point, prelude::*, px, relative, size,
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
        }
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

        let key_state = self.state.clone();
        let down_state = self.state.clone();
        let down_focus_handle = focus_handle.clone();
        let up_state = self.state.clone();
        let scroll_state = self.state.clone();

        div()
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
                        state.pointer_down(event.position, event.modifiers.shift, cx)
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
                    window.text_system(),
                    &self.display_transform,
                )
                .is_ok()
        });

        let state = self.state.read(cx);
        let snapshot = state.snapshot();
        let content = snapshot.text();
        let selection = *state.selection();
        let marked_range = state
            .marked_range()
            .map(|range| state.layout_map().buffer_range_to_display(range));
        let scroll_offset = state.scroll_offset();
        let content_origin = bounds.origin + scroll_offset;

        let mut lines = Vec::new();
        for row_index in 0..state.layout_map().logical_row_count() {
            let Some((display_start, visual_start, text, layout)) =
                state.layout_map().paint_row(row_index)
            else {
                continue;
            };
            let row_top = line_height * visual_start + scroll_offset.y;
            let row_bottom = row_top + layout.size(line_height).height;
            if !layout_ready || row_bottom <= px(0.) || row_top >= bounds.size.height {
                continue;
            }
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
                selection_quads(
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
                        TextAlign::Left,
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
                        TextAlign::Left,
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
                        TextAlign::Left,
                        None,
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

fn selection_quads(
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
    (start.row..=end.row)
        .filter_map(|row| {
            let top = origin.y + map.line_height() * row;
            if top + map.line_height() <= viewport.top() || top >= viewport.bottom() {
                return None;
            }
            let row_range = map.visual_row_range(row)?;
            let start = range.start.max(row_range.start).min(row_range.end);
            let end = range.end.min(row_range.end).max(row_range.start);
            let start_display = map.buffer_to_display(start)?;
            let end_display = map.buffer_to_display(end)?;
            let start_position =
                map.position_for_display(TextDisplayPoint::new(row, start_display.column))?;
            let end_position =
                map.position_for_display(TextDisplayPoint::new(row, end_display.column))?;
            Some(fill(
                Bounds::new(
                    origin + start_position,
                    size(
                        (end_position.x - start_position.x).max(px(1.)),
                        map.line_height(),
                    ),
                ),
                color.clone(),
            ))
        })
        .collect()
}
