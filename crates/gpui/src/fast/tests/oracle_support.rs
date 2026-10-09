//! The randomized oracle every fast-mode mechanism is checked against.
//!
//! Each run drives two applications holding the same [`OracleView`] through
//! the same seeded history of [`Change`]s. One window draws every frame the
//! way an application does, reusing whatever its mode lets it reuse. The
//! other forgets everything it retains and refreshes before every frame, so
//! it draws each frame as its first. After every step the two frames must
//! describe alike ([`crate::fast::verify::describe_frame`]); any difference is
//! something a retained shortcut got wrong.
//!
//! The windows live in separate applications built from the same seed, so
//! entities, focus handles and element ids number alike and the frame
//! description can name them directly.
//!
//! Adapted from gpui-fast's `fast/tests/oracle.rs`. gpux's view covers its
//! own features too: fixed and sticky positioning, top-layer chrome,
//! unclipped deferred overlays, frame effects, editable text, drag and drop,
//! focus and keys, globals and their presence, and scale changes.

use std::{borrow::Cow, sync::Arc, time::Duration};

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use crate::{
    AnyElement, App, AppContext as _, Bounds, Context, DevicePixels, EditableText,
    EditableTextState, Entity, FastMode, FastStats, FocusHandle, Font, FontId, FontMetrics,
    FontRun, FrameTransition, Global, GlyphId, Hitbox, HitboxBehavior, Hsla, InputEvent as _,
    KeyDownEvent, Keystroke, LineLayout, ListAlignment, ListOffset, ListState, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoopTextSystem, Pixels,
    PlatformTextSystem, Render, RenderGlyphParams, Result, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, SharedString, Size, StyleRefinement, TestAppContext, TestDispatcher,
    TextRenderingMode, TouchPhase, UniformListScrollHandle, Window, WindowHandle, anchored,
    animated_frame, canvas, deferred, div, effect_frame, hsla, layer_blur, list, point, prelude::*,
    px, size, transition_frame, uniform_list,
};

pub(crate) const WORDS: [&str; 10] = [
    "a",
    "grid",
    "cell",
    "ticking",
    "value",
    "with a longer label",
    "42",
    "lorem ipsum dolor",
    "x",
    "a label long enough to be cut short",
];

pub(crate) const PALETTE: [Hsla; 5] = [
    hsla(0.0, 0.0, 0.1, 1.0),
    hsla(0.6, 0.7, 0.5, 1.0),
    hsla(0.3, 0.6, 0.4, 1.0),
    hsla(0.0, 0.8, 0.6, 1.0),
    hsla(0.1, 0.9, 0.5, 0.5),
];

const GRID_CELLS: usize = 16;
const PANELS: usize = 6;
const ROW_HEIGHT: f32 = 20.;
const INITIAL_ROWS: u64 = 30;
const WINDOW_SIZE: (f32, f32) = (900., 700.);

/// Where things are, in window coordinates, for the input changes to aim at.
mod at {
    /// The cell grid, top left.
    pub(super) const GRID: (f32, f32, f32, f32) = (0., 0., 420., 120.);
    /// The scroll area holding sticky headers and a fixed badge.
    pub(super) const SCROLL: (f32, f32, f32, f32) = (460., 0., 200., 150.);
    /// The uniform list.
    pub(super) const UNIFORM: (f32, f32, f32, f32) = (460., 170., 200., 100.);
    /// The list.
    pub(super) const LIST: (f32, f32, f32, f32) = (460., 290., 200., 100.);
    /// The press target.
    pub(super) const PRESS: (f32, f32, f32, f32) = (20., 400., 60., 30.);
    /// The focusable key target.
    pub(super) const KEYS: (f32, f32, f32, f32) = (100., 400., 60., 30.);
    /// The drag source.
    pub(super) const DRAG: (f32, f32, f32, f32) = (20., 450., 40., 40.);
    /// The drop target.
    pub(super) const DROP: (f32, f32, f32, f32) = (100., 450., 60., 60.);
}

/// A position inside `rect`, `fraction` of the way across it.
fn inside(rect: (f32, f32, f32, f32), fraction: (f32, f32)) -> crate::Point<Pixels> {
    point(
        px(rect.0 + rect.2 * fraction.0),
        px(rect.1 + rect.3 * fraction.1),
    )
}

