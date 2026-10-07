//! The test-platform host: lane A runs scenes on `TestPlatform` with the bundled text system and
//! the deterministic executor clock; lane C adds the platform's headless renderer and checks
//! pixels and goldens.
//!
//! Lane C renders on the software adapter by default (WARP on Windows, lavapipe or llvmpipe on
//! Linux), so its output is deterministic and compared with goldens wherever it runs.
//! `GPUX_WGPU_ADAPTER=hardware` (or an adapter name) renders on a GPU instead, as lane G does;
//! hardware adapters skip goldens. macOS renders through Metal.

use crate::{
    backend::BackendId,
    compare::{GoldenName, GoldenOutcome, Goldens},
    host::{Host, StepOutcome},
    probe::{self, PixelRect},
    scene::{Expect, Lane, LogicalRect, SceneSpec, Step},
};
use anyhow::{Context as _, anyhow, bail};
use gpux::{
    AnyView, AnyWindowHandle, Context, DevtoolsElementSnapshot, IntoElement, Modifiers,
    MouseButton, Pixels, PlatformHeadlessRenderer, Point, Render, TestAppContext, TestDispatcher,
    TestWindowOptions, TouchEvent, TouchId, TouchPhase, VisualTestContext, Window, point, px,
    semantic_query::{self, Query, center},
    size,
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

/// Runs scenes on the test platform.
pub struct TestHost {
    lane: Lane,
    seed: u64,
    goldens: Goldens,
    open: Option<OpenScene>,
    backend: Option<BackendId>,
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
            open: None,
            backend: None,
        }
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

    fn physical(&self, x: f32, y: f32) -> (u32, u32) {
        (
            (x * self.scale).floor().max(0.) as u32,
            (y * self.scale).floor().max(0.) as u32,
        )
    }

    fn physical_rect(&self, (x, y, width, height): LogicalRect) -> PixelRect {
        let (left, top) = self.physical(x, y);
        PixelRect::new(
            left,
            top,
            ((width * self.scale).round() as u32).max(1),
            ((height * self.scale).round() as u32).max(1),
        )
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
            Expect::Exists(query) => {
                vcx.locate(query.clone())?;
            }
            Expect::Missing(query) => {
                let count = vcx.locator(query.clone()).count();
                if count != 0 {
                    bail!("{count} elements match `{query}`, expected none");
                }
            }
            Expect::Count(query, expected) => {
                let count = vcx.locator(query.clone()).count();
                if count != *expected {
                    bail!("{count} elements match `{query}`, expected {expected}");
                }
            }
            Expect::AtLeast(query, expected) => {
                let count = vcx.locator(query.clone()).count();
                if count < *expected {
                    bail!("{count} elements match `{query}`, expected at least {expected}");
                }
            }
            Expect::Focused(query) => {
                let snapshot = vcx.settled_frame_snapshot();
                let element = query.resolve(&snapshot)?;
                if !semantic_query::focus_is_within(&snapshot, element) {
                    bail!(
                        "`{query}` does not hold focus: {}",
                        semantic_query::describe_element(element)
                    );
                }
            }
            Expect::Bounds(query, (x, y, width, height), tolerance) => {
                let element = vcx.locate(query.clone())?;
                let bounds = element.bounds;
                let actual = [bounds.x, bounds.y, bounds.width, bounds.height];
                let expected = [*x, *y, *width, *height];
                if actual
                    .iter()
                    .zip(expected)
                    .any(|(actual, expected)| (actual - expected).abs() > *tolerance)
                {
                    bail!("`{query}` has bounds {actual:?}, expected {expected:?} ±{tolerance}");
                }
            }
            Expect::Size(query, width, height, tolerance) => {
                let element = vcx.locate(query.clone())?;
                let bounds = element.bounds;
                if (bounds.width - width).abs() > *tolerance
                    || (bounds.height - height).abs() > *tolerance
                {
                    bail!(
                        "`{query}` is {}x{}, expected {width}x{height} ±{tolerance}",
                        bounds.width,
                        bounds.height
                    );
                }
            }
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
        Ok(())
    }

    fn step(&mut self, scene: &SceneSpec, step: &Step) -> anyhow::Result<StepOutcome> {
        if step.needs_pixels() && !self.lane.has_pixels() {
            return Ok(StepOutcome::Skipped(format!(
                "pixel steps need a renderer, lane {} has none",
                self.lane.as_str()
            )));
        }
        let goldens = self.goldens.clone();
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
            Step::Probe(x, y, color, tolerance) => {
                let (px_x, px_y) = open.physical(*x, *y);
                let frame = open.frame()?;
                probe::color_at(frame, px_x, px_y, *color, *tolerance)?;
            }
            Step::ProbeUniform(rect, color, tolerance) => {
                let region = open.physical_rect(*rect);
                let frame = open.frame()?;
                let actual = probe::region_uniform(frame, region, *tolerance)?;
                if !probe::within(actual, *color, *tolerance) {
                    bail!("region {region:?} is uniformly {actual:?}, expected {color:?}");
                }
            }
            Step::ProbeDiffer(a, b, tolerance) => {
                let (a, b) = (open.physical_rect(*a), open.physical_rect(*b));
                let frame = open.frame()?;
                probe::regions_differ(frame, a, b, *tolerance)?;
            }
            Step::Capture(name) => {
                let frame = open.frame()?.clone();
                open.captures.insert(name, frame);
            }
            Step::ChangedSince(name, rect) => {
                let region = open.physical_rect(*rect);
                let before = open
                    .captures
                    .get(name)
                    .cloned()
                    .with_context(|| format!("no capture named {name:?}"))?;
                let after = open.frame()?;
                probe::changed(&before, after, region, probe::DEFAULT_TOLERANCE)?;
            }
            Step::Golden => {
                if !scene.golden {
                    return Ok(StepOutcome::Skipped("not in the golden subset".into()));
                }
                let backend = open.backend.clone().context("no backend")?;
                let Some(backend_id) = backend.golden_id() else {
                    return Ok(StepOutcome::Skipped(format!(
                        "no committed goldens for {}",
                        backend.label()
                    )));
                };
                let name = GoldenName::new(scene.id, open.scale as u32, backend_id);
                let frame = open.frame()?.clone();
                match goldens.check(&name, &frame)? {
                    GoldenOutcome::Matched | GoldenOutcome::Updated(_) => {}
                }
            }
        }
        Ok(StepOutcome::Done)
    }

    fn close(&mut self) {
        if let Some(open) = self.open.take() {
            open.cx.quit();
        }
    }
}

/// The headless renderer lane C draws with: the software adapter unless `GPUX_WGPU_ADAPTER`
/// picks another on the platforms that render through wgpu, the platform renderer elsewhere.
fn lane_c_renderer() -> anyhow::Result<Option<Box<dyn PlatformHeadlessRenderer>>> {
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

impl Drop for TestHost {
    fn drop(&mut self) {
        self.close();
    }
}
