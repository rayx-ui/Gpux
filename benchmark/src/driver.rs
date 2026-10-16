//! The frame driver: opens a window for a scenario, steps it every frame, measures the frames
//! and reports.
//!
//! A run has three phases. The window opens and its first frame is drawn and presented
//! (`open_to_first_present_ms`, `first_draw_ms`); `warmup` frames let caches, atlases and fast
//! mode's retained state settle; `frames` frames are measured. Each frame's sample spans one
//! `on_next_frame` callback to the next: the scenario's step, the draw and present it caused and
//! whatever else the platform ran on the main thread in between.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpux::{
    AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    Window, WindowOptions, div, point, px,
};
use serde_json::{Value, json};
use web_time::Instant;

use crate::{Scenario, Step, metrics, util};

/// Which fast-mode switches a run asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fast {
    /// What the window starts in without being told: gpux's compiled defaults and the
    /// `GPUX_FAST_MODE` family of environment variables.
    Default,
    /// Every mechanism off (`FastMode::OFF`).
    Off,
    /// Every mechanism on (`FastMode::ALL`, normalized for the target).
    On,
}

impl Fast {
    pub fn parse(value: &str) -> Option<Fast> {
        match value {
            "default" | "" => Some(Fast::Default),
            "0" | "off" => Some(Fast::Off),
            "1" | "on" => Some(Fast::On),
            _ => None,
        }
    }
}

/// What to run.
pub struct Config {
    pub scenario: &'static Scenario,
    pub frames: u64,
    pub warmup: u64,
    /// The configuration's name in the report (`off`, `on`).
    pub label: String,
    /// The target's name in the report (`macos`, `ios-sim`, `web-webgpu`, ...).
    pub target: String,
    pub fast: Fast,
    /// Called when the measured frames begin, so a runner outside the process (the browser's
    /// DevTools protocol) can take its own samples over the same frames.
    pub on_measuring: Option<Box<dyn FnOnce()>>,
}

impl Config {
    /// Reads `--scenario <id> [--fast default|on|off] [--frames n] [--warmup n] [--label name]
    /// [--target name]` from a command line, also returning `--out <file>` if given.
    pub fn from_args(
        args: impl IntoIterator<Item = String>,
        default_target: &str,
    ) -> Result<(Config, Option<String>), String> {
        let mut args = args.into_iter();
        let mut id = None;
        let mut fast = Fast::Default;
        let mut frames = 600;
        let mut warmup = 120;
        let mut label = String::from("unlabelled");
        let mut target = String::from(default_target);
        let mut out = None;
        let number = |value: Option<String>, name: &str| {
            value
                .and_then(|value| value.parse().ok())
                .ok_or(format!("{name} <n>"))
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--scenario" => id = args.next(),
                "--fast" => {
                    fast = args
                        .next()
                        .as_deref()
                        .and_then(Fast::parse)
                        .ok_or("--fast default|on|off")?
                }
                "--frames" => frames = number(args.next(), "--frames")?,
                "--warmup" => warmup = number(args.next(), "--warmup")?,
                "--label" => label = args.next().ok_or("--label <name>")?,
                "--target" => target = args.next().ok_or("--target <name>")?,
                "--out" => out = args.next(),
                // Launchers add their own arguments (Xcode's -NSDocumentRevisionsDebugMode ...).
                _ => {}
            }
        }
        let id = id.ok_or("--scenario <id> is required")?;
        let scenario = crate::scenario(&id).ok_or(format!("no scenario {id}"))?;
        Ok((
            Config {
                scenario,
                frames,
                warmup,
                label,
                target,
                fast,
                on_measuring: None,
            },
            out,
        ))
    }
}

/// Called once with the report, in the frame after the last measured one.
pub type Done = Box<dyn FnOnce(Value, &mut Window, &mut App)>;

struct Root {
    child: AnyView,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(gpux::rgb(0x1e1f24))
            .font_family(crate::FONT_FAMILY)
            .text_color(gpux::rgb(0xe6e6e6))
            .child(self.child.clone())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The window is open; its first frame has not been presented.
    Opening,
    WarmingUp,
    Measuring,
    Done,
}

struct Run {
    config: Config,
    step: Option<Step>,
    done: Option<Done>,
    phase: Phase,
    /// Frames since the first was presented.
    frame: u64,
    opened_at: Instant,
    open_to_first_present: Duration,
    first_draw_ns: u64,
    memory_after_open: metrics::MemorySample,
    memory_after_first_frame: metrics::MemorySample,
    memory_after_warmup: metrics::MemorySample,
    heap_after_warmup: metrics::HeapSample,
    process_cpu_at_start: Option<Duration>,
    wall_at_start: Instant,
    last_tick: Instant,
    last_thread_cpu: Option<Duration>,
    draw_at_start: Option<gpux::FrameDurationSnapshot>,
    intervals_ms: Vec<f64>,
    main_cpu_ms: Vec<f64>,
    inactive_frames: u64,
}

