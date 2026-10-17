//! The handlers VoiceOver's adapter (`accesskit_ios`) calls, forwarding to the window's
//! [`gpui::A11yCallbacks`]. (Android's adapter serves the front window of several; see
//! `android::accessibility`.)

/// Builds the initial tree when a screen reader activates the adapter.
pub(crate) struct ActivationHandler(
    pub(crate) Box<dyn Fn() -> Option<accesskit::TreeUpdate> + Send + 'static>,
);

impl accesskit::ActivationHandler for ActivationHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        (self.0)()
    }
}

/// Runs what a screen reader asks for.
pub(crate) struct ActionHandler(pub(crate) Box<dyn Fn(accesskit::ActionRequest) + Send + 'static>);

impl accesskit::ActionHandler for ActionHandler {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        (self.0)(request);
    }
}

/// Turns the platform source off when the screen reader stops.
pub(crate) struct DeactivationHandler(pub(crate) Box<dyn Fn() + Send + 'static>);

impl accesskit::DeactivationHandler for DeactivationHandler {
    fn deactivate_accessibility(&mut self) {
        (self.0)();
    }
}
