use crate::{
    Action, InputLayerId, InputMapError, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Point, ScrollDelta, ScrollWheelEvent, ScrollWheelMomentumPhase, ScrollWheelPhase,
    ScrollWheelSource,
};
use std::sync::Arc;

/// A bindable pointer gesture. Modifier matching is always exact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GesturePattern {
    /// A complete button click. The platform click count is matched exactly.
    Click {
        /// The required button.
        button: MouseButton,
        /// The exact required platform click count.
        count: u8,
        /// The complete required modifier set.
        modifiers: Modifiers,
    },
    /// A button movement that crosses a logical-pixel threshold.
    Drag {
        /// The button held while moving.
        button: MouseButton,
        /// The complete required modifier set.
        modifiers: Modifiers,
        /// The logical-pixel distance that starts the drag.
        threshold: u16,
    },
    /// A directional wheel movement from a particular platform source.
    Wheel {
        /// The required wheel direction.
        direction: WheelDirection,
        /// The required normalized source.
        source: GestureWheelSource,
        /// The complete required modifier set.
        modifiers: Modifiers,
        /// The accumulated amount required to dispatch.
        threshold: u16,
    },
}

/// A cardinal direction resolved from a wheel sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelDirection {
    /// Negative vertical movement.
    Up,
    /// Positive vertical movement.
    Down,
    /// Negative horizontal movement.
    Left,
    /// Positive horizontal movement.
    Right,
}

/// The normalized origin of a wheel gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureWheelSource {
    /// A discrete desktop mouse wheel.
    Mouse,
    /// Direct touch or touchpad movement.
    Touchpad,
    /// A platform-reported high-resolution wheel.
    PrecisionTouchpad,
}

/// A gesture rule and the action that it invokes.
pub struct GestureRule {
    pattern: GesturePattern,
    action: Box<dyn Action>,
}

impl Clone for GestureRule {
    fn clone(&self) -> Self {
        Self {
            pattern: self.pattern,
            action: self.action.boxed_clone(),
        }
    }
}

impl GestureRule {
    /// Creates a gesture rule. Rules later in a layer take precedence.
    pub fn new(pattern: GesturePattern, action: &dyn Action) -> Self {
        Self {
            pattern,
            action: action.boxed_clone(),
        }
    }

    /// Returns the normalized gesture pattern.
    pub fn pattern(&self) -> GesturePattern {
        self.pattern
    }

    /// Returns the bound action.
    pub fn action(&self) -> &dyn Action {
        self.action.as_ref()
    }
}

/// An immutable, ordered gesture layer.
#[derive(Clone)]
pub struct GestureMapLayer {
    id: InputLayerId,
    priority: i32,
    rules: Arc<[GestureRule]>,
}

impl GestureMapLayer {
    /// Creates a named gesture layer. Higher priorities win before rule order.
    pub fn new(
        id: InputLayerId,
        priority: i32,
        rules: impl IntoIterator<Item = GestureRule>,
    ) -> Self {
        Self {
            id,
            priority,
            rules: rules.into_iter().collect(),
        }
    }

    /// Returns the stable layer identity.
    pub fn id(&self) -> &InputLayerId {
        &self.id
    }

    /// Returns the layer precedence after context specificity.
    pub fn priority(&self) -> i32 {
        self.priority
    }

    /// Returns the ordered rules in this layer.
    pub fn rules(&self) -> &[GestureRule] {
        &self.rules
    }
}

/// An opaque version of the active gesture-map snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GestureMapVersion(usize);

/// An immutable gesture-map snapshot retained while a pointer sequence is active.
pub struct GestureMapSnapshot {
    layers: Vec<GestureMapLayer>,
}

impl GestureMapSnapshot {
    /// Iterates layers in deterministic low-to-high precedence order.
    pub fn layers(&self) -> impl ExactSizeIterator<Item = &GestureMapLayer> {
        self.layers.iter()
    }

    fn rules_highest_precedence(&self) -> impl Iterator<Item = &GestureRule> {
        self.layers
            .iter()
            .rev()
            .flat_map(|layer| layer.rules().iter().rev())
    }

    fn has_click_with_higher_count(
        &self,
        button: MouseButton,
        modifiers: Modifiers,
        count: u8,
    ) -> bool {
        self.rules_highest_precedence().any(|rule| {
            matches!(rule.pattern(), GesturePattern::Click { button: candidate_button, count: candidate_count, modifiers: candidate_modifiers }
                if candidate_button == button && candidate_modifiers == modifiers && candidate_count > count)
        })
    }
}

/// A generic, immutable gesture-layer map.
#[derive(Default)]
pub struct GestureMap {
    snapshot: Arc<GestureMapSnapshot>,
    version: GestureMapVersion,
}

impl Default for GestureMapSnapshot {
    fn default() -> Self {
        Self { layers: Vec::new() }
    }
}

