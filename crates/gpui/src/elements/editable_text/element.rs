use crate::AnyElement;
use crate::{
    App, Bounds, Color, ContentMask, CursorStyle, DecorationRun, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, GlobalElementId, IntoElement, KeyDownEvent, LayoutId,
    LineRowGeometry, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels,
    RenderOnce, ScrollWheelEvent, ShapedLine, SharedString, Style, TextAlign, TextRun,
    UnderlineStyle, WeakEntity, Window, WrappedLine, div, fill, point, prelude::*, px, relative,
    size,
};
use std::{ops::Range, rc::Rc};

/// Renders the element of an inline object; see [`EditableText::inline_object_renderer`].
type InlineObjectRenderer = Rc<dyn Fn(&InlineObject, &mut Window, &mut App) -> AnyElement>;

use super::{
    AttributeSpan, EditableTextDisplayTransform, EditableTextLineMode, EditableTextMode,
    EditableTextState, EditableTextStyleRun, InlineObject, TextDisplayPoint, TextLayoutStyle,
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
    aria: EditableTextAria,
    inline_object_renderer: Option<InlineObjectRenderer>,
}

/// What a control built around the field says of it: the field is the control's one
/// accessibility node, the one that holds the focus.
#[derive(Default)]
struct EditableTextAria {
    role: Option<accesskit::Role>,
    description: Option<SharedString>,
    required: bool,
    invalid: bool,
    accessibility_id: Option<SharedString>,
    labelled_by: Vec<SharedString>,
    described_by: Vec<SharedString>,
    error_message: Option<SharedString>,
    on_set_value: Option<SetValueHandler>,
}

type SetValueHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

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
            aria: EditableTextAria::default(),
            inline_object_renderer: None,
        }
    }

    /// Renders each visible inline object's element, laid out at the object's box inside the
    /// field. Its pointer listeners see events over the object before the text does; events
    /// it does not handle select text. Without a renderer objects keep their space and paint
    /// nothing.
    pub fn inline_object_renderer(
        mut self,
        renderer: impl Fn(&InlineObject, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.inline_object_renderer = Some(Rc::new(renderer));
        self
    }

    /// The field's role when it is more than a text input (a `SearchInput`, say); a masked
    /// field stays a password input.
    pub fn aria_role(mut self, role: crate::Role) -> Self {
        self.aria.role = Some(role);
        self
    }

    /// The field's accessible description.
    pub fn aria_description(mut self, description: impl Into<SharedString>) -> Self {
        self.aria.description = Some(description.into());
        self
    }

    /// Whether the field needs a value.
    pub fn aria_required(mut self, required: bool) -> Self {
        self.aria.required = required;
        self
    }

    /// Whether the field's value is invalid.
    pub fn aria_invalid(mut self, invalid: bool) -> Self {
        self.aria.invalid = invalid;
        self
    }

    /// The id other elements' relations name the field by.
    pub fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.aria.accessibility_id = Some(id.into());
        self
    }

    /// An element (by `accessibility_id`) that labels the field.
    pub fn aria_labelled_by(mut self, id: impl Into<SharedString>) -> Self {
        self.aria.labelled_by.push(id.into());
        self
    }

    /// An element (by `accessibility_id`) that describes the field.
    pub fn aria_described_by(mut self, id: impl Into<SharedString>) -> Self {
        self.aria.described_by.push(id.into());
        self
    }

    /// The element (by `accessibility_id`) holding the field's error.
    pub fn aria_error_message(mut self, id: impl Into<SharedString>) -> Self {
        self.aria.error_message = Some(id.into());
        self
    }

    /// What an assistive technology's SetValue does with the new value. Without it, SetValue
    /// replaces the field's text.
    pub fn on_set_value(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.aria.on_set_value = Some(Rc::new(handler));
        self
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
        // The text field's accessibility node: its text is copied only while the tree is built,
        // and it is the text the field shows, so a masked field never exposes what it hides.
        let masked = matches!(
            self.display_transform,
            EditableTextDisplayTransform::PerGrapheme(_)
        );
        let a11y = window.is_a11y_active().then(|| {
            let state = self.state.read(cx);
            let text = state.text();
            let shown = match self.display_transform.project(&text) {
                Some(projection) => projection.text().to_string(),
                None => text,
            };
            (shown, state.placeholder().to_string())
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
                _ if masked => accesskit::Role::PasswordInput,
                _ if self.aria.role.is_some() => {
                    self.aria.role.unwrap_or(accesskit::Role::TextInput)
                }
                EditableTextLineMode::SingleLine => accesskit::Role::TextInput,
                EditableTextLineMode::MultiLine => accesskit::Role::MultilineTextInput,
            })
            .when_some(self.aria_label, |element, label| element.aria_label(label))
            .when_some(self.aria.description, |element, description| {
                element.aria_description(description)
            })
            .when(self.aria.required, |element| element.aria_required(true))
            .when(self.aria.invalid, |element| element.aria_invalid(true))
            .when_some(self.aria.accessibility_id, |element, id| {
                element.accessibility_id(id)
            })
            .when_some(self.aria.error_message, |element, id| {
                element.aria_error_message(id)
            })
            .map(|mut element| {
                for id in self.aria.labelled_by {
                    element = element.aria_labelled_by(id);
                }
                for id in self.aria.described_by {
                    element = element.aria_described_by(id);
                }
                element
            })
            .when(mode.permits_mutation(), |element| {
                let set_state = self.state.clone();
                let handler = self.aria.on_set_value;
                element.on_a11y_action(
                    crate::AccessibleAction::SetValue,
                    move |data, window, cx| {
                        let Some(accesskit::ActionData::Value(value)) = data else {
                            return;
                        };
                        match &handler {
                            Some(handler) => handler(value, window, cx),
                            None => set_state.update(cx, |state, cx| {
                                state.select_all(cx);
                                state.replace_selection(value, cx);
                            }),
                        }
                    },
                )
            })
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
                inline_object_renderer: self.inline_object_renderer,
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
        // List editing defaults, which an owner can switch off per state.
        (false, false, "backspace") if state.list_editing() && state.list_delete_backward(cx) => {
            true
        }
        (false, false, "enter") if state.list_editing() && state.list_insert_newline(cx) => true,
        (false, false, "tab") if state.list_editing() => state.indent_list_item(cx),
        (false, true, "tab") if state.list_editing() => state.outdent_list_item(cx),
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
    inline_object_renderer: Option<InlineObjectRenderer>,
}

