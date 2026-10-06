use super::ScrollBehavior;
use crate::{Axis, Instant, Pixels, Point, Size, point};

/// The origin of an accepted scroll mutation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollMutationSource {
    /// Mouse wheel, precision trackpad, or recognized touch scroll input.
    Wheel,
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
    /// No direct or programmatic activity is active.
    #[default]
    Idle,
    /// A direct scrollbar-thumb drag is active.
    Direct,
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

/// The transient direct or programmatic activity of one scroll owner.
#[derive(Debug, Default)]
pub(crate) struct ScrollActivity {
    direct: bool,
    programmatic: Option<ProgrammaticDriver>,
    generation: u64,
}

#[derive(Clone, Copy, Debug)]
enum ProgrammaticDriver {
    Marker,
    Smooth {
        start: Point<Pixels>,
        target: Point<Pixels>,
        started_at: Instant,
        duration: std::time::Duration,
        generation: u64,
    },
}

/// One deterministic owner update from a programmatic scroll request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProgrammaticScrollFrame {
    pub(crate) offset: Point<Pixels>,
    pub(crate) generation: u64,
    pub(crate) request_next_frame: bool,
}

impl ScrollActivity {
    pub(crate) fn stop(&mut self) {
        self.direct = false;
        self.cancel_programmatic();
    }

    pub(crate) fn begin_direct(&mut self) {
        self.cancel_programmatic();
        self.direct = true;
    }

    pub(crate) fn begin_programmatic(&mut self) {
        self.direct = false;
        self.advance_generation();
        self.programmatic = Some(ProgrammaticDriver::Marker);
    }

    /// Start or replace the single programmatic driver.
    ///
    /// Offsets use positive logical coordinates. The caller remains responsible
    /// for mapping them to its physical storage convention.
    pub(crate) fn start_programmatic_at(
        &mut self,
        current: Point<Pixels>,
        target: Point<Pixels>,
        max_offset: Point<Pixels>,
        behavior: ScrollBehavior,
        timestamp: Instant,
    ) -> Option<ProgrammaticScrollFrame> {
        self.direct = false;
        self.advance_generation();
        let generation = self.generation;

        let Some((current, target)) = valid_clamped_offsets(current, target, max_offset) else {
            self.programmatic = None;
            return None;
        };
        let Some(duration) = behavior.smooth_duration() else {
            self.programmatic = None;
            return Some(ProgrammaticScrollFrame {
                offset: target,
                generation,
                request_next_frame: false,
            });
        };
        if current == target {
            self.programmatic = None;
            return Some(ProgrammaticScrollFrame {
                offset: target,
                generation,
                request_next_frame: false,
            });
        }

        self.programmatic = Some(ProgrammaticDriver::Smooth {
            start: current,
            target,
            started_at: timestamp,
            duration,
            generation,
        });
        Some(ProgrammaticScrollFrame {
            offset: current,
            generation,
            request_next_frame: true,
        })
    }

    /// Sample the active driver for one authoritative frame timestamp.
    ///
    /// A stale generation returns no update, so a canceled or retargeted frame
    /// cannot mutate the current owner.
    pub(crate) fn next_programmatic_at(
        &mut self,
        generation: u64,
        timestamp: Instant,
        max_offset: Point<Pixels>,
    ) -> Option<ProgrammaticScrollFrame> {
        let ProgrammaticDriver::Smooth {
            start,
            target,
            started_at,
            duration,
            generation: active_generation,
        } = self.programmatic?
        else {
            return None;
        };
        if generation != active_generation {
            return None;
        }

        let Some((start, target)) = valid_clamped_offsets(start, target, max_offset) else {
            self.cancel_programmatic();
            return None;
        };
        if max_offset == Point::default() || start == target {
            self.programmatic = None;
            return Some(ProgrammaticScrollFrame {
                offset: target,
                generation,
                request_next_frame: false,
            });
        }

        let progress = timestamp
            .saturating_duration_since(started_at)
            .as_secs_f32()
            / duration.as_secs_f32();
        if progress >= 1.0 {
            self.programmatic = None;
            return Some(ProgrammaticScrollFrame {
                offset: target,
                generation,
                request_next_frame: false,
            });
        }

        let eased = cubic_ease_out(progress.clamp(0.0, 1.0));
        Some(ProgrammaticScrollFrame {
            offset: point(
                start.x + (target.x - start.x) * eased,
                start.y + (target.y - start.y) * eased,
            ),
            generation,
            request_next_frame: true,
        })
    }

