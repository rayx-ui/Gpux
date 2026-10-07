//! The in-app host: runs scenes in real platform windows inside a running application (lane D's
//! desktop host, and later the browser and mobile hosts).
//!
//! Every step waits for real frames on the real clock. Input is injected into the window through
//! `Window::dispatch_event`; pixels come from `Window::render_to_image` on the real surface.
//! Platforms re-read the OS cursor while a window is hovered (and, on macOS, while it is active)
//! or a drag is in progress, so a host can sync the OS cursor to every injected pointer position
//! through [`AppHost::with_cursor_sync`]; without it, keep the cursor away from scene windows.

use crate::{
    evaluate::{self, center},
    host::{StepOutcome, describe},
    report::{Outcome, ResultLine},
    scene::{Capability, Expect, Lane, SceneSpec, Step, flatten},
};
use anyhow::{anyhow, bail};
use gpux::{
    AnyView, AnyWindowHandle, App, AppContext as _, AsyncApp, Bounds, Context, IntoElement,
    KeyUpEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, PlatformInput, Point, Render, ScrollDelta, ScrollWheelEvent,
    Styled as _, TitlebarOptions, TouchEvent, TouchId, TouchPhase, Window, WindowBounds,
    WindowOptions, div, point, px, rgb, semantic_query, size,
};
use image::RgbaImage;
use std::{
    borrow::Cow, cell::Cell, collections::HashMap, future::Future, pin::Pin, rc::Rc, time::Duration,
};

/// The smallest window the host opens, so platforms do not clamp narrow scenes; scenes render
/// at their own size in its top-left corner.
const MIN_WINDOW_SIZE: (f32, f32) = (360., 240.);

/// How long the host waits for one frame before failing the step.
const FRAME_TIMEOUT: Duration = Duration::from_secs(10);

/// One frame of an injected touch.
const TOUCH_FRAME: Duration = Duration::from_millis(16);

/// Moves the OS cursor to a window-local logical position.
pub type CursorSync = Rc<dyn Fn(&Window, Point<Pixels>)>;

/// Registers the bundled fonts with the app's text system, so scenes shape the same text as on
/// the test platform.
pub fn register_bundled_fonts(cx: &mut App) -> anyhow::Result<()> {
    cx.text_system().add_fonts(
        gpux_fonts::ALL
            .iter()
            .map(|font| Cow::Borrowed(*font))
            .collect(),
    )
}

/// Opens a scene's window in the top-left corner of `origin`'s display area.
pub fn window_options(scene: &SceneSpec, origin: Point<Pixels>) -> WindowOptions {
    let size = size(
        px(scene.size.0.max(MIN_WINDOW_SIZE.0)),
        px(scene.size.1.max(MIN_WINDOW_SIZE.1)),
    );
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin, size })),
        titlebar: Some(TitlebarOptions {
            title: Some(format!("gpux conformance: {}", scene.id).into()),
            ..Default::default()
        }),
        focus: true,
        show: true,
        is_resizable: false,
        ..Default::default()
    }
}

/// Hosts a scene at its own size in the window's top-left corner.
pub struct SceneFrame {
    view: AnyView,
    size: (f32, f32),
}

impl SceneFrame {
    /// Builds the scene's view inside a frame of the scene's size.
    pub fn new(scene: &SceneSpec, window: &mut Window, cx: &mut App) -> Self {
        Self {
            view: (scene.build)(window, cx),
            size: scene.size,
        }
    }
}

impl Render for SceneFrame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xffffff)).child(
            div()
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .w(px(self.size.0))
                .h(px(self.size.1))
                .child(self.view.clone()),
        )
    }
}

/// Runs scenes in real windows of the running application.
pub struct AppHost {
    lane: Lane,
    origin: Point<Pixels>,
    cursor_sync: Option<CursorSync>,
    failure_frames: Option<std::path::PathBuf>,
}

impl AppHost {
    /// A host for `lane` that opens its windows at `origin`.
    pub fn new(lane: Lane, origin: Point<Pixels>) -> Self {
        Self {
            lane,
            origin,
            cursor_sync: None,
            failure_frames: None,
        }
    }

