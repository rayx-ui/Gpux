//! FFI (Foreign Function Interface) module for iOS.
//!
//! This module exposes C-compatible functions that can be called from
//! Objective-C code in the iOS app delegate to initialize and control
//! the GPUI application lifecycle.
//!
//! ## Typical call sequence from Obj-C
//!
//! ```text
//! gpui_ios_run_demo()          // sets up platform + invokes finish-launching
//! gpui_ios_get_window()        // retrieve the GPUI window pointer
//! gpui_ios_request_frame(ptr)  // called every CADisplayLink tick
//! ```

use futures::future::LocalBoxFuture;
use gpui::{AppContext, Application, AsyncApp, RequestFrameOptions, WindowOptions};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::OnceLock;

/// Global storage for the GPUI application state.
/// This is set during initialization and used by FFI callbacks.
static IOS_APP_STATE: OnceLock<IosAppState> = OnceLock::new();

/// Holds the state needed for iOS FFI callbacks.
/// Note: On iOS, all UI code runs on the main thread, so we use a RefCell
/// instead of Mutex and don't require Send.
struct IosAppState {
    /// The callback to invoke when the app finishes launching.
    /// This is the closure passed to Application::run().
    /// Using std::cell::UnsafeCell since this is only accessed from the main thread.
    finish_launching: std::cell::UnsafeCell<Option<Box<dyn FnOnce()>>>,
}

// Safety: On iOS, all GPUI operations happen on the main thread.
// The FFI functions are only called from the iOS app delegate which runs on main thread.
// We implement both Send and Sync because OnceLock requires Send for its value type,
// and we need Sync for the static. The actual access is always single-threaded.
unsafe impl Send for IosAppState {}
unsafe impl Sync for IosAppState {}

// Safety wrapper for window list - only accessed from main thread
pub(crate) struct WindowListWrapper(
    pub(crate) std::cell::UnsafeCell<Vec<*const super::window::IosWindow>>,
);
unsafe impl Send for WindowListWrapper {}
unsafe impl Sync for WindowListWrapper {}

pub(crate) static IOS_WINDOW_LIST: OnceLock<WindowListWrapper> = OnceLock::new();

/// Initialize the GPUI iOS application.
///
/// This should be called from `application:didFinishLaunchingWithOptions:`
/// in the iOS app delegate, before any other GPUI functions.
///
/// Returns a pointer to the app state that should be passed to other FFI functions.
/// Returns null if initialization fails.
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_initialize() -> *mut c_void {
    log::info!("GPUI iOS: Initializing");

    // Initialize the app state
    let state = IosAppState {
        finish_launching: std::cell::UnsafeCell::new(None),
    };

    if IOS_APP_STATE.set(state).is_err() {
        log::error!("GPUI iOS: Already initialized");
        return std::ptr::null_mut();
    }

    // Initialize the window list
    let _ = IOS_WINDOW_LIST.set(WindowListWrapper(std::cell::UnsafeCell::new(Vec::new())));

    // Return a non-null pointer to indicate success
    // The actual state is stored in the static
    std::ptr::dangling_mut::<c_void>()
}

/// Register a window with the FFI layer.
///
/// This is called internally when a new IosWindow is created.
/// The window pointer can then be retrieved by Objective-C code.
///
/// # Safety
/// This must only be called from the main thread.
pub(crate) fn register_window(window: *const super::window::IosWindow) {
    if let Some(wrapper) = IOS_WINDOW_LIST.get() {
        unsafe {
            (*wrapper.0.get()).push(window);
            log::info!("GPUI iOS: Registered window {:p}", window);
        }
    }
}

/// Remove a window from the FFI layer before it is freed, so lifecycle calls and
/// [`gpui_ios_get_window`] never reach it afterwards.
///
/// # Safety
/// This must only be called from the main thread.
pub(crate) fn unregister_window(window: *const super::window::IosWindow) {
    if let Some(wrapper) = IOS_WINDOW_LIST.get() {
        unsafe {
            (*wrapper.0.get()).retain(|&registered| registered != window);
        }
    }
}

/// Get the most recently created window pointer.
///
/// Returns the pointer to the IosWindow that was most recently registered,
/// or null if no windows have been created.
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_get_window() -> *mut c_void {
    if let Some(wrapper) = IOS_WINDOW_LIST.get() {
        unsafe {
            let windows = &*wrapper.0.get();
            if let Some(&window) = windows.last() {
                log::info!("GPUI iOS: Returning window {:p}", window);
                return window as *mut c_void;
            }
        }
    }
    log::warn!("GPUI iOS: No windows registered");
    std::ptr::null_mut()
}

