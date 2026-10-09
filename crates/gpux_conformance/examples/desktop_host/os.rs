//! What the desktop host asks of the operating system: OS-level synthetic input, cursor parking
//! and scale changes. Windows uses `SendInput`; X11 uses XTest through `xdotool`; other systems
//! (Wayland, macOS) have no synthetic input for an unprivileged client, so their scenes inject
//! input in-app only. macOS re-reads the OS cursor every frame of the active window, so its host
//! moves the cursor to every injected pointer position (see [`cursor_sync`]).

use gpux::{Pixels, Point, Window};
use gpux_conformance::app_host::CursorSync;

/// One line describing the OS-input path, printed before the results.
pub fn describe() -> String {
    imp::describe()
}

/// Moves the cursor off every scene window, so the platform's hover tracking cannot replace
/// injected pointer positions.
pub fn park_cursor() {
    imp::park_cursor();
}

/// What the host passes to `AppHost::with_cursor_sync`: `None` where the platform keeps injected
/// pointer positions while the OS cursor stays parked off the scene windows.
pub fn cursor_sync() -> Option<CursorSync> {
    imp::cursor_sync()
}

/// Whether OS-level input can be sent here; the error explains why not.
pub fn os_input_available() -> Result<(), String> {
    imp::os_input_available()
}

/// Brings `window` to the foreground so OS input reaches it.
pub fn bring_to_front(window: &Window) -> Result<(), String> {
    imp::bring_to_front(window)
}

/// Clicks the primary button at a window-local logical position with OS-level input.
pub fn click(window: &Window, position: Point<Pixels>) -> Result<(), String> {
    imp::click(window, position)
}

/// Types lowercase ASCII letters with OS-level key events.
pub fn type_letters(window: &Window, text: &str) -> Result<(), String> {
    assert!(text.chars().all(|c| c.is_ascii_lowercase()));
    imp::type_letters(window, text)
}

/// Whether the host can drive a scale change here; the error explains why not.
pub fn scale_change_supported() -> Result<(), String> {
    imp::scale_change_supported()
}

/// The native window behind `window`, for platform calls made outside GPUI updates.
pub fn native(window: &Window) -> Result<Native, String> {
    imp::native(window).map(Native)
}

/// A native window handle.
pub struct Native(imp::Native);

/// Tells the platform `native` moved from scale `from` to `to`, as when it moves to a display
/// of another density. Call it outside GPUI updates: the platform handles it synchronously and
/// re-enters GPUI.
pub fn change_scale(native: &Native, from: f32, to: f32) -> Result<(), String> {
    imp::change_scale(&native.0, from, to)
}

