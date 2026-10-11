//! Hover in a list's rows (`fast/hover.rs`): a window draws a frame when a
//! move of the pointer changes which row it shows hovered, with scroll
//! layers and without them, so that the two show the same rows hovered.
//!
//! ```text
//! cargo test -p gpux --features test-support --lib fast::tests::list_hover::
//! ```

use crate::{
    AnyWindowHandle, InteractiveElement as _, IntoElement, ListAlignment, ListState,
    ParentElement as _, Render, ScrollDelta, ScrollWheelEvent, Styled as _, TestAppContext,
    TouchPhase, div,
    fast::tests::layer_support::{add_window, make_reference, with_window},
    point, px, rgb,
};

const ROWS: usize = 200;
const ROW_HEIGHT: f32 = 20.;

/// A list of rows that turn amber under the pointer: a `list` when it has a
/// state for one, a `uniform_list` otherwise.
struct Page(Option<ListState>);

fn row(index: usize) -> crate::AnyElement {
    div()
        .id(("row", index))
        .hover(|style| style.bg(rgb(0xffd9a0)))
        .w_full()
        .h(px(ROW_HEIGHT))
        .bg(if index.is_multiple_of(2) {
            rgb(0xe9e4d8)
        } else {
            rgb(0xd5dde6)
        })
        .into_any_element()
}

impl Render for Page {
    fn render(&mut self, _: &mut crate::Window, cx: &mut crate::Context<Self>) -> impl IntoElement {
        let list = match &self.0 {
            Some(state) => crate::list(state.clone(), |index, _, _| row(index))
                .w(px(200.))
                .h(px(100.))
                .bg(rgb(0xffffff))
                .into_any_element(),
            None => crate::uniform_list(
                "list",
                ROWS,
                cx.processor(|_, range: std::ops::Range<usize>, _, _| {
                    range.map(row).collect::<Vec<_>>()
                }),
            )
            .w(px(200.))
            .h(px(100.))
            .bg(rgb(0xffffff))
            .into_any_element(),
        };
        div().size_full().bg(rgb(0xffffff)).child(list)
    }
}

/// How a window draws.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// Every mechanism of fast mode.
    All,
    /// Scroll layers alone.
    Layers,
    /// None: the reference the others are held to.
    Reference,
}

/// Opens the page in `mode`.
fn open(cx: &mut TestAppContext, uniform: bool, mode: Mode) -> AnyWindowHandle {
    let window: AnyWindowHandle = add_window(cx, move |_, _| {
        Page((!uniform).then(|| ListState::new(ROWS, ListAlignment::Top, px(0.))))
    })
    .into();
    with_window(cx, window, |window, _| match mode {
        Mode::All => window.set_fast_mode(crate::FastMode::ALL),
        Mode::Layers => {}
        Mode::Reference => make_reference(window),
    });
    with_window(cx, window, |window, cx| window.draw(cx).clear(cx));
    with_window(cx, window, |window, cx| window.draw(cx).clear(cx));
    window
}

/// Something the pointer does.
#[derive(Clone, Copy, Debug)]
enum Act {
    /// The wheel turns `dy` where the pointer is.
    Wheel(f32),
    /// The pointer moves to `y`.
    Move(f32),
}

/// Does `act` in `window` with the pointer at `pointer`, and draws the frame
/// the window then asks for, if it asks for one, as a platform's frame
/// callback does. Returns whether a frame was drawn.
fn act(cx: &mut TestAppContext, window: AnyWindowHandle, pointer: f32, act: Act) -> bool {
    let frame = with_window(cx, window, |window, _| window.fast_layers.frame);
    with_window(cx, window, |window, cx| {
        let event = match act {
            Act::Wheel(dy) => crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(50.), px(pointer)),
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            }),
            Act::Move(y) => crate::PlatformInput::MouseMove(crate::MouseMoveEvent {
                position: point(px(50.), px(y)),
                pressed_button: None,
                modifiers: Default::default(),
            }),
        };
        window.dispatch_event(event, cx);
    });
    cx.run_until_parked();
    with_window(cx, window, |window, cx| {
        let asked = window.is_dirty();
        if asked {
            window.draw(cx).clear(cx);
        }
        asked || window.fast_layers.frame != frame
    })
}

