//! Frames drawn through scroll layers must match frames drawn without them.
//!
//! Each run drives two windows holding the same view through the same random
//! history: wheel scrolls with fractional deltas, programmatic scrolls,
//! pointer moves, clicks, key presses into a focused element inside scrolled
//! content, content changes and resizes, at scale factors 1 and 1.25. One
//! window has scroll layers on, the other off. After every step:
//!
//! - the two frames must draw the same thing. The layer window's scene is
//!   expanded first: the tile quads of each composited layer are replaced by
//!   the layer's content, moved to where the tiles put it and clipped to
//!   their viewport, and every piece of content must lie under a composited
//!   tile. Both scenes are then compared primitive by primitive in a
//!   canonical drawing order (see `fast::layers::verify`);
//! - hit tests at random points must find the same hitboxes;
//! - every listener must have seen the same events at the same positions,
//!   with the same hitbox bounds.
//!
//! Where layers are compiled in, the runs must composite layers; elsewhere
//! the two windows draw the same way and the run checks only that the
//! oracle itself holds.

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use crate::fast::layers::verify::drawn;
use crate::fast::tests::layer_support::{add_window, make_reference, set_layers};
use crate::{
    AnyElement, AppContext as _, Bounds, ContentMask, Context, DispatchPhase, Div, Entity,
    FocusHandle, HitboxBehavior, Hsla, InputEvent as _, InteractiveElement as _, IntoElement,
    KeyDownEvent, LayerFrame, LayerKey, ListAlignment, ListOffset, ListState, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels,
    PlatformInput, Point, PolychromeSprite, Quad, Render, ScaledPixels, Scene, ScrollDelta,
    ScrollHandle, ScrollStrategy, ScrollWheelEvent, SharedString, StatefulInteractiveElement as _,
    Styled as _, TestAppContext, TileCoord, TouchPhase, UniformListScrollHandle, Window,
    WindowHandle, canvas, div, hsla, layer_tile_id, layer_tile_texture_id, list, point, px, rgba,
    size, uniform_list, util::FluentBuilder as _, white,
};

fn sp(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(width), ScaledPixels(height)),
    }
}

fn quad(bounds: Bounds<ScaledPixels>, mask: Bounds<ScaledPixels>, hue: f32) -> Quad {
    Quad {
        bounds,
        content_mask: ContentMask::rectangular(mask),
        background: crate::fast::tests::layer_support::bg(hsla(hue, 0.5, 0.5, 1.)),
        ..Default::default()
    }
}

const WINDOW: Bounds<ScaledPixels> = Bounds {
    origin: crate::Point {
        x: ScaledPixels(0.),
        y: ScaledPixels(0.),
    },
    size: crate::Size {
        width: ScaledPixels(1000.),
        height: ScaledPixels(1000.),
    },
};

