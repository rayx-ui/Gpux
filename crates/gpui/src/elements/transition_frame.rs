use crate::{
    AbsoluteLength, AnyElement, App, Bounds, Corners, Element, ElementId, FrameTransition,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Length, Pixels, Point,
    ScreenEffectRegionId, ScreenEffectTarget, SharedString, Size, Style, Window,
};

use scheduler::Instant;
use std::time::Duration;

use super::frame_style::FrameStyle;

const SNAPSHOT_RELEASE_SAFETY_MARGIN: Duration = Duration::from_millis(100);

/// Creates a stable-id single-child frame for snapshot-backed component replacement transitions.
#[track_caller]
pub fn transition_frame(id: impl Into<ElementId>) -> TransitionFrame {
    let id = id.into();
    let region_id = ScreenEffectRegionId::new(format!("transition-frame-{id}"));

    TransitionFrame {
        id,
        style: FrameStyle::default(),
        target_key: None,
        transition: FrameTransition::default(),
        screen_effect_region: region_id,
        child: None,
        source_location: Some(core::panic::Location::caller()),
    }
}

/// A lightweight single-child frame that transitions between replacement children using snapshots.
pub struct TransitionFrame {
    pub(crate) id: ElementId,
    pub(crate) style: FrameStyle,
    pub(crate) target_key: Option<SharedString>,
    pub(crate) transition: FrameTransition,
    pub(crate) screen_effect_region: ScreenEffectRegionId,
    pub(crate) child: Option<AnyElement>,
    pub(crate) source_location: Option<&'static core::panic::Location<'static>>,
}

struct RetainedTransitionSnapshot {
    id: crate::ScreenEffectSnapshotId,
    captured_at: Instant,
    release_after: Duration,
}

struct TransitionFrameState {
    initialized: bool,
    target_key: Option<SharedString>,
    retained_snapshots: Vec<RetainedTransitionSnapshot>,
}

impl TransitionFrameState {
    fn new() -> Self {
        Self {
            initialized: false,
            target_key: None,
            retained_snapshots: Vec::new(),
        }
    }

    fn update_target(
        &mut self,
        target_key: Option<SharedString>,
        region_id: ScreenEffectRegionId,
        transition: &FrameTransition,
        now: Instant,
        window: &mut Window,
    ) -> Option<crate::ScreenEffectSnapshotId> {
        self.release_expired_snapshots(now, window);

        if !self.replace_target_key(target_key) {
            return None;
        }

        if !window.effect_capabilities().screen.snapshot_transitions {
            return None;
        }

        let snapshot_id =
            window.capture_screen_effect_snapshot(ScreenEffectTarget::region(region_id));
        self.retained_snapshots.push(RetainedTransitionSnapshot {
            id: snapshot_id,
            captured_at: now,
            release_after: Duration::from_millis(transition.duration_ms_value())
                + SNAPSHOT_RELEASE_SAFETY_MARGIN,
        });

        Some(snapshot_id)
    }

    fn replace_target_key(&mut self, target_key: Option<SharedString>) -> bool {
        if !self.initialized {
            self.initialized = true;
            self.target_key = target_key;
            return false;
        }

        if self.target_key == target_key {
            return false;
        }

        self.target_key = target_key;
        true
    }

    /// Releases snapshots whose transitions ended by `now` (the executor clock).
    fn release_expired_snapshots(&mut self, now: Instant, window: &mut Window) {
        self.retained_snapshots.retain(|snapshot| {
            if now.saturating_duration_since(snapshot.captured_at) >= snapshot.release_after {
                let _ = window.release_screen_effect_snapshot(snapshot.id);
                false
            } else {
                true
            }
        });
    }

    fn has_retained_snapshots(&self) -> bool {
        !self.retained_snapshots.is_empty()
    }
}

/// Per-frame render state for [`TransitionFrame`].
pub struct TransitionFrameLayoutState {
    _child_layout_id: Option<LayoutId>,
    style: Style,
    region_id: ScreenEffectRegionId,
    pending_snapshot: Option<crate::ScreenEffectSnapshotId>,
}

impl TransitionFrame {
    /// Sets the logical target key used to trigger a replacement transition.
    pub fn target_key(mut self, target_key: impl Into<SharedString>) -> Self {
        self.target_key = Some(target_key.into());
        self
    }

    /// Sets the replacement transition configuration.
    pub fn transition(mut self, transition: FrameTransition) -> Self {
        self.transition = transition;
        self
    }