/// One step of an oracle history, applied to both applications alike.
#[derive(Clone, Debug)]
pub(crate) enum Change {
    /// Notifies a panel without changing it.
    Notify { panel: usize },
    /// Changes a cell and notifies its cached view.
    UpdateCell {
        cell: usize,
        word: usize,
        color: usize,
    },
    /// Changes a cell's hover style flag and notifies its cached view.
    ToggleCellHover { cell: usize },
    /// Changes a panel's value and notifies it.
    UpdatePanel { panel: usize, value: usize },
    /// Changes a panel's value without notifying it: nothing may show it
    /// until something else redraws the panel.
    UpdateSilently { panel: usize, value: usize },
    /// Makes a view notify itself while it is being rendered.
    NotifyWhileDrawing,
    /// Notifies the badge, a cached view nested in the root.
    Badge,
    /// Changes the leaf nested in a panel, notifying only it.
    Leaf { panel: usize },
    /// Writes the global some panels read.
    GlobalWrite { value: usize },
    /// Adds or removes the global whose presence a panel reads.
    GlobalPresence,
    /// Changes the model panels read without observing it.
    UnobservedRead { value: usize },
    /// Scrolls the scroll area holding sticky headers and a fixed badge
    /// through its `ScrollHandle`.
    ScrollHandle { item: usize },
    /// Scrolls the list through its `ListState`.
    ListScroll { top: f32 },
    /// Scrolls the uniform list programmatically to an item.
    ProgrammaticScroll { item: usize },
    /// Scrolls with a real wheel event over one of the scroll containers.
    Wheel { target: usize, delta: f32 },
    /// Moves the pointer.
    Hover { x: f32, y: f32 },
    /// Moves the pointer over a cell of the grid, which may have a hover style.
    HoverCell { cell: usize },
    /// Shows or hides an overlay over the grid's first row, which can cover a
    /// hovered cell with the pointer still.
    CoverCells,
    /// Presses and releases the button over the press target.
    Press,
    /// Moves focus to the key target, the editable text, or nowhere.
    Focus { target: usize },
    /// Types a key into whatever is focused.
    Key { key: usize },
    /// Edits the editable text.
    Edit { text: usize },
    /// Inserts a keyed chip.
    InsertChip { at: usize },
    /// Removes a keyed chip.
    RemoveChip { at: usize },
    /// Reorders the keyed chips.
    RotateChips { by: usize },
    /// Inserts a row into both lists.
    InsertRow { at: usize },
    /// Removes a row from both lists.
    RemoveRow { at: usize },
    /// Resizes the window.
    Resize { width: f32, height: f32 },
    /// Changes the window's scale factor.
    Scale { factor: f32 },
    /// Advances the clock by a frame and runs what animation frames
    /// requested.
    AnimationFrame,
    /// Starts or stops the pulsing view that requests animation frames.
    TogglePulse,
    /// Shows or hides a top-layer overlay.
    TopLayer,
    /// Switches the deferred overlay between clipped and
    /// `Deferred::unclipped`.
    Deferred,
    /// Changes the blur of an effect frame.
    Effect { radius: usize },
    /// Toggles the animated frame's width.
    Animate,
    /// Moves the transition frame to its next page.
    Transition,
    /// Advances a drag: presses on the source, moves over the target, or
    /// drops.
    Drag,
    /// Draws a frame with nothing changed.
    Redraw,
}

impl Change {
    /// A random change.
    pub(crate) fn random(rng: &mut StdRng) -> Change {
        let panel = rng.random_range(0..PANELS);
        match rng.random_range(0..100) {
            0..6 => Change::UpdateCell {
                cell: rng.random_range(0..GRID_CELLS),
                word: rng.random_range(0..WORDS.len()),
                color: rng.random_range(0..PALETTE.len()),
            },
            6..8 => Change::ToggleCellHover {
                cell: rng.random_range(0..GRID_CELLS),
            },
            8..11 => Change::Notify { panel },
            11..15 => Change::UpdatePanel {
                panel,
                value: rng.random_range(0..WORDS.len()),
            },
            15..17 => Change::UpdateSilently {
                panel,
                value: rng.random_range(0..WORDS.len()),
            },
            17..19 => Change::NotifyWhileDrawing,
            19..22 => Change::Badge,
            22..25 => Change::Leaf { panel },
            25..27 => Change::GlobalWrite {
                value: rng.random_range(0..PALETTE.len()),
            },
            27..29 => Change::GlobalPresence,
            29..32 => Change::UnobservedRead {
                value: rng.random_range(0..WORDS.len()),
            },
            32..36 => Change::ScrollHandle {
                item: rng.random_range(0..12),
            },
            36..39 => Change::ListScroll {
                top: rng.random_range(0.0..400.0),
            },
            39..42 => Change::ProgrammaticScroll {
                item: rng.random_range(0..40),
            },
            42..47 => Change::Wheel {
                target: rng.random_range(0..3),
                delta: rng.random_range(-60.0..60.0),
            },
            47..53 => Change::Hover {
                x: rng.random_range(0.0..WINDOW_SIZE.0),
                y: rng.random_range(0.0..WINDOW_SIZE.1),
            },
            53..55 => Change::Press,
            55..58 => Change::Focus {
                target: rng.random_range(0..3),
            },
            58..61 => Change::Key {
                key: rng.random_range(0..4),
            },
            61..64 => Change::Edit {
                text: rng.random_range(0..WORDS.len()),
            },
            64..66 => Change::InsertChip {
                at: rng.random_range(0..16),
            },
            66..68 => Change::RemoveChip {
                at: rng.random_range(0..16),
            },
            68..69 => Change::RotateChips {
                by: rng.random_range(1..4),
            },
            69..71 => Change::InsertRow {
                at: rng.random_range(0..64),
            },
            71..73 => Change::RemoveRow {
                at: rng.random_range(0..64),
            },
            73..75 => Change::Resize {
                width: rng.random_range(500.0..1000.0),
                height: rng.random_range(400.0..800.0),
            },
            75..76 => Change::Scale {
                factor: [1.0, 1.5, 2.0][rng.random_range(0..3)],
            },
            76..80 => Change::AnimationFrame,
            80..81 => Change::TogglePulse,
            81..83 => Change::TopLayer,
            83..85 => Change::Deferred,
            85..87 => Change::Effect {
                radius: rng.random_range(0..4),
            },
            87..89 => Change::Animate,
            89..91 => Change::Transition,
            91..93 => Change::Drag,
            93..95 => Change::CoverCells,
            95..97 => Change::HoverCell {
                cell: rng.random_range(0..GRID_CELLS),
            },
            _ => Change::Redraw,
        }
    }
}