/// A scroll container whose viewport is `(50, 50) – (250, 150)`, scrolled
/// 400 px down since its content was painted, so that it shows the bottom of
/// tile (0, 0) and the top of tile (0, 1), over a background, with a bar
/// painted over its bottom edge afterwards: once drawn directly, and once
/// through a layer holding the content with `tiles` of it composited. `nudge`
/// moves a visible row of the layer's content.
fn scenes(tiles: &[TileCoord], nudge: f32) -> (Scene, Scene) {
    let viewport = sp(50., 50., 200., 100.);
    let translation = point(ScaledPixels(0.), ScaledPixels(-400.));
    let rows = || (0..20).map(|i| sp(60., 60. + i as f32 * 30., 100., 20.));

    let mut direct = Scene::default();
    direct.insert_primitive(quad(WINDOW, WINDOW, 0.));
    for (i, row) in rows().enumerate() {
        let row = Bounds {
            origin: row.origin + translation,
            size: row.size,
        };
        direct.insert_primitive(quad(row, viewport, 0.1 + i as f32 * 0.01));
    }
    direct.insert_primitive(quad(sp(40., 140., 300., 20.), WINDOW, 0.9));
    direct.finish();

    let painted_region = sp(50., 50., 200., 800.);
    let mut content = Scene::default();
    for (i, row) in rows().enumerate() {
        let row = if i == 14 {
            Bounds {
                origin: point(row.origin.x + ScaledPixels(nudge), row.origin.y),
                size: row.size,
            }
        } else {
            row
        };
        content.insert_primitive(quad(row, painted_region, 0.1 + i as f32 * 0.01));
    }
    content.finish();
    let frame = LayerFrame {
        key: LayerKey(3),
        generation: 1,
        background: crate::fast::tests::layer_support::paint_color(rgba(0xffffffff)),
        tile_size: 512,
        content: content.into(),
        dirty_tiles: Vec::new(),
    };

    let mut layered = Scene::default();
    layered.insert_primitive(quad(WINDOW, WINDOW, 0.));
    layered.push_layer(viewport);
    for &tile in tiles {
        let bounds = frame.tile_bounds(tile);
        layered.insert_primitive(PolychromeSprite {
            transformation: crate::TransformationMatrix::unit(),
            order: 0,
            pad: 0,
            grayscale: false,
            opacity: 1.,
            bounds: Bounds {
                origin: bounds.origin + translation,
                size: bounds.size,
            },
            content_mask: ContentMask::rectangular(viewport),
            corner_radii: Default::default(),
            tile: crate::AtlasTile {
                texture_id: layer_tile_texture_id(frame.key),
                tile_id: layer_tile_id(tile),
                padding: 0,
                bounds: Bounds {
                    origin: point(crate::DevicePixels(0), crate::DevicePixels(0)),
                    size: size(crate::DevicePixels(512), crate::DevicePixels(512)),
                },
            },
        });
    }
    layered.pop_layer();
    layered.insert_primitive(quad(sp(40., 140., 300., 20.), WINDOW, 0.9));
    layered.layers.frames.push(frame);
    layered.finish();
    (direct, layered)
}

#[test]
fn a_composited_layer_expands_to_the_scene_drawn_directly() {
    let (direct, layered) = scenes(&[TileCoord { x: 0, y: 0 }, TileCoord { x: 0, y: 1 }], 0.);
    assert_eq!(drawn(&layered), drawn(&direct));
    assert!(
        drawn(&direct).len() > 3,
        "rows in the viewport are compared"
    );
}

#[test]
fn a_layer_that_moved_a_row_does_not_match() {
    let (direct, layered) = scenes(&[TileCoord { x: 0, y: 0 }, TileCoord { x: 0, y: 1 }], 1.);
    assert_ne!(drawn(&layered), drawn(&direct));
}

#[test]
#[should_panic(expected = "no tile composited")]
fn content_under_a_tile_that_was_not_composited_is_caught() {
    let (_, layered) = scenes(&[TileCoord { x: 0, y: 1 }], 0.);
    drawn(&layered);
}

const WORDS: [&str; 6] = ["a", "row", "scrolled", "lorem ipsum", "x", "a longer label"];

const PALETTE: [Hsla; 5] = [
    hsla(0.0, 0.0, 0.1, 1.0),
    hsla(0.6, 0.7, 0.5, 1.0),
    hsla(0.3, 0.6, 0.4, 1.0),
    hsla(0.0, 0.8, 0.6, 1.0),
    hsla(0.1, 0.9, 0.5, 0.5),
];

const CONTAINER_WIDTH: f32 = 220.;
const CONTAINER_HEIGHT: f32 = 200.;
const CONTAINER_TOP: f32 = 30.;
const INITIAL_ROWS: u64 = 60;
const UNIFORM_ROW_HEIGHT: f32 = 22.;
/// The strip the scrolling `div`s leave beside their rows.
const GUTTER: f32 = 24.;
/// Wheel deltas, as trackpads (fractions of a pixel) and wheels send them.
const WHEEL_DELTAS: [f32; 5] = [0.37, 1.0, 12.5, 40., 90.];

fn container_left(container: usize) -> f32 {
    10. + container as f32 * (CONTAINER_WIDTH + 10.)
}

/// What every listener in the view saw, in order.
type Log = Rc<RefCell<Vec<String>>>;

#[derive(Clone, Copy, Debug)]
struct Row {
    id: u64,
    color: usize,
    word: usize,
}

struct Rows {
    rows: Vec<Row>,
    next_id: u64,
}

