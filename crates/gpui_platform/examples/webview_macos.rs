//! A WKWebView embedded through window composition on macOS, under a GPUI
//! menu, popover and dialog (see `webview_common`).
//!
//! ```text
//! cargo run -p gpux-platform --features webview-examples --example webview_macos
//! ```

#[path = "webview_common/mod.rs"]
mod common;

#[cfg(target_os = "macos")]
fn main() {
    common::run("webview_macos", embed);
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("webview_macos runs on macOS");
}

/// The page the web view shows: live content (a ticking clock and an
/// animation) so the overlays visibly compose over a moving view.
#[cfg(target_os = "macos")]
const PAGE: &str = r#"<!doctype html><html><body style="margin:0;font:20px -apple-system;background:linear-gradient(135deg,#0b5,#07c);color:white;height:100vh;display:flex;flex-direction:column;align-items:center;justify-content:center">
<div id="clock" style="font-size:56px"></div><p>A live WKWebView under GPUI's overlays.</p>
<input placeholder="type here" style="font-size:18px;padding:6px">
<script>setInterval(()=>document.getElementById('clock').textContent=new Date().toLocaleTimeString(),200)</script>
</body></html>"#;

#[cfg(target_os = "macos")]
fn embed(
    surface: &gpux::WindowCompositionSurface,
    _: &mut gpux::Window,
    _: &mut gpux::App,
) -> anyhow::Result<common::Embedded> {
    use objc2::{MainThreadMarker, MainThreadOnly as _, rc::Retained};
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
    use objc2_foundation::NSString;
    use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow::anyhow!("not on the main thread"))?;
    let container = *surface
        .platform_surface()?
        .platform_handle()?
        .downcast::<usize>()
        .map_err(|_| anyhow::anyhow!("the native surface is not an AppKit view"))?;
    // SAFETY: the container is the live NSView the surface owns, and this
    // runs on the main thread.
    let container: &NSView = unsafe { &*(container as *const NSView) };
    let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    let web_view: Retained<WKWebView> = unsafe {
        WKWebView::initWithFrame_configuration(
            WKWebView::alloc(mtm),
            container.bounds(),
            &configuration,
        )
    };
    web_view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    container.addSubview(&web_view);
    unsafe { web_view.loadHTMLString_baseURL(&NSString::from_str(PAGE), None) };
    Ok(common::Embedded {
        owner: Box::new(web_view),
        pointer: None,
    })
}
