//! The shell the native-view examples share: a window that composes, a
//! toolbar, a native surface filling the rest, and a GPUI menu, popover and
//! dialog with a translucent backdrop drawn over the native content.
//!
//! Each example embeds its content into the native surface through the
//! surface's platform handle. Run one with
//! `cargo run -p gpux-platform --features webview-examples --example <name>`;
//! `--open menu,popover,dialog` opens those overlays at start and
//! `--exit-after <seconds>` quits after that long, for screenshots.

use std::{any::Any, rc::Rc, time::Duration};

use gpux::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, IntoElement, Pixels, Point, Render,
    Window, WindowBounds, WindowCompositionSurface, WindowOptions, anchored, deferred, div,
    native_surface, point, prelude::*, px, rgb, rgba, size,
};

/// The pointer over the native content, in window logical pixels, for
/// content that takes no OS input of its own (DirectComposition visuals).
#[allow(dead_code)]
pub enum Pointer {
    Down(Point<Pixels>),
    Up(Point<Pixels>),
    Move(Point<Pixels>),
    Wheel(Point<Pixels>, f32),
}

/// What an example embeds.
pub struct Embedded {
    /// The content's owner, kept alive (never read) while the window is open.
    #[allow(dead_code)]
    pub owner: Box<dyn Any>,
    /// Where pointer events over the native area go, for content that the
    /// OS gives no input of its own.
    pub pointer: Option<Rc<dyn Fn(Pointer)>>,
}

/// Embeds an example's content into the native surface of its window.
pub type Embed = fn(&WindowCompositionSurface, &mut Window, &mut App) -> anyhow::Result<Embedded>;

struct Shell {
    surface: WindowCompositionSurface,
    content: Embedded,
    menu: bool,
    popover: bool,
    dialog: bool,
}

fn button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&mut Shell) + 'static,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    div()
        .id(id)
        .px(px(12.))
        .py(px(6.))
        .rounded(px(6.))
        .bg(rgb(0xe8e8ec))
        .hover(|style| style.bg(rgb(0xd8d8de)))
        .child(label)
        .on_click(cx.listener(move |shell, _, _, cx| {
            on_click(shell);
            cx.notify();
        }))
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x202024))
            .child(
                div()
                    .h(px(44.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(rgb(0xd0d0d6))
                    .child(button("menu", "Menu", |shell| shell.menu = !shell.menu, cx))
                    .child(button(
                        "popover",
                        "Popover",
                        |shell| shell.popover = !shell.popover,
                        cx,
                    ))
                    .child(button(
                        "dialog",
                        "Dialog",
                        |shell| shell.dialog = !shell.dialog,
                        cx,
                    )),
            )
            .child({
                let area = div()
                    .id("native-area")
                    .flex_1()
                    .w_full()
                    .child(native_surface(self.surface.clone()).size_full());
                match self.content.pointer.clone() {
                    None => area,
                    Some(pointer) => {
                        let (down, up, moved, wheel) =
                            (pointer.clone(), pointer.clone(), pointer.clone(), pointer);
                        area.on_mouse_down(gpux::MouseButton::Left, move |event, _, _| {
                            down(Pointer::Down(event.position))
                        })
                        .on_mouse_up(gpux::MouseButton::Left, move |event, _, _| {
                            up(Pointer::Up(event.position))
                        })
                        .on_mouse_move(move |event, _, _| moved(Pointer::Move(event.position)))
                        .on_scroll_wheel(move |event, window, _| {
                            let delta = event.delta.pixel_delta(window.line_height()).y;
                            wheel(Pointer::Wheel(event.position, f32::from(delta)))
                        })
                    }
                }
            })
            .when(self.menu, |this| {
                this.child(deferred(
                    anchored().position(point(px(8.), px(44.))).child(
                        div()
                            .w(px(220.))
                            .p(px(6.))
                            .flex()
                            .flex_col()
                            .rounded(px(8.))
                            .bg(rgb(0xfafafa))
                            .border_1()
                            .border_color(rgb(0xd0d0d6))
                            .shadow_lg()
                            .children(["New tab", "Reload", "Inspect"].map(|item| {
                                div()
                                    .id(item)
                                    .px(px(8.))
                                    .py(px(6.))
                                    .rounded(px(4.))
                                    .hover(|style| style.bg(rgb(0xe6eefc)))
                                    .child(item)
                            })),
                    ),
                ))
            })
            .when(self.popover, |this| {
                this.child(deferred(
                    anchored().position(point(px(260.), px(160.))).child(
                        div()
                            .w(px(260.))
                            .p(px(14.))
                            .rounded(px(10.))
                            .bg(rgba(0x202024e6))
                            .text_color(rgb(0xffffff))
                            .shadow_lg()
                            .child("A GPUI popover over live native content."),
                    ),
                ))
            })
            .when(self.dialog, |this| {
                this.child(deferred(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgba(0x00000066))
                        .child(
                            div()
                                .id("dialog-panel")
                                .w(px(360.))
                                .p(px(20.))
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .rounded(px(12.))
                                .bg(rgb(0xffffff))
                                .shadow_lg()
                                .child("A dialog with a backdrop over the native view.")
                                .child(button(
                                    "close-dialog",
                                    "Close",
                                    |shell| shell.dialog = false,
                                    cx,
                                )),
                        ),
                ))
            })
    }
}

/// Runs the example `title`, embedding its content with `embed`.
pub fn run(title: &'static str, embed: Embed) {
    let mut open = Vec::<String>::new();
    let mut exit_after = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--open" => {
                open = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .map(str::to_owned)
                    .collect()
            }
            "--exit-after" => {
                exit_after = args.next().and_then(|seconds| seconds.parse::<f64>().ok())
            }
            other => eprintln!("{title}: ignoring unknown argument `{other}`"),
        }
    }

    gpui_platform::application().run(move |mut cx| async move {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(80.), px(80.)),
                size: size(px(900.), px(600.)),
            })),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let composition = window
                .enable_window_composition()
                .expect("this platform composes windows");
            let surface = composition
                .create_native_surface()
                .expect("a native surface");
            let content = embed(&surface, window, cx).expect("the native content");
            cx.new(|_| Shell {
                surface,
                content,
                menu: open.iter().any(|item| item == "menu"),
                popover: open.iter().any(|item| item == "popover"),
                dialog: open.iter().any(|item| item == "dialog"),
            })
        });
        let window: AnyWindowHandle = match opened {
            Ok(window) => window.into(),
            Err(error) => {
                eprintln!("{title}: could not open the window: {error:#}");
                cx.update(|cx| cx.quit());
                return;
            }
        };
        cx.update(|cx| cx.activate(true));
        println!("{title}: embedded");
        if let Some(seconds) = exit_after {
            cx.background_executor()
                .timer(Duration::from_secs_f64(seconds))
                .await;
            window
                .update(&mut cx, |_, window, _| window.remove_window())
                .ok();
            cx.update(|cx| cx.quit());
        }
    });
}
