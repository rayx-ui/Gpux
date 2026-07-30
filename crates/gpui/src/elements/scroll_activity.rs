use crate::{Instant, Pixels, Point, point, px};

/// The transient deceleration state for a direct touch scroll.
#[derive(Debug, Default)]
pub(crate) struct ScrollActivity {
    velocity: Option<Point<Pixels>>,
    last_timestamp: Option<Instant>,
}

impl ScrollActivity {
    const REFERENCE_FRAME_SECONDS: f32 = 1.0 / 60.0;
    const DECELERATION_PER_REFERENCE_FRAME: f32 = 0.88;
    const STOP_VELOCITY: f32 = 8.0;
    const MAX_ELAPSED_SECONDS: f32 = 0.05;

    pub(crate) fn start_at(&mut self, velocity: Point<Pixels>, timestamp: Instant) {
        self.velocity = Some(point(
            velocity.x.clamp(px(-1200.0), px(1200.0)),
            velocity.y.clamp(px(-1200.0), px(1200.0)),
        ));
        self.last_timestamp = Some(timestamp);
    }

    pub(crate) fn stop(&mut self) {
        self.velocity = None;
        self.last_timestamp = None;
    }

    pub(crate) fn begin_direct_touch(&mut self, timestamp: Instant) {
        self.velocity = None;
        self.last_timestamp = Some(timestamp);
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
