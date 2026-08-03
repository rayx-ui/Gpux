use std::time::Duration;

/// Selects how an explicit programmatic scroll request reaches its resolved target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollBehavior {
    /// Apply the target offset in one owner update.
    #[default]
    Instant,
    /// Interpolate to the target over the supplied duration.
    Smooth(Duration),
}

impl ScrollBehavior {
    pub(crate) fn smooth_duration(self) -> Option<Duration> {
        match self {
            Self::Instant => None,
            Self::Smooth(duration) if duration.is_zero() => None,
            Self::Smooth(duration) => Some(duration),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programmatic_scroll_behavior_defaults_to_instant_and_normalizes_zero_duration() {
        assert_eq!(ScrollBehavior::default(), ScrollBehavior::Instant);
        assert_eq!(ScrollBehavior::Instant.smooth_duration(), None);
        assert_eq!(
            ScrollBehavior::Smooth(Duration::ZERO).smooth_duration(),
            None
        );
        assert_eq!(
            ScrollBehavior::Smooth(Duration::from_millis(250)).smooth_duration(),
            Some(Duration::from_millis(250))
        );
    }
}