    /// Saves the frame a failing pixel step saw as `<dir>/<scene>.actual.png`.
    pub fn with_failure_frames(mut self, dir: std::path::PathBuf) -> Self {
        self.failure_frames = Some(dir);
        self
    }

    /// Moves the OS cursor to every injected pointer position, so platforms that re-read the
    /// cursor while a window is hovered, active or dragging see the injected position.
    pub fn with_cursor_sync(mut self, sync: CursorSync) -> Self {
        self.cursor_sync = Some(sync);
        self
    }

    /// Runs `scene` in a new window and returns its result line.
    pub async fn run_scene(&self, cx: &mut AsyncApp, scene: &'static SceneSpec) -> ResultLine {
        let id = scene.id.to_string();
        if !scene.runs_in(self.lane) {
            return ResultLine::new(
                Outcome::Skip,
                id,
                format!("not a lane {} scene", self.lane.as_str()),
            );
        }
        let window = match cx.open_window(window_options(scene, self.origin), |window, cx| {
            cx.new(|cx| SceneFrame::new(scene, window, cx))
        }) {
            Ok(window) => AnyWindowHandle::from(window),
            Err(error) => {
                return ResultLine::new(Outcome::Fail, id, format!("could not open: {error:#}"));
            }
        };
        let mut run = SceneRun {
            host: self,
            scene_id: scene.id,
            window,
            frame: None,
            captures: HashMap::new(),
        };
        let script = (scene.script)();
        let total = flatten(&script).len();
        let mut skipped = Vec::new();
        let mut number = 0;
        let result = match run.settle(cx).await {
            Ok(()) => run.steps(cx, &script, &mut number, &mut skipped).await,
            Err(error) => Err(format!("the first frame never arrived: {error:#}")),
        };
        window
            .update(cx, |_, window, _| window.remove_window())
            .ok();
        if let Err(detail) = result {
            return ResultLine::new(Outcome::Fail, id, detail);
        }
        let ran = total - skipped.len();
        let mut detail = format!("{ran} steps");
        if !skipped.is_empty() {
            let count = skipped.len();
            skipped.sort();
            skipped.dedup();
            detail.push_str(&format!(", {count} skipped ({})", skipped.join("; ")));
        }
        ResultLine::new(Outcome::Pass, id, detail)
    }
}

