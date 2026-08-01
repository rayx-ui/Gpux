use crate::{Axis, Instant, Pixels, Point, Size, point, px};

/// The origin of an accepted scroll mutation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollMutationSource {
    /// Mouse wheel or precision trackpad input.
    Wheel,
    /// Direct touch input.
    Touch,
    /// Ballistic motion after direct touch ends.
    Momentum,
    /// A scrollbar thumb or track interaction.
    Scrollbar,
    /// An explicit application request.
    Programmatic,
    /// A request resolved after layout, such as scrolling an item into view.
    Deferred,
    /// A layout or content-size change that updates extents or clamps the offset.
    #[default]
    LayoutClamp,
}

/// The direction of the last accepted logical scroll delta.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollDirection {
    /// The published offset did not move.
    #[default]
    Idle,
    /// The logical offset increased.
    Forward,
    /// The logical offset decreased.
    Reverse,
}

/// The single activity currently represented by a scroll handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollActivityKind {
    /// No direct, ballistic, or programmatic activity is active.
    #[default]
    Idle,
    /// Direct touch is active.
    Direct,
    /// Ballistic momentum is active.
    Ballistic,
    /// A programmatic request supplied the latest offset.
    Programmatic,
}

/// An immutable, positive-logical snapshot of a scroll handle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollMetrics {
    /// Positive logical offset from the content origin.
    pub offset: Point<Pixels>,
    /// Positive logical maximum offset.
    pub max_offset: Point<Pixels>,
    /// Visible viewport size.
    pub viewport_size: Size<Pixels>,
    /// Full scrollable content size.
    pub content_size: Size<Pixels>,
    /// Positive-logical change from the prior snapshot.
    pub delta: Point<Pixels>,
    /// Main direction of the last offset change.
    pub direction: ScrollDirection,
    /// Origin of the last observable change.
    pub source: ScrollMutationSource,
    /// Current activity represented by the shared activity owner.
    pub activity: ScrollActivityKind,
    /// Monotonic revision incremented once per observable change.
    pub revision: u64,
}

impl Default for ScrollMetrics {
    fn default() -> Self {
        Self {
            offset: Point::default(),
            max_offset: Point::default(),
            viewport_size: Size::default(),
            content_size: Size::default(),
            delta: Point::default(),
            direction: ScrollDirection::Idle,
            source: ScrollMutationSource::LayoutClamp,
            activity: ScrollActivityKind::Idle,
            revision: 0,
        }
    }
}

impl ScrollMetrics {
    /// Return an axis-specific snapshot, applying reverse mapping without exposing
    /// GpuX's negative physical offset convention.
    pub fn for_axis(mut self, axis: Axis, reverse: bool) -> Self {
        let (offset, maximum, delta) = match axis {
            Axis::Horizontal => (self.offset.x, self.max_offset.x, self.delta.x),
            Axis::Vertical => (self.offset.y, self.max_offset.y, self.delta.y),
        };
        let offset = if reverse { maximum - offset } else { offset };
        let delta = if reverse { -delta } else { delta };
        self.offset = match axis {
            Axis::Horizontal => point(offset, Pixels::ZERO),
            Axis::Vertical => point(Pixels::ZERO, offset),
        };
        self.max_offset = match axis {
            Axis::Horizontal => point(maximum, Pixels::ZERO),
            Axis::Vertical => point(Pixels::ZERO, maximum),
        };
        self.delta = match axis {
            Axis::Horizontal => point(delta, Pixels::ZERO),
            Axis::Vertical => point(Pixels::ZERO, delta),
        };
        self.direction = direction_for_delta(delta);
        self
    }
}

pub(crate) fn direction_for_delta(delta: Pixels) -> ScrollDirection {
    if delta > Pixels::ZERO {
        ScrollDirection::Forward
    } else if delta < Pixels::ZERO {
        ScrollDirection::Reverse
    } else {
        ScrollDirection::Idle
    }
}

/// The transient deceleration state for a direct touch scroll.
#[derive(Debug, Default)]
pub(crate) struct ScrollActivity {
    velocity: Option<Point<Pixels>>,
    last_timestamp: Option<Instant>,
    programmatic: bool,
}

impl ScrollActivity {
    const REFERENCE_FRAME_SECONDS: f32 = 1.0 / 60.0;
    const DECELERATION_PER_REFERENCE_FRAME: f32 = 0.88;
    const STOP_VELOCITY: f32 = 8.0;
    const MAX_ELAPSED_SECONDS: f32 = 0.05;