/// Store the finish launching callback.
///
/// This is called internally by IosPlatform::run() to store the callback
/// that will be invoked when the app finishes launching.
///
/// # Safety
/// This must only be called from the main thread.
pub(crate) fn set_finish_launching_callback(callback: Box<dyn FnOnce()>) {
    if let Some(state) = IOS_APP_STATE.get() {
        // Safety: Only called from main thread
        unsafe {
            *state.finish_launching.get() = Some(callback);
        }
    }
}

/// Called when the iOS app has finished launching.
///
/// This should be called from `application:didFinishLaunchingWithOptions:`
/// in the iOS app delegate, after `gpui_ios_initialize()` returns.
///
/// This invokes the callback passed to Application::run().
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_did_finish_launching(_app_ptr: *mut c_void) {
    log::info!("GPUI iOS: Did finish launching");

    if let Some(state) = IOS_APP_STATE.get() {
        // Safety: Only called from main thread
        let callback = unsafe { (*state.finish_launching.get()).take() };
        if let Some(callback) = callback {
            log::info!("GPUI iOS: Invoking finish launching callback");
            callback();
        } else {
            log::warn!("GPUI iOS: No finish launching callback registered");
        }
    } else {
        log::error!("GPUI iOS: Not initialized");
    }
}

/// Called when the iOS app will enter the foreground.
///
/// This should be called from `applicationWillEnterForeground:` in the app delegate.
/// This notifies all GPUI windows that the app is becoming active.
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_will_enter_foreground(_app_ptr: *mut c_void) {
    log::info!("GPUI iOS: Will enter foreground");

    // Notify all windows that they're becoming active
    if let Some(wrapper) = IOS_WINDOW_LIST.get() {
        unsafe {
            let windows = &*wrapper.0.get();
            for &window_ptr in windows.iter() {
                if !window_ptr.is_null() {
                    let window = &*window_ptr;
                    window.notify_visibility_change(gpui::WindowVisibility::Visible);
                    window.notify_active_status_change(true);
                }
            }
        }
    }
}

/// Called when the iOS app did become active.
///
/// This should be called from `applicationDidBecomeActive:` in the app delegate.
/// This indicates the app is now in the foreground and receiving events.
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_did_become_active(_app_ptr: *mut c_void) {
    log::info!("GPUI iOS: Did become active");

    // App is now fully active - windows should be notified
    if let Some(wrapper) = IOS_WINDOW_LIST.get() {
        unsafe {
            let windows = &*wrapper.0.get();
            for &window_ptr in windows.iter() {
                if !window_ptr.is_null() {
                    let window = &*window_ptr;
                    window.notify_active_status_change(true);
                }
            }
        }
    }
}

/// Called when the iOS app will resign active.
///
/// This should be called from `applicationWillResignActive:` in the app delegate.
/// This indicates the app is about to become inactive (e.g., incoming call, switching apps).
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_will_resign_active(_app_ptr: *mut c_void) {
    log::info!("GPUI iOS: Will resign active");

    // App is about to become inactive
    if let Some(wrapper) = IOS_WINDOW_LIST.get() {
        unsafe {
            let windows = &*wrapper.0.get();
            for &window_ptr in windows.iter() {
                if !window_ptr.is_null() {
                    let window = &*window_ptr;
                    window.notify_active_status_change(false);
                }
            }
        }
    }
}

/// Called when the iOS app did enter the background.
///
/// This should be called from `applicationDidEnterBackground:` in the app delegate.
/// At this point, the app should have already saved any user data and released
/// shared resources. The app will be suspended shortly after this returns.
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_did_enter_background(_app_ptr: *mut c_void) {
    log::info!("GPUI iOS: Did enter background");

    // Notify windows they're no longer visible
    if let Some(wrapper) = IOS_WINDOW_LIST.get() {
        unsafe {
            let windows = &*wrapper.0.get();
            for &window_ptr in windows.iter() {
                if !window_ptr.is_null() {
                    let window = &*window_ptr;
                    window.notify_active_status_change(false);
                    window.notify_visibility_change(gpui::WindowVisibility::Hidden);
                    // A backgrounded app gives its scroll-layer tiles back.
                    gpui::PlatformWindow::release_layer_tiles(window);
                }
            }
        }
    }
}

