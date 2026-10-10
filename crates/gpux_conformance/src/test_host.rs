//! The test-platform host: lane A runs scenes on `TestPlatform` with the bundled text system and
//! the deterministic executor clock; lane C adds the platform's headless renderer and checks
//! pixels and goldens.
//!
//! Lane C renders on the software adapter by default (WARP on Windows, lavapipe on Linux; without
//! a Vulkan adapter Linux falls back to llvmpipe over GL, where the effects pipeline does not yet
//! validate), so its output is deterministic and compared with goldens wherever it runs.
//! `GPUX_WGPU_ADAPTER=hardware` (or an adapter name) renders on a GPU instead, as lane G does;
//! hardware adapters skip goldens. macOS renders through Metal. [`TestHost::probes_only`] skips
//! goldens on any backend.

use crate::{
    backend::BackendId,
    compare::{GoldenName, GoldenOutcome, Goldens},
    evaluate::{self, center},
    host::{Host, StepOutcome},
    scene::{Capability, Expect, Lane, SceneSpec, Step},
};
use anyhow::{Context as _, anyhow, bail};
use gpux::{
    AnyView, AnyWindowHandle, Context, DevtoolsElementSnapshot, FastMode, IntoElement, Modifiers,
    MouseButton, Pixels, PlatformHeadlessRenderer, Point, Render, TestAppContext, TestDispatcher,
    TestWindowOptions, TouchEvent, TouchId, TouchPhase, VisualTestContext, Window, point, px,
    semantic_query::Query, size,
};
use image::RgbaImage;
use std::{collections::HashMap, time::Duration};

/// One frame of a simulated touch.
const TOUCH_FRAME: Duration = Duration::from_millis(16);

struct SceneRoot(AnyView);

impl Render for SceneRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

struct OpenScene {
    cx: TestAppContext,
    window: AnyWindowHandle,
    scale: f32,
    frame: Option<RgbaImage>,
    captures: HashMap<&'static str, RgbaImage>,
    backend: Option<BackendId>,
}

/// What the window showed after one step, recorded by [`TestHost::recording_frames`].
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedFrame {
    /// The step's number in the flattened script, counting from one; zero is the frame the
    /// scene opened with.
    pub step: usize,
    /// [`gpux::Window::describe_rendered_frame`] of the last frame drawn.
    pub description: Vec<String>,
    /// A screenshot of the window, when the host renders pixels.
    pub pixels: Option<RgbaImage>,
}

/// Runs scenes on the test platform.
pub struct TestHost {
    lane: Lane,
    seed: u64,
    goldens: Goldens,
    probes_only: bool,
    open: Option<OpenScene>,
    backend: Option<BackendId>,
    fast_mode: Option<FastMode>,
    recording: Option<Vec<RecordedFrame>>,
    /// Steps run in the open scene, for numbering recorded frames.
    steps_run: usize,
}

impl TestHost {
    /// Lane A: behaviour with real text and the deterministic clock, no renderer.
    pub fn behaviour() -> Self {
        Self::new(Lane::A)
    }

    /// Lane C: lane A plus the platform's headless renderer for pixels and goldens. On Linux
    /// and Windows it renders on the software adapter unless `GPUX_WGPU_ADAPTER` picks another.
    pub fn pixels() -> Self {
        Self::new(Lane::C)
    }

    fn new(lane: Lane) -> Self {
        Self {
            lane,
            seed: 0,
            goldens: Goldens::for_lane(lane.as_str()),
            probes_only: false,
            open: None,
            backend: None,
            fast_mode: None,
            recording: None,
            steps_run: 0,
        }
    }

    /// Opens every scene's window in `mode` instead of the mode the environment gives it. The
    /// fast-mode parity groups run each scene with fast mode all on and all off.
    pub fn with_fast_mode(mut self, mode: FastMode) -> Self {
        self.fast_mode = Some(mode);
        self
    }

    /// Records what the window shows when the scene opens and after every step that ran: the
    /// frame description and, when the host renders pixels, a screenshot.
    pub fn recording_frames(mut self) -> Self {
        self.recording = Some(Vec::new());
        self
    }

    /// The frames recorded since the host was made, in order.
    pub fn recorded_frames(&self) -> &[RecordedFrame] {
        self.recording.as_deref().unwrap_or_default()
    }