/// A cell of the grid, drawn as a cached view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CellState {
    word: usize,
    color: usize,
    hover: bool,
}

pub(crate) struct GridCell(CellState);

impl Render for GridCell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let cell = self.0;
        div()
            .id("cell")
            .flex()
            .flex_row()
            .size_full()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_color(PALETTE[(cell.color + 1) % PALETTE.len()])
            .bg(PALETTE[cell.color])
            .when(cell.hover, |this| {
                this.hover(|style| style.bg(PALETTE[4]).text_color(PALETTE[0]))
            })
            .child(WORDS[cell.word])
    }
}

/// A model panels read without observing it.
pub(crate) struct Shared {
    value: usize,
}

/// A global panels read.
pub(crate) struct Accent(usize);

impl Global for Accent {}

/// A global whose presence a panel reads.
pub(crate) struct Marker;

impl Global for Marker {}

/// A child view that is not cached. Some panels read the shared model, the
/// global or the global's presence.
pub(crate) struct Panel {
    ix: usize,
    value: usize,
    shared: Entity<Shared>,
    leaf: Entity<Leaf>,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shared = if self.ix.is_multiple_of(2) {
            self.shared.read(cx).value
        } else {
            0
        };
        let accent = if self.ix.is_multiple_of(3) {
            cx.try_global::<Accent>().map_or(0, |accent| accent.0)
        } else {
            0
        };
        let marked = self.ix == 1 && cx.has_global::<Marker>();
        div()
            .id(("panel", self.ix))
            .flex()
            .flex_col()
            .p_1()
            .bg(PALETTE[(self.value + accent) % PALETTE.len()])
            .when(marked, |this| this.border_2().border_color(PALETTE[3]))
            .when(self.ix == 1, |this| {
                this.hover(|style| style.bg(PALETTE[4]))
            })
            .child(WORDS[(self.value + shared) % WORDS.len()])
            // Some leaves are cached, so a panel is drawn around a cached
            // view built again in it, as around one that is not.
            .when(self.ix >= PANELS / 2, |this| {
                this.child(
                    self.leaf
                        .clone()
                        .cached(StyleRefinement::default().w(px(30.)).h(px(8.))),
                )
            })
            .when(self.ix < PANELS / 2, |this| this.child(self.leaf.clone()))
    }
}

/// A view nested in a panel, notified on its own.
pub(crate) struct Leaf {
    count: usize,
}

impl Render for Leaf {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .children((0..self.count % 3 + 1).map(|ix| {
                div()
                    .w(px(4. + ix as f32 * 5.))
                    .h(px(6.))
                    .bg(PALETTE[(self.count + ix) % PALETTE.len()])
            }))
    }
}

/// A cached child view, notified on its own.
pub(crate) struct Badge {
    count: usize,
}

impl Render for Badge {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .gap_1()
            .size_full()
            .children((0..self.count % 4 + 1).map(|ix| {
                div()
                    .w(px(6. + ix as f32 * 3.))
                    .h(px(10.))
                    .bg(PALETTE[ix % PALETTE.len()])
            }))
            .child(SharedString::from(self.count.to_string()))
    }
}

/// A view that notifies itself once while it is being rendered, when asked
/// to, so the window is dirty again right after the frame that drew it.
pub(crate) struct Restless {
    pending: bool,
    generation: usize,
}

impl Render for Restless {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending {
            self.pending = false;
            self.generation += 1;
            cx.notify();
        }
        div()
            .w(px(10. + (self.generation % 5) as f32 * 4.))
            .h(px(6.))
            .bg(PALETTE[self.generation % PALETTE.len()])
    }
}

/// A view that, while pulsing, requests an animation frame on every frame
/// and shows how far the clock has moved since it started.
pub(crate) struct Pulse {
    pulsing: bool,
    started: Option<crate::Instant>,
}

impl Render for Pulse {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let elapsed = match (self.pulsing, self.started) {
            (true, Some(started)) => {
                window.request_animation_frame();
                cx.background_executor()
                    .now()
                    .duration_since(started)
                    .as_millis() as f32
            }
            _ => 0.,
        };
        div().w(px(4. + elapsed % 60.)).h(px(6.)).bg(PALETTE[1])
    }
}

/// The preview a drag shows under the pointer.
pub(crate) struct DragPreview;

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(20.)).h(px(20.)).bg(PALETTE[2])
    }
}

/// What is dragged.
#[derive(Clone)]
pub(crate) struct Card(usize);

/// Where a drag the history drives is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum DragState {
    #[default]
    Idle,
    Pressed,
    Dragging,
}

