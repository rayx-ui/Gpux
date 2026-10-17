//! Deterministic content for the scenarios: every run of a scenario draws the same thing.

use gpux::{
    App, Hsla, MouseMoveEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent,
    TouchPhase, Window, hsla, point, px,
};

/// A small xorshift generator, seeded per scenario.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound.max(1)
    }

    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

const WORDS: &[&str] = &[
    "render",
    "frame",
    "layout",
    "window",
    "scroll",
    "glyph",
    "shader",
    "atlas",
    "pixel",
    "vertex",
    "texture",
    "buffer",
    "element",
    "view",
    "entity",
    "context",
    "present",
    "surface",
    "metal",
    "quad",
    "shadow",
    "border",
    "corner",
    "radius",
    "gradient",
    "opacity",
    "clip",
    "mask",
    "path",
    "sprite",
    "text",
    "line",
    "wrap",
    "shape",
    "font",
    "weight",
    "style",
    "flex",
    "grid",
    "gap",
    "padding",
    "margin",
    "hover",
    "focus",
    "cursor",
    "input",
    "event",
    "dispatch",
    "notify",
    "update",
    "state",
    "model",
    "store",
    "cache",
    "retain",
    "splice",
    "tile",
    "layer",
    "composite",
];

/// `count` words, separated by spaces.
pub fn words(rng: &mut Rng, count: usize) -> String {
    let mut text = String::with_capacity(count * 7);
    for index in 0..count {
        if index > 0 {
            text.push(' ');
        }
        text.push_str(WORDS[rng.below(WORDS.len() as u64) as usize]);
    }
    text
}

/// A color from a fixed palette of mid-saturation hues.
pub fn color(index: u64) -> Hsla {
    hsla(((index * 37) % 360) as f32 / 360., 0.55, 0.45, 1.)
}

/// A wheel step of `dy` logical pixels with the pointer at `position`.
pub fn wheel(window: &mut Window, cx: &mut App, position: Point<Pixels>, dy: f32) {
    window.dispatch_event(
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            modifiers: Default::default(),
            touch_phase: TouchPhase::Moved,
        }),
        cx,
    );
}

/// The pointer moving to `position` with no button down.
pub fn mouse_move(window: &mut Window, cx: &mut App, position: Point<Pixels>) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Default::default(),
        }),
        cx,
    );
}

/// Frame `frame` of a scroll scenario: the pointer is put over the content on the first frame
/// (a wheel only scrolls what the pointer is over, and the real pointer may be anywhere), then
/// every frame is one wheel step of `step` pixels, reversing every `range` pixels.
pub fn scroll_step(
    frame: u64,
    window: &mut Window,
    cx: &mut App,
    position: Point<Pixels>,
    range: f32,
    step: f32,
) {
    if frame == 0 {
        mouse_move(window, cx, position);
    }
    wheel(window, cx, position, ping_pong(frame, range, step));
}

/// Scrolls back and forth over `range` logical pixels in steps of `step`: frame `frame`'s wheel
/// delta.
pub fn ping_pong(frame: u64, range: f32, step: f32) -> f32 {
    let steps = (range / step).max(1.) as u64;
    if (frame / steps).is_multiple_of(2) {
        -step
    } else {
        step
    }
}
