//! iOS windows end cleanly: a closed window leaves the FFI window list and detaches from the
//! UIKit objects that point back at it, drops its renderer before the view it draws into, and
//! hides its UIWindow. The iOS code cannot run on the hosts these tests run on, so they guard
//! its source; lane F runs it.

const WINDOW: &str = include_str!("../src/ios/window.rs");
const FFI: &str = include_str!("../src/ios/ffi.rs");

/// The body of `impl Drop for IosWindow`.
fn window_drop() -> &'static str {
    let start = WINDOW
        .find("impl Drop for IosWindow")
        .expect("IosWindow implements Drop");
    let rest = &WINDOW[start..];
    let end = rest.find("\nimpl ").expect("an item follows the Drop impl");
    &rest[..end]
}

#[test]
fn closed_windows_leave_the_ffi_window_list() {
    assert!(
        FFI.contains("pub(crate) fn unregister_window("),
        "the FFI layer must be able to forget a window"
    );
    assert!(
        window_drop().contains("unregister_window(self as *const Self)"),
        "lifecycle calls would reach a freed window"
    );
}

#[test]
fn closed_windows_detach_from_uikit_and_drop_their_renderer_first() {
    let drop = window_drop();
    for object in [
        "self.view",
        "self.text_input_view",
        "self.display_link_target.get()",
    ] {
        assert!(
            drop.contains(object),
            "{object} keeps a pointer to the freed window"
        );
    }
    assert!(
        drop.contains("= ptr::null_mut();"),
        "the back-pointers must be cleared"
    );
    let renderer = drop
        .find("self.renderer.lock().take()")
        .expect("the renderer must be dropped explicitly");
    let release = drop
        .find("autorelease")
        .expect("the UIKit objects must be released");
    assert!(
        renderer < release,
        "the renderer's surface must go before the view it draws into"
    );
    assert!(
        drop.contains("setHidden: true"),
        "a closed window must leave the screen"
    );
}

#[test]
fn ios_windows_render_to_images_through_the_shared_renderer() {
    let start = WINDOW
        .find("fn render_to_image(&self, scene: &Scene)")
        .expect("IosWindow implements render_to_image");
    let body = &WINDOW[start..start + 400];
    assert!(body.contains("renderer.render_to_image(scene)"), "{body}");
    assert!(body.contains("Metal is unavailable here"), "{body}");
}
