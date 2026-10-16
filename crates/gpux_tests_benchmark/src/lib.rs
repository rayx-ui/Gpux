//! gpux rendering benchmarks: scenarios and the frame driver every target shares.
//!
//! A scenario builds a window's content once and changes it at the start of every frame the way
//! an application would (data arriving, a wheel step, the pointer moving). [`driver::open`] opens
//! a real platform window, drives the scenario from `Window::on_next_frame`, measures the frames
//! and hands a JSON report to the target's entry point, which prints or posts it:
//!
//! - desktop (macOS, Linux, Windows): `src/bin/gpux-bench.rs`;
//! - the browser (WASM): `src/web.rs`;
//! - iOS: `src/mobile.rs`.
//!
//! Fast mode is chosen per window with `Window::set_fast_mode`, so both configurations run the
//! same build and identical scenario code.

pub mod driver;
pub mod metrics;
pub mod scenarios;
pub mod util;

#[cfg(target_family = "wasm")]
mod web;

#[cfg(target_os = "ios")]
pub mod mobile;

use gpux::{AnyView, App, Window};

#[global_allocator]
static ALLOCATOR: metrics::CountingAllocator = metrics::CountingAllocator;

/// What a scenario's frame does: called at the start of frame `frame` (0 is the first frame after
/// the window's first one was presented).
pub type Step = Box<dyn FnMut(u64, &mut Window, &mut App)>;

/// A scenario's window content and its per-frame change.
pub struct Built {
    pub root: AnyView,
    pub step: Step,
}

/// One benchmark scenario.
pub struct Scenario {
    pub id: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    pub build: fn(&mut Window, &mut App) -> Built,
}

/// The scenario called `id`.
pub fn scenario(id: &str) -> Option<&'static Scenario> {
    scenarios::all().iter().find(|scenario| scenario.id == id)
}

/// The logical size the scenarios are laid out for. Desktop windows open at this size; on a
/// phone or in a browser canvas the window is what the target gives, and the scenarios fill it.
pub const WINDOW_WIDTH: f32 = 1280.;
pub const WINDOW_HEIGHT: f32 = 800.;

/// Registers gpux's bundled fonts (IBM Plex Sans, Lilex), which every scenario draws with, so
/// each target shapes the same faces.
pub fn register_fonts(cx: &mut App) {
    cx.text_system()
        .add_fonts(
            gpux_fonts::ALL
                .iter()
                .map(|font| std::borrow::Cow::Borrowed(*font))
                .collect(),
        )
        .expect("registering the bundled fonts");
}

/// The font family every scenario's text uses.
pub const FONT_FAMILY: &str = "IBM Plex Sans";