/// A row of scrolled content: text, a swatch, some with a hover style, a
/// click listener and a probe recording where the mouse went down in it.
fn render_row(
    container: &'static str,
    row: Row,
    tint: usize,
    log: &Log,
    fixed: bool,
) -> AnyElement {
    let label = SharedString::from(format!("{container}/{}", row.id));
    div()
        .id(label.clone())
        .relative()
        .flex()
        .flex_row()
        .gap_1()
        // Glyphs stay inside their line and the line inside the row. A glyph
        // reaching past its row is drawn from a layer's overscan row where a
        // list leaves the row out, and one reaching past its line is drawn
        // at the line's ordering: both differ from drawing from scratch in
        // ways the rows are not here to test.
        .text_size(px(8.))
        .line_height(px(12.))
        .h(px(if fixed {
            UNIFORM_ROW_HEIGHT
        } else {
            18. + (row.id % 3) as f32 * 7.
        }))
        .when(row.id.is_multiple_of(2), |this| {
            this.bg(PALETTE[(row.color + tint) % PALETTE.len()])
        })
        .when(row.id.is_multiple_of(3), |this| {
            this.hover(|style| style.bg(PALETTE[4]))
        })
        .child(SharedString::from(format!(
            "{} {}",
            WORDS[row.word], row.id
        )))
        .child(
            div()
                .w(px(10. + (row.id % 4) as f32 * 5.))
                .h(px(8.))
                .bg(PALETTE[row.id as usize % PALETTE.len()]),
        )
        .on_click({
            let log = log.clone();
            let label = label.clone();
            move |event, _, _| {
                log.borrow_mut()
                    .push(format!("click {label} at {:?}", event.position()))
            }
        })
        .child(probe(label, log.clone()))
        .into_any_element()
}

/// Records, for every mouse down over it, where it happened and the bounds
/// of the hitbox its listener was given.
fn probe(label: SharedString, log: Log) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    log.borrow_mut().push(format!(
                        "down {label} at {:?} in {:?}",
                        event.position, hitbox.bounds
                    ));
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Content of the first container: a child view (pattern A) holding a
/// focusable field and the rows.
/// A model the first container's content view reads as it renders and does
/// not observe.
struct Shade(usize);

struct ContentView {
    rows: Entity<Rows>,
    shade: Entity<Shade>,
    log: Log,
    focus: FocusHandle,
    /// Where the field was last prepainted, which its key listener reports.
    field_bounds: Rc<Cell<Bounds<Pixels>>>,
    tint: usize,
}

impl Render for ContentView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let log = self.log.clone();
        let shade = self.shade.read(cx).0;
        let field_bounds = self.field_bounds.clone();
        let field = div()
            .id("field")
            .relative()
            .h(px(20.))
            .bg(PALETTE[1])
            .track_focus(&self.focus)
            .on_key_down({
                let field_bounds = field_bounds.clone();
                move |event: &KeyDownEvent, _, _| {
                    log.borrow_mut().push(format!(
                        "key {} in {:?}",
                        event.keystroke.key,
                        field_bounds.get()
                    ))
                }
            })
            .child(
                canvas(
                    move |bounds, _, _| field_bounds.set(bounds),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        div().flex().flex_col().child(field).children(
            self.rows
                .read(cx)
                .rows
                .iter()
                .map(|&row| render_row("a", row, self.tint + shade, &self.log, false)),
        )
    }
}

/// Four scroll containers side by side on opaque panels, one per pattern:
/// a scrolling div holding a child view, a scrolling div holding the rows
/// directly, a uniform list and a list, with a bar painted over the bottom of
/// the first one afterwards.
struct LayerOracleView {
    rows: Entity<Rows>,
    content: Entity<ContentView>,
    shade: Entity<Shade>,
    /// Tints the second container's rows; changed without a notification.
    stripe: usize,
    scroll_a: ScrollHandle,
    scroll_b: ScrollHandle,
    uniform: UniformListScrollHandle,
    list: ListState,
    log: Log,
}

impl LayerOracleView {
    fn new(cx: &mut Context<Self>) -> Self {
        let rows = cx.new(|_| Rows {
            rows: (0..INITIAL_ROWS)
                .map(|id| Row {
                    id,
                    color: id as usize % PALETTE.len(),
                    word: id as usize % WORDS.len(),
                })
                .collect(),
            next_id: INITIAL_ROWS,
        });
        let log = Log::default();
        let shade = cx.new(|_| Shade(0));
        let content = cx.new(|cx| ContentView {
            rows: rows.clone(),
            shade: shade.clone(),
            log: log.clone(),
            focus: cx.focus_handle(),
            field_bounds: Rc::default(),
            tint: 0,
        });
        Self {
            rows,
            content,
            shade,
            stripe: 0,
            scroll_a: ScrollHandle::new(),
            scroll_b: ScrollHandle::new(),
            uniform: UniformListScrollHandle::new(),
            list: ListState::new(INITIAL_ROWS as usize, ListAlignment::Top, px(40.)),
            log,
        }
    }
}