/// A small application holding every gpux feature the oracle checks
/// fast mode against.
pub(crate) struct OracleView {
    cells: Vec<CellState>,
    cell_views: Vec<Entity<GridCell>>,
    panels: Vec<Entity<Panel>>,
    shared: Entity<Shared>,
    badge: Entity<Badge>,
    restless: Entity<Restless>,
    pulse: Entity<Pulse>,
    chips: Vec<u64>,
    rows: Vec<u64>,
    next_id: u64,
    scroll: ScrollHandle,
    uniform_scroll: UniformListScrollHandle,
    list_state: ListState,
    presses: usize,
    keys_focus: FocusHandle,
    keys: Vec<SharedString>,
    text: Entity<EditableTextState>,
    dropped: Vec<usize>,
    top_layer: bool,
    covered: bool,
    unclipped: bool,
    blur: usize,
    wide: bool,
    page: usize,
}

impl OracleView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let shared = cx.new(|_| Shared { value: 0 });
        let cells: Vec<CellState> = (0..GRID_CELLS)
            .map(|ix| CellState {
                word: ix % WORDS.len(),
                color: ix % PALETTE.len(),
                hover: ix.is_multiple_of(3),
            })
            .collect();
        let text = cx.new(|cx| EditableTextState::with_text("edit me", cx));
        cx.observe(&text, |_, _, cx| cx.notify()).detach();
        Self {
            cell_views: cells
                .iter()
                .map(|&cell| cx.new(|_| GridCell(cell)))
                .collect(),
            cells,
            panels: (0..PANELS)
                .map(|ix| {
                    let shared = shared.clone();
                    cx.new(|cx| Panel {
                        ix,
                        value: ix,
                        shared,
                        leaf: cx.new(|_| Leaf { count: ix }),
                    })
                })
                .collect(),
            shared,
            badge: cx.new(|_| Badge { count: 0 }),
            restless: cx.new(|_| Restless {
                pending: false,
                generation: 0,
            }),
            pulse: cx.new(|_| Pulse {
                pulsing: false,
                started: None,
            }),
            chips: (100..106).collect(),
            rows: (0..INITIAL_ROWS).collect(),
            next_id: 1000,
            scroll: ScrollHandle::new(),
            uniform_scroll: UniformListScrollHandle::new(),
            list_state: ListState::new(INITIAL_ROWS as usize, ListAlignment::Top, px(40.)),
            presses: 0,
            keys_focus: cx.focus_handle(),
            keys: Vec::new(),
            text,
            dropped: Vec::new(),
            top_layer: false,
            covered: false,
            unclipped: false,
            blur: 1,
            wide: false,
            page: 0,
        }
    }

    /// Applies the changes that belong to this view's own state.
    fn apply(&mut self, change: &Change, window: &mut Window, cx: &mut Context<Self>) {
        match *change {
            Change::UpdateCell { cell, word, color } => {
                self.cells[cell].word = word;
                self.cells[cell].color = color;
                let state = self.cells[cell];
                self.cell_views[cell].update(cx, |view, cx| {
                    view.0 = state;
                    cx.notify();
                });
                return;
            }
            Change::ToggleCellHover { cell } => {
                self.cells[cell].hover = !self.cells[cell].hover;
                let state = self.cells[cell];
                self.cell_views[cell].update(cx, |view, cx| {
                    view.0 = state;
                    cx.notify();
                });
                return;
            }
            Change::ScrollHandle { item } => self.scroll.scroll_to_item(item),
            Change::ListScroll { top } => {
                if !self.rows.is_empty() {
                    let item_ix = ((top / ROW_HEIGHT).floor() as usize).min(self.rows.len() - 1);
                    self.list_state.scroll_to(ListOffset {
                        item_ix,
                        offset_in_item: px(top % ROW_HEIGHT),
                    });
                }
            }
            Change::ProgrammaticScroll { item } => self
                .uniform_scroll
                .scroll_to_item(item, crate::ScrollStrategy::Top),
            Change::Focus { target } => match target {
                0 => window.focus(&self.keys_focus, cx),
                1 => {
                    let handle = self.text.read(cx).focus_handle();
                    window.focus(&handle, cx)
                }
                _ => window.blur(cx),
            },
            Change::Edit { text } => {
                self.text.update(cx, |state, cx| {
                    state.replace_selection(WORDS[text], cx);
                });
            }
            Change::InsertChip { at } => {
                let at = at % (self.chips.len() + 1);
                self.chips.insert(at, self.next_id);
                self.next_id += 1;
            }
            Change::RemoveChip { at } => {
                if !self.chips.is_empty() {
                    let at = at % self.chips.len();
                    self.chips.remove(at);
                }
            }
            Change::RotateChips { by } => {
                if !self.chips.is_empty() {
                    let by = by % self.chips.len();
                    self.chips.rotate_left(by);
                }
            }
            Change::InsertRow { at } => {
                let at = at % (self.rows.len() + 1);
                self.rows.insert(at, self.next_id);
                self.next_id += 1;
                self.list_state.splice(at..at, 1);
            }
            Change::RemoveRow { at } => {
                if self.rows.is_empty() {
                    return;
                }
                let at = at % self.rows.len();
                self.rows.remove(at);
                self.list_state.splice(at..at + 1, 0);
            }
            Change::TopLayer => self.top_layer = !self.top_layer,
            Change::CoverCells => self.covered = !self.covered,
            Change::Deferred => self.unclipped = !self.unclipped,
            Change::Effect { radius } => self.blur = radius,
            Change::Animate => self.wide = !self.wide,
            Change::Transition => self.page += 1,
            _ => return,
        }
        cx.notify();
    }
}