/// Called when the iOS app received a memory warning.
///
/// This should be called from `applicationDidReceiveMemoryWarning:` in the app delegate (or from
/// a view controller's `didReceiveMemoryWarning`). The windows give their scroll-layer tiles
/// back; the next frame that composites a layer rasterizes them again.
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_did_receive_memory_warning(_app_ptr: *mut c_void) {
    log::warn!("GPUI iOS: Received a memory warning");

    if let Some(wrapper) = IOS_WINDOW_LIST.get() {
        unsafe {
            let windows = &*wrapper.0.get();
            for &window_ptr in windows.iter() {
                if !window_ptr.is_null() {
                    gpui::PlatformWindow::release_layer_tiles(&*window_ptr);
                }
            }
        }
    }
}

/// Called when the iOS app will terminate.
///
/// This should be called from `applicationWillTerminate:` in the app delegate.
/// This is a good place to save any unsaved data.
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_will_terminate(_app_ptr: *mut c_void) {
    log::info!("GPUI iOS: Will terminate");

    // Quit callbacks would be invoked here if registered.
}

/// Called when a touch event occurs.
///
/// This bridges UIKit touch events to GPUI's input system.
/// Parameters:
/// - `window_ptr`: Pointer to the IosWindow
/// - `touch_ptr`: Pointer to the UITouch object
/// - `event_ptr`: Pointer to the UIEvent object
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_handle_touch(
    window_ptr: *mut c_void,
    touch_ptr: *mut c_void,
    event_ptr: *mut c_void,
) {
    if window_ptr.is_null() || touch_ptr.is_null() {
        return;
    }

    // Cast to IosWindow and forward the touch event
    let window = unsafe { &*(window_ptr as *const super::window::IosWindow) };
    window.handle_touch(
        touch_ptr as *mut objc2::runtime::AnyObject,
        event_ptr as *mut objc2::runtime::AnyObject,
    );
}

/// Request a frame to be rendered.
///
/// This should be called from CADisplayLink callback to trigger GPUI rendering.
/// The window_ptr should be the value returned by gpui_ios_get_window().
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_request_frame(window_ptr: *mut c_void) {
    if window_ptr.is_null() {
        return;
    }

    // Safety: window_ptr must be a valid pointer to an IosWindow
    let window = unsafe { &*(window_ptr as *const super::window::IosWindow) };

    // Take the callback, invoke it, then restore it
    // We must complete the borrow before invoking the callback,
    // as the callback might try to borrow the same RefCell
    let callback = window.request_frame_callback.borrow_mut().take();
    if let Some(mut cb) = callback {
        // Check if text input, startup, activation, or layout work arrived
        // since last frame; those paths need GPUI to render even when the
        // normal invalidator has already been drained.
        let text_dirty = crate::TEXT_INPUT_DIRTY.swap(false, std::sync::atomic::Ordering::AcqRel);
        let lifecycle_dirty = window.take_force_next_frame_render();
        let force_render = text_dirty || lifecycle_dirty;

        cb(RequestFrameOptions {
            force_render,
            ..Default::default()
        });
        // Restore the callback for the next frame
        window.request_frame_callback.borrow_mut().replace(cb);
    }
}

/// Show the software keyboard.
///
/// Call this when a text input field gains focus.
/// The window_ptr should be the value returned by gpui_ios_get_window().
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_show_keyboard(window_ptr: *mut c_void) {
    if window_ptr.is_null() {
        return;
    }

    log::info!("GPUI iOS: Show keyboard requested");

    let window = unsafe { &*(window_ptr as *const super::window::IosWindow) };
    window.show_keyboard_with_type(crate::KeyboardType::Default);
}

/// Hide the software keyboard.
///
/// Call this when a text input field loses focus.
/// The window_ptr should be the value returned by gpui_ios_get_window().
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_hide_keyboard(window_ptr: *mut c_void) {
    if window_ptr.is_null() {
        return;
    }

    log::info!("GPUI iOS: Hide keyboard requested");

    let window = unsafe { &*(window_ptr as *const super::window::IosWindow) };
    window.hide_keyboard();
}