fn panel(container: usize) -> Div {
    div()
        .absolute()
        .left(px(container_left(container)))
        .top(px(CONTAINER_TOP))
        .w(px(CONTAINER_WIDTH))
        .h(px(CONTAINER_HEIGHT))
        .bg(white())
}

impl Render for LayerOracleView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.read(cx).rows.clone();
        let log = self.log.clone();
        div()
            .size_full()
            .relative()
            .bg(hsla(0.6, 0.1, 0.9, 1.))
            .text_color(PALETTE[0])
            .child(
                panel(0).child(
                    div()
                        .id("a")
                        .size_full()
                        .pr(px(GUTTER))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll_a)
                        .child(self.content.clone()),
                ),
            )
            .child(
                panel(1).child(
                    div()
                        .id("b")
                        .size_full()
                        .pr(px(GUTTER))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll_b)
                        .child(
                            div().flex().flex_col().children(
                                rows.iter()
                                    .map(|&row| render_row("b", row, self.stripe, &log, false)),
                            ),
                        ),
                ),
            )
            .child(
                panel(2).child(
                    uniform_list("u", rows.len(), {
                        let rows = rows.clone();
                        let log = log.clone();
                        move |range, _, _| {
                            range
                                .map(|ix| render_row("u", rows[ix], 0, &log, true))
                                .collect()
                        }
                    })
                    .track_scroll(&self.uniform)
                    .size_full(),
                ),
            )
            .child(
                panel(3).child(
                    list(self.list.clone(), move |ix, _, _| {
                        render_row("l", rows[ix], 0, &log, false)
                    })
                    .size_full(),
                ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(container_left(0) - 4.))
                    .top(px(CONTAINER_TOP + CONTAINER_HEIGHT - 8.))
                    .w(px(CONTAINER_WIDTH + 40.))
                    .h(px(14.))
                    .bg(PALETTE[2]),
            )
    }
}

#[derive(Clone, Debug)]
enum Change {
    /// A wheel turned over a container, `y` below its top: over its rows,
    /// or with `gutter` over the strip its scrolling `div` leaves beside
    /// them, where no row's hover changes as the rows move.
    Wheel {
        container: usize,
        y: f32,
        delta: f32,
        gutter: bool,
    },
    /// A scroll by the application, to `top`.
    Scroll {
        container: usize,
        top: f32,
    },
    MoveMouse {
        x: f32,
        y: f32,
    },
    Click {
        x: f32,
        y: f32,
    },
    /// A click where the pointer is, without moving it first.
    ClickInPlace,
    /// Focuses the field inside the first container's content.
    Focus,
    Key {
        key: &'static str,
    },
    Recolor {
        row: usize,
        color: usize,
    },
    Reword {
        row: usize,
        word: usize,
    },
    /// Changes the first container's content view alone.
    Tint,
    InsertRow {
        at: usize,
    },
    RemoveRow {
        at: usize,
    },
    Resize {
        width: f32,
        height: f32,
    },
    /// Changes a model the first container's content view reads without
    /// observing it, notifying the model or not: nothing notifies the view.
    Shade {
        notify: bool,
    },
    /// Changes what the view holding the containers renders in the second
    /// one, without notifying it.
    Stripe,
    Redraw,
}