impl OracleNested {
    /// Applies the changes to views nested in an [`OracleView`], or to its
    /// models, without updating the view, so that it is dirty only because of
    /// what is nested in it.
    fn apply(&self, change: &Change, cx: &mut App) {
        match *change {
            Change::Notify { panel } => self.panels[panel].update(cx, |_, cx| cx.notify()),
            Change::UpdatePanel { panel, value } => self.panels[panel].update(cx, |panel, cx| {
                panel.value = value;
                cx.notify();
            }),
            Change::UpdateSilently { panel, value } => {
                self.panels[panel].update(cx, |panel, _| panel.value = value)
            }
            Change::NotifyWhileDrawing => self.restless.update(cx, |restless, cx| {
                restless.pending = true;
                cx.notify();
            }),
            Change::Badge => self.badge.update(cx, |badge, cx| {
                badge.count += 1;
                cx.notify();
            }),
            Change::Leaf { panel } => {
                let leaf = self.panels[panel].read(cx).leaf.clone();
                leaf.update(cx, |leaf, cx| {
                    leaf.count += 1;
                    cx.notify();
                });
            }
            Change::UnobservedRead { value } => self.shared.update(cx, |shared, cx| {
                shared.value = value;
                cx.notify();
            }),
            Change::TogglePulse => {
                let now = cx.background_executor().now();
                self.pulse.update(cx, |pulse, cx| {
                    pulse.pulsing = !pulse.pulsing;
                    pulse.started = pulse.pulsing.then_some(now);
                    cx.notify();
                })
            }
            _ => unreachable!("not a change to a nested view: {change:?}"),
        }
    }
}

impl OracleView {
    fn render_row(row: u64) -> AnyElement {
        div()
            .id(("row", row))
            .flex()
            .flex_row()
            .gap_2()
            .h(px(ROW_HEIGHT))
            .child(SharedString::from(format!("row {row}")))
            .child(
                div()
                    .w(px(8. + (row % 4) as f32 * 6.))
                    .h(px(8.))
                    .bg(PALETTE[row as usize % PALETTE.len()]),
            )
            .when(row.is_multiple_of(3), |this| {
                this.hover(|style| style.bg(PALETTE[2]))
            })
            .into_any_element()
    }

    fn absolute(rect: (f32, f32, f32, f32)) -> crate::Div {
        div()
            .absolute()
            .left(px(rect.0))
            .top(px(rect.1))
            .w(px(rect.2))
            .h(px(rect.3))
    }
}