    /// Overrides the default region id used for snapshot targeting.
    pub fn screen_effect_region(mut self, id: impl Into<ScreenEffectRegionId>) -> Self {
        self.screen_effect_region = id.into();
        self
    }

    /// Sets the frame's single child element.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        debug_assert!(
            self.child.is_none(),
            "TransitionFrame accepts exactly one child"
        );
        self.child = Some(child.into_any_element());
        self
    }

    /// Sets both layout dimensions.
    pub fn size(mut self, size: impl Into<Size<Length>>) -> Self {
        self.style = self.style.size(size);
        self
    }

    /// Sets the layout width.
    pub fn w(mut self, width: impl Into<Length>) -> Self {
        self.style = self.style.w(width);
        self
    }

    /// Sets the layout height.
    pub fn h(mut self, height: impl Into<Length>) -> Self {
        self.style = self.style.h(height);
        self
    }

    /// Sets the layout width to fill the parent.
    pub fn w_full(mut self) -> Self {
        self.style = self.style.w_full();
        self
    }

    /// Sets the layout height to fill the parent.
    pub fn h_full(mut self) -> Self {
        self.style = self.style.h_full();
        self
    }

    /// Sets both layout dimensions to fill the parent.
    pub fn size_full(mut self) -> Self {
        self.style = self.style.size_full();
        self
    }

    /// Sets the minimum layout width.
    pub fn min_w(mut self, width: impl Into<Length>) -> Self {
        self.style = self.style.min_w(width);
        self
    }

    /// Sets the minimum layout height.
    pub fn min_h(mut self, height: impl Into<Length>) -> Self {
        self.style = self.style.min_h(height);
        self
    }

    /// Sets the maximum layout width.
    pub fn max_w(mut self, width: impl Into<Length>) -> Self {
        self.style = self.style.max_w(width);
        self
    }

    /// Sets the maximum layout height.
    pub fn max_h(mut self, height: impl Into<Length>) -> Self {
        self.style = self.style.max_h(height);
        self
    }

    /// Uses absolute positioning for this frame.
    pub fn absolute(mut self) -> Self {
        self.style = self.style.absolute();
        self
    }

    /// Uses relative positioning for this frame.
    pub fn relative(mut self) -> Self {
        self.style = self.style.relative();
        self
    }

    /// Sets all inset edges to the same value.
    pub fn inset(mut self, inset: impl Into<Length>) -> Self {
        self.style = self.style.inset(inset);
        self
    }

    /// Sets all inset edges to zero.
    pub fn inset_0(mut self) -> Self {
        self.style = self.style.inset_0();
        self
    }

    /// Sets the left inset.
    pub fn left(mut self, left: impl Into<Length>) -> Self {
        self.style = self.style.left(left);
        self
    }

    /// Sets the right inset.
    pub fn right(mut self, right: impl Into<Length>) -> Self {
        self.style = self.style.right(right);
        self
    }

    /// Sets the top inset.
    pub fn top(mut self, top: impl Into<Length>) -> Self {
        self.style = self.style.top(top);
        self
    }

    /// Sets the bottom inset.
    pub fn bottom(mut self, bottom: impl Into<Length>) -> Self {
        self.style = self.style.bottom(bottom);
        self
    }

    /// Rotates the frame in radians.
    pub fn rotate(mut self, radians: f32) -> Self {
        self.style = self.style.rotate(radians);
        self
    }

    /// Scales the frame uniformly.
    pub fn scale(mut self, scale: f32) -> Self {
        self.style = self.style.scale(scale);
        self
    }

    /// Scales the frame independently on each axis.
    pub fn scale_xy(mut self, x: f32, y: f32) -> Self {
        self.style = self.style.scale_xy(x, y);
        self
    }

    /// Translates the frame during painting.
    pub fn translate(mut self, translation: Point<Pixels>) -> Self {
        self.style = self.style.translate(translation);
        self
    }

    /// Sets the transform origin as normalized x/y coordinates.
    pub fn transform_origin(mut self, origin: Point<f32>) -> Self {
        self.style = self.style.transform_origin(origin);
        self
    }

    /// Skews the frame independently on each axis in radians.
    pub fn skew(mut self, x_radians: f32, y_radians: f32) -> Self {
        self.style = self.style.skew(x_radians, y_radians);
        self
    }

    /// Skews the frame on the x axis in radians.
    pub fn skew_x(mut self, radians: f32) -> Self {
        self.style = self.style.skew_x(radians);
        self
    }

    /// Skews the frame on the y axis in radians.
    pub fn skew_y(mut self, radians: f32) -> Self {
        self.style = self.style.skew_y(radians);
        self
    }

    /// Sets the opacity used while painting this frame and its child.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.style = self.style.opacity(opacity);
        self
    }

    /// Sets the same corner radius on every corner.
    pub fn rounded(mut self, radius: impl Into<AbsoluteLength>) -> Self {
        self.style = self.style.rounded(radius);
        self
    }

    /// Sets individual corner radii.
    pub fn corner_radii(mut self, corner_radii: impl Into<Corners<AbsoluteLength>>) -> Self {
        self.style = self.style.corner_radii(corner_radii);
        self
    }

    /// Clips descendant content to the frame bounds.
    pub fn clip(mut self) -> Self {
        self.style = self.style.clip();
        self
    }

    /// Disables descendant content clipping.
    pub fn unclip(mut self) -> Self {
        self.style = self.style.unclip();
        self
    }
}