impl Change {
    /// A random change, wheels and application scrolls going to one of
    /// `containers`.
    fn random(rng: &mut StdRng, containers: &Range<usize>) -> Self {
        let container = rng.random_range(containers.clone());
        match rng.random_range(0..100) {
            0..30 => Change::Wheel {
                container,
                y: rng.random_range(2.0..CONTAINER_HEIGHT - 2.),
                delta: WHEEL_DELTAS[rng.random_range(0..WHEEL_DELTAS.len())]
                    * if rng.random_bool(0.6) { -1. } else { 1. },
                gutter: rng.random_bool(0.5),
            },
            30..36 => Change::Scroll {
                container,
                top: rng.random_range(0.0..900.0),
            },
            36..46 => Change::MoveMouse {
                x: rng.random_range(0.0..960.0),
                y: rng.random_range(0.0..260.0),
            },
            46..54 => Change::Click {
                x: rng.random_range(0.0..960.0),
                y: rng.random_range(0.0..260.0),
            },
            54..60 => Change::ClickInPlace,
            60..63 => Change::Focus,
            63..68 => Change::Key {
                key: ["a", "b", "space", "enter"][rng.random_range(0..4)],
            },
            68..75 => Change::Recolor {
                row: rng.random_range(0..80),
                color: rng.random_range(0..PALETTE.len()),
            },
            75..80 => Change::Reword {
                row: rng.random_range(0..80),
                word: rng.random_range(0..WORDS.len()),
            },
            80..85 => Change::Tint,
            85..89 => Change::InsertRow {
                at: rng.random_range(0..80),
            },
            89..93 => Change::RemoveRow {
                at: rng.random_range(0..80),
            },
            93..95 => Change::Resize {
                width: rng.random_range(500.0..1000.0),
                height: rng.random_range(200.0..700.0),
            },
            95..97 => Change::Shade {
                notify: rng.random_bool(0.5),
            },
            97..99 => Change::Stripe,
            _ => Change::Redraw,
        }
    }
}

fn dispatch(cx: &mut TestAppContext, window: WindowHandle<LayerOracleView>, event: PlatformInput) {
    cx.update_window(window.into(), |_, window, cx| {
        window.dispatch_event(event, cx);
    })
    .unwrap();
}

fn apply(cx: &mut TestAppContext, window: WindowHandle<LayerOracleView>, change: &Change) {
    match *change {
        Change::Wheel {
            container,
            y,
            delta,
            gutter,
        } => dispatch(
            cx,
            window,
            ScrollWheelEvent {
                position: point(
                    px(container_left(container)
                        + if gutter {
                            CONTAINER_WIDTH - GUTTER / 2.
                        } else {
                            CONTAINER_WIDTH / 2.
                        }),
                    px(CONTAINER_TOP + y),
                ),
                delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }
            .to_platform_input(),
        ),
        Change::Scroll { container, top } => {
            window
                .update(cx, |view, _, cx| {
                    let top = px(top);
                    match container {
                        0 => crate::fast::tests::layer_support::set_offset(
                            &view.scroll_a,
                            point(px(0.), -top),
                        ),
                        1 => crate::fast::tests::layer_support::set_offset(
                            &view.scroll_b,
                            point(px(0.), -top),
                        ),
                        2 => view.uniform.scroll_to_item(
                            (top / px(UNIFORM_ROW_HEIGHT)) as usize,
                            ScrollStrategy::Top,
                        ),
                        _ => view.list.scroll_to(ListOffset {
                            item_ix: (top / px(25.)) as usize,
                            offset_in_item: px(top.as_f32() % 11.),
                        }),
                    }
                    cx.notify();
                })
                .unwrap();
        }
        Change::MoveMouse { x, y } => dispatch(
            cx,
            window,
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
        ),
        // The two halves of a click are applied one at a time, by `run`.
        Change::Click { .. } | Change::ClickInPlace => {}
        Change::Focus => {
            window
                .update(cx, |view, window, cx| {
                    let focus = view.content.read(cx).focus.clone();
                    window.focus(&focus, cx);
                })
                .unwrap();
        }
        Change::Key { key } => cx.simulate_keystrokes(window.into(), key),
        Change::Recolor { .. }
        | Change::Reword { .. }
        | Change::InsertRow { .. }
        | Change::RemoveRow { .. } => {
            window
                .update(cx, |view, _, cx| {
                    let list = view.list.clone();
                    view.rows
                        .update(cx, |rows, cx| change_rows(rows, &list, change, cx));
                    view.content.update(cx, |_, cx| cx.notify());
                    cx.notify();
                })
                .unwrap();
        }
        Change::Tint => {
            let content = window
                .read_with(cx, |view, _| view.content.clone())
                .unwrap();
            content.update(cx, |content, cx| {
                content.tint += 1;
                cx.notify();
            });
        }
        Change::Resize { width, height } => {
            cx.simulate_window_resize(window.into(), size(px(width), px(height)));
        }
        Change::Shade { notify } => {
            let shade = window.read_with(cx, |view, _| view.shade.clone()).unwrap();
            shade.update(cx, |shade, cx| {
                shade.0 += 1;
                if notify {
                    cx.notify();
                }
            });
        }
        Change::Stripe => {
            window.update(cx, |view, _, _| view.stripe += 1).unwrap();
        }
        Change::Redraw => {}
    }
}