impl Render for OracleView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let uniform_rows = uniform_list("uniform rows", rows.len(), {
            let rows = rows.clone();
            move |range, _, _| range.map(|ix| OracleView::render_row(rows[ix])).collect()
        })
        .track_scroll(&self.uniform_scroll)
        .size_full();
        let list_rows = list(self.list_state.clone(), move |ix, _, _| {
            OracleView::render_row(rows[ix])
        })
        .size_full();

        let keys_label: SharedString = self.keys.join(" ").into();
        div()
            .size_full()
            .relative()
            // The cell grid: cached views, a cached badge, chips and panels
            // in flex rows that wrap.
            .child(
                div()
                    .absolute()
                    .left(px(at::GRID.0))
                    .top(px(at::GRID.1))
                    .w(px(at::GRID.2))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().flex().flex_row().flex_wrap().gap_1().children(
                        self.cell_views.iter().map(|view| {
                            view.clone()
                                .cached(StyleRefinement::default().w(px(100.)).h(px(18.)))
                        }),
                    ))
                    .child(
                        self.badge
                            .clone()
                            .cached(StyleRefinement::default().w(px(120.)).h(px(24.))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .children(self.chips.iter().map(|&chip| {
                                div()
                                    .id(("chip", chip))
                                    .px_1()
                                    .h(px(16.))
                                    .min_w(px(10. + (chip % 5) as f32 * 8.))
                                    .bg(PALETTE[chip as usize % PALETTE.len()])
                                    .child(SharedString::from(chip.to_string()))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .children(self.panels.iter().cloned())
                            .child(self.restless.clone())
                            .child(self.pulse.clone()),
                    ),
            )
            // A scroll area holding sticky headers and a fixed badge.
            .child(
                OracleView::absolute(at::SCROLL).child(
                    div()
                        .id("scroller")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .children(["first", "second"].into_iter().map(|section| {
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .id(SharedString::from(format!("header-{section}")))
                                        .sticky()
                                        .top(px(0.))
                                        .h(px(24.))
                                        .bg(PALETTE[2])
                                        .child(section),
                                )
                                .children((0..6).map(move |row| {
                                    div()
                                        .id(SharedString::from(format!("{section}-{row}")))
                                        .h(px(30.))
                                        .bg(PALETTE[row % PALETTE.len()])
                                        .hover(|style| style.bg(PALETTE[4]))
                                        .child(SharedString::from(format!("{section} {row}")))
                                }))
                        }))
                        .child(
                            div()
                                .id("pinned")
                                .fixed()
                                .top(px(10.))
                                .right(px(10.))
                                .w(px(40.))
                                .h(px(20.))
                                .bg(PALETTE[1]),
                        ),
                ),
            )
            .child(OracleView::absolute(at::UNIFORM).child(uniform_rows))
            .child(OracleView::absolute(at::LIST).child(list_rows))
            // Pointer, focus and keys.
            .child(
                OracleView::absolute(at::PRESS)
                    .id("press")
                    .bg(PALETTE[self.presses % PALETTE.len()])
                    .active(|style| style.bg(PALETTE[3]))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.presses += 1;
                        cx.notify();
                    }))
                    .child(SharedString::from(format!("{}", self.presses))),
            )
            .child(
                OracleView::absolute(at::KEYS)
                    .id("keys")
                    .track_focus(&self.keys_focus)
                    .focus(|style| style.border_2().border_color(PALETTE[3]))
                    .bg(PALETTE[0])
                    .text_color(PALETTE[4])
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        this.keys.push(event.keystroke.key.clone().into());
                        if this.keys.len() > 4 {
                            this.keys.remove(0);
                        }
                        cx.notify();
                    }))
                    .child(keys_label),
            )
            .child(
                div()
                    .absolute()
                    .left(px(200.))
                    .top(px(400.))
                    .w(px(200.))
                    .child(EditableText::new(&self.text)),
            )
            // Drag and drop.
            .child(
                OracleView::absolute(at::DRAG)
                    .id("drag source")
                    .bg(PALETTE[3])
                    .on_drag(Card(self.dropped.len()), |_, _, _, cx| {
                        cx.new(|_| DragPreview)
                    }),
            )
            .child(
                OracleView::absolute(at::DROP)
                    .id("drop target")
                    .bg(PALETTE[0])
                    .drag_over::<Card>(|style, _, _, _| style.bg(PALETTE[2]))
                    .on_drop(cx.listener(|this, card: &Card, _, cx| {
                        this.dropped.push(card.0);
                        cx.notify();
                    }))
                    .child(SharedString::from(format!("{}", self.dropped.len()))),
            )
            // Frame effects.
            .child(
                div().absolute().left(px(200.)).top(px(450.)).child(
                    effect_frame()
                        .effect(layer_blur(px(self.blur as f32 * 2.)))
                        .child(div().w(px(40.)).h(px(40.)).bg(PALETTE[1])),
                ),
            )
            .child(
                div().absolute().left(px(260.)).top(px(450.)).child(
                    animated_frame("bar")
                        .duration(Duration::from_millis(200))
                        .easing(|delta| delta)
                        .w(px(if self.wide { 120. } else { 60. }))
                        .h(px(20.))
                        .child_with_progress(|progress, _, _| {
                            div()
                                .size_full()
                                .bg(PALETTE[2])
                                .child(SharedString::from(format!("{progress:.2}")))
                        }),
                ),
            )
            .child(
                div().absolute().left(px(400.)).top(px(450.)).child(
                    transition_frame("pages")
                        .target_key(format!("page-{}", self.page))
                        .transition(FrameTransition::crossfade().duration_ms(200))
                        .w(px(80.))
                        .h(px(40.))
                        .child(
                            div()
                                .size_full()
                                .bg(PALETTE[self.page % PALETTE.len()])
                                .child(SharedString::from(format!("page {}", self.page))),
                        ),
                ),
            )
            // A clipped dialog with a deferred overlay, clipped or not.
            .child(
                div()
                    .absolute()
                    .left(px(600.))
                    .top(px(400.))
                    .w(px(120.))
                    .h(px(50.))
                    .overflow_hidden()
                    .bg(PALETTE[0])
                    .child({
                        let overlay = deferred(
                            anchored().child(
                                div()
                                    .id("overlay")
                                    .w(px(40.))
                                    .h(px(80.))
                                    .bg(PALETTE[3])
                                    .hover(|style| style.bg(PALETTE[1])),
                            ),
                        );
                        if self.unclipped {
                            overlay.unclipped()
                        } else {
                            overlay
                        }
                    }),
            )
            // An overlay over the grid's first row, blocking the pointer.
            .when(self.covered, |this| {
                this.child(
                    div()
                        .id("cover")
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .w(px(at::GRID.2))
                        .h(px(20.))
                        .occlude()
                        .bg(PALETTE[0].opacity(0.5)),
                )
            })
            // Top-layer chrome above everything positioned.
            .when(self.top_layer, |this| {
                this.child(
                    canvas(
                        |bounds, window, _| {
                            window.insert_top_layer_hitbox(bounds, HitboxBehavior::BlockMouse)
                        },
                        |bounds, hitbox: Hitbox, window, _| {
                            window.paint_top_layer(bounds, |window| {
                                window.paint_quad(crate::fill(bounds, PALETTE[4]));
                            });
                            window.set_cursor_style(crate::CursorStyle::Crosshair, &hitbox);
                        },
                    )
                    .absolute()
                    .left(px(500.))
                    .top(px(40.))
                    .w(px(100.))
                    .h(px(60.)),
                )
            })
    }
}

/// The no-op text system, except that every glyph rasterizes to a small box,
/// so text paints a sprite per glyph and where each glyph went is compared.
pub(crate) struct GlyphBoxTextSystem(pub(crate) NoopTextSystem);