impl GestureMap {
    /// Returns the active snapshot version.
    pub fn version(&self) -> GestureMapVersion {
        self.version
    }

    /// Returns the immutable active snapshot for a recognizer to retain.
    pub fn snapshot(&self) -> Arc<GestureMapSnapshot> {
        self.snapshot.clone()
    }

    /// Atomically replaces one named gesture layer and retains unrelated layers.
    pub fn set_layer(
        &mut self,
        layer: GestureMapLayer,
    ) -> Result<Option<GestureMapLayer>, InputMapError> {
        let mut layers = self.snapshot.layers.clone();
        let previous = layers
            .iter()
            .position(|existing| existing.id() == layer.id())
            .map(|index| std::mem::replace(&mut layers[index], layer.clone()));
        if previous.is_none() {
            layers.push(layer);
        }
        layers.sort_by(|left, right| {
            left.priority()
                .cmp(&right.priority())
                .then_with(|| left.id().cmp(right.id()))
        });
        self.snapshot = Arc::new(GestureMapSnapshot { layers });
        self.version.0 += 1;
        Ok(previous)
    }

    /// Removes one gesture layer. Missing layers leave the snapshot unchanged.
    pub fn remove_layer(&mut self, id: &InputLayerId) -> Option<GestureMapLayer> {
        let mut layers = self.snapshot.layers.clone();
        let index = layers.iter().position(|layer| layer.id() == id)?;
        let removed = layers.remove(index);
        self.snapshot = Arc::new(GestureMapSnapshot { layers });
        self.version.0 += 1;
        Some(removed)
    }

    /// Iterates over installed layers in deterministic resolution order.
    pub fn layers(&self) -> impl ExactSizeIterator<Item = &GestureMapLayer> {
        self.snapshot.layers()
    }
}

/// A dispatch-ready gesture result. A target is owned by the window that began the sequence.
pub struct RecognizedGesture {
    action: Box<dyn Action>,
    target: usize,
    revision: GestureMapVersion,
}

impl RecognizedGesture {
    /// Returns the bound action to dispatch exactly once.
    pub fn action(&self) -> &dyn Action {
        self.action.as_ref()
    }

    /// Returns the target retained at pointer-down or wheel hit testing.
    pub fn target(&self) -> usize {
        self.target
    }

    /// Returns the map revision captured for this recognition sequence.
    pub fn revision(&self) -> GestureMapVersion {
        self.revision
    }
}

struct PendingPointerGesture {
    down: MouseDownEvent,
    target: usize,
    snapshot: Arc<GestureMapSnapshot>,
    revision: GestureMapVersion,
}

struct PendingClick {
    action: Box<dyn Action>,
    target: usize,
    revision: GestureMapVersion,
}

#[derive(Default)]
struct WheelAccumulator {
    direction: Option<WheelDirection>,
    source: Option<GestureWheelSource>,
    modifiers: Modifiers,
    amount: f32,
    target: usize,
    revision: GestureMapVersion,
    snapshot: Option<Arc<GestureMapSnapshot>>,
}

/// Window-local generic recognizer for click, drag, and wheel gesture rules.
#[derive(Default)]
pub struct GestureRecognizer {
    pending_pointer: Option<PendingPointerGesture>,
    pending_click: Option<PendingClick>,
    wheel: WheelAccumulator,
}