fn change_rows(rows: &mut Rows, list: &ListState, change: &Change, cx: &mut Context<Rows>) {
    let len = rows.rows.len();
    match *change {
        Change::Recolor { row, color } if len > 0 => rows.rows[row % len].color = color,
        Change::Reword { row, word } if len > 0 => rows.rows[row % len].word = word,
        Change::InsertRow { at } => {
            let at = at % (len + 1);
            let id = rows.next_id;
            rows.next_id += 1;
            rows.rows.insert(
                at,
                Row {
                    id,
                    color: id as usize % PALETTE.len(),
                    word: id as usize % WORDS.len(),
                },
            );
            list.splice(at..at, 1);
        }
        Change::RemoveRow { at } if len > 0 => {
            let at = at % len;
            rows.rows.remove(at);
            list.splice(at..at + 1, 0);
        }
        _ => {}
    }
    cx.notify();
}

/// One half of a click at `position`, or where the pointer is: the press,
/// or the release.
fn press_or_release(
    cx: &mut TestAppContext,
    window: WindowHandle<LayerOracleView>,
    position: Option<Point<Pixels>>,
    press: bool,
) {
    let position = position.unwrap_or_else(|| {
        cx.update_window(window.into(), |_, window, _| window.mouse_position())
            .unwrap()
    });
    let event = if press {
        MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input()
    } else {
        MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }
        .to_platform_input()
    };
    dispatch(cx, window, event);
}

/// What a window drew, what `points` hit in it, and what its listeners saw
/// since the last call.
struct Observed {
    drawn: Vec<String>,
    hits: Vec<String>,
    log: Vec<String>,
}

fn draw(
    cx: &mut TestAppContext,
    window: WindowHandle<LayerOracleView>,
    points: &[Point<Pixels>],
) -> Observed {
    let log = window.read_with(cx, |view, _| view.log.take()).unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let frame = &window.rendered_frame;
        let hits = points
            .iter()
            .map(|&position| {
                let hit = frame.hit_test(position);
                let hitboxes: Vec<String> = hit
                    .ids
                    .iter()
                    .map(|id| {
                        let hitbox = frame
                            .hitboxes
                            .iter()
                            .find(|hitbox| hitbox.id == *id)
                            .unwrap();
                        // To a thousandth of a pixel: a row of a list's layer
                        // keeps the hitboxes it inserted, moved, and a bound
                        // laid out and moved can differ in its last bits
                        // from one laid out where it lies now.
                        let bounds = hitbox.bounds.intersect(&hitbox.content_mask.bounds);
                        format!(
                            "({:.3}, {:.3}) {:.3} × {:.3} {:?}",
                            bounds.origin.x.as_f32(),
                            bounds.origin.y.as_f32(),
                            bounds.size.width.as_f32(),
                            bounds.size.height.as_f32(),
                            hitbox.behavior
                        )
                    })
                    .collect();
                format!(
                    "{position:?}: {hitboxes:?}, hover {}",
                    hit.hover_hitbox_count
                )
            })
            .collect();
        Observed {
            drawn: drawn(&frame.scene),
            hits,
            log,
        }
    })
    .unwrap()
}

fn first_difference(actual: &[String], expected: &[String]) -> String {
    let first = actual
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected)
        .unwrap_or(actual.len().min(expected.len()));
    // Where the first differing lines part, with what leads up to it.
    let excerpt = |line: Option<&String>, at: usize| {
        line.map_or(String::from("(no line)"), |line| {
            let from = line.floor_char_boundary(at.saturating_sub(160));
            let to = line.floor_char_boundary((at + 120).min(line.len()));
            line[from..to].to_string()
        })
    };
    let (a, b) = (actual.get(first), expected.get(first));
    let at = match (a, b) {
        (Some(a), Some(b)) => a
            .bytes()
            .zip(b.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(a.len().min(b.len())),
        _ => 0,
    };
    format!(
        "at line {first} ({} lines against {}), column {at}\nwith layers:\n  …{}\nwithout:\n  …{}",
        actual.len(),
        expected.len(),
        excerpt(a, at),
        excerpt(b, at)
    )
}