#[cfg(target_os = "windows")]
mod imp {
    use super::{Pixels, Point, Window};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::time::{Duration, Instant};
    use windows::Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::ClientToScreen,
        UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
                KEYEVENTF_KEYUP, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
                MOUSEINPUT, SendInput, VIRTUAL_KEY, VK_MENU,
            },
            WindowsAndMessaging::{
                GetForegroundWindow, GetWindowRect, SW_RESTORE, SendMessageW, SetCursorPos,
                SetForegroundWindow, ShowWindow, WM_DPICHANGED,
            },
        },
    };

    pub fn describe() -> String {
        "Windows, OS input through SendInput".into()
    }

    pub fn park_cursor() {
        // The scene windows open at (80, 80) and below; the screen's corner is clear of them.
        unsafe { SetCursorPos(0, 0) }.ok();
    }

    pub fn cursor_sync() -> Option<super::CursorSync> {
        None
    }

    pub fn os_input_available() -> Result<(), String> {
        Ok(())
    }

    fn hwnd(window: &Window) -> Result<HWND, String> {
        // `Window::window_handle` is GPUI's own handle; the native one comes from the trait.
        match HasWindowHandle::window_handle(window)
            .map_err(|error| error.to_string())?
            .as_raw()
        {
            RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut _)),
            other => Err(format!("not a Win32 window: {other:?}")),
        }
    }

    fn send(inputs: &[INPUT]) -> Result<(), String> {
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            Err(format!(
                "SendInput delivered {sent} of {} events ({})",
                inputs.len(),
                std::io::Error::last_os_error()
            ))
        }
    }

    fn mouse(flags: MOUSE_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn key(virtual_key: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: virtual_key,
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn bring_to_front(window: &Window) -> Result<(), String> {
        let hwnd = hwnd(window)?;
        unsafe {
            ShowWindow(hwnd, SW_RESTORE).ok().ok();
            if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
                return Ok(());
            }
        }
        // Windows lets a process take the foreground after it sent the last input event; a
        // bare Alt press and release is the least intrusive such event.
        send(&[key(VK_MENU, false), key(VK_MENU, true)])?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            unsafe {
                if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
                    return Ok(());
                }
            }
            if Instant::now() > deadline {
                return Err("another window keeps the foreground".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn click(window: &Window, position: Point<Pixels>) -> Result<(), String> {
        let hwnd = hwnd(window)?;
        let scale = window.scale_factor();
        let mut point = POINT {
            x: (f32::from(position.x) * scale).round() as i32,
            y: (f32::from(position.y) * scale).round() as i32,
        };
        if !unsafe { ClientToScreen(hwnd, &mut point) }.as_bool() {
            return Err("ClientToScreen failed".into());
        }
        unsafe { SetCursorPos(point.x, point.y) }.map_err(|error| error.to_string())?;
        send(&[mouse(MOUSEEVENTF_LEFTDOWN), mouse(MOUSEEVENTF_LEFTUP)])
    }

    pub fn type_letters(_: &Window, text: &str) -> Result<(), String> {
        let inputs: Vec<INPUT> = text
            .bytes()
            .flat_map(|letter| {
                // Virtual-key codes of letters are their uppercase ASCII codes.
                let virtual_key = VIRTUAL_KEY(letter.to_ascii_uppercase() as u16);
                [key(virtual_key, false), key(virtual_key, true)]
            })
            .collect();
        send(&inputs)
    }

    pub type Native = HWND;

    pub fn native(window: &Window) -> Result<Native, String> {
        hwnd(window)
    }

    pub fn scale_change_supported() -> Result<(), String> {
        Ok(())
    }

    pub fn change_scale(hwnd: &Native, from: f32, to: f32) -> Result<(), String> {
        let hwnd = *hwnd;
        let dpi = (to * 96.).round() as usize;
        let ratio = to / from;
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|error| error.to_string())?;
        // Windows suggests the window's rectangle scaled by the DPI ratio, so its logical size
        // stays the same at the new density.
        rect.right = rect.left + ((rect.right - rect.left) as f32 * ratio).round() as i32;
        rect.bottom = rect.top + ((rect.bottom - rect.top) as f32 * ratio).round() as i32;
        // What Windows sends when a window moves to a display of another DPI: the new DPI in
        // both halves of `wparam` and the suggested window rectangle in `lparam`.
        unsafe {
            SendMessageW(
                hwnd,
                WM_DPICHANGED,
                Some(WPARAM(dpi | (dpi << 16))),
                Some(LPARAM(&rect as *const RECT as isize)),
            );
        }
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod imp {
    use super::{Pixels, Point, Window};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::process::Command;

    fn x11() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some()
    }

    pub fn describe() -> String {
        if x11() {
            "Linux X11, OS input through XTest (xdotool)".into()
        } else {
            "Linux Wayland, in-app input only".into()
        }
    }

    #[allow(
        clippy::disallowed_methods,
        reason = "each xdotool call is a short synchronous request the host waits for"
    )]
    fn xdotool(args: &[String]) -> Result<(), String> {
        let output = Command::new("xdotool")
            .args(args)
            .output()
            .map_err(|error| format!("could not run xdotool: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "xdotool {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    pub fn park_cursor() {
        if x11() {
            xdotool(&["mousemove".into(), "0".into(), "0".into()]).ok();
        }
    }

    pub fn cursor_sync() -> Option<super::CursorSync> {
        None
    }

    pub fn os_input_available() -> Result<(), String> {
        if x11() {
            Ok(())
        } else {
            Err("Wayland offers no synthetic input to clients; scenes inject input in-app".into())
        }
    }

    fn window_id(window: &Window) -> Result<String, String> {
        // `Window::window_handle` is GPUI's own handle; the native one comes from the trait.
        match HasWindowHandle::window_handle(window)
            .map_err(|error| error.to_string())?
            .as_raw()
        {
            RawWindowHandle::Xcb(handle) => Ok(handle.window.get().to_string()),
            RawWindowHandle::Xlib(handle) => Ok(handle.window.to_string()),
            other => Err(format!("not an X11 window: {other:?}")),
        }
    }

    pub fn bring_to_front(window: &Window) -> Result<(), String> {
        xdotool(&["windowfocus".into(), "--sync".into(), window_id(window)?])
    }

    pub fn click(window: &Window, position: Point<Pixels>) -> Result<(), String> {
        let scale = window.scale_factor();
        let x = (f32::from(position.x) * scale).round() as i32;
        let y = (f32::from(position.y) * scale).round() as i32;
        xdotool(&[
            "mousemove".into(),
            "--window".into(),
            window_id(window)?,
            x.to_string(),
            y.to_string(),
            "click".into(),
            "1".into(),
        ])
    }

    pub fn type_letters(_: &Window, text: &str) -> Result<(), String> {
        xdotool(&["type".into(), "--delay".into(), "30".into(), text.into()])
    }

    pub type Native = ();

    pub fn native(_: &Window) -> Result<Native, String> {
        Ok(())
    }

    pub fn scale_change_supported() -> Result<(), String> {
        Err(
            "Linux windows take their scale from the display server, which a client cannot change"
                .into(),
        )
    }

    pub fn change_scale(_: &Native, _: f32, _: f32) -> Result<(), String> {
        scale_change_supported()
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "freebsd")))]
mod imp {
    use super::{Pixels, Point, Window};

    const NO_OS_INPUT: &str = "posting OS input needs an accessibility permission that hosted \
                               systems do not grant; scenes inject input in-app";

    #[cfg(not(target_os = "macos"))]
    pub fn describe() -> String {
        format!("{}, in-app input only", std::env::consts::OS)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn park_cursor() {}

    #[cfg(not(target_os = "macos"))]
    pub fn cursor_sync() -> Option<super::CursorSync> {
        None
    }

    #[cfg(target_os = "macos")]
    pub use super::macos::{cursor_sync, describe, park_cursor};

    pub fn os_input_available() -> Result<(), String> {
        Err(NO_OS_INPUT.into())
    }

    pub fn bring_to_front(_: &Window) -> Result<(), String> {
        Err(NO_OS_INPUT.into())
    }

    pub fn click(_: &Window, _: Point<Pixels>) -> Result<(), String> {
        Err(NO_OS_INPUT.into())
    }

    pub fn type_letters(_: &Window, _: &str) -> Result<(), String> {
        Err(NO_OS_INPUT.into())
    }

    pub type Native = ();

    pub fn native(_: &Window) -> Result<Native, String> {
        Ok(())
    }

    pub fn scale_change_supported() -> Result<(), String> {
        Err("the platform takes the scale from the display, which a client cannot change".into())
    }

    pub fn change_scale(_: &Native, _: f32, _: f32) -> Result<(), String> {
        scale_change_supported()
    }
}

/// macOS: the active window takes its mouse position from the OS cursor every frame, so the host
/// moves the cursor, which needs no accessibility permission (`CGWarpMouseCursorPosition` posts no
/// events), to every injected pointer position, and parks it between scenes.
#[cfg(target_os = "macos")]
mod macos {
    use super::{CursorSync, Pixels, Point, Window};
    use core_graphics::{display::CGDisplay, geometry::CGPoint};
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::rc::Rc;

    pub fn describe() -> String {
        "macOS, in-app input with the OS cursor moved to injected positions".into()
    }

    pub fn park_cursor() {
        // The scene windows open at (80, 80) and below; the screen's corner is clear of them.
        CGDisplay::warp_mouse_cursor_position(CGPoint::new(0., 0.)).ok();
    }

    pub fn cursor_sync() -> Option<CursorSync> {
        Some(Rc::new(|window, position| {
            match screen_position(window, position) {
                Ok(point) => {
                    if let Err(error) = CGDisplay::warp_mouse_cursor_position(point) {
                        eprintln!("desktop_host: could not move the cursor: CGError {error}");
                    }
                }
                Err(error) => eprintln!("desktop_host: could not move the cursor: {error}"),
            }
        }))
    }

    /// A window-local logical position in CoreGraphics' global display space, whose origin is the
    /// primary display's top-left (AppKit's screen space starts at its bottom-left).
    fn screen_position(window: &Window, position: Point<Pixels>) -> Result<CGPoint, String> {
        // `Window::window_handle` is GPUI's own handle; the native one comes from the trait.
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)
            .map_err(|error| error.to_string())?
            .as_raw()
        else {
            return Err("not an AppKit window".into());
        };
        // SAFETY: the handle names the window's content view, alive while the window is, and the
        // host runs on the main thread.
        let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        let native = view.window().ok_or("the content view has no window")?;
        let on_screen = native.convertRectToScreen(view.convertRect_toView(view.bounds(), None));
        let primary_height = CGDisplay::main().bounds().size.height;
        Ok(CGPoint::new(
            on_screen.origin.x + f64::from(f32::from(position.x)),
            primary_height - (on_screen.origin.y + on_screen.size.height)
                + f64::from(f32::from(position.y)),
        ))
    }
}
