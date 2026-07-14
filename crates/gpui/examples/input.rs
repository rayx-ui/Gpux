#![cfg_attr(target_family = "wasm", no_main)]

use gpui_platform::application;
use gpux::{
    App, Bounds, Context, EditableText, EditableTextState, Entity, FocusHandle, Focusable,
    KeyBinding, Keystroke, MouseButton, MouseUpEvent, Render, Window, WindowBounds, WindowOptions,
    actions, black, div, opaque_grey, prelude::*, px, rgb, size, white, yellow,
};
actions!(input_example, [Quit]);

struct InputExample {
    text_input: Entity<EditableTextState>,
    recent_keystrokes: Vec<Keystroke>,
    focus_handle: FocusHandle,
}

impl Focusable for InputExample {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl InputExample {
    fn on_reset_click(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.recent_keystrokes.clear();
        self.text_input
            .update(cx, |text_input, cx| text_input.reset(cx));
        cx.notify();
    }
}

impl Render for InputExample {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .bg(rgb(0xaaaaaa))
            .track_focus(&self.focus_handle(cx))
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .bg(white())
                    .border_b_1()
                    .border_color(black())
                    .flex()
                    .flex_row()
                    .justify_between()
                    .child(format!("Keyboard {}", cx.keyboard_layout().name()))
                    .child(
                        div()
                            .border_1()
                            .border_color(black())
                            .px_2()
                            .bg(yellow())
                            .child("Reset")
                            .hover(|style| {
                                style
                                    .bg(yellow().blend(opaque_grey(0.5, 0.5)))
                                    .cursor_pointer()
                            })
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_reset_click)),
                    ),
            )
            .child(
                div()
                    .h(px(38.))
                    .w_full()
                    .p(px(4.))
                    .bg(white())
                    .line_height(px(30.))
                    .text_size(px(24.))
                    .child(EditableText::new(&self.text_input)),
            )
            .children(self.recent_keystrokes.iter().rev().map(|ks| {
                format!(
                    "{:} {}",
                    ks.unparse(),
                    if let Some(key_char) = ks.key_char.as_ref() {
                        format!("-> {:?}", key_char)
                    } else {
                        "".to_owned()
                    }
                )
            }))
    }
}

fn run_example() {
    application().run(|cx| async move {
        cx.update(|cx| {
            let bounds = Bounds::centered(None, size(px(300.0), px(300.0)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        ..Default::default()
                    },
                    |_, cx| {
                        let text_input = cx.new(|cx| {
                            let mut state = EditableTextState::new(cx);
                            state.set_placeholder("Type here...", cx);
                            state
                        });
                        cx.new(|cx| InputExample {
                            text_input,
                            recent_keystrokes: vec![],
                            focus_handle: cx.focus_handle(),
                        })
                    },
                )
                .unwrap();
            let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
            cx.observe_keystrokes(move |ev, _, cx| {
                view.update(cx, |view, cx| {
                    view.recent_keystrokes.push(ev.keystroke.clone());
                    cx.notify();
                })
            })
            .detach();
            cx.on_keyboard_layout_change({
                move |cx| {
                    window.update(cx, |_, _, cx| cx.notify()).ok();
                }
            })
            .detach();

            window
                .update(cx, |view, window, cx| {
                    let focus_handle = view.text_input.read(cx).focus_handle();
                    window.focus(&focus_handle, cx);
                    cx.activate(true);
                })
                .unwrap();
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        });
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_example();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_example();
}