/// What a run went through, to tell that it compared something.
#[derive(Debug, Default)]
struct Coverage {
    /// Frames of the layer window that composited a scroll layer.
    composited: u64,
    /// Events listeners saw, by kind: clicks, mouse downs, key presses.
    clicks: usize,
    downs: usize,
    keys: usize,
    /// Frames drawn with some container scrolled.
    scrolled: usize,
    /// Inputs dispatched after a frame painting a layer where it shows.
    frames_before_input: usize,
}

/// Drives a window with layers and one without through one random history
/// at `scale_factor`, scrolling `containers`, a wheel turning on for
/// another frame with probability `keep_turning`.
fn run(
    seed: u64,
    scale_factor: f32,
    steps: usize,
    containers: Range<usize>,
    keep_turning: f64,
    keep_layers: bool,
) -> Coverage {
    let mut cx = crate::fast::tests::oracle_support::oracle_app(seed);
    let layered = add_window(&mut cx, |_, cx| LayerOracleView::new(cx));
    let plain = add_window(&mut cx, |_, cx| LayerOracleView::new(cx));
    for (window, layers) in [(layered, true), (plain, false)] {
        cx.simulate_window_scale_factor_change(window.into(), scale_factor);
        cx.update_window(window.into(), |_, window, cx| {
            if layers {
                set_layers(window, true);
                window.fast_layers.never_demote = keep_layers;
            } else {
                // The reference draws every frame from scratch, and snaps
                // what layers snap.
                make_reference(window);
            }
            window.draw(cx).clear(cx);
            window.reset_fast_stats();
        })
        .unwrap();
    }
    let mut rng = StdRng::seed_from_u64(seed);
    let mut history: Vec<Vec<Change>> = Vec::new();
    let mut last_wheel: Option<Change> = None;
    let mut coverage = Coverage::default();

    for step in 0..steps {
        // A wheel turns for several frames in a row, which is what promotes
        // a container to a layer.
        let changes: Vec<Change> = match &last_wheel {
            Some(wheel) if rng.random_bool(keep_turning) => vec![wheel.clone()],
            _ => (0..rng.random_range(1..=2))
                .map(|_| Change::random(&mut rng, &containers))
                .collect(),
        };
        last_wheel = changes
            .iter()
            .find(|change| matches!(change, Change::Wheel { .. }))
            .cloned();
        let points: Vec<Point<Pixels>> = (0..32)
            .map(|_| {
                point(
                    px(rng.random_range(0.0..960.0)),
                    px(rng.random_range(0.0..260.0)),
                )
            })
            .collect();
        for change in &changes {
            // Input other than a wheel that reaches content shown away from
            // where it was painted is dispatched after a frame painting it
            // where it shows, and that frame draws whatever else was pending
            // (an earlier change of the same step) before the input, where
            // without layers the input comes first. That is the one
            // difference layers make to when things happen: the reference
            // draws its pending frame before the same event. A click is two
            // events, each of which may be the one.
            let mut in_step = |cx: &mut TestAppContext,
                               apply: &dyn Fn(
                &mut TestAppContext,
                WindowHandle<LayerOracleView>,
            )| {
                let rebuilds = |cx: &mut TestAppContext| {
                    cx.update_window(layered.into(), |_, window, _| {
                        window.fast_layers.input_rebuilds
                    })
                    .unwrap()
                };
                let before = rebuilds(cx);
                apply(cx, layered);
                if rebuilds(cx) > before {
                    coverage.frames_before_input += 1;
                    cx.update_window(plain.into(), |_, window, cx| window.draw(cx).clear(cx))
                        .unwrap();
                }
                apply(cx, plain);
            };
            match *change {
                Change::Click { x, y } => {
                    let position = Some(point(px(x), px(y)));
                    for press in [true, false] {
                        in_step(&mut cx, &|cx, window| {
                            press_or_release(cx, window, position, press)
                        });
                    }
                }
                Change::ClickInPlace => {
                    for press in [true, false] {
                        in_step(&mut cx, &|cx, window| {
                            press_or_release(cx, window, None, press)
                        });
                    }
                }
                _ => in_step(&mut cx, &|cx, window| apply(cx, window, change)),
            }
        }
        history.push(changes);

        let expected = draw(&mut cx, plain, &points);
        let actual = draw(&mut cx, layered, &points);
        let failure = if actual.log != expected.log {
            Some(format!(
                "listeners saw different events {}",
                first_difference(&actual.log, &expected.log)
            ))
        } else if actual.drawn != expected.drawn {
            Some(format!(
                "the frames differ {}",
                first_difference(&actual.drawn, &expected.drawn)
            ))
        } else if actual.hits != expected.hits {
            Some(format!(
                "hit tests differ {}",
                first_difference(&actual.hits, &expected.hits)
            ))
        } else {
            None
        };
        if let Some(failure) = failure {
            let history = history
                .iter()
                .enumerate()
                .map(|(step, changes)| format!("  {step}: {changes:?}"))
                .collect::<Vec<_>>()
                .join("\n");
            panic!(
                "seed {seed}, scale {scale_factor}, step {step}: {failure}\n\
                 changes so far:\n{history}"
            );
        }
        let count = |kind: &str| {
            expected
                .log
                .iter()
                .filter(|line| line.starts_with(kind))
                .count()
        };
        coverage.clicks += count("click ");
        coverage.downs += count("down ");
        coverage.keys += count("key ");
        coverage.scrolled += plain
            .read_with(&cx, |view, _| {
                view.scroll_a.offset().y != px(0.)
                    || view.scroll_b.offset().y != px(0.)
                    || view.uniform.0.borrow().base_handle.offset().y != px(0.)
                    || view.list.logical_scroll_top().item_ix > 0
            })
            .unwrap() as usize;
    }
    coverage.composited = cx
        .update_window(layered.into(), |_, window, _| {
            window.fast_stats().layers_composited
        })
        .unwrap();
    coverage
}

