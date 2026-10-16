//! Scene specifications: what a conformance scene renders, which lanes run it, and the script of
//! interactions and expectations every host executes the same way.

use crate::{
    compare::GoldenTolerance,
    probe::{DEFAULT_TOLERANCE, Rgba},
};
use gpux::{AnyView, App, Window, semantic_query::Query};
use std::{fmt, time::Duration};

/// Where a test runs. See `docs/testing.md` for the lane table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lane {
    /// Behaviour on the test platform: no GPU, no display.
    A,
    /// A real browser through `gpui_web`.
    B,
    /// Native offscreen pixels through a headless renderer.
    C,
    /// Real desktop windows.
    D,
    /// Android.
    E,
    /// iOS.
    F,
}

impl Lane {
    /// Every lane a scene can declare.
    pub const ALL: [Lane; 6] = [Lane::A, Lane::B, Lane::C, Lane::D, Lane::E, Lane::F];

    /// The lane letter, lowercase.
    pub fn as_str(self) -> &'static str {
        match self {
            Lane::A => "a",
            Lane::B => "b",
            Lane::C => "c",
            Lane::D => "d",
            Lane::E => "e",
            Lane::F => "f",
        }
    }

    /// Whether hosts in this lane capture pixels.
    pub fn has_pixels(self) -> bool {
        !matches!(self, Lane::A)
    }
}

/// A behaviour or rendering area the catalog must cover (SCENE-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum Area {
    LayoutFlex,
    LayoutGrid,
    LayoutAbsolute,
    LayoutOverflow,
    LayoutFixed,
    LayoutSticky,
    StickyStacking,
    TextShaping,
    TextRuns,
    TextDecorations,
    TextWrapping,
    TextTruncation,
    TextSelection,
    EditableText,
    Ime,
    UniformList,
    ListVirtualization,
    VirtualList,
    WheelScroll,
    NestedWheel,
    Scrollbars,
    ProgrammaticScroll,
    TouchPan,
    TouchFling,
    FocusOrder,
    Actions,
    LayeredKeymaps,
    HoverActive,
    DragDrop,
    Tooltips,
    AnchoredOverlay,
    DeferredOverlay,
    RasterImage,
    Svg,
    CanvasPath,
    Borders,
    DashedBorders,
    BorderRadii,
    Shadows,
    Gradients,
    Opacity,
    Clipping,
    EffectFrame,
    AnimatedFrame,
    TransitionFrame,
    ShaderEffects,
    SrgbCompositing,
    AccessibilityTree,
    DevtoolsSemantics,
}

impl Area {
    /// Every area SCENE-1 requires a scene for.
    pub const REQUIRED: &'static [Area] = &[
        Area::LayoutFlex,
        Area::LayoutGrid,
        Area::LayoutAbsolute,
        Area::LayoutOverflow,
        Area::LayoutFixed,
        Area::LayoutSticky,
        Area::StickyStacking,
        Area::TextShaping,
        Area::TextRuns,
        Area::TextDecorations,
        Area::TextWrapping,
        Area::TextTruncation,
        Area::TextSelection,
        Area::EditableText,
        Area::Ime,
        Area::UniformList,
        Area::ListVirtualization,
        Area::VirtualList,
        Area::WheelScroll,
        Area::NestedWheel,
        Area::Scrollbars,
        Area::ProgrammaticScroll,
        Area::TouchPan,
        Area::TouchFling,
        Area::FocusOrder,
        Area::Actions,
        Area::LayeredKeymaps,
        Area::HoverActive,
        Area::DragDrop,
        Area::Tooltips,
        Area::AnchoredOverlay,
        Area::DeferredOverlay,
        Area::RasterImage,
        Area::Svg,
        Area::CanvasPath,
        Area::Borders,
        Area::DashedBorders,
        Area::BorderRadii,
        Area::Shadows,
        Area::Gradients,
        Area::Opacity,
        Area::Clipping,
        Area::EffectFrame,
        Area::AnimatedFrame,
        Area::TransitionFrame,
        Area::ShaderEffects,
        Area::SrgbCompositing,
        Area::AccessibilityTree,
        Area::DevtoolsSemantics,
    ];
}

