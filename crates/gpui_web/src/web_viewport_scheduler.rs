use std::cell::Cell;

use gpux::{Pixels, Size};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WebViewportObservation {
    pub(crate) logical_size: Size<Pixels>,
    pub(crate) scale_factor: f32,
    pub(crate) allow_zero_size: bool,
}

/// Retains only the newest browser viewport observation until the next frame.
///
/// Browser callbacks must enqueue here instead of synchronously touching the
/// GPUI window, which prevents `ResizeObserver` re-entry from recursively
/// publishing a resize.
#[derive(Default)]
pub(crate) struct WebViewportScheduler {
    pending: Cell<Option<WebViewportObservation>>,
}

impl WebViewportScheduler {
    pub(crate) fn enqueue(&self, observation: WebViewportObservation) {
        self.pending.set(Some(observation));
    }

    pub(crate) fn flush(&self) -> Option<WebViewportObservation> {
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpux::px;

    #[test]
    fn web_viewport_scheduler_coalesces_latest() {
        let scheduler = WebViewportScheduler::default();
        scheduler.enqueue(WebViewportObservation {
            logical_size: Size {
                width: px(320.0),
                height: px(640.0),
            },
            scale_factor: 1.0,
            allow_zero_size: true,
        });
        scheduler.enqueue(WebViewportObservation {
            logical_size: Size {
                width: px(960.0),
                height: px(720.0),
            },
            scale_factor: 2.0,
            allow_zero_size: true,
        });

        assert_eq!(
            scheduler.flush(),
            Some(WebViewportObservation {
                logical_size: Size {
                    width: px(960.0),
                    height: px(720.0),
                },
                scale_factor: 2.0,
                allow_zero_size: true,
            })
        );
        assert_eq!(scheduler.flush(), None);
    }
}