/// The containers the runs scroll: the two scrolling divs. The lists beside
/// them get their layers, and their own runs, with list layers.
const DIVS: Range<usize> = 0..2;

/// Runs random histories at `scale_factor`, as [`run`] does, and adds up
/// what they went through. `SEED` and `ITERATIONS` pick the seeds.
///
/// The histories change the content every few frames, which the policy
/// answers by demoting the layers for most of a run. So each seed runs
/// twice: with the policy as it is, and with the layers kept whatever they
/// cost (`keep_layers`), where every frame that only scrolled composites.
fn run_all(scale_factor: f32, containers: Range<usize>, keep_turning: f64) -> [Coverage; 2] {
    let (seeds, steps) = crate::fast::tests::oracle_support::seeds_and_steps(3, 300);
    let mut totals = [Coverage::default(), Coverage::default()];
    for seed in seeds {
        for (total, keep_layers) in totals.iter_mut().zip([false, true]) {
            let coverage = run(
                seed,
                scale_factor,
                steps,
                containers.clone(),
                keep_turning,
                keep_layers,
            );
            total.composited += coverage.composited;
            total.clicks += coverage.clicks;
            total.downs += coverage.downs;
            total.keys += coverage.keys;
            total.scrolled += coverage.scrolled;
            total.frames_before_input += coverage.frames_before_input;
        }
    }
    totals
}

fn frames_match_at(scale_factor: f32) {
    let [policy, kept] = run_all(scale_factor, DIVS, 0.7);
    for total in [&policy, &kept] {
        assert!(
            total.clicks > 0 && total.downs > 0 && total.keys > 0,
            "no listener saw a click, a mouse down and a key press, so their positions \
             were not compared: {total:?}"
        );
        assert!(total.scrolled > 0, "nothing ever scrolled");
    }
    // Where scroll layers are compiled in, the runs must have composited
    // layers, or they compared nothing but drawing from scratch with itself.
    if crate::fast::layers::COMPILED {
        assert!(policy.composited > 0, "{policy:?}");
        assert!(
            kept.frames_before_input > 0,
            "no input reached content that had moved since it was painted: {kept:?}"
        );
        assert!(
            kept.composited * 10 >= kept.scrolled as u64,
            "too few frames composited a scroll layer: {kept:?}"
        );
    }
}

#[test]
fn frames_drawn_through_scroll_layers_match_frames_drawn_without() {
    frames_match_at(1.);
}

/// At a fractional scale factor, where scroll offsets, glyph origins and
/// paths land between device pixels unless they are snapped.
#[test]
fn frames_drawn_through_scroll_layers_match_at_a_fractional_scale_factor() {
    frames_match_at(1.25);
}
