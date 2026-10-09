//! Hot paths redraw the view that owns them rather than refreshing the window.
//!
//! `Window::refresh` turns every cache off for the next frame. Programmatic
//! scroll steps, active and hover styles, interactive and editable text and
//! key dispatch used to (or could) refresh; they now notify their owning view.
//! Each test drives one of those paths in a window that also holds a cached
//! view, and checks, frame by frame, that the window is not refreshing, that
//! the cached view is replayed, and that the frame equals the one a window
//! drawing from scratch draws.

use std::time::Duration;

use super::oracle_support::{PALETTE, oracle_app};
use crate::{
    AppContext as _, Context, EditableText, EditableTextState, Entity, FocusHandle,
    InputEvent as _, InteractiveText, KeyDownEvent, Keystroke, ListAlignment, ListOffset,
    ListState, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Render,
    ScrollBehavior, ScrollHandle, SharedString, StyleRefinement, StyledText, TestAppContext,
    UniformListScrollHandle, Window, WindowHandle, div, list, point, prelude::*, px, size,
    uniform_list,
};

const ROWS: usize = 200;
const SMOOTH: ScrollBehavior = ScrollBehavior::Smooth(Duration::from_millis(160));

/// A view nothing notifies, drawn cached: replayed on every frame no refresh
/// forces it to build.
struct Still;

impl Render for Still {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(PALETTE[1]).child("still")
    }
}

/// Every hot path at a known place.
struct Scoped {
    still: Entity<Still>,
    scroll: ScrollHandle,
    list: ListState,
    uniform: UniformListScrollHandle,
    text: Entity<EditableTextState>,
    keys_focus: FocusHandle,
    keys: usize,
    clicks: usize,
}

impl Scoped {
    fn new(cx: &mut Context<Self>) -> Self {
        let text = cx.new(|cx| {
            EditableTextState::with_line_mode(
                (0..40)
                    .map(|line| format!("line {line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                crate::EditableTextLineMode::MultiLine,
                cx,
            )
        });
        cx.observe(&text, |_, _, cx| cx.notify()).detach();
        Self {
            still: cx.new(|_| Still),
            scroll: ScrollHandle::new(),
            list: ListState::new(ROWS, ListAlignment::Top, px(40.)).measure_all(),
            uniform: UniformListScrollHandle::new(),
            text,
            keys_focus: cx.focus_handle(),
            keys: 0,
            clicks: 0,
        }
    }
}

fn row(index: usize) -> impl IntoElement {
    div()
        .id(("row", index))
        .h(px(20.))
        .bg(PALETTE[index % PALETTE.len()])
        .child(SharedString::from(format!("row {index}")))
}

/// Where each path is, in window coordinates.
const ACTIVE: (f32, f32) = (230., 10.);
const GROUP_HOVER: (f32, f32) = (230., 50.);
const LINK: (f32, f32) = (330., 12.);

impl Render for Scoped {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .child(
                div().absolute().left(px(0.)).top(px(0.)).child(
                    self.still
                        .clone()
                        .cached(StyleRefinement::default().w(px(100.)).h(px(20.))),
                ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(30.))
                    .w(px(200.))
                    .h(px(100.))
                    .id("scroller")
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children((0..ROWS).map(row)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(140.))
                    .w(px(200.))
                    .h(px(100.))
                    .child(
                        list(self.list.clone(), |index, _, _| {
                            row(index).into_any_element()
                        })
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(250.))
                    .w(px(200.))
                    .h(px(100.))
                    .child(
                        uniform_list("uniform", ROWS, |range, _, _| {
                            range.map(|index| row(index).into_any_element()).collect()
                        })
                        .track_scroll(&self.uniform)
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .id("active")
                    .absolute()
                    .left(px(ACTIVE.0 - 20.))
                    .top(px(ACTIVE.1 - 10.))
                    .w(px(40.))
                    .h(px(20.))
                    .bg(PALETTE[0])
                    .active(|style| style.bg(PALETTE[3]))
                    .hover(|style| style.bg(PALETTE[2]))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    }))
                    .child(SharedString::from(self.clicks.to_string())),
            )
            .child(
                div()
                    .id("group")
                    .group("group")
                    .absolute()
                    .left(px(GROUP_HOVER.0 - 20.))
                    .top(px(GROUP_HOVER.1 - 10.))
                    .w(px(40.))
                    .h(px(20.))
                    .child(
                        div()
                            .size_full()
                            .bg(PALETTE[0])
                            .group_hover("group", |style| style.bg(PALETTE[4])),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(LINK.0 - 30.))
                    .top(px(LINK.1 - 12.))
                    .w(px(120.))
                    .child(
                        InteractiveText::new("link", StyledText::new("click here"))
                            .on_click(vec![0..10], |_, _, _| {}),
                    ),
            )
            .child(
                div()
                    .id("keys")
                    .absolute()
                    .left(px(220.))
                    .top(px(100.))
                    .w(px(60.))
                    .h(px(20.))
                    .track_focus(&self.keys_focus)
                    .on_key_down(cx.listener(|this, _: &KeyDownEvent, _, cx| {
                        this.keys += 1;
                        cx.notify();
                    }))
                    .child(SharedString::from(self.keys.to_string())),
            )
            .child(
                div()
                    .absolute()
                    .left(px(220.))
                    .top(px(140.))
                    .w(px(160.))
                    .h(px(60.))
                    .child(EditableText::new(&self.text)),
            )
    }
}