    #[cfg(test)]
    fn programmatic_generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn kind(&self) -> ScrollActivityKind {
        if self.programmatic.is_some() {
            ScrollActivityKind::Programmatic
        } else if self.direct {
            ScrollActivityKind::Direct
        } else {
            ScrollActivityKind::Idle
        }
    }

    pub(crate) fn is_direct(&self) -> bool {
        self.direct
    }

    fn cancel_programmatic(&mut self) {
        if self.programmatic.take().is_some() {
            self.advance_generation();
        }
    }

    fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

fn cubic_ease_out(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

fn valid_clamped_offsets(
    current: Point<Pixels>,
    target: Point<Pixels>,
    max_offset: Point<Pixels>,
) -> Option<(Point<Pixels>, Point<Pixels>)> {
    if !point_is_finite(current) || !point_is_finite(target) || !point_is_finite(max_offset) {
        return None;
    }
    let maximum = point(
        max_offset.x.max(Pixels::ZERO),
        max_offset.y.max(Pixels::ZERO),
    );
    Some((
        clamp_offset(current, maximum),
        clamp_offset(target, maximum),
    ))
}

fn point_is_finite(point: Point<Pixels>) -> bool {
    f32::from(point.x).is_finite() && f32::from(point.y).is_finite()
}

fn clamp_offset(offset: Point<Pixels>, maximum: Point<Pixels>) -> Point<Pixels> {
    point(
        offset.x.clamp(Pixels::ZERO, maximum.x),
        offset.y.clamp(Pixels::ZERO, maximum.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::px;
    use std::time::Duration;

    #[test]
    fn programmatic_and_direct_activity_replace_each_other() {
        let mut activity = ScrollActivity::default();

        activity.begin_programmatic();
        assert_eq!(activity.kind(), ScrollActivityKind::Programmatic);

        activity.begin_direct();
        assert_eq!(activity.kind(), ScrollActivityKind::Direct);

        activity.begin_programmatic();
        assert_eq!(activity.kind(), ScrollActivityKind::Programmatic);
        assert!(!activity.is_direct());

        activity.stop();
        assert_eq!(activity.kind(), ScrollActivityKind::Idle);
    }

    #[test]
    fn programmatic_scroll_samples_both_axes_with_cubic_ease_out_and_exact_endpoints() {
        let started_at = Instant::now();
        let mut activity = ScrollActivity::default();
        let initial = activity
            .start_programmatic_at(
                point(px(10.0), px(20.0)),
                point(px(100.0), px(200.0)),
                point(px(300.0), px(300.0)),
                ScrollBehavior::Smooth(Duration::from_secs(1)),
                started_at,
            )
            .unwrap();

        assert_eq!(initial.offset, point(px(10.0), px(20.0)));
        assert!(initial.request_next_frame);
        assert_eq!(activity.kind(), ScrollActivityKind::Programmatic);

        let midpoint = activity
            .next_programmatic_at(
                initial.generation,
                started_at + Duration::from_millis(500),
                point(px(300.0), px(300.0)),
            )
            .unwrap();
        assert_eq!(midpoint.offset, point(px(88.75), px(177.5)));
        assert!(midpoint.request_next_frame);

        let endpoint = activity
            .next_programmatic_at(
                initial.generation,
                started_at + Duration::from_secs(1),
                point(px(300.0), px(300.0)),
            )
            .unwrap();
        assert_eq!(endpoint.offset, point(px(100.0), px(200.0)));
        assert!(!endpoint.request_next_frame);
        assert_eq!(activity.kind(), ScrollActivityKind::Idle);
        assert_eq!(
            activity.next_programmatic_at(
                initial.generation,
                started_at + Duration::from_millis(1_100),
                point(px(300.0), px(300.0)),
            ),
            None
        );
    }

    #[test]
    fn programmatic_scroll_instant_and_zero_duration_apply_one_clamped_offset() {
        let started_at = Instant::now();
        for behavior in [
            ScrollBehavior::Instant,
            ScrollBehavior::Smooth(Duration::ZERO),
        ] {
            let mut activity = ScrollActivity::default();
            let frame = activity
                .start_programmatic_at(
                    point(px(20.0), px(30.0)),
                    point(px(500.0), px(-20.0)),
                    point(px(120.0), px(160.0)),
                    behavior,
                    started_at,
                )
                .unwrap();

            assert_eq!(frame.offset, point(px(120.0), Pixels::ZERO));
            assert!(!frame.request_next_frame);
            assert_eq!(activity.kind(), ScrollActivityKind::Idle);
        }
    }

    #[test]
    fn programmatic_scroll_retarget_starts_at_rendered_offset_and_rejects_stale_generation() {
        let started_at = Instant::now();
        let mut activity = ScrollActivity::default();
        let first = activity
            .start_programmatic_at(
                Point::default(),
                point(px(100.0), px(200.0)),
                point(px(300.0), px(300.0)),
                ScrollBehavior::Smooth(Duration::from_secs(1)),
                started_at,
            )
            .unwrap();
        let rendered = activity
            .next_programmatic_at(
                first.generation,
                started_at + Duration::from_millis(500),
                point(px(300.0), px(300.0)),
            )
            .unwrap()
            .offset;

        let second = activity
            .start_programmatic_at(
                rendered,
                point(px(200.0), px(40.0)),
                point(px(300.0), px(300.0)),
                ScrollBehavior::Smooth(Duration::from_secs(1)),
                started_at + Duration::from_millis(500),
            )
            .unwrap();
        assert_ne!(second.generation, first.generation);
        assert_eq!(second.offset, rendered);
        assert_eq!(
            activity.next_programmatic_at(
                first.generation,
                started_at + Duration::from_millis(750),
                point(px(300.0), px(300.0)),
            ),
            None
        );

        let second_midpoint = activity
            .next_programmatic_at(
                second.generation,
                started_at + Duration::from_secs(1),
                point(px(300.0), px(300.0)),
            )
            .unwrap();
        assert_eq!(
            second_midpoint.offset,
            point(
                rendered.x + (px(200.0) - rendered.x) * 0.875,
                rendered.y + (px(40.0) - rendered.y) * 0.875,
            )
        );
    }

    #[test]
    fn programmatic_scroll_direct_input_cancels_active_generation() {
        let started_at = Instant::now();
        let mut activity = ScrollActivity::default();
        let smooth = activity
            .start_programmatic_at(
                Point::default(),
                point(px(100.0), px(100.0)),
                point(px(200.0), px(200.0)),
                ScrollBehavior::Smooth(Duration::from_secs(1)),
                started_at,
            )
            .unwrap();
        activity.begin_direct();
        assert_eq!(activity.kind(), ScrollActivityKind::Direct);
        assert_ne!(activity.programmatic_generation(), smooth.generation);
        assert_eq!(
            activity.next_programmatic_at(
                smooth.generation,
                started_at + Duration::from_millis(20),
                point(px(200.0), px(200.0)),
            ),
            None
        );
    }

    #[test]
    fn programmatic_scroll_geometry_changes_clamp_or_complete_without_stale_frames() {
        let started_at = Instant::now();
        let mut activity = ScrollActivity::default();
        let smooth = activity
            .start_programmatic_at(
                Point::default(),
                point(px(100.0), px(80.0)),
                point(px(100.0), px(80.0)),
                ScrollBehavior::Smooth(Duration::from_secs(1)),
                started_at,
            )
            .unwrap();

        let clamped = activity
            .next_programmatic_at(
                smooth.generation,
                started_at + Duration::from_millis(500),
                point(px(40.0), px(20.0)),
            )
            .unwrap();
        assert_eq!(clamped.offset, point(px(35.0), px(17.5)));
        assert!(clamped.request_next_frame);

        let completed = activity
            .next_programmatic_at(
                smooth.generation,
                started_at + Duration::from_millis(600),
                Point::default(),
            )
            .unwrap();
        assert_eq!(completed.offset, Point::default());
        assert!(!completed.request_next_frame);
        assert_eq!(activity.kind(), ScrollActivityKind::Idle);

        let invalid = activity.start_programmatic_at(
            Point::default(),
            point(px(f32::NAN), px(20.0)),
            point(px(100.0), px(100.0)),
            ScrollBehavior::Smooth(Duration::from_secs(1)),
            started_at,
        );
        assert_eq!(invalid, None);
        assert_eq!(activity.kind(), ScrollActivityKind::Idle);
    }

    #[test]
    fn scroll_activity_allocation_contract() {
        // The activity stores only scalar driver state. Its update path cannot
        // retain heap-owned work between refresh callbacks.
        assert!(std::mem::size_of::<ScrollActivity>() <= 96);
    }
}