/// Renders the window's current frame back from its surface. A windowed renderer captures into
/// a freshly acquired surface texture, which can briefly be unavailable while earlier frames
/// are still presenting (software rasterizers without a vertical sync source); those captures
/// are retried after another frame.
pub async fn render_to_image(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
) -> anyhow::Result<RgbaImage> {
    const ATTEMPTS: usize = 5;
    let mut attempt = 1;
    loop {
        match window.update(cx, |_, window, _| window.render_to_image())? {
            Ok(image) => return Ok(image),
            Err(error) if attempt < ATTEMPTS && is_transient_capture_error(&error) => {
                attempt += 1;
                wait_frames(cx, window, 1).await?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_transient_capture_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    [
        "Timed out acquiring",
        "became suboptimal",
        "was lost",
        "while the renderer is recovering",
    ]
    .iter()
    .any(|transient| message.contains(transient))
}

/// Waits until the window has drawn `frames` more frames.
pub async fn wait_frames(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    frames: usize,
) -> anyhow::Result<()> {
    for _ in 0..frames {
        let drawn = Rc::new(Cell::new(false));
        let flag = drawn.clone();
        window.update(cx, |_, window, _| {
            window.on_next_frame(move |_, _| flag.set(true));
            window.refresh();
        })?;
        let executor = cx.background_executor().clone();
        let start = executor.now();
        while !drawn.get() {
            if executor.now().duration_since(start) > FRAME_TIMEOUT {
                bail!("no frame within {FRAME_TIMEOUT:?}");
            }
            executor.timer(Duration::from_millis(4)).await;
        }
    }
    Ok(())
}

struct SceneRun<'a> {
    host: &'a AppHost,
    scene_id: &'static str,
    window: AnyWindowHandle,
    frame: Option<RgbaImage>,
    captures: HashMap<&'static str, RgbaImage>,
}

impl SceneRun<'_> {
    /// Waits for two frames, so the last change has been laid out, painted and presented.
    async fn settle(&mut self, cx: &mut AsyncApp) -> anyhow::Result<()> {
        wait_frames(cx, self.window, 2).await
    }

    fn steps<'s>(
        &'s mut self,
        cx: &'s mut AsyncApp,
        steps: &'s [Step],
        number: &'s mut usize,
        skipped: &'s mut Vec<String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + 's>> {
        Box::pin(async move {
            for step in steps {
                if let Step::When(capability, inner) = step {
                    if self.supports(cx, *capability) {
                        self.steps(cx, inner, number, skipped).await?;
                    } else {
                        let count = flatten(inner).len();
                        *number += count;
                        skipped.extend(std::iter::repeat_n(
                            format!("the renderer has no {}", capability.describe()),
                            count,
                        ));
                    }
                    continue;
                }
                *number += 1;
                match self.step(cx, step).await {
                    Ok(StepOutcome::Done) => {}
                    Ok(StepOutcome::Skipped(reason)) => skipped.push(reason),
                    Err(error) => {
                        return Err(format!("step {} {}: {error:#}", number, describe(step)));
                    }
                }
            }
            Ok(())
        })
    }

    fn supports(&self, cx: &mut AsyncApp, capability: Capability) -> bool {
        let Ok(capabilities) = self
            .window
            .update(cx, |_, window, _| window.effect_capabilities())
        else {
            return false;
        };
        match capability {
            Capability::ElementEffects => {
                capabilities.element.backdrop_blur && capabilities.element.glow
            }
            Capability::SnapshotTransitions => capabilities.screen.snapshot_transitions,
        }
    }

    fn snapshot(&self, cx: &mut AsyncApp) -> anyhow::Result<gpux::DevtoolsFrameSnapshot> {
        self.window
            .update(cx, |_, window, _| window.devtools_frame_snapshot())
    }

    fn scale(&self, cx: &mut AsyncApp) -> anyhow::Result<f32> {
        self.window.update(cx, |_, window, _| window.scale_factor())
    }

    async fn locate(
        &mut self,
        cx: &mut AsyncApp,
        query: &semantic_query::Query,
    ) -> anyhow::Result<gpux::DevtoolsElementSnapshot> {
        self.settle(cx).await?;
        evaluate::locate(&self.snapshot(cx)?, query)
    }

    /// Injects `event`, syncing the OS cursor to its position first when the host can.
    fn inject(&self, cx: &mut AsyncApp, event: PlatformInput) -> anyhow::Result<()> {
        let position = match &event {
            PlatformInput::MouseDown(event) => Some(event.position),
            PlatformInput::MouseUp(event) => Some(event.position),
            PlatformInput::MouseMove(event) => Some(event.position),
            PlatformInput::ScrollWheel(event) => Some(event.position),
            _ => None,
        };
        let sync = self.host.cursor_sync.clone();
        self.window.update(cx, |_, window, cx| {
            if let (Some(sync), Some(position)) = (sync, position) {
                sync(window, position);
            }
            window.dispatch_event(event, cx);
        })
    }

    fn mouse_down(position: Point<Pixels>, click_count: usize) -> PlatformInput {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::none(),
            click_count,
            first_mouse: false,
        })
    }

    fn mouse_up(position: Point<Pixels>, click_count: usize) -> PlatformInput {
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::none(),
            click_count,
        })
    }

    fn mouse_move(position: Point<Pixels>, pressed: Option<MouseButton>) -> PlatformInput {
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: pressed,
            modifiers: Modifiers::none(),
        })
    }

    async fn click(
        &mut self,
        cx: &mut AsyncApp,
        position: Point<Pixels>,
        click_count: usize,
    ) -> anyhow::Result<()> {
        self.inject(cx, Self::mouse_move(position, None))?;
        self.inject(cx, Self::mouse_down(position, click_count))?;
        // A press and its release arrive in separate frames on real platforms; elements that
        // record the press while painting (interactive text) depend on it.
        wait_frames(cx, self.window, 1).await?;
        self.inject(cx, Self::mouse_up(position, click_count))
    }

    async fn touch(
        &mut self,
        cx: &mut AsyncApp,
        phase: TouchPhase,
        position: Point<Pixels>,
    ) -> anyhow::Result<()> {
        self.inject(
            cx,
            PlatformInput::Touch(TouchEvent {
                id: TouchId(1),
                phase,
                position,
                predicted_position: None,
                force: None,
                timestamp: None,
            }),
        )
    }

    async fn focus(
        &mut self,
        cx: &mut AsyncApp,
        query: &semantic_query::Query,
    ) -> anyhow::Result<()> {
        self.settle(cx).await?;
        let snapshot = self.snapshot(cx)?;
        let element = query.resolve(&snapshot)?.clone();
        if !semantic_query::focus_is_within(&snapshot, &element) {
            self.click(cx, center(&element), 1).await?;
            // The focused element registers its text input handler while it paints.
            self.settle(cx).await?;
        }
        Ok(())
    }

    async fn frame(&mut self, cx: &mut AsyncApp) -> anyhow::Result<RgbaImage> {
        if self.frame.is_none() {
            self.settle(cx).await?;
            let image = render_to_image(cx, self.window).await?;
            self.frame = Some(image);
        }
        Ok(self.frame.clone().expect("rendered above"))
    }

    async fn step(&mut self, cx: &mut AsyncApp, step: &Step) -> anyhow::Result<StepOutcome> {
        if !step.needs_pixels() && !matches!(step, Step::Expect(_)) {
            self.frame = None;
        }
        match step {
            Step::Click(query) => {
                let at = center(&self.locate(cx, query).await?);
                self.click(cx, at, 1).await?;
            }
            Step::ClickCount(query, count) => {
                let at = center(&self.locate(cx, query).await?);
                self.click(cx, at, *count).await?;
            }
            Step::ClickAt(x, y) => self.click(cx, point(px(*x), px(*y)), 1).await?,
            Step::ClickCountAt(x, y, count) => {
                self.click(cx, point(px(*x), px(*y)), *count).await?
            }
            Step::Hover(query) => {
                let at = center(&self.locate(cx, query).await?);
                self.inject(cx, Self::mouse_move(at, None))?;
            }
            Step::MoveTo(x, y) => self.inject(cx, Self::mouse_move(point(px(*x), px(*y)), None))?,
            Step::Press(query) => {
                let at = center(&self.locate(cx, query).await?);
                self.inject(cx, Self::mouse_move(at, None))?;
                self.inject(cx, Self::mouse_down(at, 1))?;
            }
            Step::Release(query) => {
                let at = center(&self.locate(cx, query).await?);
                self.inject(cx, Self::mouse_up(at, 1))?;
            }
            Step::Drag(from, to) => {
                let start = center(&self.locate(cx, from).await?);
                let end = center(&self.locate(cx, to).await?);
                self.inject(cx, Self::mouse_move(start, None))?;
                self.inject(cx, Self::mouse_down(start, 1))?;
                const STEPS: usize = 4;
                for index in 1..=STEPS {
                    let t = index as f32 / STEPS as f32;
                    let at = point(
                        start.x + (end.x - start.x) * t,
                        start.y + (end.y - start.y) * t,
                    );
                    self.inject(cx, Self::mouse_move(at, Some(MouseButton::Left)))?;
                    self.settle(cx).await?;
                }
                self.inject(cx, Self::mouse_up(end, 1))?;
            }
            Step::Wheel(query, dx, dy) => {
                let at = center(&self.locate(cx, query).await?);
                self.inject(
                    cx,
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: at,
                        delta: ScrollDelta::Pixels(point(px(*dx), px(*dy))),
                        modifiers: Modifiers::none(),
                        touch_phase: TouchPhase::Moved,
                    }),
                )?;
            }
            Step::TouchPan(query, dx, dy, steps) => {
                let start = center(&self.locate(cx, query).await?);
                self.touch(cx, TouchPhase::Started, start).await?;
                let steps = (*steps).max(1);
                for index in 1..=steps {
                    cx.background_executor().timer(TOUCH_FRAME).await;
                    let t = index as f32 / steps as f32;
                    let at = point(start.x + px(dx * t), start.y + px(dy * t));
                    self.touch(cx, TouchPhase::Moved, at).await?;
                }
                cx.background_executor().timer(TOUCH_FRAME).await;
                let end = point(start.x + px(*dx), start.y + px(*dy));
                self.touch(cx, TouchPhase::Ended, end).await?;
            }
            Step::TouchTap(query) => {
                let at = center(&self.locate(cx, query).await?);
                self.touch(cx, TouchPhase::Started, at).await?;
                cx.background_executor().timer(TOUCH_FRAME).await;
                self.touch(cx, TouchPhase::Ended, at).await?;
            }
            Step::Type(query, text) => {
                self.focus(cx, query).await?;
                let accepted = self
                    .window
                    .update(cx, |_, window, cx| window.dispatch_text_input(text, cx))?;
                if !accepted {
                    bail!("no element accepts text input");
                }
            }
            Step::Keys(keys) => {
                for key in keys.split_whitespace() {
                    let keystroke = Keystroke::parse(key)
                        .map_err(|error| anyhow!("bad keystroke {key:?}: {error}"))?;
                    self.window
                        .update(cx, |_, window, cx| window.dispatch_keystroke(keystroke, cx))?;
                    self.settle(cx).await?;
                }
            }
            Step::KeyUp(key) => {
                let keystroke = Keystroke::parse(key)
                    .map_err(|error| anyhow!("bad keystroke {key:?}: {error}"))?;
                self.inject(cx, PlatformInput::KeyUp(KeyUpEvent { keystroke }))?;
            }
            Step::ImeCompose(..) | Step::ImeCommit(_) | Step::ImeCancel => {
                return Ok(StepOutcome::Skipped(
                    "IME composition needs the platform's input method".into(),
                ));
            }
            Step::Focus(query) => self.focus(cx, query).await?,
            Step::Advance(duration) => {
                cx.background_executor().timer(*duration).await;
            }
            Step::Expect(Expect::Accessible(_)) => {
                return Ok(StepOutcome::Skipped(
                    "the accessibility tree needs an assistive-technology client".into(),
                ));
            }
            Step::Expect(expect) => {
                self.settle(cx).await?;
                let scale = self.scale(cx)?;
                // Layout snaps to device pixels, so allow one device pixel at the platform scale.
                evaluate::expect(&self.snapshot(cx)?, expect, 1. / scale.max(1.))?;
            }
            Step::Probe(..)
            | Step::ProbeUniform(..)
            | Step::ProbeDiffer(..)
            | Step::Capture(_)
            | Step::ChangedSince(..) => {
                let frame = self.frame(cx).await?;
                let scale = self.scale(cx)?;
                if let Err(error) = evaluate::pixel_step(step, &frame, scale, &mut self.captures) {
                    let mut error = error.context(format!("at scale {scale}"));
                    if let Some(dir) = &self.host.failure_frames {
                        let path = dir.join(format!("{}.actual.png", self.scene_id));
                        if std::fs::create_dir_all(dir).is_ok()
                            && crate::compare::save_png(&frame, &path).is_ok()
                        {
                            error = error.context(format!("frame saved to {}", path.display()));
                        }
                    }
                    return Err(error);
                }
            }
            Step::Golden | Step::GoldenWithin(_) => {
                return Ok(StepOutcome::Skipped(format!(
                    "lane {} keeps no goldens",
                    self.host.lane.as_str()
                )));
            }
            Step::When(..) => bail!("`When` steps are resolved before they reach a step"),
        }
        Ok(StepOutcome::Done)
    }
}