impl GestureRecognizer {
    /// Starts a pointer sequence with the exact snapshot and target selected by the window.
    pub fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        snapshot: Arc<GestureMapSnapshot>,
        revision: GestureMapVersion,
        target: usize,
    ) {
        self.pending_pointer = Some(PendingPointerGesture {
            down: event.clone(),
            target,
            snapshot,
            revision,
        });
    }

    /// Commits the highest-precedence drag rule once its threshold is crossed.
    pub fn mouse_move(&mut self, event: &MouseMoveEvent) -> Option<RecognizedGesture> {
        let pending = self.pending_pointer.as_ref()?;
        if event.pressed_button != Some(pending.down.button)
            || event.modifiers != pending.down.modifiers
        {
            return None;
        }
        let moved_squared = distance_squared(event.position, pending.down.position);
        let rule = pending.snapshot.rules_highest_precedence().find(|rule| {
            matches!(rule.pattern(), GesturePattern::Drag { button, modifiers, threshold }
                if button == pending.down.button
                    && modifiers == pending.down.modifiers
                    && moved_squared >= (threshold as f32).powi(2))
        })?;
        let action = rule.action().boxed_clone();
        let target = pending.target;
        let revision = pending.revision;
        self.pending_pointer.take().expect("pending gesture exists");
        Some(RecognizedGesture {
            action,
            target,
            revision,
        })
    }

    /// Resolves an exact-count click, deferring only when a higher-count candidate overlaps it.
    pub fn mouse_up(&mut self, event: &MouseUpEvent) -> Option<RecognizedGesture> {
        let pending = self.pending_pointer.take()?;
        if event.button != pending.down.button || event.modifiers != pending.down.modifiers {
            return None;
        }
        let count = u8::try_from(event.click_count).ok()?;
        let rule = pending.snapshot.rules_highest_precedence().find(|rule| {
            matches!(rule.pattern(), GesturePattern::Click { button, count: expected_count, modifiers }
                if button == event.button && expected_count == count && modifiers == event.modifiers)
        })?;
        if pending
            .snapshot
            .has_click_with_higher_count(event.button, event.modifiers, count)
        {
            self.pending_click = Some(PendingClick {
                action: rule.action().boxed_clone(),
                target: pending.target,
                revision: pending.revision,
            });
            None
        } else {
            self.pending_click = None;
            Some(recognized(rule, pending.target, pending.revision))
        }
    }

    /// Dispatches a deferred single click after the double-click arbitration timeout.
    pub fn flush_pending_click(&mut self) -> Option<RecognizedGesture> {
        let pending = self.pending_click.take()?;
        Some(RecognizedGesture {
            action: pending.action,
            target: pending.target,
            revision: pending.revision,
        })
    }

    /// Accumulates a wheel sample and emits at most one matching action when its threshold is met.
    pub fn scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        snapshot: Arc<GestureMapSnapshot>,
        revision: GestureMapVersion,
        target: usize,
    ) -> Option<RecognizedGesture> {
        if matches!(event.scroll_phase, ScrollWheelPhase::Cancelled)
            || matches!(
                event.native_momentum_phase,
                Some(ScrollWheelMomentumPhase::Cancelled)
            )
        {
            self.wheel = WheelAccumulator::default();
            return None;
        }
        let (direction, amount) = wheel_direction_and_amount(event.delta)?;
        let source = normalize_wheel_source(event.scroll_source, event.precise_delta);
        let reset = self.wheel.direction != Some(direction)
            || self.wheel.source != Some(source)
            || self.wheel.modifiers != event.modifiers
            || self.wheel.target != target
            || self.wheel.revision != revision;
        if reset {
            self.wheel = WheelAccumulator {
                direction: Some(direction),
                source: Some(source),
                modifiers: event.modifiers,
                amount: 0.0,
                target,
                revision,
                snapshot: Some(snapshot),
            };
        }
        // Coalescing is bounded: a pathological momentum stream cannot grow the accumulator.
        self.wheel.amount = (self.wheel.amount + amount).min(4096.0);
        let snapshot = self.wheel.snapshot.as_ref()?;
        let rule = snapshot.rules_highest_precedence().find(|rule| {
            matches!(rule.pattern(), GesturePattern::Wheel { direction: expected_direction, source: expected_source, modifiers, threshold }
                if expected_direction == direction
                    && expected_source == source
                    && modifiers == event.modifiers
                    && self.wheel.amount >= threshold as f32)
        })?;
        self.wheel.amount = 0.0;
        Some(recognized(rule, target, revision))
    }

    /// Cancels pending click, drag, and wheel state without dispatching an action.
    pub fn cancel(&mut self) {
        self.pending_pointer = None;
        self.pending_click = None;
        self.wheel = WheelAccumulator::default();
    }
}

fn recognized(rule: &GestureRule, target: usize, revision: GestureMapVersion) -> RecognizedGesture {
    RecognizedGesture {
        action: rule.action().boxed_clone(),
        target,
        revision,
    }
}

fn distance_squared(left: Point<crate::Pixels>, right: Point<crate::Pixels>) -> f32 {
    let x = left.x.0 - right.x.0;
    let y = left.y.0 - right.y.0;
    x.mul_add(x, y * y)
}

fn normalize_wheel_source(source: ScrollWheelSource, precise: bool) -> GestureWheelSource {
    match source {
        ScrollWheelSource::MouseWheel => GestureWheelSource::Mouse,
        ScrollWheelSource::PrecisionWheel => GestureWheelSource::PrecisionTouchpad,
        ScrollWheelSource::Touch | ScrollWheelSource::DirectManipulation => {
            GestureWheelSource::Touchpad
        }
        ScrollWheelSource::Unknown if precise => GestureWheelSource::PrecisionTouchpad,
        ScrollWheelSource::Unknown => GestureWheelSource::Mouse,
    }
}

fn wheel_direction_and_amount(delta: ScrollDelta) -> Option<(WheelDirection, f32)> {
    let (x, y) = match delta {
        ScrollDelta::Pixels(point) => (point.x.0, point.y.0),
        ScrollDelta::Lines(point) => (point.x, point.y),
    };
    if x.abs() > y.abs() {
        (x != 0.0).then_some((
            if x > 0.0 {
                WheelDirection::Right
            } else {
                WheelDirection::Left
            },
            x.abs(),
        ))
    } else {
        (y != 0.0).then_some((
            if y > 0.0 {
                WheelDirection::Down
            } else {
                WheelDirection::Up
            },
            y.abs(),
        ))
    }
}
