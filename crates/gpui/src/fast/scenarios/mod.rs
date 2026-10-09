//! The fast-mode comparison scenarios: fixtures the `fast_mode` bench measures
//! in every cumulative mode and the attribution tests count work in.
//!
//! A [`Scenario`] builds a [`Fixture`]: a root view and a step that changes
//! what one frame changes, the way an application does (notifying views,
//! typing, real wheel and pointer events). After the measured frames the
//! fixture checks the work counters the frames recorded against what its mode
//! must have done ([`Fixture::expect`]), so a faster frame that skipped
//! required work fails the bench and the attribution test alike.
//!
//! Adapted from gpui-fast's `crates/gpui_perf/src/scenarios/` and
//! `crates/gpui/src/fast/tests/retained_bench.rs`. Compiled only under
//! `bench-support` and `test-support`.

mod animation;
mod form;
mod layout;
mod panels;
mod scroll;
mod table;

use std::fmt;

use crate::{AnyView, App, Context, FastMode, FastStats, IntoElement, Render, Window};

/// The cumulative fast modes every scenario runs in: each turns on one more
/// mechanism than the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Fast mode off: the frame gpux draws without it.
    Off,
    /// Layout retention.
    Layout,
    /// Layout and view retention.
    Views,
    /// Layout and view retention and splice.
    Splice,
    /// Every mechanism, scroll layers included where compiled.
    Layers,
}

impl Mode {
    /// Every mode, in cumulative order.
    pub const ALL: [Mode; 5] = [
        Mode::Off,
        Mode::Layout,
        Mode::Views,
        Mode::Splice,
        Mode::Layers,
    ];

    /// The mode's name in bench ids: `off`, `layout`, `views`, `splice`, `layers`.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Layout => "layout",
            Mode::Views => "views",
            Mode::Splice => "splice",
            Mode::Layers => "layers",
        }
    }

    /// The mode before this one, `None` for `off`.
    pub fn previous(self) -> Option<Mode> {
        let index = Mode::ALL.iter().position(|mode| *mode == self)?;
        index.checked_sub(1).map(|index| Mode::ALL[index])
    }

    /// The switches this mode sets, normalized.
    pub fn fast_mode(self) -> FastMode {
        let on = |mode: Mode| self as u8 >= mode as u8;
        FastMode {
            layout_retention: on(Mode::Layout),
            view_retention: on(Mode::Views),
            splice: on(Mode::Splice),
            scroll_layers: on(Mode::Layers),
        }
        .normalized()
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// A scenario's running state: its views and what its steps change.
pub trait Fixture {
    /// The view the window shows.
    fn root(&self) -> AnyView;

    /// Changes what frame `frame` (counting from zero after warm-up) changes.
    fn step(&mut self, frame: usize, window: &mut Window, cx: &mut App);

    /// Checks the counters of the measured frames, `stats`, against what
    /// `mode` must have done. Bounds are per drawn frame (`stats.frames`), so
    /// they hold however a bench's frames were paced.
    fn expect(&self, mode: Mode, stats: &FastStats) -> Result<(), String>;
}

/// A named scenario.
#[derive(Clone, Copy)]
pub struct Scenario {
    /// Short, unique and kebab-case: the `<scenario>` of `fast/<scenario>/<mode>`.
    pub name: &'static str,
    /// What the scenario simulates.
    pub description: &'static str,
    /// Builds the fixture in a window.
    pub build: fn(&mut Window, &mut App) -> Box<dyn Fixture>,
    /// Frames drawn before the measured ones, so caches and retained state
    /// reach their steady state.
    pub warm_up_frames: usize,
}

impl fmt::Display for Scenario {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name)
    }
}

/// Every scenario, in report order.
pub fn all() -> Vec<Scenario> {
    let mut scenarios = Vec::new();
    scenarios.extend(panels::scenarios());
    scenarios.extend(table::scenarios());
    scenarios.extend(layout::scenarios());
    scenarios.extend(form::scenarios());
    scenarios.extend(scroll::scenarios());
    scenarios.extend(animation::scenarios());
    scenarios
}