/// Handle text input from the software keyboard.
///
/// This is called when the user types on the keyboard.
/// Parameters:
/// - `window_ptr`: Pointer to the IosWindow
/// - `text_ptr`: Pointer to NSString with the entered text
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_handle_text_input(window_ptr: *mut c_void, text_ptr: *mut c_void) {
    if window_ptr.is_null() || text_ptr.is_null() {
        return;
    }

    log::info!("GPUI iOS: Handle text input");

    let window = unsafe { &*(window_ptr as *const super::window::IosWindow) };
    window.handle_text_input(text_ptr as *mut objc2::runtime::AnyObject);
}

/// Handle a key event from an external keyboard.
///
/// Parameters:
/// - `window_ptr`: Pointer to the IosWindow
/// - `key_code`: The key code from UIKeyboardHIDUsage
/// - `modifiers`: Modifier flags from UIKeyModifierFlags
/// - `is_key_down`: true for key down, false for key up
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_handle_key_event(
    window_ptr: *mut c_void,
    key_code: u32,
    modifiers: u32,
    is_key_down: bool,
) {
    if window_ptr.is_null() {
        return;
    }

    log::info!(
        "GPUI iOS: Handle key event - code: {}, modifiers: {}, down: {}",
        key_code,
        modifiers,
        is_key_down
    );

    let window = unsafe { &*(window_ptr as *const super::window::IosWindow) };
    window.handle_key_event(key_code, modifiers, is_key_down);
}

/// Called from ObjC when the app receives a URL to open.
///
/// Parameters:
/// - url_ptr: Pointer to an NSString containing the URL
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_handle_open_url(url_ptr: *mut c_void) {
    if url_ptr.is_null() {
        return;
    }

    let url_string = unsafe {
        use objc2::msg_send;
        let ns_str = url_ptr as *mut objc2::runtime::AnyObject;
        let cstr: *const std::ffi::c_char = msg_send![ns_str, UTF8String];
        if cstr.is_null() {
            return;
        }
        std::ffi::CStr::from_ptr(cstr)
            .to_string_lossy()
            .into_owned()
    };

    log::info!("GPUI iOS: Received deep link: {}", url_string);
}

// ── App callback storage ─────────────────────────────────────────────────────

/// Wrapper around an `UnsafeCell<Option<AppCallback>>`.
///
/// # Safety
/// On iOS all UI work happens on the main thread.  The FFI entry points
/// (`set_app_callback`, `run_app`, `gpui_ios_run_demo`) are only ever
/// called from the main thread, so interior-mutable access is safe.
#[allow(clippy::type_complexity)]
type AppCallback = Box<dyn FnOnce(AsyncApp) -> LocalBoxFuture<'static, ()>>;

struct AppCallbackCell(std::cell::UnsafeCell<Option<AppCallback>>);

// Safety: only accessed from the iOS main thread.
unsafe impl Send for AppCallbackCell {}
unsafe impl Sync for AppCallbackCell {}

static APP_CALLBACK: OnceLock<AppCallbackCell> = OnceLock::new();

/// Applies a host's asset source to the GPUI application `run_app` builds.
type AppAssets = Box<dyn FnOnce(Application) -> Application>;

struct AppAssetsCell(std::cell::UnsafeCell<Option<AppAssets>>);

// Safety: only accessed from the iOS main thread.
unsafe impl Send for AppAssetsCell {}
unsafe impl Sync for AppAssetsCell {}

static APP_ASSETS: OnceLock<AppAssetsCell> = OnceLock::new();

/// Registers the asset source that images and SVGs load through.
///
/// `run_app` creates the GPUI `Application` itself, so without this an iOS host has no way to give
/// it assets (the Android host passes them to `Application::with_assets`). Call it before
/// `gpui_ios_run_demo`, from the registration entry point.
pub fn set_app_assets(source: impl gpui::AssetSource) {
    let cell = APP_ASSETS.get_or_init(|| AppAssetsCell(std::cell::UnsafeCell::new(None)));
    unsafe {
        *cell.0.get() = Some(Box::new(move |app| app.with_assets(source)));
    }
}

fn take_app_assets() -> Option<AppAssets> {
    APP_ASSETS
        .get()
        .and_then(|cell| unsafe { (*cell.0.get()).take() })
}

/// Retains the GPUI application after `Application::run` returns.
///
/// On iOS, UIKit owns the outer application loop and GPUI startup is completed
/// through later app-delegate callbacks. Keeping a clone here preserves GPUI
/// application state for windows, frame callbacks, and other FFI entry points
/// after the one-shot finish-launching callback has run.
struct RetainedApplicationCell(std::cell::UnsafeCell<Option<Application>>);