    pub(crate) fn start_at(&mut self, velocity: Point<Pixels>, timestamp: Instant) {
        self.programmatic = false;
        self.velocity = Some(point(
            velocity.x.clamp(px(-1200.0), px(1200.0)),
            velocity.y.clamp(px(-1200.0), px(1200.0)),
        ));
        self.last_timestamp = Some(timestamp);
    }

    pub(crate) fn stop(&mut self) {
        self.velocity = None;
        self.last_timestamp = None;
        self.programmatic = false;
    }

    pub(crate) fn begin_direct_touch(&mut self, timestamp: Instant) {
        self.velocity = None;
        self.last_timestamp = Some(timestamp);
        self.programmatic = false;
    }

    pub(crate) fn begin_programmatic(&mut self) {
        self.velocity = None;
        self.last_timestamp = None;
        self.programmatic = true;
    }

    pub(crate) fn kind(&self) -> ScrollActivityKind {
        if self.programmatic {
            ScrollActivityKind::Programmatic
        } else if self.velocity.is_some() {
            ScrollActivityKind::Ballistic
        } else if self.last_timestamp.is_some() {
            ScrollActivityKind::Direct
        } else {
            ScrollActivityKind::Idle
        }
    }

    pub(crate) fn has_direct_touch(&self) -> bool {
        self.velocity.is_none() && self.last_timestamp.is_some()
    }

    pub(crate) fn velocity(&self) -> Option<Point<Pixels>> {
        self.velocity
    }

    /// Advance using the frame's authoritative timestamp instead of assuming a refresh rate.
    pub(crate) fn next_delta_at(&mut self, timestamp: Instant) -> Option<Point<Pixels>> {
        let velocity = self.velocity?;
        let previous = self.last_timestamp.replace(timestamp).unwrap_or(timestamp);
        let elapsed_seconds = timestamp
            .saturating_duration_since(previous)
            .as_secs_f32()
            .min(Self::MAX_ELAPSED_SECONDS);
        if elapsed_seconds == 0.0 {
            return Some(Point::default());
        }

        let delta = velocity.map(|value| value * elapsed_seconds);
        let decay = Self::DECELERATION_PER_REFERENCE_FRAME
            .powf(elapsed_seconds / Self::REFERENCE_FRAME_SECONDS);
        let next_velocity = velocity.map(|value| value * decay);

        if next_velocity.x.abs() < px(Self::STOP_VELOCITY)
            && next_velocity.y.abs() < px(Self::STOP_VELOCITY)
        {
            self.stop();
        } else {
            self.velocity = Some(next_velocity);
        }

        if delta.x.abs() < px(0.01) && delta.y.abs() < px(0.01) {
            Some(Point::default())
        } else {
            Some(delta)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn scroll_activity_uses_elapsed_frame_time() {
        let started_at = Instant::now();
        let mut activity = ScrollActivity::default();
        activity.start_at(point(px(600.), px(0.)), started_at);

        let delta = activity
            .next_delta_at(started_at + Duration::from_millis(10))
            .unwrap();

        assert!((f32::from(delta.x) - 6.).abs() < 0.01);
    }

    #[test]
    fn scroll_activity_clamps_a_stalled_frame() {
        let started_at = Instant::now();
        let mut activity = ScrollActivity::default();
        activity.start_at(point(px(600.), px(0.)), started_at);

        let delta = activity
            .next_delta_at(started_at + Duration::from_secs(1))
            .unwrap();

        assert!((f32::from(delta.x) - 30.).abs() < 0.01);
    }

    #[test]
    fn programmatic_and_ballistic_activity_replace_each_other() {
        let started_at = Instant::now();
        let mut activity = ScrollActivity::default();

        activity.begin_programmatic();
        assert_eq!(activity.kind(), ScrollActivityKind::Programmatic);

        activity.start_at(point(px(0.0), px(-600.0)), started_at);
        assert_eq!(activity.kind(), ScrollActivityKind::Ballistic);

        activity.begin_programmatic();
        assert_eq!(activity.kind(), ScrollActivityKind::Programmatic);
        assert_eq!(activity.velocity(), None);
        assert_eq!(
            activity.next_delta_at(started_at + Duration::from_millis(10)),
            None
        );

        activity.stop();
        assert_eq!(activity.kind(), ScrollActivityKind::Idle);
    }

    #[test]
    fn scroll_activity_allocation_contract() {
        let started_at = Instant::now();
        let mut activity = ScrollActivity::default();
        activity.start_at(point(px(1200.), px(0.)), started_at);

        for frame in 1..=60 {
            std::hint::black_box(
                activity.next_delta_at(started_at + Duration::from_millis(frame * 10)),
            );
        }

        // The activity stores only scalar frame state. Its update path cannot
        // retain heap-owned work between refresh callbacks.
        assert!(std::mem::size_of::<ScrollActivity>() <= 32);
    }
}