/// The scenario called `name`.
pub fn scenario(name: &str) -> Option<Scenario> {
    all().into_iter().find(|scenario| scenario.name == name)
}

/// The window's root while a scenario runs: the fixture's view.
pub struct ScenarioRoot(pub AnyView);

impl Render for ScenarioRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

/// Opens `scenario` in `window` in `mode`: builds its fixture, shows its root
/// and sets the mode. The caller draws the warm-up frames.
pub fn open(
    scenario: &Scenario,
    mode: Mode,
    window: &mut Window,
    cx: &mut App,
) -> Box<dyn Fixture> {
    let fixture = (scenario.build)(window, cx);
    let root = fixture.root();
    window.replace_root(cx, |_, _| ScenarioRoot(root));
    window.set_fast_mode(mode.fast_mode());
    fixture
}

/// A check that fails with `message` unless `holds`.
pub(crate) fn check(holds: bool, message: impl FnOnce() -> String) -> Result<(), String> {
    if holds { Ok(()) } else { Err(message()) }
}

/// The work every scenario's frames do in every mode: frames were drawn and
/// painted something, a mode with layout retention claimed kept nodes, and
/// fast mode decided nothing for a mechanism that has not landed or that the
/// mode leaves off.
pub(crate) fn expect_frames(mode: Mode, stats: &FastStats) -> Result<(), String> {
    check(stats.frames > 0, || "no frame was drawn".into())?;
    check(
        stats.primitives_painted + stats.primitives_copied > 0,
        || format!("the frames painted nothing: {stats:?}"),
    )?;
    let layout_retention = mode.fast_mode().layout_retention;
    if layout_retention {
        check(stats.nodes_claimed > 0, || {
            format!("{mode} claimed no kept layout node: {stats:?}")
        })?;
    }
    // Layout and view retention have landed; the other mechanisms work as
    // `off` does.
    let mut landed = 0;
    if layout_retention {
        landed += stats.layout_retention_total();
    }
    if mode.fast_mode().view_retention {
        landed += stats.view_retention_total();
    }
    check(stats.mechanism_total() == landed, || {
        format!("{mode} made a fast-mode decision for a mechanism it does not run: {stats:?}")
    })
}

/// Requires at least `per_frame` of a counter on every drawn frame.
pub(crate) fn at_least_per_frame(
    stats: &FastStats,
    counter: &str,
    per_frame: u64,
) -> Result<(), String> {
    let value = stats.get(counter).unwrap_or(0);
    check(value >= per_frame * stats.frames, || {
        format!(
            "{counter} = {value} over {} frames, below {per_frame} a frame: {stats:?}",
            stats.frames
        )
    })
}

/// Requires every drawn frame to show at least `per_frame` primitives, painted or copied: the
/// glyphs of the scenario's text among them, so a text system that draws nothing fails.
pub(crate) fn drawn_per_frame(stats: &FastStats, per_frame: u64) -> Result<(), String> {
    let drawn = stats.primitives_painted + stats.primitives_copied;
    check(drawn >= per_frame * stats.frames, || {
        format!(
            "{drawn} primitives over {} frames, below {per_frame} a frame: the text drew no \
             glyphs? {stats:?}",
            stats.frames
        )
    })
}

/// Requires exactly `per_frame` of a counter on every drawn frame.
pub(crate) fn exactly_per_frame(
    stats: &FastStats,
    counter: &str,
    per_frame: u64,
) -> Result<(), String> {
    let value = stats.get(counter).unwrap_or(0);
    check(value == per_frame * stats.frames, || {
        format!(
            "{counter} = {value} over {} frames, not {per_frame} a frame: {stats:?}",
            stats.frames
        )
    })
}
