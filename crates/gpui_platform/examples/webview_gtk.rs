//! WebKitGTK embedded through window composition on X11 (or XWayland),
//! under a GPUI menu, popover and dialog (see `webview_common`).
//!
//! The web view lives in an undecorated GTK window that is reparented into
//! the native surface's child X window, and GTK's main loop is pumped from a
//! GPUI timer. Needs WebKitGTK 4.1 (`libwebkit2gtk-4.1-dev`) and an X11
//! session (`GDK_BACKEND=x11` under XWayland):
//!
//! ```text
//! cargo run -p gpux-platform --features webview-examples,x11 --example webview_gtk
//! ```

#[path = "webview_common/mod.rs"]
mod common;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() {
    // GTK talks X11 to reach the native surface's X window.
    if std::env::var_os("GDK_BACKEND").is_none() {
        // SAFETY: nothing else runs yet.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
    common::run("webview_gtk", embed);
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    eprintln!("webview_gtk runs on Linux under X11 or XWayland");
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
const PAGE: &str = r#"<!doctype html><html><body style="margin:0;font:20px sans-serif;background:linear-gradient(135deg,#0b5,#07c);color:white;height:100vh;display:flex;flex-direction:column;align-items:center;justify-content:center">
<div id="clock" style="font-size:56px"></div><p>A live WebKitGTK view under GPUI's overlays.</p>
<input placeholder="type here" style="font-size:18px;padding:6px">
<script>setInterval(()=>document.getElementById('clock').textContent=new Date().toLocaleTimeString(),200)</script>
</body></html>"#;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn embed(
    surface: &gpux::WindowCompositionSurface,
    _: &mut gpux::Window,
    cx: &mut gpux::App,
) -> anyhow::Result<common::Embedded> {
    use gtk::prelude::*;
    use raw_window_handle::RawWindowHandle;
    use std::time::Duration;
    use webkit2gtk::{WebView, WebViewExt as _};
    use x11rb::{connection::Connection as _, protocol::xproto::ConnectionExt as _};

    let handle = *surface
        .platform_surface()?
        .platform_handle()?
        .downcast::<RawWindowHandle>()
        .map_err(|_| anyhow::anyhow!("the native surface has no raw window handle"))?;
    let RawWindowHandle::Xcb(handle) = handle else {
        anyhow::bail!("webview_gtk needs an X11 session (try GDK_BACKEND=x11 under XWayland)");
    };
    let parent = handle.window.get();

    gtk::init()?;
    let window = gtk::Window::new(gtk::WindowType::Popup);
    window.set_decorated(false);
    let bounds = surface.platform_surface()?.bounds();
    window.set_default_size(bounds.size.width.0.max(1), bounds.size.height.0.max(1));
    let web_view = WebView::new();
    web_view.load_html(PAGE, None);
    window.add(&web_view);
    window.show_all();

    let gdk_window = window
        .window()
        .ok_or_else(|| anyhow::anyhow!("the GTK window is not realized"))?;
    let xid = gdk_window
        .downcast::<gdkx11::X11Window>()
        .map_err(|_| anyhow::anyhow!("GTK is not on X11"))?
        .xid() as u32;
    let (connection, _) = x11rb::connect(None)?;
    connection.reparent_window(xid, parent, 0, 0)?;
    connection.map_window(xid)?;
    connection.flush()?;

    // Pump GTK, and keep the web view the native surface's size.
    let surface = surface.clone();
    let pump = cx.spawn({
        let window = window.clone();
        async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(8))
                .await;
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            if let Ok(platform_surface) = surface.platform_surface() {
                let size = platform_surface.bounds().size;
                let (width, height) = (size.width.0.max(1), size.height.0.max(1));
                if window.size() != (width, height) {
                    window.resize(width, height);
                }
            }
        }
    });
    Ok(common::Embedded {
        owner: Box::new((window, web_view, connection, pump)),
        pointer: None,
    })
}
