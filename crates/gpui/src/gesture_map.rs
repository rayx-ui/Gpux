use crate::{
    Action, InputLayerId, InputMapError, KeyBindingContextPredicate, KeyContext, Modifiers,
    MouseButton, Point, PointerEvent, ScrollDelta, ScrollWheelEvent, ScrollWheelMomentumPhase,
    ScrollWheelPhase, ScrollWheelSource,
};
use collections::FxHashMap;
use scheduler::Instant;
use std::{rc::Rc, time::Duration};

/// A bindable pointer gesture. Modifier matching is always exact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GesturePattern {
    /// A touch or pen tap. The reported count is matched exactly.
    Tap {
        /// The exact required tap count.
        count: u8,
        /// The complete required modifier set.
        modifiers: Modifiers,
    },
    /// A stationary pointer contact that survives for the configured duration.
    LongPress {
        /// The complete required modifier set.
        modifiers: Modifiers,
        /// The minimum contact duration in milliseconds.
        duration_ms: u64,
        /// The maximum logical-pixel movement before rejection.
        threshold: u16,
    },
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
    /// A drag constrained to one cardinal axis, or free in either direction.
    DirectionalDrag {
        /// The button held while moving. Touch accepts its platform default button.
        button: MouseButton,
        /// The permitted direction of motion.
        direction: GestureDirection,
        /// The complete required modifier set.
        modifiers: Modifiers,
        /// The logical-pixel distance that starts the drag.
        threshold: u16,
    },
    /// A direct-touch or pen translation.
    Pan {
        /// The permitted direction of motion.
        direction: GestureDirection,
        /// The complete required modifier set.
        modifiers: Modifiers,
        /// The logical-pixel distance that starts the pan.
        threshold: u16,
    },
    /// A two-pointer distance change.
    Scale {
        /// The minimum absolute logical-pixel distance change.
        threshold: u16,
        /// The complete required modifier set.
        modifiers: Modifiers,
    },
    /// A two-pointer angle change.
    Rotate {
        /// The minimum absolute rotation in degrees.
        threshold_degrees: u16,
        /// The complete required modifier set.
        modifiers: Modifiers,
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
    /// A precision-wheel scale gesture, normally emitted by a trackpad pinch.
    TrackpadScale {
        /// The accumulated absolute scale delta required to dispatch.
        threshold: u16,
        /// The complete required modifier set.
        modifiers: Modifiers,
    },
}

/// The allowed dominant direction for a drag or pan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureDirection {
    /// Movement with no directional restriction.
    Free,
    /// Horizontal movement with a dominant x component.
    Horizontal,
    /// Vertical movement with a dominant y component.
    Vertical,
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
    context_predicate: Option<Rc<KeyBindingContextPredicate>>,
}

impl Clone for GestureRule {
    fn clone(&self) -> Self {
        Self {
            pattern: self.pattern,
            action: self.action.boxed_clone(),
            context_predicate: self.context_predicate.clone(),
        }
    }
}

impl GestureRule {
    /// Creates a gesture rule. Rules later in a layer take precedence.
    pub fn new(pattern: GesturePattern, action: &dyn Action) -> Self {
        Self::load(pattern, action, None)
    }

