//! WebView2 embedded through window composition on Windows, under a GPUI
//! menu, popover and dialog (see `webview_common`).
//!
//! The web view is hosted by a composition controller whose root visual
//! target is the native surface's DirectComposition visual. Visuals take no
//! input, so the example forwards the pointer over the native area to the
//! controller (`SendMouseInput`). Needs the WebView2 runtime, which current
//! Windows ships:
//!
//! ```text
//! cargo run -p gpux-platform --features webview-examples --example webview_windows
//! ```

#[path = "webview_common/mod.rs"]
mod common;

#[cfg(target_os = "windows")]
fn main() {
    common::run("webview_windows", embed);
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("webview_windows runs on Windows");
}

#[cfg(target_os = "windows")]
const PAGE: &str = r#"<!doctype html><html><body style="margin:0;font:20px 'Segoe UI';background:linear-gradient(135deg,#0b5,#07c);color:white;height:100vh;display:flex;flex-direction:column;align-items:center;justify-content:center">
<div id="clock" style="font-size:56px"></div><p>A live WebView2 under GPUI's overlays.</p>
<input placeholder="type here" style="font-size:18px;padding:6px">
<script>setInterval(()=>document.getElementById('clock').textContent=new Date().toLocaleTimeString(),200)</script>
</body></html>"#;

#[cfg(target_os = "windows")]
fn embed(
    surface: &gpux::WindowCompositionSurface,
    window: &mut gpux::Window,
    cx: &mut gpux::App,
) -> anyhow::Result<common::Embedded> {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use std::{rc::Rc, sync::mpsc, time::Duration};
    use webview2_com::{
        CreateCoreWebView2CompositionControllerCompletedHandler,
        CreateCoreWebView2EnvironmentCompletedHandler,
        Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_MOUSE_EVENT_KIND, COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_DOWN,
            COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_UP, COREWEBVIEW2_MOUSE_EVENT_KIND_MOVE,
            COREWEBVIEW2_MOUSE_EVENT_KIND_WHEEL, COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS_NONE,
            CreateCoreWebView2Environment, ICoreWebView2CompositionController,
            ICoreWebView2Controller, ICoreWebView2Environment3,
        },
    };
    use windows::Win32::Foundation::{E_POINTER, HWND, POINT, RECT};
    use windows_core::{HSTRING, IUnknown, Interface as _};

    let hwnd = match window
        .window_handle()
        .map_err(|error| anyhow::anyhow!("the window has no handle: {error}"))?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
        _ => anyhow::bail!("not a Win32 window"),
    };
    let visual = *surface
        .platform_surface()?
        .platform_handle()?
        .downcast::<IUnknown>()
        .map_err(|_| anyhow::anyhow!("the native surface is not a DirectComposition visual"))?;

    let environment = {
        let (sender, receiver) = mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(|handler| unsafe {
                CreateCoreWebView2Environment(&handler).map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, environment| {
                error_code?;
                sender
                    .send(environment.ok_or_else(|| windows_core::Error::from(E_POINTER)))
                    .ok();
                Ok(())
            }),
        )?;
        receiver.recv()??
    };
    let composition_controller: ICoreWebView2CompositionController = {
        let environment = environment.cast::<ICoreWebView2Environment3>()?;
        let (sender, receiver) = mpsc::channel();
        CreateCoreWebView2CompositionControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                environment
                    .CreateCoreWebView2CompositionController(hwnd, &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, controller| {
                error_code?;
                sender
                    .send(controller.ok_or_else(|| windows_core::Error::from(E_POINTER)))
                    .ok();
                Ok(())
            }),
        )?;
        receiver.recv()??
    };
    let controller = composition_controller.cast::<ICoreWebView2Controller>()?;
    unsafe {
        composition_controller.SetRootVisualTarget(&visual)?;
        controller.SetIsVisible(true)?;
        controller
            .CoreWebView2()?
            .NavigateToString(&HSTRING::from(PAGE))?;
    }

    // Keep the web view the native surface's size.
    let resize = cx.spawn({
        let controller = controller.clone();
        let surface = surface.clone();
        async move |cx| {
            let mut last = None;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let Ok(size) = surface
                    .platform_surface()
                    .map(|native| native.bounds().size)
                else {
                    break;
                };
                if last != Some(size) {
                    last = Some(size);
                    unsafe {
                        controller
                            .SetBounds(RECT {
                                left: 0,
                                top: 0,
                                right: size.width.0,
                                bottom: size.height.0,
                            })
                            .ok();
                    }
                }
            }
        }
    });

    let scale = window.scale_factor();
    let pointer = {
        let composition_controller = composition_controller.clone();
        let surface = surface.clone();
        move |pointer: common::Pointer| {
            let (kind, position, data): (COREWEBVIEW2_MOUSE_EVENT_KIND, _, i32) = match pointer {
                common::Pointer::Down(position) => {
                    (COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_DOWN, position, 0)
                }
                common::Pointer::Up(position) => {
                    (COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_UP, position, 0)
                }
                common::Pointer::Move(position) => {
                    (COREWEBVIEW2_MOUSE_EVENT_KIND_MOVE, position, 0)
                }
                common::Pointer::Wheel(position, delta) => (
                    COREWEBVIEW2_MOUSE_EVENT_KIND_WHEEL,
                    position,
                    (delta * 1.2) as i32,
                ),
            };
            let Ok(origin) = surface
                .platform_surface()
                .map(|native| native.bounds().origin)
            else {
                return;
            };
            let point = POINT {
                x: (f32::from(position.x) * scale) as i32 - origin.x.0,
                y: (f32::from(position.y) * scale) as i32 - origin.y.0,
            };
            unsafe {
                composition_controller
                    .SendMouseInput(
                        kind,
                        COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS_NONE,
                        data as u32,
                        point,
                    )
                    .ok();
            }
        }
    };
    Ok(common::Embedded {
        owner: Box::new((composition_controller, controller, resize)),
        pointer: Some(Rc::new(pointer)),
    })
}