    /// Records the open scene's current frame as the frame after `step`.
    fn record(&mut self, step: usize) -> anyhow::Result<()> {
        let Some(recording) = self.recording.as_mut() else {
            return Ok(());
        };
        let open = self
            .open
            .as_mut()
            .ok_or_else(|| anyhow!("no scene is open"))?;
        let mut vcx = open.vcx();
        let description = vcx.update(|window, _| window.describe_rendered_frame());
        let pixels = if open.backend.is_some() {
            Some(open.frame()?.clone())
        } else {
            None
        };
        recording.push(RecordedFrame {
            step,
            description,
            pixels,
        });
        Ok(())
    }

    /// Skips golden steps, leaving the relative probes: lane G renders on the developer's GPU,
    /// which no committed golden matches (Metal goldens come from CI's virtual Macs).
    pub fn probes_only(mut self) -> Self {
        self.probes_only = true;
        self
    }

    /// Uses `goldens` instead of the committed ones (for tests of the host itself).
    pub fn with_goldens(mut self, goldens: Goldens) -> Self {
        self.goldens = goldens;
        self
    }

    /// The backend the most recently opened scene rendered on, when the host renders pixels.
    pub fn backend(&self) -> Option<&BackendId> {
        self.backend.as_ref()
    }

    fn scene(&mut self) -> anyhow::Result<&mut OpenScene> {
        self.open
            .as_mut()
            .ok_or_else(|| anyhow!("no scene is open"))
    }
}

impl OpenScene {
    fn vcx(&self) -> VisualTestContext {
        VisualTestContext::from_window(self.window, &self.cx)
    }

    fn locate(&self, query: &Query) -> anyhow::Result<DevtoolsElementSnapshot> {
        Ok(self.vcx().locate(query.clone())?)
    }

    fn frame(&mut self) -> anyhow::Result<&RgbaImage> {
        if self.frame.is_none() {
            self.frame = Some(self.vcx().capture_screenshot()?);
        }
        Ok(self.frame.as_ref().expect("captured above"))
    }

    fn touch(&mut self, phase: TouchPhase, position: Point<Pixels>) {
        self.vcx().simulate_event(TouchEvent {
            id: TouchId(1),
            phase,
            position,
            predicted_position: None,
            force: None,
            timestamp: None,
        });
    }

    fn expect(&mut self, expect: &Expect) -> anyhow::Result<()> {
        let mut vcx = self.vcx();
        match expect {
            Expect::Accessible(label) => {
                vcx.deactivate_accessibility(self.window);
                vcx.take_accessibility_updates(self.window);
                let initial = vcx
                    .activate_accessibility(self.window)
                    .context("the window has no accessibility adapter")?;
                // Activation builds the tree on the next frame.
                vcx.update(|window, _| window.refresh());
                vcx.run_until_parked();
                let updates = vcx.take_accessibility_updates(self.window);
                let labels = std::iter::once(&initial)
                    .chain(&updates)
                    .flat_map(|update| update.nodes.iter())
                    .filter_map(|(_, node)| node.label().map(str::to_string))
                    .collect::<Vec<_>>();
                vcx.deactivate_accessibility(self.window);
                if !labels.iter().any(|candidate| candidate == label) {
                    bail!("no accessibility node is labelled {label:?}; labels: {labels:?}");
                }
            }
            expect => evaluate::expect(&vcx.settled_frame_snapshot(), expect, 0.)?,
        }
        Ok(())
    }
}

impl Host for TestHost {
    fn lane(&self) -> Lane {
        self.lane
    }