    /// Creates a gesture rule from an already parsed context predicate.
    pub fn load(
        pattern: GesturePattern,
        action: &dyn Action,
        context_predicate: Option<Rc<KeyBindingContextPredicate>>,
    ) -> Self {
        Self {
            pattern,
            action: action.boxed_clone(),
            context_predicate,
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

    /// Returns the predicate used to match this rule.
    pub fn predicate(&self) -> Option<Rc<KeyBindingContextPredicate>> {
        self.context_predicate.clone()
    }

    /// Returns the deepest matching context depth, or zero for an unscoped rule.
    pub fn context_depth(&self, contexts: &[KeyContext]) -> Option<usize> {
        self.context_predicate
            .as_ref()
            .map_or(Some(0), |predicate| predicate.depth_of(contexts))
    }
}

/// An immutable, ordered gesture layer.
#[derive(Clone)]
pub struct GestureMapLayer {
    id: InputLayerId,
    priority: i32,
    rules: Rc<[GestureRule]>,
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
#[derive(Default)]
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

    /// Retains only actions that can be dispatched by one already hit-tested
    /// target. Recognition must never select a higher-precedence action that
    /// belongs to an unrelated component.
    pub fn filtered_for_target(
        &self,
        contexts: &[KeyContext],
        mut is_available: impl FnMut(&dyn Action) -> bool,
    ) -> Rc<Self> {
        let mut rules = Vec::new();
        for (layer_index, layer) in self.layers.iter().enumerate() {
            for (rule_index, rule) in layer.rules().iter().enumerate() {
                let Some(depth) = rule.context_depth(contexts) else {
                    continue;
                };
                if is_available(rule.action()) {
                    rules.push((
                        depth,
                        layer.priority(),
                        layer_index,
                        rule_index,
                        rule.clone(),
                    ));
                }
            }
        }
        rules.sort_by_key(|(depth, priority, layer_index, rule_index, _)| {
            (*depth, *priority, *layer_index, *rule_index)
        });
        Rc::new(Self {
            layers: vec![GestureMapLayer::new(
                InputLayerId::new("gpui.resolved-gesture-target"),
                0,
                rules.into_iter().map(|(_, _, _, _, rule)| rule),
            )],
        })
    }
}

/// A generic, immutable gesture-layer map.
#[derive(Default)]
pub struct GestureMap {
    snapshot: Rc<GestureMapSnapshot>,
    version: GestureMapVersion,
}

impl GestureMap {
    /// Returns the active snapshot version.
    pub fn version(&self) -> GestureMapVersion {
        self.version
    }

    /// Returns the immutable active snapshot for a recognizer to retain.
    pub fn snapshot(&self) -> Rc<GestureMapSnapshot> {
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
        self.snapshot = Rc::new(GestureMapSnapshot { layers });
        self.version.0 += 1;
        Ok(previous)
    }

    /// Removes one gesture layer. Missing layers leave the snapshot unchanged.
    pub fn remove_layer(&mut self, id: &InputLayerId) -> Option<GestureMapLayer> {
        let mut layers = self.snapshot.layers.clone();
        let index = layers.iter().position(|layer| layer.id() == id)?;
        let removed = layers.remove(index);
        self.snapshot = Rc::new(GestureMapSnapshot { layers });
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
    event: Option<PointerEvent>,
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

    /// Returns the factual pointer sample that resolved this gesture, when it
    /// originated from direct pointer input.
    pub fn event(&self) -> Option<&PointerEvent> {
        self.event.as_ref()
    }
}

struct PendingPointerGesture {
    down: PointerEvent,
    current: PointerEvent,
    target: usize,
    snapshot: Rc<GestureMapSnapshot>,
    revision: GestureMapVersion,
    started_at: Instant,
}

#[derive(Default)]
struct WheelAccumulator {
    direction: Option<WheelDirection>,
    source: Option<GestureWheelSource>,
    modifiers: Modifiers,
    amount: f32,
    target: usize,
    revision: GestureMapVersion,
    snapshot: Option<Rc<GestureMapSnapshot>>,
}

/// Window-local gesture arena. Each active pointer owns one immutable map snapshot.
#[derive(Default)]
pub struct GestureRecognizer {
    arenas: FxHashMap<crate::PointerId, PendingPointerGesture>,
    wheel: WheelAccumulator,
}

impl GestureRecognizer {
    /// Starts a pointer sequence with the exact snapshot and target selected by the window.
    pub fn pointer_down(
        &mut self,
        event: &PointerEvent,
        snapshot: Rc<GestureMapSnapshot>,
        revision: GestureMapVersion,
        target: usize,
    ) {
        self.arenas.insert(
            event.id,
            PendingPointerGesture {
                down: event.clone(),
                current: event.clone(),
                target,
                snapshot,
                revision,
                started_at: event.timestamp.unwrap_or_else(Instant::now),
            },
        );
    }

    /// Commits the highest-precedence drag rule once its threshold is crossed.
    pub fn pointer_move(&mut self, event: &PointerEvent) -> Option<RecognizedGesture> {
        let pending = self.arenas.get_mut(&event.id)?;
        if event.modifiers != pending.down.modifiers {
            self.arenas.remove(&event.id);
            return None;
        }
        pending.current = event.clone();
        if let Some(gesture) = self.two_pointer_gesture(event.id) {
            return Some(gesture);
        }
        if self.holds_pan_for_two_pointer_candidate(event.id) {
            return None;
        }

        let current = self.arenas.get(&event.id)?;
        let moved_squared = distance_squared(event.position, current.down.position);
        let (dx, dy) = pointer_delta(&current.down, event);
        let rule =
            current
                .snapshot
                .rules_highest_precedence()
                .find(|rule| match rule.pattern() {
                    GesturePattern::Drag {
                        button,
                        modifiers,
                        threshold,
                    } => {
                        pointer_button_matches(event, current, button)
                            && modifiers == current.down.modifiers
                            && moved_squared >= (threshold as f32).powi(2)
                    }
                    GesturePattern::DirectionalDrag {
                        button,
                        direction,
                        modifiers,
                        threshold,
                    } => {
                        pointer_button_matches(event, current, button)
                            && modifiers == current.down.modifiers
                            && direction_matches(direction, dx, dy)
                            && moved_squared >= (threshold as f32).powi(2)
                    }
                    GesturePattern::Pan {
                        direction,
                        modifiers,
                        threshold,
                    } => {
                        matches!(
                            current.down.kind,
                            crate::PointerKind::Touch | crate::PointerKind::Pen
                        ) && modifiers == current.down.modifiers
                            && direction_matches(direction, dx, dy)
                            && moved_squared >= (threshold as f32).powi(2)
                    }
                    _ => false,
                })?;
        let gesture = recognized(
            rule,
            current.target,
            current.revision,
            Some(&current.current),
        );
        self.arenas.remove(&event.id);
        Some(gesture)
    }

    /// Resolves the exact click count reported by the platform.
    ///
    /// A multi-click sequence dispatches its single-, double-, and triple-click
    /// actions in order as the platform increments `click_count`. This keeps the
    /// first click responsive while later counts can replace its transient state.
    pub fn pointer_up(&mut self, event: &PointerEvent) -> Option<RecognizedGesture> {
        let pending = self.arenas.remove(&event.id)?;
        if event.phase == crate::PointerPhase::Cancel || event.modifiers != pending.down.modifiers {
            return None;
        }
        let count = u8::try_from(event.click_count).ok()?;
        let rule =
            pending
                .snapshot
                .rules_highest_precedence()
                .find(|rule| match rule.pattern() {
                    GesturePattern::Click {
                        button,
                        count: expected_count,
                        modifiers,
                    } => {
                        pointer_button_matches(event, &pending, button)
                            && expected_count == count
                            && modifiers == event.modifiers
                    }
                    GesturePattern::Tap {
                        count: expected_count,
                        modifiers,
                    } => {
                        matches!(
                            pending.down.kind,
                            crate::PointerKind::Touch | crate::PointerKind::Pen
                        ) && expected_count == count
                            && modifiers == event.modifiers
                    }
                    _ => false,
                })?;
        Some(recognized(
            rule,
            pending.target,
            pending.revision,
            Some(event),
        ))
    }

    /// Returns the earliest matching long-press delay for one active pointer.
    pub fn long_press_delay(&self, pointer_id: crate::PointerId) -> Option<Duration> {
        let pending = self.arenas.get(&pointer_id)?;
        pending
            .snapshot
            .rules_highest_precedence()
            .filter_map(|rule| match rule.pattern() {
                GesturePattern::LongPress {
                    modifiers,
                    duration_ms,
                    ..
                } if modifiers == pending.down.modifiers => {
                    Some(Duration::from_millis(duration_ms))
                }
                _ => None,
            })
            .min()
    }

    /// Resolves one stationary long-press candidate at a deterministic clock value.
    ///
    /// Resolving a long press removes the arena, so a later release cannot also
    /// dispatch a tap from the same sequence.
    pub fn advance_time_for(
        &mut self,
        pointer_id: crate::PointerId,
        now: Instant,
    ) -> Option<RecognizedGesture> {
        let gesture = {
            let pending = self.arenas.get(&pointer_id)?;
            let moved_squared = distance_squared(pending.current.position, pending.down.position);
            pending.snapshot.rules_highest_precedence().find_map(|rule| {
                matches!(rule.pattern(), GesturePattern::LongPress { modifiers, duration_ms, threshold }
                    if modifiers == pending.down.modifiers
                        && now.saturating_duration_since(pending.started_at) >= Duration::from_millis(duration_ms)
                        && moved_squared <= (threshold as f32).powi(2))
                .then(|| recognized(rule, pending.target, pending.revision, Some(&pending.current)))
            })
        }?;
        self.arenas.remove(&pointer_id);
        Some(gesture)
    }

    /// Resolves stationary long-press candidates at a deterministic clock value.
    pub fn advance_time(&mut self, now: Instant) -> Vec<RecognizedGesture> {
        let pointer_ids = self.arenas.keys().copied().collect::<Vec<_>>();
        pointer_ids
            .into_iter()
            .filter_map(|pointer_id| self.advance_time_for(pointer_id, now))
            .collect()
    }

    /// Accumulates a wheel sample and emits at most one matching action when its threshold is met.
    pub fn scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        snapshot: Rc<GestureMapSnapshot>,
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
        let trackpad_scale = (source == GestureWheelSource::PrecisionTouchpad)
            .then(|| {
                snapshot.rules_highest_precedence().find(|rule| {
                    matches!(rule.pattern(), GesturePattern::TrackpadScale { threshold, modifiers }
                    if modifiers == event.modifiers && self.wheel.amount >= threshold as f32)
                })
            })
            .flatten();
        if let Some(rule) = trackpad_scale {
            self.wheel.amount = 0.0;
            return Some(recognized(rule, target, revision, None));
        }
        let rule = snapshot.rules_highest_precedence().find(|rule| {
            matches!(rule.pattern(), GesturePattern::Wheel { direction: expected_direction, source: expected_source, modifiers, threshold }
                if expected_direction == direction
                    && expected_source == source
                    && modifiers == event.modifiers
                    && self.wheel.amount >= threshold as f32)
        })?;
        self.wheel.amount = 0.0;
        Some(recognized(rule, target, revision, None))
    }

    /// Cancels pending click, drag, and wheel state without dispatching an action.
    pub fn cancel(&mut self) {
        self.arenas.clear();
        self.wheel = WheelAccumulator::default();
    }

    /// Cancels one pointer arena without affecting independent active pointers.
    pub fn cancel_pointer(&mut self, pointer_id: crate::PointerId) {
        self.arenas.remove(&pointer_id);
    }

    /// Returns the number of independent active pointer arenas.
    pub fn active_pointer_count(&self) -> usize {
        self.arenas.len()
    }

    fn two_pointer_gesture(&mut self, pointer_id: crate::PointerId) -> Option<RecognizedGesture> {
        let current = self.arenas.get(&pointer_id)?;
        if !matches!(
            current.down.kind,
            crate::PointerKind::Touch | crate::PointerKind::Pen
        ) {
            return None;
        }
        let partner_id = self
            .arenas
            .iter()
            .filter(|(id, pending)| {
                **id != pointer_id
                    && matches!(
                        pending.down.kind,
                        crate::PointerKind::Touch | crate::PointerKind::Pen
                    )
            })
            .map(|(id, _)| *id)
            .min()?;
        let partner = self.arenas.get(&partner_id)?;
        let start_distance = distance_squared(current.down.position, partner.down.position).sqrt();
        let current_distance =
            distance_squared(current.current.position, partner.current.position).sqrt();
        let scale_delta = (current_distance - start_distance).abs();
        let rotation = angle_degrees(
            current.down.position,
            partner.down.position,
            current.current.position,
            partner.current.position,
        )
        .abs();
        let rule =
            current
                .snapshot
                .rules_highest_precedence()
                .find(|rule| match rule.pattern() {
                    GesturePattern::Scale {
                        threshold,
                        modifiers,
                    } => modifiers == current.down.modifiers && scale_delta >= threshold as f32,
                    GesturePattern::Rotate {
                        threshold_degrees,
                        modifiers,
                    } => {
                        modifiers == current.down.modifiers && rotation >= threshold_degrees as f32
                    }
                    _ => false,
                })?;
        let gesture = recognized(
            rule,
            current.target,
            current.revision,
            Some(&current.current),
        );
        self.arenas.remove(&pointer_id);
        self.arenas.remove(&partner_id);
        Some(gesture)
    }

    fn holds_pan_for_two_pointer_candidate(&self, pointer_id: crate::PointerId) -> bool {
        let Some(current) = self.arenas.get(&pointer_id) else {
            return false;
        };
        if !matches!(
            current.down.kind,
            crate::PointerKind::Touch | crate::PointerKind::Pen
        ) {
            return false;
        }
        let has_partner = self.arenas.iter().any(|(id, pending)| {
            *id != pointer_id
                && matches!(
                    pending.down.kind,
                    crate::PointerKind::Touch | crate::PointerKind::Pen
                )
        });
        has_partner && current.snapshot.rules_highest_precedence().any(|rule| {
            matches!(
                rule.pattern(),
                GesturePattern::Scale { modifiers, .. } | GesturePattern::Rotate { modifiers, .. }
                    if modifiers == current.down.modifiers
            )
        })
    }
}

fn recognized(
    rule: &GestureRule,
    target: usize,
    revision: GestureMapVersion,
    event: Option<&PointerEvent>,
) -> RecognizedGesture {
    RecognizedGesture {
        action: rule.action().boxed_clone(),
        target,
        revision,
        event: event.cloned(),
    }
}

fn distance_squared(left: Point<crate::Pixels>, right: Point<crate::Pixels>) -> f32 {
    let x = left.x.0 - right.x.0;
    let y = left.y.0 - right.y.0;
    x.mul_add(x, y * y)
}

fn pointer_delta(down: &PointerEvent, current: &PointerEvent) -> (f32, f32) {
    (
        current.position.x.0 - down.position.x.0,
        current.position.y.0 - down.position.y.0,
    )
}

fn pointer_button_matches(
    event: &PointerEvent,
    pending: &PendingPointerGesture,
    expected: MouseButton,
) -> bool {
    event.kind == crate::PointerKind::Touch
        || (pending.down.button == expected
            && (event.pressed_button == Some(expected) || event.button == expected))
}

fn direction_matches(direction: GestureDirection, dx: f32, dy: f32) -> bool {
    match direction {
        GestureDirection::Free => true,
        GestureDirection::Horizontal => dx.abs() >= dy.abs(),
        GestureDirection::Vertical => dy.abs() >= dx.abs(),
    }
}

fn angle_degrees(
    start_left: Point<crate::Pixels>,
    start_right: Point<crate::Pixels>,
    current_left: Point<crate::Pixels>,
    current_right: Point<crate::Pixels>,
) -> f32 {
    let start = (start_right.y.0 - start_left.y.0).atan2(start_right.x.0 - start_left.x.0);
    let current =
        (current_right.y.0 - current_left.y.0).atan2(current_right.x.0 - current_left.x.0);
    (current - start).to_degrees()
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
