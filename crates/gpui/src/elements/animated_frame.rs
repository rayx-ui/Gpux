use crate::{
    AbsoluteLength, AnyElement, App, Bounds, Corners, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Length, Pixels, Point, SharedString, Size, Style,
    Window,
};

use scheduler::Instant;
use std::time::Duration;

use super::{animation::FrameAnimation, frame_style::FrameStyle};

const DEFAULT_FRAME_ANIMATION_DURATION: Duration = Duration::from_millis(200);

/// Creates a stable-id single-child frame for explicit frame property animation.
#[track_caller]
pub fn animated_frame(id: impl Into<ElementId>) -> AnimatedFrame {
    AnimatedFrame {
        id: id.into(),
        style: FrameStyle::default(),
        target_key: None,
        child: AnimatedFrameChild::None,
        animation: FrameAnimation::new(DEFAULT_FRAME_ANIMATION_DURATION),
        source_location: Some(core::panic::Location::caller()),
    }
}

/// A lightweight single-child frame that can animate frame-owned properties.
pub struct AnimatedFrame {
    pub(crate) id: ElementId,
    pub(crate) style: FrameStyle,
    pub(crate) target_key: Option<SharedString>,
    pub(crate) child: AnimatedFrameChild,
    pub(crate) animation: FrameAnimation,
    pub(crate) source_location: Option<&'static core::panic::Location<'static>>,
}

pub(crate) enum AnimatedFrameChild {
    None,
    Static(AnyElement),
    Progress(Box<dyn FnMut(f32, &mut Window, &mut App) -> AnyElement>),
}

impl AnimatedFrameChild {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Clone)]
struct AnimatedFrameState {
    from: FrameStyle,
    target: FrameStyle,
    target_key: Option<SharedString>,
    current: FrameStyle,
    progress: f32,
    started_at: Instant,
    animation: FrameAnimation,
    done: bool,
}

impl AnimatedFrameState {
    /// A completed frame at `target`. `now` comes from the executor clock.
    fn new(
        target: FrameStyle,
        target_key: Option<SharedString>,
        animation: FrameAnimation,
        now: Instant,
    ) -> Self {
        Self {
            from: target.clone(),
            target: target.clone(),
            target_key,
            current: target,
            progress: 1.0,
            started_at: now,
            animation,
            done: true,
        }
    }

    fn update_target(
        &mut self,
        target: FrameStyle,
        target_key: Option<SharedString>,
        animation: FrameAnimation,
        now: Instant,
    ) {
        let elapsed = now.saturating_duration_since(self.started_at);
        self.update_target_at(target, target_key, animation, elapsed, now);
    }

    fn update_target_at(
        &mut self,
        target: FrameStyle,
        target_key: Option<SharedString>,
        animation: FrameAnimation,
        elapsed: Duration,
        now: Instant,
    ) {
        if self.target == target && self.target_key == target_key {
            self.animation = animation;
            return;
        }

        let current = self.current_at_elapsed(elapsed);
        self.from = current;
        self.target = target;
        self.target_key = target_key;
        self.current = self.from.clone();
        self.progress = 0.0;
        self.started_at = now;
        self.animation = animation;
        self.done = false;
    }

    fn tick(&mut self, now: Instant) {
        self.tick_at_elapsed(now.saturating_duration_since(self.started_at));
    }

    fn tick_at_elapsed(&mut self, elapsed: Duration) {
        if self.done {
            self.current = self.target.clone();
            self.progress = 1.0;
            return;
        }

        let progress = self.animation.progress_for_elapsed(elapsed);
        self.progress = progress.eased;
        self.current = self.from.interpolate(&self.target, progress.eased);
        self.done = progress.done;

        if self.done {
            self.current = self.target.clone();
            self.progress = 1.0;
        }
    }

    fn current_at_elapsed(&self, elapsed: Duration) -> FrameStyle {
        if self.done {
            return self.current.clone();
        }

        let progress = self.animation.progress_for_elapsed(elapsed);
        if progress.done {
            self.target.clone()
        } else {
            self.from.interpolate(&self.target, progress.eased)
        }
    }
}

/// Per-frame render state for [`AnimatedFrame`].
pub struct AnimatedFrameLayoutState {
    _child_layout_id: Option<LayoutId>,
    style: Style,
    _progress: f32,
    progress_child: Option<AnyElement>,
}

impl AnimatedFrame {
    /// Sets the logical target key used to restart an animation when style values are unchanged.
    pub fn target_key(mut self, target_key: impl Into<SharedString>) -> Self {
        self.target_key = Some(target_key.into());
        self
    }