    fn open(&mut self, scene: &SceneSpec, scale: f32) -> anyhow::Result<()> {
        self.close();
        let dispatcher = TestDispatcher::new(self.seed);
        let mut cx = if self.lane.has_pixels() {
            TestAppContext::build_with_renderer(
                dispatcher,
                None,
                gpui_platform::bundled_text_system(),
                lane_c_renderer,
            )
        } else {
            TestAppContext::build_with_text_system(
                dispatcher,
                None,
                gpui_platform::bundled_text_system(),
            )
        };
        let build = scene.build;
        let handle = cx.add_window_with_options(
            TestWindowOptions::new(size(px(scene.size.0), px(scene.size.1))).scale_factor(scale),
            move |window, cx| SceneRoot(build(window, cx)),
        );
        let window: AnyWindowHandle = handle.into();
        let mut vcx = VisualTestContext::from_window(window, &cx);
        if let Some(mode) = self.fast_mode {
            vcx.update(|window, _| window.set_fast_mode(mode));
        }
        // Active windows deliver every frame; inactive ones throttle frames to ~30 per second.
        vcx.update(|window, _| window.activate_window());
        vcx.run_until_parked();
        let backend = if self.lane.has_pixels() {
            let specs = vcx.update(|window, _| window.gpu_specs()).context(
                "the headless renderer reports no GPU; this OS has no headless renderer",
            )?;
            Some(BackendId::from_specs(&specs))
        } else {
            None
        };
        self.backend = backend.clone();
        self.open = Some(OpenScene {
            cx,
            window,
            scale,
            frame: None,
            captures: HashMap::new(),
            backend,
        });
        self.steps_run = 0;
        self.record(0)
    }

    fn step(&mut self, scene: &SceneSpec, step: &Step) -> anyhow::Result<StepOutcome> {
        self.steps_run += 1;
        let outcome = self.run_step(scene, step)?;
        if outcome == StepOutcome::Done {
            self.record(self.steps_run)?;
        }
        Ok(outcome)
    }

    fn supports(&mut self, capability: Capability) -> bool {
        // The test platform advances its executor clock only through `Step::Advance`.
        if capability == Capability::ExecutorClock {
            return true;
        }
        if !self.lane.has_pixels() {
            return false;
        }
        let Some(open) = self.open.as_ref() else {
            return false;
        };
        let capabilities = open.vcx().update(|window, _| window.effect_capabilities());
        match capability {
            Capability::ElementEffects => {
                capabilities.element.backdrop_blur && capabilities.element.glow
            }
            Capability::SnapshotTransitions => capabilities.screen.snapshot_transitions,
            Capability::ExecutorClock => true,
        }
    }

    fn close(&mut self) {
        if let Some(open) = self.open.take() {
            open.cx.quit();
        }
    }
}

/// The headless renderer lane C draws with: the software adapter unless `GPUX_WGPU_ADAPTER`
/// picks another on the platforms that render through wgpu, the platform renderer elsewhere.
pub fn lane_c_renderer() -> anyhow::Result<Option<Box<dyn PlatformHeadlessRenderer>>> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        use gpui_wgpu::{HeadlessAdapterSelection, WgpuHeadlessRenderer};
        let selection = std::env::var(HeadlessAdapterSelection::ENV_VAR)
            .map(|value| HeadlessAdapterSelection::parse(&value))
            .unwrap_or(HeadlessAdapterSelection::Software);
        WgpuHeadlessRenderer::with_adapter(&selection)
            .map(|renderer| Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        gpui_platform::current_headless_renderer()
    }
}