/// A window drawing incrementally and one drawing every frame from scratch,
/// driven alike. The test platform's draw-on-flush is off: every frame is
/// drawn by [`Pair::frame`].
struct Pair {
    windows: Vec<(TestAppContext, WindowHandle<Scoped>)>,
}

impl Pair {
    fn open() -> Self {
        let windows = (0..2)
            .map(|_| {
                let mut cx = oracle_app(0);
                let window = cx.add_window(|_, cx| Scoped::new(cx));
                cx.simulate_window_resize(window.into(), size(px(400.), px(400.)));
                cx.set_draw_on_flush(false);
                (cx, window)
            })
            .collect();
        let mut pair = Self { windows };
        pair.frame("opening", true);
        pair.frame("settling", true);
        pair
    }

    /// Applies `change` to both windows.
    fn each(&mut self, change: impl Fn(&mut TestAppContext, WindowHandle<Scoped>)) {
        for (cx, window) in &mut self.windows {
            change(cx, *window);
        }
    }

    fn dispatch(&mut self, event: crate::PlatformInput) {
        self.each(|cx, window| {
            cx.update_window(window.into(), |_, window, cx| {
                window.dispatch_event(event.clone(), cx);
            })
            .unwrap();
        });
    }

    /// Advances both clocks by a frame, runs what the last frames asked to
    /// run, and draws: the incremental window without a refresh pending, the
    /// reference from scratch. Panics if the incremental window would refresh
    /// or its frame differs. Returns how many cached views it replayed.
    fn frame(&mut self, what: &str, may_refresh: bool) -> u64 {
        let mut drawn = Vec::new();
        for (index, (cx, window)) in self.windows.iter_mut().enumerate() {
            cx.executor().advance_clock(Duration::from_millis(16));
            let incremental = index == 0;
            drawn.push(
                cx.update_window((*window).into(), |_, window, cx| {
                    let callbacks = window.next_frame_callbacks.take();
                    for callback in callbacks {
                        callback(window, cx);
                    }
                    if incremental && !may_refresh {
                        assert!(
                            !window.refreshing,
                            "{what}: the window refreshes, which turns every cache off"
                        );
                    } else if !incremental {
                        window.forget_retained_state();
                    }
                    window.draw(cx).clear(cx);
                    (
                        window.describe_rendered_frame(),
                        window.fast_frame_stats().cached_views_replayed,
                    )
                })
                .unwrap(),
            );
        }
        if let Some(difference) =
            crate::fast::verify::first_frame_difference(&drawn[0].0, &drawn[1].0)
        {
            panic!("{what}: {difference}");
        }
        drawn[0].1
    }

    /// Draws `frames` frames, requiring the cached view to be replayed on
    /// every one.
    fn frames(&mut self, what: &str, frames: usize) {
        for frame in 0..frames {
            let replayed = self.frame(&format!("{what}, frame {frame}"), false);
            assert!(
                replayed > 0,
                "{what}, frame {frame}: the cached view was built again"
            );
        }
    }

    /// Reads the incremental window's view.
    fn read<R>(&self, read: impl FnOnce(&Scoped, &crate::App) -> R) -> R {
        let (cx, window) = &self.windows[0];
        window.read_with(cx, read).unwrap()
    }

    fn update(&mut self, change: impl Fn(&mut Scoped, &mut Context<Scoped>)) {
        self.each(|cx, window| {
            window
                .update(cx, |view, _, cx| {
                    change(view, cx);
                    cx.notify();
                })
                .unwrap();
        });
    }
}

fn mouse_move(x: f32, y: f32) -> crate::PlatformInput {
    MouseMoveEvent {
        position: point(px(x), px(y)),
        pressed_button: None,
        modifiers: Modifiers::default(),
    }
    .to_platform_input()
}

fn mouse_down(x: f32, y: f32) -> crate::PlatformInput {
    MouseDownEvent {
        button: MouseButton::Left,
        position: point(px(x), px(y)),
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    }
    .to_platform_input()
}

