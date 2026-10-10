//! Pages with a scroll container and the helpers the scroll-layer groups
//! drive them with. Adapted from gpui-fast's `fast/tests/layers.rs`.

use crate::fast::layers::policy::{Decision, last_decision};
use crate::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, GlobalElementId,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _, TestAppContext,
    TouchPhase, Window, WindowHandle, canvas, div, point, px, rgb, size,
};
use std::rc::Rc;

pub(crate) const ROWS: usize = 40;
pub(crate) const ROW_HEIGHT: f32 = 20.;

pub(crate) fn viewport() -> Bounds<Pixels> {
    Bounds {
        origin: point(px(0.), px(0.)),
        size: size(px(200.), px(100.)),
    }
}

/// How far short of `width` a row with a hover style is: the wheel scrolls
/// the page from [`BESIDE_ROWS`], over none of them.
pub(crate) const HOVER_ROW_GAP: f32 = 60.;

/// A point of the scroll container beside its rows, when they have a hover
/// style.
pub(crate) fn beside_rows() -> crate::Point<Pixels> {
    point(px(viewport().size.width.0 - HOVER_ROW_GAP / 2.), px(20.))
}

/// A row, `width` wide; with a hover style and [`HOVER_ROW_GAP`] narrower
/// when `hover` is set.
fn row(index: usize, hover: bool, width: f32) -> AnyElement {
    let row = div()
        .w(px(if hover { width - HOVER_ROW_GAP } else { width }))
        .h(px(ROW_HEIGHT))
        .bg(rgb(0x100000 + index as u32 * 0x10));
    if hover {
        row.id(("row", index))
            .hover(|style| style.bg(rgb(0x00ff00)))
            .into_any_element()
    } else {
        row.into_any_element()
    }
}

/// The rows in a view of their own, for a page whose scroll container
/// holds a child view (pattern A).
pub(crate) struct Rows {
    pub(crate) tint: u32,
}

impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().children((0..ROWS).map(|index| {
            div()
                .h(px(ROW_HEIGHT))
                .bg(rgb(self.tint + index as u32 * 0x10))
        }))
    }
}

/// A page with a 100 px tall scroll container of forty 20 px rows at the
/// top left of the window: in the page's own view (pattern B), or in a
/// child view (pattern A), after `extra`, if any. The rows are as wide
/// as the container, or twice as wide when `wide` is set, which the
/// container, scrolling on y only, clips.
pub(crate) struct LayerPage {
    pub(crate) handle: ScrollHandle,
    pub(crate) rows: Option<Entity<Rows>>,
    pub(crate) hover: bool,
    pub(crate) read_offset_in_render: bool,
    pub(crate) extra: Option<Rc<dyn Fn() -> AnyElement>>,
    pub(crate) wide: bool,
    /// Whether its render asks for an animation frame.
    pub(crate) animate: bool,
    /// Whether it draws a scrollbar beside the scroll container, whose
    /// prepaint and paint read the offset, as GPUI Kit's does.
    pub(crate) scrollbar: bool,
    /// Whether the page's background is translucent, which no layer can
    /// bake into its tiles.
    pub(crate) translucent: bool,
}

impl Render for LayerPage {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if self.read_offset_in_render {
            let _ = self.handle.offset();
        }
        if self.animate {
            window.request_animation_frame();
        }
        let content_width = viewport().size.width.0 * if self.wide { 2. } else { 1. };
        let mut scroller = div()
            .id("scroller")
            .overflow_y_scroll()
            .track_scroll(&self.handle)
            .w(viewport().size.width)
            .h(viewport().size.height);
        if let Some(extra) = &self.extra {
            scroller = scroller.child(extra());
        }
        scroller = match &self.rows {
            Some(rows) => scroller.child(rows.clone()),
            None => scroller.children((0..ROWS).map(|index| row(index, self.hover, content_width))),
        };
        let background = if self.translucent {
            crate::rgba(0xffffff80)
        } else {
            rgb(0xffffff)
        };
        let page = div().size_full().bg(background).child(scroller);
        if !self.scrollbar {
            return page;
        }
        let prepaint_handle = self.handle.clone();
        let paint_handle = self.handle.clone();
        page.child(
            canvas(
                move |_, _, _| prepaint_handle.offset().y,
                move |bounds, _, window, _| {
                    let offset = paint_handle.offset().y;
                    let thumb = crate::Bounds::new(
                        point(bounds.origin.x, bounds.origin.y - offset / 8.),
                        size(px(6.), px(20.)),
                    );
                    window.paint_quad(crate::fill(thumb, rgb(0x888888)));
                },
            )
            .absolute()
            .left(viewport().size.width)
            .top(px(0.))
            .w(px(6.))
            .h(viewport().size.height),
        )
    }
}

pub(crate) fn new_page(child_view: bool, cx: &mut App) -> LayerPage {
    LayerPage {
        handle: ScrollHandle::new(),
        rows: child_view.then(|| cx.new(|_| Rows { tint: 0x100000 })),
        hover: false,
        read_offset_in_render: false,
        extra: None,
        wide: false,
        animate: false,
        scrollbar: false,
        translucent: false,
    }
}

pub(crate) fn page(cx: &mut TestAppContext, child_view: bool) -> WindowHandle<LayerPage> {
    let window = add_window(cx, move |_, cx| new_page(child_view, cx));
    draw(cx, window.into());
    draw(cx, window.into());
    window
}

pub(crate) fn scroller_id(cx: &mut TestAppContext, window: AnyWindowHandle) -> GlobalElementId {
    with_window(cx, window, |window, _| {
        window
            .fast_layers
            .scrolls
            .containers()
            .find(|id| id.last() == Some(&"scroller".into()))
            .cloned()
            .expect("the scroll container was painted")
    })
}

