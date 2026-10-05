//! Closing a `WebWindow` releases every browser resource it created, so applications that open
//! and close windows (RayX's multi-window action history) do not leak frames, observers or DOM
//! nodes. `WebWindow` only compiles for the web, so the guard reads its source.

#[test]
fn closing_a_web_window_releases_its_browser_resources() {
    let window = include_str!("../src/window.rs");
    for cleanup in [
        "cancel_animation_frame(request_id)",
        "observer.disconnect()",
        "self.inner.canvas.remove()",
        "self.inner.input_element.remove()",
    ] {
        assert!(
            window.contains(cleanup),
            "closing a WebWindow must retain cleanup step {cleanup}"
        );
    }
}
