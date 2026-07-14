use crate::{
    App, Bounds, Color, CursorStyle, DispatchPhase, Element, ElementId, ElementInputHandler,
    Entity, GlobalElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, RenderOnce, ShapedLine, SharedString, Style,
    TextRun, UnderlineStyle, Window, div, fill, point, prelude::*, px, relative, size,
};

use super::{EditableTextMode, EditableTextState};

/// A reusable single-line editable-text element backed by [`EditableTextState`].
#[derive(IntoElement)]
pub struct EditableText {
    state: Entity<EditableTextState>,
    placeholder_color: Option<Color>,
    selection_color: Option<Color>,
    caret_color: Option<Color>,
}

#[allow(missing_docs)]
impl EditableText {
    pub fn new(state: &Entity<EditableTextState>) -> Self {
        Self {
            state: state.clone(),
            placeholder_color: None,
            selection_color: None,
            caret_color: None,
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
}

impl RenderOnce for EditableText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state
            .update(cx, |state, cx| state.ensure_focus_tracking(window, cx));
        let (focus_handle, mode) = {
            let state = self.state.read(cx);
            (state.focus_handle(), state.mode())
        };

        let key_state = self.state.clone();
        let down_state = self.state.clone();
        let up_state = self.state.clone();

        div()
            .flex()
            .w_full()
            .key_context("EditableText")
            .cursor(CursorStyle::IBeam)
            .on_key_down(move |event, window, cx| {
                if handle_key_down(&key_state, event, window, cx) {
                    cx.stop_propagation();
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, _window, cx| {
                    down_state.update(cx, |state, cx| {
                        state.pointer_down(event.position, event.modifiers.shift, cx)
                    });
                },
            )
            .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _window, cx| {
                up_state.update(cx, |state, _| state.pointer_up());
            })
            .when(mode.permits_focus(), |element| {
                element.track_focus(&focus_handle)
            })
            .child(EditableTextPaintElement {
                state: self.state,
                placeholder_color: self.placeholder_color,
                selection_color: self.selection_color,
                caret_color: self.caret_color,
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
    let platform = modifiers.platform;
    state.update(cx, |state, cx| match (platform, modifiers.shift, key) {
        (false, false, "backspace") => state.delete_backward(cx),
        (false, false, "delete") => state.delete_forward(cx),
        (false, shift, "left") => {
            state.move_left(shift, cx);
            true
        }
        (false, shift, "right") => {
            state.move_right(shift, cx);
            true
        }
        (false, false, "home") => {
            state.move_to_start(cx);
            true
        }
        (false, true, "home") => {
            state.move_to_start_extending(cx);
            true
        }
        (false, false, "end") => {
            state.move_to_end(cx);
            true
        }
        (false, true, "end") => {
            state.move_to_end_extending(cx);
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
}

struct EditableTextPrepaintState {
    revision: u64,
    line: ShapedLine,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
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
        style.size.height = window.line_height().into();
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
        let state = self.state.read(cx);
        let snapshot = state.snapshot();
        let content = snapshot.text();
        let selection = *state.selection();
        let marked_range = state.marked_range();
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
        let (display_text, color): (SharedString, _) = if content.is_empty() {
            (state.placeholder().to_owned().into(), placeholder_color)
        } else {
            (content.into(), style.color.clone())
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked) = marked_range.filter(|_| !display_text.is_empty()) {
            [
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color.clone()),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect::<Vec<_>>()
        } else {
            vec![run]
        };
        let line = window.text_system().shape_line(
            display_text,
            style.font_size.to_pixels(window.rem_size()),
            &runs,
            None,
        );

        let selection_quad = (!selection.is_empty()).then(|| {
            fill(
                Bounds::from_corners(
                    point(
                        bounds.left() + line.x_for_index(selection.range().start),
                        bounds.top(),
                    ),
                    point(
                        bounds.left() + line.x_for_index(selection.range().end),
                        bounds.bottom(),
                    ),
                ),
                selection_color,
            )
        });
        let cursor =
            (selection.is_empty() && state.is_focused() && state.caret_visible()).then(|| {
                fill(
                    Bounds::new(
                        point(
                            bounds.left() + line.x_for_index(selection.head()),
                            bounds.top(),
                        ),
                        size(px(2.), bounds.size.height),
                    ),
                    caret_color,
                )
            });

        EditableTextPrepaintState {
            revision: snapshot.revision(),
            line,
            cursor,
            selection: selection_quad,
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
        if mode != EditableTextMode::Disabled {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.state.clone()),
                cx,
            );
        }
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        prepaint
            .line
            .paint(
                bounds.origin,
                window.line_height(),
                crate::TextAlign::Left,
                None,
                window,
                cx,
            )
            .expect("editable text line should paint");
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }

        let line = prepaint.line.clone();
        let revision = prepaint.revision;
        self.state.update(cx, |state, _| {
            state.install_geometry(revision, bounds, line);
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
