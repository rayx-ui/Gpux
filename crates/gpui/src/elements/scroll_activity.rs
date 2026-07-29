use crate::{Pixels, Point, point, px};

/// The transient deceleration state for a direct touch scroll.
#[derive(Debug, Default)]
pub(crate) struct ScrollActivity {
    velocity: Option<Point<Pixels>>,
}

impl ScrollActivity {
    const FRAME_SECONDS: f32 = 1.0 / 60.0;
    const DECELERATION: f32 = 0.88;
    const STOP_VELOCITY: f32 = 8.0;

    pub(crate) fn start(&mut self, velocity: Point<Pixels>) {
        self.velocity = Some(point(
            velocity.x.clamp(px(-1200.0), px(1200.0)),
            velocity.y.clamp(px(-1200.0), px(1200.0)),
        ));
    }

    pub(crate) fn stop(&mut self) {
        self.velocity = None;
    }

    pub(crate) fn next_delta(&mut self) -> Option<Point<Pixels>> {
        let velocity = self.velocity?;
        let delta = velocity.map(|value| value * Self::FRAME_SECONDS);
        let next_velocity = velocity.map(|value| value * Self::DECELERATION);

        if next_velocity.x.abs() < px(Self::STOP_VELOCITY)
            && next_velocity.y.abs() < px(Self::STOP_VELOCITY)
        {
            self.stop();
        } else {
            self.velocity = Some(next_velocity);
        }

        Some(delta)
    }
}