impl Element for TransitionFrame {
    type RequestLayoutState = TransitionFrameLayoutState;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.source_location
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let region_id = self.screen_effect_region.clone();
        let now = cx.background_executor().now();
        let pending_snapshot = window.with_element_state(
            id.unwrap(),
            |state: Option<TransitionFrameState>, window| {
                let mut state = state.unwrap_or_else(TransitionFrameState::new);
                let pending_snapshot = state.update_target(
                    self.target_key.clone(),
                    region_id.clone(),
                    &self.transition,
                    now,
                    window,
                );

                if state.has_retained_snapshots() {
                    window.request_animation_frame();
                }

                (pending_snapshot, state)
            },
        );

        let child_layout_id = self
            .child
            .as_mut()
            .map(|child| child.request_layout(window, cx));
        let style = self.style.layout_style();
        let layout_id = window.request_layout(style.clone(), child_layout_id.iter().copied(), cx);
        (
            layout_id,
            TransitionFrameLayoutState {
                _child_layout_id: child_layout_id,
                style,
                region_id,
                pending_snapshot,
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = &request_layout.style;
        // Painting applies the opacity and transform to the child, so cached
        // views below must see them while prepainting.
        let transformation = style.transformation(bounds, window.scale_factor());
        window.with_element_paint_context(style.opacity, transformation, |window| {
            window.with_content_mask(style.overflow_mask(bounds, window.rem_size()), |window| {
                if let Some(child) = self.child.as_mut() {
                    child.prepaint(window, cx);
                }
            });
        });
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = &request_layout.style;
        window.with_element_opacity(style.opacity, |window| {
            style.paint(bounds, window, cx, |window, cx| {
                let rem_size = window.rem_size();
                let corner_radii = style
                    .corner_radii
                    .to_pixels(rem_size)
                    .clamp_radii_for_quad_size(bounds.size);

                window.paint_screen_effect_region(
                    request_layout.region_id.clone(),
                    bounds,
                    corner_radii,
                );

                if let Some(snapshot_id) = request_layout.pending_snapshot.take() {
                    let effect = self.transition.resolve_screen_effect(
                        snapshot_id,
                        ScreenEffectTarget::region(request_layout.region_id.clone()),
                        bounds.center(),
                    );
                    window.play_screen_effect_at(effect, cx.background_executor().now());
                }

                window.with_content_mask(
                    style.overflow_mask(bounds, window.rem_size()),
                    |window| {
                        if let Some(child) = self.child.as_mut() {
                            child.paint(window, cx);
                        }
                    },
                );
            });
        });
    }
}

impl IntoElement for TransitionFrame {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::SharedString;

    use super::*;

    #[test]
    fn transition_frame_state_skips_first_render_and_unchanged_key() {
        let mut state = TransitionFrameState::new();

        assert!(!state.replace_target_key(Some(SharedString::from("compact"))));
        assert!(!state.replace_target_key(Some(SharedString::from("compact"))));
    }

    #[test]
    fn transition_frame_state_reports_changed_key_after_initialization() {
        let mut state = TransitionFrameState::new();

        assert!(!state.replace_target_key(Some(SharedString::from("compact"))));
        assert!(state.replace_target_key(Some(SharedString::from("expanded"))));
    }
}