impl TestHost {
    fn run_step(&mut self, scene: &SceneSpec, step: &Step) -> anyhow::Result<StepOutcome> {
        if step.needs_pixels() && !self.lane.has_pixels() {
            return Ok(StepOutcome::Skipped(format!(
                "pixel steps need a renderer, lane {} has none",
                self.lane.as_str()
            )));
        }
        let goldens = self.goldens.clone();
        let probes_only = self.probes_only;
        let open = self.scene()?;
        if !step.needs_pixels() && !matches!(step, Step::Expect(_)) {
            open.frame = None;
        }
        match step {
            Step::Click(query) => {
                open.vcx().locator(query.clone()).click()?;
            }
            Step::ClickCount(query, count) => {
                let element = open.locate(query)?;
                open.vcx().simulate_click_count(
                    center(&element),
                    MouseButton::Left,
                    *count,
                    Modifiers::none(),
                );
            }
            Step::Hover(query) => {
                open.vcx().locator(query.clone()).hover()?;
            }
            Step::ClickAt(x, y) => {
                open.vcx()
                    .simulate_click(point(px(*x), px(*y)), Modifiers::none());
            }
            Step::ClickCountAt(x, y, count) => {
                open.vcx().simulate_click_count(
                    point(px(*x), px(*y)),
                    MouseButton::Left,
                    *count,
                    Modifiers::none(),
                );
            }
            Step::Press(query) => {
                let at = center(&open.locate(query)?);
                open.vcx()
                    .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
            }
            Step::Release(query) => {
                let at = center(&open.locate(query)?);
                open.vcx()
                    .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
            }
            Step::MoveTo(x, y) => {
                open.vcx()
                    .simulate_mouse_move(point(px(*x), px(*y)), None, Modifiers::none());
            }
            Step::Drag(from, to) => {
                open.vcx().locator(from.clone()).drag_to(to.clone())?;
            }
            Step::Wheel(query, dx, dy) => {
                open.vcx()
                    .locator(query.clone())
                    .scroll(point(px(*dx), px(*dy)))?;
            }
            Step::TouchPan(query, dx, dy, steps) => {
                let start = center(&open.locate(query)?);
                open.touch(TouchPhase::Started, start);
                let steps = (*steps).max(1);
                for index in 1..=steps {
                    open.vcx().background_executor.advance_clock(TOUCH_FRAME);
                    let t = index as f32 / steps as f32;
                    open.touch(
                        TouchPhase::Moved,
                        point(start.x + px(dx * t), start.y + px(dy * t)),
                    );
                }
                open.vcx().background_executor.advance_clock(TOUCH_FRAME);
                open.touch(
                    TouchPhase::Ended,
                    point(start.x + px(*dx), start.y + px(*dy)),
                );
            }
            Step::TouchTap(query) => {
                let at = center(&open.locate(query)?);
                open.touch(TouchPhase::Started, at);
                open.vcx().background_executor.advance_clock(TOUCH_FRAME);
                open.touch(TouchPhase::Ended, at);
            }
            Step::Type(query, text) => {
                open.vcx().locator(query.clone()).type_text(text)?;
            }
            Step::Keys(keys) => open.vcx().simulate_keystrokes(keys),
            Step::KeyUp(key) => open.vcx().simulate_key_up(key),
            Step::ImeCompose(text, caret) => {
                if !open.vcx().simulate_ime_compose(text, Some(*caret..*caret)) {
                    bail!("no element accepts text input");
                }
            }
            Step::ImeCommit(text) => {
                if !open.vcx().simulate_ime_commit(text) {
                    bail!("no element accepts text input");
                }
            }
            Step::ImeCancel => {
                if !open.vcx().simulate_ime_cancel() {
                    bail!("no element accepts text input");
                }
            }
            Step::Focus(query) => {
                open.vcx().locator(query.clone()).focus()?;
            }
            Step::Advance(duration) => open.vcx().advance_frame(*duration),
            Step::Expect(expect) => open.expect(expect)?,
            Step::Probe(..)
            | Step::ProbeUniform(..)
            | Step::ProbeDiffer(..)
            | Step::Capture(_)
            | Step::ChangedSince(..) => {
                let frame = open.frame()?.clone();
                evaluate::pixel_step(step, &frame, open.scale, &mut open.captures)?;
            }
            Step::When(..) => bail!("`run_scene` resolves `When` steps"),
            Step::Golden | Step::GoldenWithin(_) => {
                if !scene.golden {
                    return Ok(StepOutcome::Skipped("not in the golden subset".into()));
                }
                if probes_only {
                    return Ok(StepOutcome::Skipped(
                        "goldens are off: this run checks relative probes only".into(),
                    ));
                }
                let backend = open.backend.clone().context("no backend")?;
                let Some(backend_id) = backend.golden_id() else {
                    return Ok(StepOutcome::Skipped(format!(
                        "no committed goldens for {}",
                        backend.label()
                    )));
                };
                let name = GoldenName::new(scene.id, open.scale as u32, backend_id);
                let options = match step {
                    Step::GoldenWithin(tolerance) => {
                        tolerance.validate().map_err(|error| anyhow!(error))?;
                        tolerance.options()
                    }
                    _ => goldens.options,
                };
                let frame = open.frame()?.clone();
                match goldens.check_with(&name, &frame, options)? {
                    GoldenOutcome::Matched | GoldenOutcome::Updated(_) => {}
                }
            }
        }
        Ok(StepOutcome::Done)
    }
}

impl Drop for TestHost {
    fn drop(&mut self) {
        self.close();
    }
}
