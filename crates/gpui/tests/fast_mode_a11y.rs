//! Fast mode's counters with the accessibility tree built on every frame
//! (`AccessibilityMode::Always`): a frame where one view changes builds only that view and draws
//! the rest from last frame, a scroll composites the container's layer, a window a screen reader
//! reads (the platform source) still draws unchanged views from last frame, and no view or layer is bypassed because the
//! tree is built. The counts are deterministic; timing is lane G's.
//!
//! `cargo test -p gpux --features test-support --test fast_mode_a11y`
#![cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]

use gpux::{
    AccessibilityMode, AppContext as _, Context, Entity, FastMode, FastStats, IntoElement,
    LayerBypassReason, Modifiers, Render, Role, ScrollDelta, ScrollHandle, ScrollWheelEvent,
    SharedString, TestAppContext, TestDispatcher, TestWindowOptions, TouchPhase, VisualTestContext,
    Window, WindowHandle, div, point, prelude::*, px, rgb, size,
};

struct Panel {
    ix: usize,
    value: usize,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("panel", self.ix))
            .role(Role::Group)
            .aria_label(SharedString::from(format!("panel {}", self.ix)))
            .h(px(30.))
            .bg(rgb(0xeeeeee))
            .child(SharedString::from(format!("value {}", self.value)))
    }
}

struct Board {
    panels: Vec<Entity<Panel>>,
    scroll: ScrollHandle,
}

impl Render for Board {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .flex()
            .child(
                div()
                    .w(px(150.))
                    .flex()
                    .flex_col()
                    .children(self.panels.iter().cloned()),
            )
            .child(
                div()
                    .id("rows")
                    .w(px(200.))
                    .h(px(200.))
                    .bg(rgb(0xffffff))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children((0..60).map(|row| {
                        div()
                            .h(px(20.))
                            .bg(rgb(if row % 2 == 0 { 0xdddddd } else { 0xcccccc }))
                            .child(SharedString::from(format!("row {row}")))
                    })),
            )
    }
}

fn open() -> (TestAppContext, WindowHandle<Board>) {
    open_in(AccessibilityMode::Always)
}

fn open_in(mode: AccessibilityMode) -> (TestAppContext, WindowHandle<Board>) {
    let mut cx = TestAppContext::build_with_renderer(
        TestDispatcher::new(0),
        None,
        gpui_platform::bundled_text_system(),
        gpui_platform::current_headless_renderer,
    );
    cx.set_draw_on_flush(false);
    cx.update(|cx| cx.set_accessibility_mode(mode));
    let window = cx.add_window_with_options(
        TestWindowOptions::new(size(px(400.), px(300.))).scale_factor(1.),
        |_, cx| Board {
            panels: (0..6)
                .map(|ix| cx.new(|_| Panel { ix, value: 0 }))
                .collect(),
            scroll: ScrollHandle::new(),
        },
    );
    cx.update_window(window.into(), |_, window, _| {
        window.set_fast_mode(FastMode::ALL);
        window.activate_window();
    })
    .unwrap();
    (cx, window)
}

fn frame(cx: &mut TestAppContext, window: WindowHandle<Board>) -> FastStats {
    VisualTestContext::from_window(window.into(), cx)
        .capture_screenshot()
        .expect("the window rendered");
    cx.update_window(window.into(), |_, window, _| {
        assert!(window.is_a11y_active(), "the tree is built on every frame");
        window.fast_frame_stats()
    })
    .unwrap()
}

fn labels(cx: &mut TestAppContext, window: WindowHandle<Board>) -> Vec<String> {
    cx.update_window(window.into(), |_, window, _| {
        window
            .a11y_tree()
            .unwrap()
            .nodes
            .iter()
            .filter_map(|(_, node)| node.label().or(node.value()).map(str::to_string))
            .collect()
    })
    .unwrap()
}

fn assert_no_bypass(stats: &FastStats) {
    assert_eq!(stats.views_bypassed(), 0, "{stats:?}");
    let inactive = LayerBypassReason::ALL
        .iter()
        .position(|reason| *reason == LayerBypassReason::Inactive)
        .unwrap();
    assert_eq!(
        stats.layer_bypasses[inactive], 0,
        "layers stay active: {stats:?}"
    );
}

#[test]
fn a_frame_where_one_view_changes_replays_the_others() {
    let (mut cx, window) = open();
    for _ in 0..3 {
        frame(&mut cx, window);
    }
    let panel = window
        .read_with(&cx, |board, _| board.panels[2].clone())
        .unwrap();
    panel.update(&mut cx, |panel, cx| {
        panel.value = 7;
        cx.notify();
    });
    let stats = frame(&mut cx, window);
    // Only the notified panel is built; the board around it is drawn from last frame around
    // it (splice), the other panels with it.
    assert_eq!(stats.views_built, 1, "{stats:?}");
    assert!(stats.views_spliced + stats.views_reused > 0, "{stats:?}");
    assert_no_bypass(&stats);
    let labels = labels(&mut cx, window);
    for ix in 0..6 {
        assert!(labels.contains(&format!("panel {ix}")), "{labels:?}");
    }
    assert!(labels.contains(&"value 7".to_string()), "{labels:?}");
}

#[test]
fn a_scroll_composites_the_container_s_layer() {
    let (mut cx, window) = open();
    for _ in 0..3 {
        frame(&mut cx, window);
    }
    let mut composited = 0;
    for _ in 0..6 {
        VisualTestContext::from_window(window.into(), &cx).simulate_event(ScrollWheelEvent {
            position: point(px(250.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-20.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        let stats = frame(&mut cx, window);
        assert_no_bypass(&stats);
        composited += stats.layers_composited;
    }
    assert!(composited > 0, "the scrolls composited the layer");
    let labels = labels(&mut cx, window);
    assert!(
        labels.iter().any(|label| label.starts_with("row ")),
        "{labels:?}"
    );
}

#[test]
fn a_window_a_screen_reader_reads_keeps_drawing_views_from_last_frame() {
    let (mut cx, window) = open_in(AccessibilityMode::Auto);
    VisualTestContext::from_window(window.into(), &cx)
        .activate_accessibility(window.into())
        .expect("the test window has an adapter");
    for _ in 0..3 {
        frame(&mut cx, window);
    }
    // One panel changes: the others are drawn from last frame, and the adapter still gets
    // every panel's node.
    let panel = window
        .read_with(&cx, |board, _| board.panels[4].clone())
        .unwrap();
    panel.update(&mut cx, |panel, cx| {
        panel.value = 9;
        cx.notify();
    });
    let stats = frame(&mut cx, window);
    assert_eq!(stats.views_built, 1, "{stats:?}");
    assert!(stats.views_spliced + stats.views_reused > 0, "{stats:?}");
    assert_no_bypass(&stats);
    let updates = VisualTestContext::from_window(window.into(), &cx)
        .take_accessibility_updates(window.into());
    assert!(
        updates.last().is_some_and(|update| {
            (0..6).all(|ix| {
                update
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some(format!("panel {ix}").as_str()))
            })
        }),
        "the adapter receives the carried panels"
    );
}