impl PlatformTextSystem for GlyphBoxTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.0.add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        self.0.all_font_names()
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        self.0.font_id(descriptor)
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        self.0.font_metrics(font_id)
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        self.0.typographic_bounds(font_id, glyph_id)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.0.advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.0.glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(&self, _params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        Ok(Bounds {
            origin: point(DevicePixels(0), DevicePixels(-8)),
            size: size(DevicePixels(5), DevicePixels(9)),
        })
    }

    fn rasterize_glyph(
        &self,
        _params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        let area = raster_bounds.size.width.0 * raster_bounds.size.height.0;
        Ok((raster_bounds.size, vec![u8::MAX; area as usize]))
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        self.0.layout_line(text, font_size, runs)
    }

    fn recommended_rendering_mode(&self, font_id: FontId, font_size: Pixels) -> TextRenderingMode {
        self.0.recommended_rendering_mode(font_id, font_size)
    }
}

/// An application whose text system draws a box per glyph, built from
/// `seed` so that two built from the same seed run alike.
pub(crate) fn oracle_app(seed: u64) -> TestAppContext {
    TestAppContext::build_with_text_system(
        TestDispatcher::new(seed),
        None,
        Arc::new(GlyphBoxTextSystem(NoopTextSystem)),
    )
}

/// One application of a lockstep pair and its window.
pub(crate) struct OracleWindow {
    pub(crate) cx: TestAppContext,
    pub(crate) window: WindowHandle<OracleView>,
    drag: DragState,
}

impl OracleWindow {
    /// Opens an [`OracleView`] in an application built from `seed`, in
    /// `mode`, and draws its first frame.
    pub(crate) fn open(seed: u64, mode: FastMode) -> Self {
        let mut cx = oracle_app(seed);
        let window = cx.add_window(|_, cx| OracleView::new(cx));
        cx.simulate_window_resize(window.into(), size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)));
        cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
            .unwrap();
        // Every frame is drawn by `draw`, so the frame compared is the first
        // one after a step's changes, as an application's next frame is.
        cx.set_draw_on_flush(false);
        Self {
            cx,
            window,
            drag: DragState::Idle,
        }
    }

    fn dispatch(&mut self, event: crate::PlatformInput) {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.dispatch_event(event, cx);
            })
            .unwrap();
    }

    fn press_at(&mut self, position: crate::Point<Pixels>) {
        self.dispatch(
            MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
        );
    }

    fn release_at(&mut self, position: crate::Point<Pixels>) {
        self.dispatch(
            MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }
            .to_platform_input(),
        );
    }

    fn move_to(&mut self, position: crate::Point<Pixels>, pressed: bool) {
        self.dispatch(
            MouseMoveEvent {
                position,
                pressed_button: pressed.then_some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
        );
    }

    /// Applies `change` to this application.
    pub(crate) fn apply(&mut self, change: &Change) {
        let handle = self.window;
        match *change {
            Change::Hover { x, y } => {
                let pressed = self.drag != DragState::Idle;
                self.move_to(point(px(x), px(y)), pressed);
            }
            Change::HoverCell { cell } => {
                // Four 100px cells to a row, 4px apart, 18px tall, 4px apart.
                let x = (cell % 4) as f32 * 104. + 50.;
                let y = (cell / 4) as f32 * 22. + 9.;
                let pressed = self.drag != DragState::Idle;
                self.move_to(point(px(x), px(y)), pressed);
            }
            Change::Press => {
                if self.drag != DragState::Idle {
                    return;
                }
                let position = inside(at::PRESS, (0.5, 0.5));
                self.move_to(position, false);
                self.press_at(position);
                self.release_at(position);
            }
            Change::Wheel { target, delta } => {
                let rect = [at::SCROLL, at::UNIFORM, at::LIST][target];
                let position = inside(rect, (0.5, 0.5));
                self.dispatch(
                    ScrollWheelEvent {
                        position,
                        delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    }
                    .to_platform_input(),
                );
            }
            Change::Key { key } => {
                let keystroke = Keystroke::parse(["a", "b", "left", "backspace"][key]).unwrap();
                self.cx.dispatch_keystroke(handle.into(), keystroke);
            }
            Change::Drag => match self.drag {
                DragState::Idle => {
                    let position = inside(at::DRAG, (0.5, 0.5));
                    self.move_to(position, false);
                    self.press_at(position);
                    self.drag = DragState::Pressed;
                }
                DragState::Pressed => {
                    self.move_to(inside(at::DROP, (0.5, 0.5)), true);
                    self.drag = DragState::Dragging;
                }
                DragState::Dragging => {
                    self.release_at(inside(at::DROP, (0.5, 0.5)));
                    self.drag = DragState::Idle;
                }
            },
            Change::Resize { width, height } => {
                self.cx
                    .simulate_window_resize(handle.into(), size(px(width), px(height)));
            }
            Change::Scale { factor } => {
                self.cx
                    .simulate_window_scale_factor_change(handle.into(), factor);
            }
            Change::GlobalWrite { value } => self.cx.update(|cx| cx.set_global(Accent(value))),
            Change::GlobalPresence => self.cx.update(|cx| {
                if cx.has_global::<Marker>() {
                    cx.remove_global::<Marker>();
                } else {
                    cx.set_global(Marker);
                }
            }),
            Change::AnimationFrame => {
                self.cx.executor().advance_clock(Duration::from_millis(16));
            }
            Change::Notify { .. }
            | Change::UpdatePanel { .. }
            | Change::UpdateSilently { .. }
            | Change::NotifyWhileDrawing
            | Change::Badge
            | Change::Leaf { .. }
            | Change::UnobservedRead { .. }
            | Change::TogglePulse => {
                let nested = handle.read_with(&self.cx, |view, _| view.nested()).unwrap();
                self.cx.update(|cx| nested.apply(change, cx));
            }
            Change::Redraw => {}
            _ => handle
                .update(&mut self.cx, |view, window, cx| {
                    view.apply(change, window, cx)
                })
                .unwrap(),
        }
    }

    /// Runs the callbacks the last frame asked to run before the next one,
    /// as the platform's frame callback does, and draws a frame: from
    /// scratch, after forgetting what the window retains, when asked.
    /// Returns the frame's description and counters.
    pub(crate) fn draw(&mut self, from_scratch: bool) -> (Vec<String>, FastStats) {
        self.draw_after(from_scratch, |_| {})
    }

    /// [`Self::draw`], running `before` on the window first, in the same
    /// update as the frame, so nothing draws in between.
    pub(crate) fn draw_after(
        &mut self,
        from_scratch: bool,
        before: impl FnOnce(&mut Window),
    ) -> (Vec<String>, FastStats) {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                let callbacks = window.next_frame_callbacks.take();
                for callback in callbacks {
                    callback(window, cx);
                }
            })
            .unwrap();
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                before(window);
                if from_scratch {
                    window.forget_retained_state();
                }
                window.draw(cx).clear(cx);
                (
                    crate::fast::verify::describe_frame(window),
                    window.fast_frame_stats(),
                )
            })
            .unwrap()
    }
}

