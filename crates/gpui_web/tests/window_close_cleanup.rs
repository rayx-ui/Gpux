//! Closing a `WebWindow` releases every browser resource it created, so applications that open
//! and close windows (RayX's multi-window action history) do not leak frames, observers or DOM
//! nodes. `WebWindow` only compiles for the web, so the guard reads its source.

#[test]
fn closing_a_web_window_releases_its_browser_resources() {
    let window = include_str!("../src/window.rs");
    for cleanup in [
        "cancel_animation_frame(raf_id)",
        "observer.disconnect()",
        "self.inner.mql_handle.borrow_mut().take()",
        "self.inner.canvas.remove()",
        "self.inner.ime_mirror.remove()",
    ] {
        assert!(
            window.contains(cleanup),
            "closing a WebWindow must retain cleanup step {cleanup}"
        );
    }
}