/// Opens the scenario's window with `options` and runs it; `done` gets the report.
pub fn open(cx: &mut App, config: Config, options: WindowOptions, done: Done) {
    let now = Instant::now();
    let run = Rc::new(RefCell::new(Run {
        config,
        step: None,
        done: Some(done),
        phase: Phase::Opening,
        frame: 0,
        opened_at: now,
        open_to_first_present: Duration::ZERO,
        first_draw_ns: 0,
        memory_after_open: Default::default(),
        memory_after_first_frame: Default::default(),
        memory_after_warmup: Default::default(),
        heap_after_warmup: Default::default(),
        process_cpu_at_start: None,
        wall_at_start: now,
        last_tick: now,
        last_thread_cpu: None,
        draw_at_start: None,
        intervals_ms: Vec::new(),
        main_cpu_ms: Vec::new(),
        inactive_frames: 0,
    }));

    let build_run = run.clone();
    let window = cx
        .open_window(options, move |window, cx| {
            let (scenario, fast) = {
                let run = build_run.borrow();
                (run.config.scenario, run.config.fast)
            };
            set_fast_mode(window, fast);
            let built = (scenario.build)(window, cx);
            build_run.borrow_mut().step = Some(built.step);
            cx.new(|_| Root { child: built.root })
        })
        .expect("opening the benchmark window");
    run.borrow_mut().memory_after_open = metrics::memory();
    window
        .update(cx, |_, window, _| schedule(&run, window))
        .expect("the window is open");
}

fn set_fast_mode(window: &mut Window, fast: Fast) {
    match fast {
        Fast::Default => {}
        Fast::Off => window.set_fast_mode(gpux::FastMode::OFF),
        Fast::On => window.set_fast_mode(gpux::FastMode::ALL),
    }
}

fn schedule(run: &Rc<RefCell<Run>>, window: &mut Window) {
    let run = run.clone();
    window.on_next_frame(move |window, cx| tick(&run, window, cx));
}

fn tick(run: &Rc<RefCell<Run>>, window: &mut Window, cx: &mut App) {
    let now = Instant::now();
    let thread_cpu = metrics::thread_cpu();
    let mut state = run.borrow_mut();
    match state.phase {
        Phase::Opening => {
            // The first callback can run before the window's first draw; wait for one that runs
            // after it was presented.
            if window
                .frame_duration_snapshot()
                .draw_duration_histogram
                .is_empty()
            {
                drop(state);
                schedule(run, window);
                return;
            }
            state.open_to_first_present = now - state.opened_at;
            state.first_draw_ns = window
                .frame_duration_snapshot()
                .draw_duration_histogram
                .max();
            state.memory_after_first_frame = metrics::memory();
            state.phase = Phase::WarmingUp;
            state.frame = 0;
        }
        Phase::WarmingUp => {
            if state.frame >= state.config.warmup {
                state.phase = Phase::Measuring;
                state.memory_after_warmup = metrics::memory();
                state.heap_after_warmup = metrics::heap();
                state.process_cpu_at_start = metrics::process_cpu();
                state.wall_at_start = now;
                state.draw_at_start = Some(window.frame_duration_snapshot());
                reset_fast_stats(window);
                metrics::gpu_times_begin();
                if let Some(on_measuring) = state.config.on_measuring.take() {
                    on_measuring();
                }
            }
        }
        Phase::Measuring => {
            let interval = (now - state.last_tick).as_secs_f64() * 1e3;
            state.intervals_ms.push(interval);
            if let (Some(now_cpu), Some(last_cpu)) = (thread_cpu, state.last_thread_cpu) {
                let main_cpu = now_cpu.saturating_sub(last_cpu).as_secs_f64() * 1e3;
                state.main_cpu_ms.push(main_cpu);
            }
            if !window.is_window_active() {
                state.inactive_frames += 1;
            }
            if state.intervals_ms.len() as u64 >= state.config.frames {
                state.phase = Phase::Done;
                let report = report(&state, window);
                let done = state.done.take().expect("a run reports once");
                drop(state);
                done(report, window, cx);
                return;
            }
        }
        Phase::Done => return,
    }
    state.last_tick = now;
    let frame = state.frame;
    state.frame += 1;
    let first_step = state.phase == Phase::WarmingUp && frame == 0;
    let mut step = state.step.take().expect("the scenario's step");
    drop(state);
    if first_step {
        // The real pointer may be anywhere; every run starts with it at the same place, in the
        // window's bottom-right corner, until the scenario moves it.
        let viewport = window.viewport_size();
        util::mouse_move(
            window,
            cx,
            point(viewport.width - px(2.), viewport.height - px(2.)),
        );
    }
    step(frame, window, cx);
    let mut state = run.borrow_mut();
    state.step = Some(step);
    // Measured CPU starts after the step, so a frame's sample is its draw and present, the
    // platform's work around them, and the next frame's step.
    state.last_thread_cpu = metrics::thread_cpu();
    drop(state);
    schedule(run, window);
}