struct EditableTextPrepaintState {
    lines: Vec<EditableTextPaintLine>,
    placeholder: Option<ShapedLine>,
    text_align: TextAlign,
    /// Where the text's first row starts: the viewport origin, scroll and alignment offset.
    content_origin: crate::Point<Pixels>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    /// List markers: shaped marker text at its origin, or checkbox quads.
    list_marker_lines: Vec<(ShapedLine, crate::Point<Pixels>)>,
    list_marker_quads: Vec<PaintQuad>,
    /// The owner's elements of the visible inline objects, prepainted at their boxes.
    inline_objects: Vec<crate::AnyElement>,
}

struct EditableTextPaintLine {
    line: WrappedLine,
    /// Each visual row's geometry, from the top of the text.
    rows: Vec<LineRowGeometry>,
    /// The inline objects' characters, which paint nothing.
    object_offsets: Vec<usize>,
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
            // A single row as tall as its tallest run.
            EditableTextLineMode::SingleLine => {
                let height = window
                    .line_height()
                    .max(self.state.read(cx).layout_map().size().height);
                self.state
                    .update(cx, |state, _| state.requested_height = Some(height));
                height.into()
            }
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
        // A single-line field asks layout for its row's height; when the row's height changed
        // since it last asked, lay the field out again next frame. (Laid-out bounds are snapped
        // to device pixels, so they are not compared.)
        let height_changed = {
            let state = self.state.read(cx);
            state.line_mode() == EditableTextLineMode::SingleLine
                && state.requested_height != Some(line_height.max(state.layout_map().size().height))
        };
        if height_changed && layout_ready {
            let entity_id = self.state.entity_id();
            window.on_next_frame(move |_, cx| cx.notify(entity_id));
        }
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
            let map = state.layout_map();
            let first_visible_row = map.visual_row_at_y((-scroll_offset.y).max(px(0.)));
            let visible_row_end = map.visual_row_at_y(bounds.size.height - scroll_offset.y) + 1;
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
                state.layout_map().projected_attributes(),
                marked_range.clone(),
                style.color.clone(),
            );
            let visual_len = layout.wrap_boundaries().len() + 1;
            let rows = (visual_start..visual_start + visual_len)
                .filter_map(|row| state.layout_map().visual_row(row))
                .map(|row| LineRowGeometry {
                    top: row.top,
                    height: row.height,
                    baseline: row.baseline,
                    x: row.x_offset,
                    justify: row.justify,
                })
                .collect();
            lines.push(EditableTextPaintLine {
                line: WrappedLine {
                    layout,
                    text,
                    decoration_runs: decorations,
                },
                rows,
                object_offsets: state.layout_map().logical_row_object_offsets(row_index),
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
                metrics: Default::default(),
            };
            window
                .text_system()
                .shape_line(text, font_size, &[run], None)
        });

        // List markers of the visible rows, right-aligned before each item's first row on its
        // baseline, in the size and color of the item's first character.
        let mut list_marker_lines = Vec::new();
        let mut list_marker_quads = Vec::new();
        if layout_ready {
            let map = state.layout_map();
            let visible = map.visual_row_at_y((-scroll_offset.y).max(px(0.)))
                ..map.visual_row_at_y(bounds.size.height - scroll_offset.y) + 1;
            for marker in map
                .list_markers()
                .iter()
                .filter(|marker| visible.contains(&marker.visual_row))
            {
                let first = state.attributes_at(marker.paragraph_start);
                let color = first.color.clone().unwrap_or_else(|| style.color.clone());
                if let Some(checked) = marker.checkbox {
                    let Some(bounds) = map.list_checkbox_bounds(marker) else {
                        continue;
                    };
                    let bounds = Bounds::new(content_origin + bounds.origin, bounds.size);
                    list_marker_quads.push(crate::outline(
                        bounds,
                        color.clone(),
                        crate::BorderStyle::Solid,
                    ));
                    if checked {
                        let inset = bounds.size.width * 0.25;
                        list_marker_quads.push(fill(
                            Bounds::new(
                                bounds.origin + point(inset, inset),
                                size(
                                    bounds.size.width - inset * 2.,
                                    bounds.size.height - inset * 2.,
                                ),
                            ),
                            color,
                        ));
                    }
                    continue;
                }
                let Some(row) = map.visual_row(marker.visual_row) else {
                    continue;
                };
                let mut font = style.font();
                if let Some(family) = &first.font_family {
                    font.family = family.clone();
                }
                let run = TextRun {
                    len: marker.text.len(),
                    font,
                    color,
                    ..Default::default()
                };
                let line = window.text_system().shape_line(
                    marker.text.clone(),
                    marker.font_size,
                    &[run],
                    None,
                );
                let Some(x) = map.list_marker_x(marker, line.width) else {
                    continue;
                };
                let origin = content_origin + point(x, row.top + row.baseline - line.ascent);
                list_marker_lines.push((line, origin));
            }
        }

        // The owner's element of each visible inline object, laid out at the object's box
        // and clipped by the field.
        let mut inline_objects = Vec::new();
        if layout_ready && let Some(renderer) = self.inline_object_renderer.clone() {
            let boxes = state.layout_map().inline_object_bounds();
            window.with_content_mask(Some(ContentMask::rectangular(bounds)), |window| {
                for (_, object, object_bounds) in boxes {
                    let origin = content_origin + object_bounds.origin;
                    let visible = Bounds::new(origin, object_bounds.size).intersects(&bounds);
                    if !visible {
                        continue;
                    }
                    let mut element = renderer(&object, window, cx);
                    element.layout_as_root(
                        size(
                            crate::AvailableSpace::Definite(object_bounds.size.width),
                            crate::AvailableSpace::Definite(object_bounds.size.height),
                        ),
                        window,
                        cx,
                    );
                    element.prepaint_at(origin, window, cx);
                    inline_objects.push(element);
                }
            });
        }
        let state = self.state.read(cx);

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
            let caret = state
                .layout_map()
                .caret_bounds(selection.head())
                .unwrap_or_else(|| {
                    Bounds::new(
                        crate::Point::default(),
                        size(px(0.), state.layout_map().line_height()),
                    )
                });
            fill(
                Bounds::new(
                    content_origin + caret.origin,
                    size(px(2.), caret.size.height),
                ),
                caret_color,
            )
        });

        EditableTextPrepaintState {
            lines,
            placeholder,
            text_align: style.text_align,
            content_origin,
            cursor,
            selection: selection_quads,
            list_marker_lines,
            list_marker_quads,
            inline_objects,
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
                    .paint_background_rows(prepaint.content_origin, &paint_line.rows, window, cx)
                    .expect("editable text line background should paint");
            }
            for selection in prepaint.selection.drain(..) {
                window.paint_quad(selection);
            }
            for paint_line in &prepaint.lines {
                paint_line
                    .line
                    .paint_rows(
                        prepaint.content_origin,
                        &paint_line.rows,
                        &paint_line.object_offsets,
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
            for (line, origin) in &prepaint.list_marker_lines {
                line.paint(
                    *origin,
                    line.ascent + line.descent,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .expect("editable text list marker should paint");
            }
            for quad in prepaint.list_marker_quads.drain(..) {
                window.paint_quad(quad);
            }
            for element in &mut prepaint.inline_objects {
                element.paint(window, cx);
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

/// Paint runs for one row: stored color, background, underline and strikethrough, with the
/// owner's style runs over them for the fields they set, and the IME underline.
fn decoration_runs(
    display_start: usize,
    len: usize,
    style_runs: &[EditableTextStyleRun],
    attributes: &[AttributeSpan],
    marked_range: Option<Range<usize>>,
    color: Color,
) -> Vec<DecorationRun> {
    let row_range = display_start..display_start + len;
    if row_range.is_empty() {
        return Vec::new();
    }

    let first_attribute = attributes.partition_point(|span| span.range.end <= row_range.start);
    let attributes = &attributes[first_attribute..];
    let mut boundaries = vec![row_range.start, row_range.end];
    for span in attributes
        .iter()
        .take_while(|span| span.range.start < row_range.end)
    {
        let start = span.range.start.max(row_range.start);
        let end = span.range.end.min(row_range.end);
        if start < end {
            boundaries.extend([start, end]);
        }
    }
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
                if let Some(span) = attributes
                    .iter()
                    .take_while(|span| span.range.start < end)
                    .find(|span| span.range.start <= start && span.range.end >= end)
                {
                    let stored = &span.attributes;
                    if let Some(stored_color) = &stored.color {
                        run_color = stored_color.clone();
                    }
                    background_color = stored.background_color.clone();
                    underline = stored.underline.clone();
                    strikethrough = stored.strikethrough.clone();
                }
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

/// Returns the paint runs the editable-text paint path prepares for one logical row, over
/// text painted in `color`.
///
/// Publicly hidden, like [`editable_text_selection_quads`], so integration tests can inspect
/// exact paint attributes without screenshots.
#[doc(hidden)]
pub fn editable_text_decoration_runs(
    state: &EditableTextState,
    row: usize,
    color: Color,
) -> Vec<DecorationRun> {
    let map = state.layout_map();
    let Some((display_start, _, text, _)) = map.paint_row(row) else {
        return Vec::new();
    };
    let marked_range = state
        .marked_range()
        .map(|range| map.buffer_range_to_display(range));
    decoration_runs(
        display_start,
        text.len(),
        map.projected_style_runs(),
        map.projected_attributes(),
        marked_range,
        color,
    )
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
    if map.line_height() <= px(0.) {
        return Vec::new();
    }
    let first_visible_row = map.visual_row_at_y((viewport.top() - origin.y).max(px(0.)));
    let visible_row_end = if viewport.bottom() <= origin.y {
        0
    } else {
        map.visual_row_at_y(viewport.bottom() - origin.y) + 1
    };
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
            let height = map.visual_row(row)?.height;
            Some(fill(
                Bounds::new(
                    origin + start_position,
                    size((end_position.x - start_position.x).max(px(1.)), height),
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