    /// Sets the transition duration for frame-owned property changes.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.animation = FrameAnimation::new(duration);
        self
    }

    /// Sets the easing function for frame-owned property changes.
    pub fn easing(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.animation = self.animation.with_easing(easing);
        self
    }

    /// Sets the frame's single child element.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        debug_assert!(
            self.child.is_none(),
            "AnimatedFrame accepts exactly one child"
        );
        self.child = AnimatedFrameChild::Static(child.into_any_element());
        self
    }

    /// Sets the frame's single child element from the current eased animation progress.
    pub fn child_with_progress<E>(
        mut self,
        mut child: impl FnMut(f32, &mut Window, &mut App) -> E + 'static,
    ) -> Self
    where
        E: IntoElement + 'static,
    {
        debug_assert!(
            self.child.is_none(),
            "AnimatedFrame accepts exactly one child"
        );
        self.child = AnimatedFrameChild::Progress(Box::new(move |progress, window, cx| {
            child(progress, window, cx).into_any_element()
        }));
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

impl Element for AnimatedFrame {
    type RequestLayoutState = AnimatedFrameLayoutState;
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
        let now = cx.background_executor().now();
        let frame =
            window.with_element_state(id.unwrap(), |state: Option<AnimatedFrameState>, window| {
                let mut state = state.unwrap_or_else(|| {
                    AnimatedFrameState::new(
                        self.style.clone(),
                        self.target_key.clone(),
                        self.animation.clone(),
                        now,
                    )
                });
                state.update_target(
                    self.style.clone(),
                    self.target_key.clone(),
                    self.animation.clone(),
                    now,
                );
                state.tick(now);

                if !state.done {
                    window.request_animation_frame();
                }

                let frame = (state.current.clone(), state.progress);
                (frame, state)
            });
        let (frame_style, progress) = frame;
        let mut progress_child = None;
        let child_layout_id = match &mut self.child {
            AnimatedFrameChild::None => None,
            AnimatedFrameChild::Static(child) => Some(child.request_layout(window, cx)),
            AnimatedFrameChild::Progress(child) => {
                let mut child = child(progress, window, cx);
                let child_layout_id = child.request_layout(window, cx);
                progress_child = Some(child);
                Some(child_layout_id)
            }
        };
        let style = frame_style.layout_style();
        let layout_id = window.request_layout(style.clone(), child_layout_id.iter().copied(), cx);
        (
            layout_id,
            AnimatedFrameLayoutState {
                _child_layout_id: child_layout_id,
                style,
                _progress: progress,
                progress_child,
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
                match &mut self.child {
                    AnimatedFrameChild::Static(child) => {
                        child.prepaint(window, cx);
                    }
                    AnimatedFrameChild::Progress(_) => {
                        if let Some(child) = request_layout.progress_child.as_mut() {
                            child.prepaint(window, cx);
                        }
                    }
                    AnimatedFrameChild::None => {}
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
                window.with_content_mask(
                    style.overflow_mask(bounds, window.rem_size()),
                    |window| match &mut self.child {
                        AnimatedFrameChild::Static(child) => child.paint(window, cx),
                        AnimatedFrameChild::Progress(_) => {
                            if let Some(child) = request_layout.progress_child.as_mut() {
                                child.paint(window, cx);
                            }
                        }
                        AnimatedFrameChild::None => {}
                    },
                );
            });
        });
    }
}

impl IntoElement for AnimatedFrame {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::{Length, SharedString, px};

    use super::*;

    fn test_now() -> Instant {
        scheduler::Clock::now(&scheduler::TestClock::new())
    }

    fn width(style: &FrameStyle) -> Length {
        style.size.width
    }

    #[test]
    fn animated_frame_state_first_render_is_complete() {
        let target = FrameStyle::default().w(px(100.0));
        let state = AnimatedFrameState::new(
            target.clone(),
            None,
            FrameAnimation::new(Duration::from_millis(200)),
            test_now(),
        );

        assert!(state.done);
        assert_eq!(state.progress, 1.0);
        assert_eq!(state.current, target);
    }

    #[test]
    fn animated_frame_state_retargets_from_current_interpolated_style() {
        let animation = FrameAnimation::new(Duration::from_millis(200));
        let now = test_now();
        let mut state = AnimatedFrameState::new(
            FrameStyle::default().w(px(100.0)),
            None,
            animation.clone(),
            now,
        );

        state.update_target_at(
            FrameStyle::default().w(px(300.0)),
            None,
            animation.clone(),
            Duration::ZERO,
            now,
        );
        state.tick_at_elapsed(Duration::from_millis(100));
        assert_eq!(width(&state.current), Length::from(px(200.0)));

        state.update_target_at(
            FrameStyle::default().w(px(500.0)),
            None,
            animation,
            Duration::from_millis(100),
            now,
        );
        assert_eq!(width(&state.from), Length::from(px(200.0)));

        state.tick_at_elapsed(Duration::from_millis(100));
        assert_eq!(width(&state.current), Length::from(px(350.0)));
    }

    #[test]
    fn animated_frame_state_restarts_when_target_key_changes() {
        let animation = FrameAnimation::new(Duration::from_millis(200));
        let now = test_now();
        let mut state = AnimatedFrameState::new(
            FrameStyle::default().w(px(100.0)),
            Some(SharedString::from("collapsed")),
            animation.clone(),
            now,
        );

        state.update_target_at(
            FrameStyle::default().w(px(100.0)),
            Some(SharedString::from("expanded")),
            animation,
            Duration::ZERO,
            now,
        );

        assert!(!state.done);
        assert_eq!(state.progress, 0.0);
    }
}