// Safety: only accessed from the iOS main thread.
unsafe impl Send for RetainedApplicationCell {}
unsafe impl Sync for RetainedApplicationCell {}

static RETAINED_APPLICATION: OnceLock<RetainedApplicationCell> = OnceLock::new();

fn retain_application(app: Application) {
    let cell = RETAINED_APPLICATION
        .get_or_init(|| RetainedApplicationCell(std::cell::UnsafeCell::new(None)));
    unsafe {
        *cell.0.get() = Some(app);
    }
}

/// Register a callback that will be invoked inside `Application::run`.
///
/// This must be called **before** [`run_app`] so that the run-loop
/// has something to do (open a window, create views, etc.).
///
/// # Safety
/// Must be called from the main thread only.
pub fn set_app_callback(cb: AppCallback) {
    let cell = APP_CALLBACK.get_or_init(|| AppCallbackCell(std::cell::UnsafeCell::new(None)));
    unsafe {
        *cell.0.get() = Some(cb);
    }
}

#[allow(clippy::type_complexity)]
fn take_app_callback() -> Option<AppCallback> {
    APP_CALLBACK
        .get()
        .and_then(|cell| unsafe { (*cell.0.get()).take() })
}

/// C entry point called from `main.m`'s app delegate.
///
/// Consumer crates should call [`set_app_callback`] **before** this function
/// to register their root view.  If no callback is registered an empty
/// window is opened as a fallback.
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_run_demo() {
    run_app();
}

/// Writes `log` records to stderr, which `xcrun simctl launch --console` and Xcode show; iOS has
/// no other sink for Rust logging.
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
    }

    fn flush(&self) {}
}

/// Installs the stderr logger unless the host app already set one. The level comes from the
/// `GPUI_IOS_LOG` environment variable (`error`, `warn`, `info`, `debug`, `trace`; `warn` when
/// unset); with `xcrun simctl launch` pass it as `SIMCTL_CHILD_GPUI_IOS_LOG=debug`.
pub fn init_logging() {
    static LOGGER: StderrLogger = StderrLogger;
    let level = std::env::var("GPUI_IOS_LOG")
        .ok()
        .and_then(|value| value.parse::<log::LevelFilter>().ok())
        .unwrap_or(log::LevelFilter::Warn);
    let installed = log::set_logger(&LOGGER).is_ok();
    if installed {
        log::set_max_level(level);
    }
    eprintln!("GPUI iOS: logging at {level} (stderr logger installed: {installed})");
}

/// Run the GPUI iOS application.
///
/// This initialises the platform, creates the `Application`, and enters the
/// GPUI run loop.  The actual UI is determined by a callback previously
/// registered via [`set_app_callback`].  If no callback was registered a
/// default empty window is opened so the app doesn't crash.
pub fn run_app() {
    init_logging();
    log::info!("GPUI iOS: Starting application");

    // Initialise the FFI layer if not already done.
    if IOS_APP_STATE.get().is_none() {
        let state = IosAppState {
            finish_launching: std::cell::UnsafeCell::new(None),
        };
        let _ = IOS_APP_STATE.set(state);
        let _ = IOS_WINDOW_LIST.set(WindowListWrapper(std::cell::UnsafeCell::new(Vec::new())));
    }

    let platform = Rc::new(super::IosPlatform::new());
    let app = Application::with_platform(platform);
    let app = match take_app_assets() {
        Some(apply) => apply(app),
        None => app,
    };
    retain_application(app.clone());
    app.run(|cx| async move {
        if let Some(cb) = take_app_callback() {
            log::info!("GPUI iOS: Invoking user-provided app callback");
            cb(cx).await;
        } else {
            log::warn!("GPUI iOS: No app callback registered — opening default empty window");
            cx.update(|cx| {
                cx.open_window(
                    WindowOptions {
                        window_bounds: None,
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| gpui::Empty),
                )
                .expect("Failed to open default window");
                cx.activate(true);
            });
        }
    });

    // On iOS, Application::run() stores the callback and returns immediately.
    // The finish-launching callback is forwarded to set_finish_launching_callback
    // and invoked here synchronously (in a real app the app delegate does this).
    if let Some(state) = IOS_APP_STATE.get() {
        let callback = unsafe { (*state.finish_launching.get()).take() };
        if let Some(callback) = callback {
            log::info!("GPUI iOS: Invoking Application::run callback");
            callback();
        }
    }
}