/// Where the frame the window shows draws amber: the rows it shows hovered,
/// by the tops of their quads in the frame's description.
fn hovered(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<String> {
    with_window(cx, window, |window, _| {
        crate::fast::verify::describe_frame(window)
            .into_iter()
            // Amber, as a paint colour is written.
            .filter(|line| line.contains("g: 0.8509804, b: 0.627451"))
            .collect()
    })
}

/// The pointer over a row, the wheel turned so that another row comes under
/// it, and the pointer moved back onto the first row.
fn back_onto_a_row_after_a_scroll() -> Vec<Act> {
    vec![
        // Row 3, from 60 to 80.
        Act::Move(70.),
        // Row 4 comes under the pointer.
        Act::Wheel(-25.),
        // Row 3 again, now from 35 to 55.
        Act::Move(45.),
        Act::Wheel(-40.),
        Act::Move(5.),
        Act::Wheel(30.),
        Act::Move(35.),
    ]
}

/// Many moves between small turns of the wheel both ways.
fn moves_between_scrolls() -> Vec<Act> {
    let mut acts = vec![Act::Move(50.), Act::Wheel(-200.)];
    for round in 0..120 {
        acts.push(Act::Move(5. + (round * 53 % 90) as f32));
        let dy = if round % 3 == 0 { 7. } else { -11. };
        acts.extend((0..1 + round % 4).map(|_| Act::Wheel(dy)));
    }
    acts
}

/// Runs `acts` in a window drawing in `mode` and in the reference, and
/// checks after each that both show hovered the row the pointer is over,
/// and the same row. Returns how many frames each drew.
fn compare(cx: &mut TestAppContext, uniform: bool, mode: Mode, acts: &[Act]) -> (usize, usize) {
    let layered = open(cx, uniform, mode);
    let reference = open(cx, uniform, Mode::Reference);
    let mut drawn = (0, 0);
    let mut y = 0.;
    for (index, step) in acts.iter().enumerate() {
        if let Act::Move(to) = *step {
            y = to;
        }
        drawn.0 += act(cx, layered, y, *step) as usize;
        drawn.1 += act(cx, reference, y, *step) as usize;
        let (shown, expected) = (hovered(cx, layered), hovered(cx, reference));
        let at = format!("after act {index} ({step:?}), uniform: {uniform}, {mode:?}");
        // The pointer is at `y`, in device pixels twice that: a test
        // window's scale factor is 2.
        for lines in [&expected, &shown] {
            let [line] = &lines[..] else {
                panic!("{} rows are shown hovered {at}", lines.len());
            };
            let top: f32 = line
                .split("y: ")
                .nth(1)
                .and_then(|rest| rest.split("px").next())
                .and_then(|top| top.parse().ok())
                .expect("a quad's bounds");
            assert!(
                top <= 2. * y && 2. * y < top + 2. * ROW_HEIGHT,
                "the row from {top} is shown hovered with the pointer at {} {at}",
                2. * y
            );
        }
        assert_eq!(shown, expected, "{at}");
    }
    drawn
}

/// The pointer comes back onto the row it was over before the wheel turned
/// with the pointer elsewhere: the frame that painted the other row hovered
/// is what the move is compared with, so the window draws the row under the
/// pointer hovered again, and the other not. With every mechanism of fast
/// mode, with scroll layers alone and with none.
#[crate::test]
fn a_move_back_onto_a_row_after_a_scroll_draws_its_hover(cx: &mut TestAppContext) {
    for uniform in [true, false] {
        for mode in [Mode::All, Mode::Layers] {
            let acts = back_onto_a_row_after_a_scroll();
            let drawn = compare(cx, uniform, mode, &acts);
            assert_eq!(drawn, (acts.len(), acts.len()), "frames drawn");
        }
    }
}

/// A list shows hovered the row under the pointer after every move and
/// every turn of the wheel, the row its reference shows.
#[crate::test]
fn a_list_shows_the_hover_its_reference_shows(cx: &mut TestAppContext) {
    for uniform in [true, false] {
        for mode in [Mode::All, Mode::Layers] {
            compare(cx, uniform, mode, &moves_between_scrolls());
        }
    }
}

/// A move within the row the pointer is over changes nothing a window
/// shows, and draws no frame.
#[crate::test]
fn a_move_within_a_row_draws_nothing(cx: &mut TestAppContext) {
    for mode in [Mode::All, Mode::Layers, Mode::Reference] {
        let window = open(cx, true, mode);
        act(cx, window, 30., Act::Move(30.));
        act(cx, window, 30., Act::Wheel(-40.));
        act(cx, window, 31., Act::Move(31.));
        let shown = hovered(cx, window);
        assert_eq!(shown.len(), 1);
        for y in [33., 35., 38., 39.] {
            assert!(
                !act(cx, window, y, Act::Move(y)),
                "a frame was drawn for a move to {y} ({mode:?})"
            );
        }
        assert_eq!(hovered(cx, window), shown);
    }
}