/// Draws the frame that follows `change`, unless the change drew one.
pub(crate) fn frame_after(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    change: impl FnOnce(&mut TestAppContext),
) {
    let frame = with_window(cx, window, |window, _| window.fast_layers.frame);
    change(cx);
    if with_window(cx, window, |window, _| window.fast_layers.frame) == frame {
        draw(cx, window);
    }
}

/// Scrolls by `dy` with the wheel, returning what the scroll container
/// decided in the frame that followed.
pub(crate) fn scroll(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    dy: f32,
) -> Option<Decision> {
    scroll_and(cx, window, dy, |_| {})
}

/// Scrolls by `dy` with the wheel after `change`, in one frame,
/// returning what the scroll container decided in it.
pub(crate) fn scroll_and(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    dy: f32,
    change: impl FnOnce(&mut App),
) -> Option<Decision> {
    scroll_at_and(cx, window, point(px(20.), px(20.)), dy, change)
}

/// Scrolls by `dy` with the wheel at `position`, returning what the scroll
/// container decided in the frame that followed.
pub(crate) fn scroll_at(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    position: crate::Point<Pixels>,
    dy: f32,
) -> Option<Decision> {
    scroll_at_and(cx, window, position, dy, |_| {})
}

fn scroll_at_and(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    position: crate::Point<Pixels>,
    dy: f32,
    change: impl FnOnce(&mut App),
) -> Option<Decision> {
    // In one update, for the change and the scroll to be taken in by the
    // same frame.
    frame_after(cx, window, |cx| {
        with_window(cx, window, |window, cx| {
            change(cx);
            window.dispatch_event(
                crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position,
                    delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                    modifiers: Default::default(),
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            );
        })
    });
    decision(cx, window)
}

/// What the scroll container decided in the last frame drawn, if it
/// decided anything in it.
pub(crate) fn decision(cx: &mut TestAppContext, window: AnyWindowHandle) -> Option<Decision> {
    let id = scroller_id(cx, window);
    with_window(cx, window, |window, _| last_decision(window, &id))
}

/// Scrolls until the container has a layer, checking it is promoted on
/// the second scrolled frame.
pub(crate) fn promote(cx: &mut TestAppContext, window: AnyWindowHandle) {
    assert_eq!(scroll(cx, window, -20.), Some(Decision::Bypass));
    assert_eq!(scroll(cx, window, -20.), Some(Decision::Repaint));
}

/// Layers mode: every mechanism scroll layers build on, and scroll layers.
pub(crate) const LAYERS: crate::FastMode = crate::FastMode {
    splice: false,
    ..crate::FastMode::ALL
};

/// Opens a window in [`LAYERS`] mode, drawn only when the test draws it.
pub(crate) fn add_window<V: Render + 'static>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> WindowHandle<V> {
    let window = cx.add_window(build);
    with_window(cx, window.into(), |window, _| window.set_fast_mode(LAYERS));
    // Every frame is drawn by the test, so that the frame a decision is read
    // from is the first one after a change, as an application's next frame
    // is, and not one a notification sent while it was drawn asks for.
    cx.set_draw_on_flush(false);
    window
}

pub(crate) fn with_window<R>(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    cx.update_window(window, |_, window, cx| f(window, cx))
        .unwrap()
}

pub(crate) fn draw(cx: &mut TestAppContext, window: AnyWindowHandle) {
    with_window(cx, window, |window, cx| window.draw(cx).clear(cx));
}

/// An input handler that answers nothing, named for its test.
struct NamedInput(&'static str);

impl crate::InputHandler for NamedInput {
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut crate::App,
    ) -> Option<crate::UTF16Selection> {
        None
    }
    fn marked_text_range(
        &mut self,
        _: &mut Window,
        _: &mut crate::App,
    ) -> Option<std::ops::Range<usize>> {
        None
    }
    fn text_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut crate::App,
    ) -> Option<String> {
        Some(self.0.to_string())
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: &mut Window,
        _: &mut crate::App,
    ) {
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut crate::App,
    ) {
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut crate::App) {}
    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Window,
        _: &mut crate::App,
    ) -> Option<crate::Bounds<crate::Pixels>> {
        None
    }
    fn character_index_for_point(
        &mut self,
        _: crate::Point<crate::Pixels>,
        _: &mut Window,
        _: &mut crate::App,
    ) -> Option<usize> {
        None
    }
}

/// An element that handles text input while `focus` is focused.
pub(crate) fn text_input(focus: crate::FocusHandle, name: &'static str) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, cx| window.handle_input(&focus, NamedInput(name), cx),
    )
    .size_full()
}

/// Switches scroll layers on or off in `window`, the rest of [`LAYERS`] on.
pub(crate) fn set_layers(window: &mut Window, on: bool) {
    window.set_fast_mode(crate::FastMode {
        scroll_layers: on,
        ..LAYERS
    });
}

/// Scrolls `handle` to `offset` as an application does: the request is
/// applied when the container is next prepainted.
pub(crate) fn set_offset(handle: &ScrollHandle, offset: crate::Point<Pixels>) {
    handle.request_offset(offset, crate::ScrollMutationSource::Programmatic);
}

pub(crate) struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        crate::Empty
    }
}

/// `color` as a quad's background is painted, at full opacity.
pub(crate) fn bg(color: impl Into<crate::Background>) -> crate::PreparedBackground {
    color.into().prepare_with_opacity(1.)
}

/// `color` as it is painted, at full opacity.
pub(crate) fn paint_color(color: impl Into<crate::Color>) -> crate::PaintColor {
    crate::color::prepare_color_with_opacity(&color.into(), 1.)
}