/// A renderer capability that some pixel expectations depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Element effects: layer and backdrop blur and glow (`EffectCapabilities::element`).
    ElementEffects,
    /// Snapshot transitions such as crossfades (`EffectCapabilities::screen`).
    SnapshotTransitions,
    /// A clock that moves only through `Step::Advance` (the test platform's executor clock), so
    /// a script can observe the state before a delay elapses. Hosts on the real clock cannot
    /// hold a delay back.
    ExecutorClock,
}

impl Capability {
    /// What the capability is.
    pub fn describe(self) -> &'static str {
        match self {
            Capability::ElementEffects => "element effects",
            Capability::SnapshotTransitions => "snapshot transitions",
            Capability::ExecutorClock => "the executor clock",
        }
    }

    /// The skip reason of steps that need the capability, where the host lacks it.
    pub fn missing(self) -> String {
        match self {
            Capability::ElementEffects | Capability::SnapshotTransitions => {
                format!("the renderer has no {}", self.describe())
            }
            Capability::ExecutorClock => "the real clock cannot hold a delay back".to_string(),
        }
    }
}

/// A rectangle in logical pixels: `(x, y, width, height)`.
pub type LogicalRect = (f32, f32, f32, f32);

/// One step of a scene script. Coordinates are logical pixels; hosts scale them.
#[derive(Clone, Debug)]
pub enum Step {
    /// Clicks the primary button at the element's center.
    Click(Query),
    /// Presses and releases at the element's center with an explicit click count.
    ClickCount(Query, usize),
    /// Moves the mouse to the element's center.
    Hover(Query),
    /// Moves the mouse to a window position with no button held.
    MoveTo(f32, f32),
    /// Clicks the primary button at a window position.
    ClickAt(f32, f32),
    /// Presses and releases at a window position with an explicit click count (2 for a double
    /// click, 3 for a triple click).
    ClickCountAt(f32, f32, usize),
    /// Presses the primary button at the element's center and keeps it held.
    Press(Query),
    /// Releases the primary button at the element's center.
    Release(Query),
    /// Drags with the primary button from one element's center to another's.
    Drag(Query, Query),
    /// Scrolls the wheel by `(dx, dy)` pixels over the element.
    Wheel(Query, f32, f32),
    /// Pans a touch from the element's center by `(dx, dy)` over `steps` 16 ms frames and
    /// lifts it, so fling momentum follows a fast pan.
    TouchPan(Query, f32, f32, usize),
    /// Taps a touch at the element's center.
    TouchTap(Query),
    /// Focuses the element (by clicking it unless focus is already inside it) and types text.
    Type(Query, String),
    /// Presses space-separated keystrokes on the focused element.
    Keys(String),
    /// Releases a key on the focused element.
    KeyUp(String),
    /// An IME composition step: marked text and the caret within it (UTF-16).
    ImeCompose(String, usize),
    /// Commits an IME composition.
    ImeCommit(String),
    /// Cancels an IME composition.
    ImeCancel,
    /// Focuses the element.
    Focus(Query),
    /// Advances the executor clock and delivers a frame.
    Advance(Duration),
    /// Asserts something about the rendered element tree.
    Expect(Expect),
    /// Pixel lanes: the pixel at a window position is a color within a tolerance.
    Probe(f32, f32, Rgba, u8),
    /// Pixel lanes: a region is uniformly one color within a tolerance.
    ProbeUniform(LogicalRect, Rgba, u8),
    /// Pixel lanes: two regions differ by more than a tolerance.
    ProbeDiffer(LogicalRect, LogicalRect, u8),
    /// Pixel lanes: remembers the current frame under a name.
    Capture(&'static str),
    /// Pixel lanes: a region changed since a captured frame.
    ChangedSince(&'static str, LogicalRect),
    /// Pixel lanes: compares the frame with the scene's golden on backends that keep goldens.
    Golden,
    /// Like [`Step::Golden`], under the scene's own tolerance instead of the defaults.
    GoldenWithin(GoldenTolerance),
    /// Runs the steps only where the renderer has the capability; elsewhere they are reported
    /// skipped, because the renderer draws the content without the effect.
    When(Capability, Vec<Step>),
}

impl Step {
    /// A probe at the default tolerance.
    pub fn probe(x: f32, y: f32, color: Rgba) -> Self {
        Step::Probe(x, y, color, DEFAULT_TOLERANCE)
    }

    /// Whether the step needs pixels.
    pub fn needs_pixels(&self) -> bool {
        matches!(
            self,
            Step::Probe(..)
                | Step::ProbeUniform(..)
                | Step::ProbeDiffer(..)
                | Step::Capture(_)
                | Step::ChangedSince(..)
                | Step::Golden
                | Step::GoldenWithin(_)
        ) || matches!(self, Step::When(_, steps) if steps.iter().any(Step::needs_pixels))
    }

    /// Whether the step compares with the scene's golden.
    pub fn is_golden(&self) -> bool {
        matches!(self, Step::Golden | Step::GoldenWithin(_))
    }
}

/// Every step of `script`, with the steps inside [`Step::When`] in place.
pub fn flatten(script: &[Step]) -> Vec<&Step> {
    let mut steps = Vec::new();
    for step in script {
        match step {
            Step::When(_, inner) => steps.extend(flatten(inner)),
            step => steps.push(step),
        }
    }
    steps
}

/// An assertion about the rendered element tree, checked through semantic queries.
#[derive(Clone, Debug)]
pub enum Expect {
    /// Exactly one element matches.
    Exists(Query),
    /// No element matches.
    Missing(Query),
    /// This many elements match.
    Count(Query, usize),
    /// At least this many elements match.
    AtLeast(Query, usize),
    /// Focus is on the element or inside it.
    Focused(Query),
    /// The element's bounds are `(x, y, width, height)` within a tolerance.
    Bounds(Query, LogicalRect, f32),
    /// The element's size is `(width, height)` within a tolerance.
    Size(Query, f32, f32, f32),
    /// The accessibility tree has a node with this label.
    Accessible(String),
}

/// One conformance scene.
pub struct SceneSpec {
    /// Stable kebab-case id, used in results and golden names.
    pub id: &'static str,
    /// The areas it covers.
    pub areas: &'static [Area],
    /// Logical window size.
    pub size: (f32, f32),
    /// Scale factors pixel lanes render it at.
    pub scales: &'static [f32],
    /// The lanes that run it.
    pub lanes: &'static [Lane],
    /// Whether lane C compares it with per-backend goldens (the curated subset).
    pub golden: bool,
    /// Builds the scene's root view.
    pub build: fn(&mut Window, &mut App) -> AnyView,
    /// The interactions and expectations, in order.
    pub script: fn() -> Vec<Step>,
}

impl SceneSpec {
    /// Whether `lane` runs this scene.
    pub fn runs_in(&self, lane: Lane) -> bool {
        self.lanes.contains(&lane)
    }

    /// The result id at `scale`: the plain id at scale 1, `<id>@<scale>x` otherwise.
    pub fn result_id(&self, scale: f32) -> String {
        if scale == 1. {
            self.id.to_string()
        } else {
            format!("{}@{}x", self.id, scale)
        }
    }
}

impl fmt::Debug for SceneSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SceneSpec")
            .field("id", &self.id)
            .field("areas", &self.areas)
            .field("size", &self.size)
            .field("scales", &self.scales)
            .field("lanes", &self.lanes)
            .field("golden", &self.golden)
            .finish()
    }
}