fn ms(nanos: u64) -> f64 {
    nanos as f64 / 1e6
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024. * 1024.)
}

fn summary_json(samples: &[f64]) -> Value {
    if samples.is_empty() {
        return Value::Null;
    }
    let summary = metrics::summarize(samples);
    json!({
        "mean": summary.mean,
        "p50": summary.p50,
        "p95": summary.p95,
        "p99": summary.p99,
        "max": summary.max,
    })
}

fn histogram_json(
    end: &gpux::FrameDurationSnapshot,
    start: &gpux::FrameDurationSnapshot,
    draw: bool,
) -> Value {
    let (mut histogram, earlier) = if draw {
        (
            end.draw_duration_histogram.clone(),
            &start.draw_duration_histogram,
        )
    } else {
        (
            end.dirty_to_present_histogram.clone(),
            &start.dirty_to_present_histogram,
        )
    };
    histogram.subtract(earlier).ok();
    json!({
        "count": histogram.len(),
        "mean": histogram.mean() / 1e6,
        "p50": ms(histogram.value_at_quantile(0.50)),
        "p95": ms(histogram.value_at_quantile(0.95)),
        "p99": ms(histogram.value_at_quantile(0.99)),
        "max": ms(histogram.max()),
    })
}

fn report(state: &Run, window: &mut Window) -> Value {
    let frames = state.intervals_ms.len() as f64;
    let wall = state.wall_at_start.elapsed();
    let process_cpu = metrics::process_cpu()
        .zip(state.process_cpu_at_start)
        .map(|(end, start)| end.saturating_sub(start));
    let heap = metrics::heap();
    let memory = metrics::memory();
    // A frame that took more than one and a half of the display's interval missed a vsync.
    let mut sorted = state.intervals_ms.clone();
    sorted.sort_by(f64::total_cmp);
    let vsync = sorted[sorted.len() / 10];
    let janky = state
        .intervals_ms
        .iter()
        .filter(|interval| **interval > vsync * 1.5)
        .count();
    let draw_end = window.frame_duration_snapshot();
    let draw_start = state
        .draw_at_start
        .as_ref()
        .expect("measured from the warm-up's end");
    let viewport = window.viewport_size();

    json!({
        "scenario": state.config.scenario.id,
        "group": state.config.scenario.group,
        "label": state.config.label,
        "target": state.config.target,
        "fast_mode": fast_mode(window),
        "fast_stats": fast_stats(window),
        "viewport": [f32::from(viewport.width), f32::from(viewport.height)],
        "scale_factor": window.scale_factor(),
        "frames": frames,
        "warmup": state.config.warmup,
        "wall_s": wall.as_secs_f64(),
        "fps": frames / wall.as_secs_f64(),
        "vsync_ms": vsync,
        "janky_frames": janky,
        "inactive_frames": state.inactive_frames,
        "open_to_first_present_ms": state.open_to_first_present.as_secs_f64() * 1e3,
        "first_draw_ms": ms(state.first_draw_ns),
        "draw_ms": histogram_json(&draw_end, draw_start, true),
        "frame_ms": histogram_json(&draw_end, draw_start, false),
        "main_cpu_ms": summary_json(&state.main_cpu_ms),
        "gpu_ms": summary_json(&metrics::gpu_times_take()),
        "interval_ms": summary_json(&state.intervals_ms),
        "process_cpu_ms_per_frame": process_cpu.map(|cpu| cpu.as_secs_f64() * 1e3 / frames),
        "process_cpu_pct": process_cpu.map(|cpu| cpu.as_secs_f64() / wall.as_secs_f64() * 100.),
        "allocations_per_frame": (heap.allocations - state.heap_after_warmup.allocations) as f64 / frames,
        "allocated_kb_per_frame": (heap.allocated_bytes - state.heap_after_warmup.allocated_bytes) as f64 / frames / 1024.,
        "heap_live_mb": mb(heap.live_bytes),
        "heap_peak_mb": mb(heap.peak_live_bytes),
        "footprint_mb": {
            "after_open": mb(state.memory_after_open.footprint),
            "after_first_frame": mb(state.memory_after_first_frame.footprint),
            "after_warmup": mb(state.memory_after_warmup.footprint),
            "end": mb(memory.footprint),
            "peak": mb(memory.footprint_peak),
        },
        "resident_mb": mb(memory.resident),
    })
}

fn fast_mode(window: &Window) -> Value {
    Value::String(format!("{:?}", window.fast_mode()))
}

#[cfg(feature = "fast-stats")]
fn fast_stats(window: &Window) -> Value {
    Value::String(window.fast_stats().bench_line("frames"))
}

#[cfg(not(feature = "fast-stats"))]
fn fast_stats(_: &Window) -> Value {
    Value::Null
}

/// Counters cover the measured frames only.
#[cfg(feature = "fast-stats")]
fn reset_fast_stats(window: &mut Window) {
    window.reset_fast_stats();
}

#[cfg(not(feature = "fast-stats"))]
fn reset_fast_stats(_: &mut Window) {}