impl OracleView {
    /// A copy of the handles changes to nested views go through.
    fn nested(&self) -> OracleNested {
        OracleNested {
            panels: self.panels.clone(),
            shared: self.shared.clone(),
            badge: self.badge.clone(),
            restless: self.restless.clone(),
            pulse: self.pulse.clone(),
        }
    }
}

/// The nested views and models of an [`OracleView`], for changes applied to
/// them alone.
struct OracleNested {
    panels: Vec<Entity<Panel>>,
    shared: Entity<Shared>,
    badge: Entity<Badge>,
    restless: Entity<Restless>,
    pulse: Entity<Pulse>,
}

/// What a lockstep run found.
#[derive(Debug, Default)]
pub(crate) struct OracleRun {
    /// The incremental window's counters, accumulated over the run.
    pub(crate) incremental: FastStats,
    /// The frames compared.
    pub(crate) frames: usize,
}

/// Drives an incremental window in `mode` and a from-scratch reference
/// window through one random history of `steps` steps from `seed`, comparing
/// their frames after every step, and panics with the seed, the step, the
/// first difference and the history so far on any difference.
pub(crate) fn run_lockstep(seed: u64, steps: usize, mode: FastMode) -> OracleRun {
    let mut incremental = OracleWindow::open(seed, mode);
    let mut reference = OracleWindow::open(seed, FastMode::OFF);
    let mut rng = StdRng::seed_from_u64(seed);
    let mut history: Vec<Vec<Change>> = Vec::new();
    let mut run = OracleRun::default();

    for step in 0..steps {
        let changes: Vec<Change> = if step == 0 {
            Vec::new()
        } else {
            (0..rng.random_range(1..=3))
                .map(|_| Change::random(&mut rng))
                .collect()
        };
        for change in &changes {
            incremental.apply(change);
            reference.apply(change);
        }
        history.push(changes);

        let (expected, reference_stats) = reference.draw(true);
        let (actual, stats) = incremental.draw(false);
        assert_eq!(
            reference_stats.cached_views_replayed, 0,
            "a window drawing from scratch replayed a cached view"
        );
        assert_eq!(
            reference_stats.mechanism_total(),
            0,
            "a window drawing from scratch with fast mode off made a fast-mode decision"
        );
        run.incremental.accumulate(&stats);
        run.frames += 1;

        if let Some(difference) = crate::fast::verify::first_frame_difference(&actual, &expected) {
            let history = history
                .iter()
                .enumerate()
                .map(|(step, changes)| format!("  {step}: {changes:?}"))
                .collect::<Vec<_>>()
                .join("\n");
            panic!(
                "seed {seed}, step {step}, mode {mode:?}: the incremental frame differs from \
                 the frame drawn from scratch: {difference}\nchanges so far:\n{history}"
            );
        }
    }
    run
}

/// The seeds and steps of a run: `SEED` picks one seed, `ITERATIONS` the
/// number of seeds; the defaults are `seeds` seeds of `steps` steps.
pub(crate) fn seeds_and_steps(seeds: u64, steps: usize) -> (Vec<u64>, usize) {
    let iterations = std::env::var("ITERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(seeds);
    let seeds = match std::env::var("SEED")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        Some(seed) => vec![seed],
        None => (0..iterations).collect(),
    };
    (seeds, steps)
}