fn mouse_up(x: f32, y: f32) -> crate::PlatformInput {
    MouseUpEvent {
        button: MouseButton::Left,
        position: point(px(x), px(y)),
        modifiers: Modifiers::default(),
        click_count: 1,
    }
    .to_platform_input()
}

#[test]
fn programmatic_scroll_steps_redraw_the_scroll_owner() {
    let mut pair = Pair::open();
    pair.update(|view, _| {
        view.scroll.scroll_to(point(px(0.), px(-600.)), SMOOTH);
    });
    pair.frames("scroll area", 14);
    assert!(
        pair.read(|view, _| view.scroll.offset().y) < px(-500.),
        "the scroll area did not scroll"
    );
    pair.update(|view, _| {
        view.list.scroll_to_with_behavior(
            ListOffset {
                item_ix: 120,
                offset_in_item: px(0.),
            },
            SMOOTH,
        );
    });
    pair.frames("list", 14);
    assert!(
        pair.read(|view, _| view.list.logical_scroll_top().item_ix) > 100,
        "the list did not scroll"
    );
}

#[test]
fn programmatic_uniform_list_scroll_redraws_its_owner() {
    let mut pair = Pair::open();
    pair.update(|view, _| {
        view.uniform.scroll_to_item_with_behavior(
            crate::DeferredScrollToItem {
                item_index: 150,
                strategy: crate::ScrollStrategy::Top,
                offset: 0,
                scroll_strict: false,
            },
            SMOOTH,
        );
    });
    pair.frames("uniform list", 14);
    assert!(
        pair.read(|view, _| view.uniform.0.borrow().base_handle.offset().y) < px(-2000.),
        "the uniform list did not scroll"
    );
}

#[test]
fn active_and_hover_styles_redraw_their_view() {
    let mut pair = Pair::open();
    pair.dispatch(mouse_move(ACTIVE.0, ACTIVE.1));
    pair.frames("hover", 1);
    pair.dispatch(mouse_down(ACTIVE.0, ACTIVE.1));
    pair.frames("press", 1);
    pair.dispatch(mouse_up(ACTIVE.0, ACTIVE.1));
    pair.frames("release", 1);
    pair.dispatch(mouse_move(GROUP_HOVER.0, GROUP_HOVER.1));
    pair.frames("group hover", 1);
    pair.dispatch(mouse_move(390., 390.));
    pair.frames("unhover", 1);
}

#[test]
fn interactive_text_clicks_redraw_their_view() {
    let mut pair = Pair::open();
    pair.dispatch(mouse_move(LINK.0, LINK.1));
    pair.frames("hover link", 1);
    pair.dispatch(mouse_down(LINK.0, LINK.1));
    pair.frames("press link", 1);
    pair.dispatch(mouse_up(LINK.0, LINK.1));
    pair.frames("release link", 1);
}

#[test]
fn editable_text_redraws_and_scrolls_its_field() {
    let mut pair = Pair::open();
    pair.each(|cx, window| {
        window
            .update(cx, |view, window, cx| {
                let handle = view.text.read(cx).focus_handle();
                window.focus(&handle, cx);
            })
            .unwrap();
    });
    // Focus moves restyle any view, so focusing refreshes; the frames after
    // it must not.
    pair.frame("focus", true);
    pair.each(|cx, window| {
        window
            .update(cx, |view, _, cx| {
                view.text
                    .update(cx, |state, cx| state.replace_selection("typed ", cx));
            })
            .unwrap();
    });
    pair.frames("typing", 1);
    let before = pair.read(|view, cx| view.text.read(cx).scroll_offset());
    pair.each(|cx, window| {
        window
            .update(cx, |view, _, cx| {
                view.text.update(cx, |state, cx| {
                    state.request_scroll_offset_with_behavior(point(px(0.), px(300.)), SMOOTH, cx)
                });
            })
            .unwrap();
    });
    pair.frames("editable scroll", 14);
    let after = pair.read(|view, cx| view.text.read(cx).scroll_offset());
    assert!(
        (after.y - before.y).abs() > px(100.),
        "the editable text did not scroll: {before:?} then {after:?}"
    );
}

#[test]
fn key_dispatch_redraws_the_focused_view() {
    let mut pair = Pair::open();
    pair.each(|cx, window| {
        window
            .update(cx, |view, window, cx| window.focus(&view.keys_focus, cx))
            .unwrap();
    });
    pair.frame("focus", true);
    for key in ["a", "b", "c"] {
        pair.each(|cx, window| {
            cx.dispatch_keystroke(window.into(), Keystroke::parse(key).unwrap());
        });
        pair.frames(&format!("key {key}"), 1);
    }
    let presses = pair.windows[0]
        .1
        .read_with(&pair.windows[0].0, |view, _| view.keys)
        .unwrap();
    assert_eq!(presses, 3, "the keys did not reach the focused view");
}
