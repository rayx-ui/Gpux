//! Android entry point and event loop using the `android-activity` crate.
//!
//! This module replaces the previous hand-rolled `ANativeActivity_onCreate`,
//! `JNI_OnLoad`, and lifecycle callback implementations with the higher-level
//! `android-activity` glue layer.
//!
//! ## Entry sequence
//!
//! ```text
//! android-activity loads the .so and calls android_main(app: AndroidApp)
//!   └── We store the AndroidApp globally
//!       └── Call the user-supplied gpui_android_main(app)
//! ```
//!
//! ## Threading model
//!
//! `android-activity` spawns a dedicated native thread and calls `android_main`
//! on it.  All GPUI draw / event callbacks run on this thread.  The
//! `AndroidApp` handle is `Send + Sync` and can be shared across threads.
//!
//! ## User entry point
//!
//! Applications must define:
//!
//! ```rust,no_run
//! #[no_mangle]
//! fn android_main(app: android_activity::AndroidApp) {
//!     // Initialise GPUI and run the application.
//! }
//! ```
//!
//! ## Event handling
//!
//! Lifecycle events (window creation/destruction, focus changes, etc.) are
//! delivered via `AndroidApp::poll_events()`.  Input events are obtained via
//! `AndroidApp::input_events_iter()`.

#![allow(unsafe_code)]
#![allow(non_snake_case)]

use std::{
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};

#[cfg(feature = "android-native-activity")]
use std::{sync::Arc, time::Duration};

/// Whether the deferred init-window callback has already been invoked.
///
/// Reset to `false` on `TerminateWindow` so that when the surface is
/// recreated on resume the init callbacks run again.
#[cfg(feature = "android-native-activity")]
static INIT_WINDOW_DONE: AtomicBool = AtomicBool::new(false);

/// Whether the GPUI native library has completed initialization.
///
/// Set to `true` after the first frame is rendered. This can be queried
/// via JNI by a custom Activity to dismiss the splash screen, although
/// with NativeActivity the system splash handles this automatically.
pub static NATIVE_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Deferred lifecycle flags.
///
/// We must NOT call `win.set_active()` or `platform.did_enter_background()`
/// inside `handle_main_event` (which runs within `poll_events`).
/// The android-activity crate's Java-side callbacks block on a condvar
/// waiting for the native thread to finish processing the command.
/// If our handler tries to acquire the window `state` lock (a
/// `parking_lot::Mutex`), and a background thread is holding it (e.g.
/// during a render pass), we deadlock: native waits on the lock,
/// the lock holder waits for native rendering to complete, but native
/// is stuck.
///
/// Instead, the handlers set these flags and the main loop body
/// processes them AFTER `poll_events` returns.
#[cfg(feature = "android-native-activity")]
static PAUSE_PENDING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "android-native-activity")]
static RESUME_PENDING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "android-native-activity")]
static TERM_WINDOW_PENDING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "android-native-activity")]
static INIT_WINDOW_PENDING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "android-native-activity")]
static WINDOW_RESIZED_PENDING: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "android-native-activity")]
static CONFIG_CHANGED_PENDING: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "android-native-activity")]
use android_activity::{AndroidApp, MainEvent, PollEvent};

#[cfg(feature = "android-native-activity")]
use super::platform::{AndroidPlatform, SharedPlatform};
#[cfg(feature = "android-native-activity")]
use super::window::AndroidWindow;

use jni::JavaVM;
use jni::objects::{JObject, JString, JValue};

// ── JNI helpers (safe `jni` crate wrappers) ──────────────────────────────────

static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
static EMBEDDED_JAVA_VM: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static EMBEDDED_ACTIVITY: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Get or create the static `JavaVM` wrapper.
fn java_vm_safe() -> Result<&'static JavaVM, String> {
    if let Some(vm) = JAVA_VM.get() {
        return Ok(vm);
    }
    let ptr = java_vm();
    if ptr.is_null() {
        return Err("JavaVM not available".into());
    }
    Ok(JAVA_VM.get_or_init(|| unsafe { JavaVM::from_raw(ptr as *mut jni::sys::JavaVM) }))
}

/// Run a closure with an attached `jni::Env` for the current thread.
///
/// In jni 0.22 the `attach_current_thread` API is closure-based.
/// The thread is auto-detached when the closure returns (if it was
/// not already attached).
pub fn with_env<T>(f: impl FnOnce(&mut jni::Env) -> Result<T, String>) -> Result<T, String> {
    let vm = java_vm_safe()?;
    let mut result: Option<Result<T, String>> = None;
    vm.attach_current_thread(|env: &mut jni::Env| -> Result<(), jni::errors::Error> {
        result = Some(f(env));
        Ok(())
    })
    .map_err(|e: jni::errors::Error| e.to_string())?;
    result.unwrap()
}

/// Convenience alias: kept so existing callers that import `obtain_env`
/// compile with minimal changes. Returns a result by running the given
/// closure inside `with_env`.
#[inline]
pub fn obtain_env<T>(f: impl FnOnce(&mut jni::Env) -> Result<T, String>) -> Result<T, String> {
    with_env(f)
}

/// Get the Activity as a [`JObject`].
///
/// `activity_as_ptr()` returns a JNI global reference from `android-activity`
/// that is valid for the lifetime of the app. We wrap it in a `JObject`.
///
/// Requires `&Env` because jni 0.22's `JObject::from_raw` binds the
/// local-reference-frame lifetime.
pub fn activity<'local>(env: &jni::Env<'local>) -> Result<JObject<'local>, String> {
    let ptr = activity_as_ptr();
    if ptr.is_null() {
        return Err("Activity not available".into());
    }
    Ok(unsafe { JObject::from_raw(env, ptr as jni::sys::jobject) })
}

/// Convert a Java String (`JObject` wrapping a `java.lang.String`) to a Rust `String`.
///
/// Returns an empty string on null or error.
pub fn get_string(env: &mut jni::Env<'_>, obj: &JObject<'_>) -> String {
    if obj.is_null() {
        return String::new();
    }
    let jstr = unsafe { JString::from_raw(env, obj.as_raw()) };
    jstr.to_string()
}

/// Extension trait for converting `jni::errors::Result<T>` to `Result<T, String>`.
pub(crate) trait JniExt<T> {
    fn e(self) -> Result<T, String>;
}

impl<T> JniExt<T> for jni::errors::Result<T> {
    fn e(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
}

/// Find an application class by name using the Activity's classloader.
///
/// From native threads, `JNIEnv::FindClass` uses the system classloader
/// which doesn't know about application classes.  This helper uses the
/// Activity's classloader via `activity.getClass().getClassLoader().loadClass(name)`.
///
/// `class_name` uses Java dot notation (e.g. `"dev.rayx.mobile.RayXHelper"`).
pub fn find_app_class<'local>(
    env: &mut jni::Env<'local>,
    class_name: &str,
) -> Result<jni::objects::JClass<'local>, String> {
    let act = activity(env)?;

    // activity.getClassLoader() — call on the Context instance directly.
    // Do NOT use activity.getClass().getClassLoader(): NativeActivity is a
    // framework class loaded by BootClassLoader, which cannot see app classes.
    let class_loader = env
        .call_method(
            &act,
            jni::jni_str!("getClassLoader"),
            jni::jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )
        .and_then(|v| v.l())
        .map_err(|e| {
            env.exception_clear();
            let msg = format!("getClassLoader failed: {e}");
            log::error!("find_app_class({class_name}): {msg}");
            msg
        })?;

    // classLoader.loadClass("dev.rayx.mobile.RayXHelper")
    let jname = env.new_string(class_name).e()?;
    let loaded = env
        .call_method(
            &class_loader,
            jni::jni_str!("loadClass"),
            jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&jname)],
        )
        .and_then(|v| v.l())
        .map_err(|e| {
            // Print full Java stack trace to logcat, then clear.
            env.exception_describe();
            env.exception_clear();
            let msg = format!("loadClass({class_name}) failed: {e}");
            log::error!("{msg}");
            msg
        })?;

    log::debug!("find_app_class: loaded {class_name}");
    Ok(unsafe { jni::objects::JClass::from_raw(env, loaded.as_raw()) })
}

// ── global state ─────────────────────────────────────────────────────────────

/// The `AndroidApp` handle from `android-activity`.
///
/// Set by every `android_main`: see [`Current`].
#[cfg(feature = "android-native-activity")]
static ANDROID_APP: Current<AndroidApp> = Current::new();

/// Process-global `AndroidPlatform` instance.
///
/// Created by every `android_main`: see [`Current`].
#[cfg(feature = "android-native-activity")]
static PLATFORM: Current<Arc<AndroidPlatform>> = Current::new();

/// A process-wide value that each `android_main` replaces. Android can start
/// the native activity again in the same process after destroying it (the
/// lane E emulator on CI does at startup), and that run must not get the
/// previous run's `AndroidApp`, or its platform, whose dispatcher belongs to
/// the previous run's thread and looper (App construction then panics with
/// "must construct App on main thread"). A replaced value is leaked, so a
/// reference still held elsewhere stays valid.
#[cfg(feature = "android-native-activity")]
struct Current<T>(std::sync::atomic::AtomicPtr<T>);

#[cfg(feature = "android-native-activity")]
impl<T: Send + Sync> Current<T> {
    const fn new() -> Self {
        Self(std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()))
    }

    fn get(&self) -> Option<&'static T> {
        // SAFETY: the pointer is null or a leaked `Box`, never freed.
        unsafe { self.0.load(std::sync::atomic::Ordering::Acquire).as_ref() }
    }

    fn set(&self, value: T) -> &'static T {
        let leaked: &'static mut T = Box::leak(Box::new(value));
        self.0.store(leaked, std::sync::atomic::Ordering::Release);
        leaked
    }
}

/// Get the unicode character produced by an Android key event via JNI.
///
/// This creates a `android.view.KeyEvent` Java object and calls
/// `getUnicodeChar(metaState)` on it.  Returns 0 on failure.
pub fn unicode_char_for_key_event(key_code: i32, action: i32, meta_state: i32) -> u32 {
    with_env(|env| {
        let key_event = match env.new_object(
            jni::jni_str!("android/view/KeyEvent"),
            jni::jni_sig!("(II)V"),
            &[JValue::Int(action), JValue::Int(key_code)],
        ) {
            Ok(o) => o,
            Err(_) => {
                env.exception_clear();
                return Ok(0);
            }
        };
        match env.call_method(
            &key_event,
            jni::jni_str!("getUnicodeChar"),
            jni::jni_sig!("(I)I"),
            &[JValue::Int(meta_state)],
        ) {
            Ok(v) => {
                let c = v.i().unwrap_or(0);
                Ok(if c > 0 { c as u32 } else { 0 })
            }
            Err(_) => {
                env.exception_clear();
                Ok(0)
            }
        }
    })
    .unwrap_or(0)
}

// ── public accessors ──────────────────────────────────────────────────────────

/// Public accessor for the JavaVM pointer.
///
/// Uses `AndroidApp::vm_as_ptr()` from the stored `AndroidApp`.
/// Used by `platform.rs` for JNI calls.
pub fn java_vm() -> *mut c_void {
    #[cfg(feature = "android-native-activity")]
    if let Some(app) = ANDROID_APP.get() {
        return app.vm_as_ptr();
    }

    EMBEDDED_JAVA_VM.load(Ordering::Acquire)
}

/// Store Java VM and Activity handles for managed-Activity embedded Android hosts.
pub fn set_embedded_context(java_vm: *mut c_void, activity: *mut c_void) -> bool {
    if java_vm.is_null() || activity.is_null() {
        return false;
    }

    EMBEDDED_JAVA_VM.store(java_vm, Ordering::Release);
    EMBEDDED_ACTIVITY.store(activity, Ordering::Release);
    true
}

/// Clear the managed-Activity embedded JNI handles.
pub fn clear_embedded_context() {
    // TalkBack's adapter is on the cleared Activity's view: it goes with it.
    #[cfg(feature = "accessibility")]
    super::accessibility::release();
    EMBEDDED_JAVA_VM.store(std::ptr::null_mut(), Ordering::Release);
    EMBEDDED_ACTIVITY.store(std::ptr::null_mut(), Ordering::Release);
}

/// Public accessor for the current Activity's JNI object reference.
///
/// Uses `AndroidApp::activity_as_ptr()` from the stored `AndroidApp` when
/// NativeActivity owns the app, or the managed Activity handle installed by
/// RayX's embedded Android host.
pub fn activity_as_ptr() -> *mut c_void {
    #[cfg(feature = "android-native-activity")]
    if let Some(app) = ANDROID_APP.get() {
        return app.activity_as_ptr();
    }

    EMBEDDED_ACTIVITY.load(Ordering::Acquire)
}

/// Returns a clone of the stored `AndroidApp`, if initialised.
#[cfg(feature = "android-native-activity")]
pub fn android_app() -> Option<AndroidApp> {
    ANDROID_APP.get().cloned()
}

/// Returns a reference to the global `AndroidPlatform`, if initialised.
///
/// Returns `None` before `android_main` has set it up.
#[cfg(feature = "android-native-activity")]
pub fn platform() -> Option<&'static Arc<AndroidPlatform>> {
    PLATFORM.get()
}

/// Returns a [`SharedPlatform`] wrapping the global `Arc<AndroidPlatform>`.
///
/// This is the value you hand to `Application::with_platform(...)`:
///
/// ```rust,no_run
/// let platform = jni::shared_platform().unwrap();
/// Application::with_platform(platform.into_rc()).run(|cx| { … });
/// ```
///
/// Returns `None` before `init_platform` has been called.
#[cfg(feature = "android-native-activity")]
pub fn shared_platform() -> Option<SharedPlatform> {
    PLATFORM
        .get()
        .map(|arc| SharedPlatform::new(Arc::clone(arc)))
}

// ── input event types ─────────────────────────────────────────────────────────

/// Motion event action constants from the NDK.
#[cfg(feature = "android-native-activity")]
const AMOTION_EVENT_ACTION_DOWN: u32 = 0;
#[cfg(feature = "android-native-activity")]
const AMOTION_EVENT_ACTION_UP: u32 = 1;
#[cfg(feature = "android-native-activity")]
const AMOTION_EVENT_ACTION_MOVE: u32 = 2;

// ── night mode query via NDK Configuration ───────────────────────────────────

/// Query the current night mode using the NDK Configuration API.
///
/// Returns `true` if the system is in dark mode.
pub fn query_night_mode_via_jni() -> bool {
    #[cfg(not(feature = "android-native-activity"))]
    {
        return false;
    }

    #[cfg(feature = "android-native-activity")]
    {
        let app = match android_app() {
            Some(app) => app,
            None => return false,
        };

        // Build an ndk::configuration::Configuration from the app's asset manager.
        let config = ndk::configuration::Configuration::from_asset_manager(&app.asset_manager());
        let is_dark = config.ui_mode_night() == ndk::configuration::UiModeNight::Yes;

        log::debug!("query_night_mode (ndk): is_dark={}", is_dark);
        is_dark
    }
}

// ── input event processing ────────────────────────────────────────────────────

/// Process input events from the `AndroidApp` and dispatch them to the window.
#[cfg(feature = "android-native-activity")]
fn process_input_events(app: &AndroidApp) {
    let platform = match PLATFORM.get() {
        Some(p) => p,
        None => {
            log::trace!("process_input_events: no platform yet");
            return;
        }
    };

    let win = match platform.primary_window() {
        Some(w) => w,
        None => {
            log::trace!("process_input_events: no primary window yet");
            return;
        }
    };

    match app.input_events_iter() {
        Ok(mut iter) => {
            loop {
                let read_input = iter.next(|event| {
                    use android_activity::input::{InputEvent, MotionAction};

                    match event {
                        InputEvent::MotionEvent(motion_event) => {
                            let action = motion_event.action();
                            let pointer_count = motion_event.pointer_count();

                            log::debug!(
                                "process_input_events: MotionEvent action={:?} pointers={}",
                                action,
                                pointer_count,
                            );

                            let timestamp = input_event_instant(motion_event.event_time());
                            for i in 0..pointer_count {
                                let pointer = motion_event.pointer_at_index(i);

                                let touch_action = match action {
                                    MotionAction::Down => AMOTION_EVENT_ACTION_DOWN,
                                    MotionAction::PointerDown => {
                                        // For pointer down, only dispatch the specific pointer
                                        if i != motion_event.pointer_index() {
                                            continue;
                                        }
                                        AMOTION_EVENT_ACTION_DOWN
                                    }
                                    MotionAction::Up => AMOTION_EVENT_ACTION_UP,
                                    MotionAction::PointerUp => {
                                        // For pointer up, only dispatch the specific pointer
                                        if i != motion_event.pointer_index() {
                                            continue;
                                        }
                                        AMOTION_EVENT_ACTION_UP
                                    }
                                    MotionAction::Move => AMOTION_EVENT_ACTION_MOVE,
                                    MotionAction::Cancel => AMOTION_EVENT_ACTION_UP,
                                    _ => continue,
                                };

                                // Android batches the samples of a fast gesture into the
                                // event's history; replay them as moves, each at its own time,
                                // so pans and flings see the whole motion.
                                if touch_action != AMOTION_EVENT_ACTION_DOWN {
                                    for sample in pointer.history() {
                                        win.handle_touch(crate::android::TouchPoint {
                                            id: pointer.pointer_id(),
                                            x: sample.x(),
                                            y: sample.y(),
                                            action: AMOTION_EVENT_ACTION_MOVE,
                                            timestamp: input_event_instant(sample.event_time()),
                                        });
                                    }
                                }

                                let touch = crate::android::TouchPoint {
                                    id: pointer.pointer_id(),
                                    x: pointer.x(),
                                    y: pointer.y(),
                                    action: touch_action,
                                    timestamp,
                                };

                                log::debug!(
                                    "process_input_events: dispatching touch id={} x={:.0} y={:.0} action={}",
                                    touch.id, touch.x, touch.y, touch.action,
                                );

                                win.handle_touch(touch);
                            }

                            android_activity::InputStatus::Handled
                        }
                        InputEvent::KeyEvent(key_event) => {
                            use android_activity::input::KeyAction;

                            let action = match key_event.action() {
                                KeyAction::Down => 0,
                                KeyAction::Up => 1,
                                _ => return android_activity::InputStatus::Unhandled,
                            };

                            let key_code: u32 = key_event.key_code().into();
                            let meta_state: u32 = key_event.meta_state().0;

                            let unicode_char = unicode_char_for_key_event(
                                key_code as i32,
                                action,
                                meta_state as i32,
                            );

                            if unicode_char != 0 {
                                log::trace!(
                                    "dispatch_key_event: code={} action={} meta={:#x} → unicode=U+{:04X}",
                                    key_code,
                                    action,
                                    meta_state,
                                    unicode_char
                                );
                            }

                            let key_event = crate::android::AndroidKeyEvent {
                                key_code: key_code as i32,
                                action,
                                meta_state: meta_state as i32,
                                unicode_char,
                            };

                            win.handle_key_event(key_event);
                            android_activity::InputStatus::Handled
                        }
                        _ => android_activity::InputStatus::Unhandled,
                    }
                });

                if !read_input {
                    break;
                }
            }
        }
        Err(err) => {
            log::error!("Failed to get input events iterator: {err:?}");
        }
    }
}

/// The instant an input event stamped with `event_time_nanos` occurred.
///
/// Android stamps input on the `System.nanoTime()` timeline, which reads
/// `CLOCK_MONOTONIC` like `Instant`, so a stamp converts by its age.
#[cfg(feature = "android-native-activity")]
fn input_event_instant(event_time_nanos: i64) -> Option<std::time::Instant> {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid, writable timespec for the duration of the call.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) } != 0 {
        return None;
    }
    let now_nanos = i128::from(now.tv_sec) * 1_000_000_000 + i128::from(now.tv_nsec);
    let age_nanos = u64::try_from(now_nanos - i128::from(event_time_nanos)).unwrap_or(0);
    std::time::Instant::now().checked_sub(Duration::from_nanos(age_nanos))
}

/// Queue the next display-synchronised frame, unless one is already queued for
/// this window: every posted callback re-posts itself while the window is
/// active, so a second one would start a second chain and run the
/// request-frame callback twice per vsync. The raw `Arc` is consumed by the
/// callback so the choreographer never observes a borrowed window.
#[cfg(feature = "android-native-activity")]
fn request_choreographer_frame(window: Arc<AndroidWindow>) {
    if !window.begin_frame_callback() {
        return;
    }
    let data = Arc::into_raw(window).cast_mut().cast::<c_void>();
    // SAFETY: this runs on Android's native activity thread, which owns its
    // ALooper. The callback reconstructs and consumes exactly this Arc pointer.
    unsafe {
        let choreographer = ndk_sys::AChoreographer_getInstance();
        if choreographer.is_null() {
            let window = Arc::from_raw(data.cast::<AndroidWindow>());
            window.end_frame_callback();
            return;
        }
        ndk_sys::AChoreographer_postFrameCallback(
            choreographer,
            Some(on_choreographer_frame),
            data,
        );
    }
}

#[cfg(feature = "android-native-activity")]
unsafe extern "C" fn on_choreographer_frame(_frame_time_nanos: libc::c_long, data: *mut c_void) {
    // SAFETY: `request_choreographer_frame` created this pointer with
    // `Arc::into_raw`, and this callback is invoked once by AChoreographer.
    let window = unsafe { Arc::from_raw(data.cast::<AndroidWindow>()) };
    window.end_frame_callback();
    window.request_frame();
    if window.is_active() {
        request_choreographer_frame(window);
    }
}

/// Waits on the main looper for up to `timeout` (`None` waits indefinitely),
/// then hands a ready `android-activity` event to its handler.
///
/// The main looper also runs callbacks: AChoreographer frames and the
/// dispatcher's main-thread tasks. android-activity logs every callback that
/// runs inside its own poll as a spurious `ALOOPER_POLL_CALLBACK` error, once
/// per frame. Running them in this poll instead means android-activity polls
/// only when one of its own events is ready.
#[cfg(feature = "android-native-activity")]
fn poll_main_looper(app: &AndroidApp, timeout: Option<Duration>) {
    let timeout_millis = timeout.map_or(-1, |timeout| {
        i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX)
    });
    // SAFETY: this runs on the native activity thread, which owns the main
    // looper; the out-parameters are optional and may be null.
    let ident = unsafe {
        ndk_sys::ALooper_pollOnce(
            timeout_millis,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ident == ndk_sys::ALOOPER_POLL_ERROR {
        panic!("ALooper_pollOnce returned POLL_ERROR");
    }
    // Callbacks carry no identifier, so a non-negative one is android-activity's
    // own event. Its source is still ready, so android-activity's poll sees it.
    if ident >= 0 {
        app.poll_events(Some(Duration::ZERO), |event| {
            if let PollEvent::Main(main_event) = event {
                handle_main_event(app, main_event);
            }
        });
    }
}

// ── main event loop ───────────────────────────────────────────────────────────

/// The event loop that processes `android-activity` events and drives the
/// platform.
///
/// Called from `android_main` after the platform is initialised.
/// Runs until the platform requests quit or the activity is destroyed.
#[cfg(feature = "android-native-activity")]
pub fn run_event_loop(app: &AndroidApp) {
    log::info!("run_event_loop: entering main loop");

    // Track whether the on_init_window callback has already been invoked.
    // We do NOT invoke it inside handle_main_event (which runs inside
    // poll_events) because the callback can be heavy (shader compilation,
    // GPUI Application setup).  Running it there blocks the event loop and
    // prevents the system's FocusEvent from being consumed, triggering an
    // ANR after 10 seconds.
    //
    // Instead we check each loop iteration: if a primary window exists and
    // the callback is still pending, invoke it *after* poll_events has
    // returned so focus/input events have already been drained.
    INIT_WINDOW_DONE.store(false, Ordering::Relaxed);
    let mut iteration: u64 = 0;
    let mut last_heartbeat = std::time::Instant::now();
    let mut app_is_active = false;

    loop {
        iteration += 1;

        // Log a heartbeat every 5 seconds so we can tell if the loop is alive.
        let now = std::time::Instant::now();
        if now.duration_since(last_heartbeat) >= Duration::from_secs(5) {
            log::info!(
                "run_event_loop: heartbeat — iteration={}, init_done={}, active={}",
                iteration,
                INIT_WINDOW_DONE.load(Ordering::Relaxed),
                app_is_active,
            );
            last_heartbeat = now;
        }

        // Check if quit was requested.
        if let Some(platform) = PLATFORM.get() {
            if platform.should_quit() {
                log::info!("run_event_loop: platform requested quit");
                dispose_all_platform_views();
                break;
            }
            platform.tick();
        }

        // Block until the Android looper has real work. Rendering is driven by
        // AChoreographer callbacks, which run inside this poll, not by a
        // polling/sleep loop.
        poll_main_looper(app, None);

        // ── Deferred lifecycle processing ──
        //
        // Between each handler, poll again to drain any events the Java UI
        // thread may have queued while we were processing.  This keeps the
        // condvar wait short and prevents the InputDispatcher ANR (10s
        // timeout on MotionEvent delivery).
        //
        // Helper closure to drain events quickly:
        let drain_events = |app: &AndroidApp| poll_main_looper(app, Some(Duration::ZERO));

        // 1. TerminateWindow — unconfigure surface, release native window
        if TERM_WINDOW_PENDING.swap(false, Ordering::Relaxed) {
            log::info!("deferred: TerminateWindow (iter={})", iteration);
            INIT_WINDOW_DONE.store(false, Ordering::Relaxed);
            *LAST_CHROME_STYLE.lock().unwrap() = None;
            if let Some(platform) = PLATFORM.get() {
                if let Some(win) = platform.primary_window() {
                    win.term_window();
                }
            }
            // Drain immediately — Java thread may be blocked on InitWindow condvar.
            drain_events(app);
        }

        // 2. InitWindow — replace surface on existing renderer, or create new
        if INIT_WINDOW_PENDING.swap(false, Ordering::Relaxed) {
            log::info!("deferred: InitWindow (iter={})", iteration);
            if let Some(platform) = PLATFORM.get() {
                if let Some(native_window) = app.native_window() {
                    let width = native_window.width();
                    let height = native_window.height();
                    log::info!("InitWindow: {}×{}", width, height);

                    platform.update_primary_display(&native_window, &app.asset_manager());

                    let scale_factor = platform
                        .primary_display()
                        .map(|d| d.scale_factor())
                        .unwrap_or(1.0);

                    if let Some(existing) = platform.primary_window() {
                        let gpu_ctx = platform.gpu_context();
                        match existing.init_window(native_window, gpu_ctx) {
                            Ok(()) => {
                                log::info!("InitWindow: reinitialised existing window");
                            }
                            Err(e) => {
                                log::error!("failed to reinit window surface: {e:#}");
                            }
                        }

                        // Trigger GPUI resize so layout adapts to new dimensions.
                        existing.handle_resize();

                        let cr = app.content_rect();
                        existing.update_safe_area_from_content_rect(
                            cr.left, cr.top, cr.right, cr.bottom,
                        );

                        INIT_WINDOW_DONE.store(true, Ordering::Relaxed);
                    } else {
                        match platform.open_window(native_window, scale_factor, false) {
                            Ok(win) => {
                                log::info!(
                                    "window opened — id={:#x} scale={:.1}",
                                    win.id(),
                                    scale_factor
                                );

                                let cr = app.content_rect();
                                win.update_safe_area_from_content_rect(
                                    cr.left, cr.top, cr.right, cr.bottom,
                                );
                            }
                            Err(e) => {
                                log::error!("failed to open window: {e:#}");
                            }
                        }
                    }
                }
            }
            // Drain — Java thread may have queued ConfigChanged/WindowResized.
            drain_events(app);
        }

        // 3. WindowResized
        if WINDOW_RESIZED_PENDING.swap(false, Ordering::Relaxed) {
            log::debug!("deferred: WindowResized");
            if let Some(platform) = PLATFORM.get() {
                if let Some(win) = platform.primary_window() {
                    win.handle_resize();
                    let cr = app.content_rect();
                    win.update_safe_area_from_content_rect(cr.left, cr.top, cr.right, cr.bottom);
                }
            }
        }

        // 4. ConfigChanged
        if CONFIG_CHANGED_PENDING.swap(false, Ordering::Relaxed) {
            log::debug!("deferred: ConfigChanged");
            if let Some(platform) = PLATFORM.get() {
                platform.notify_keyboard_layout_change();
                let is_dark = query_night_mode_via_jni();
                if let Some(win) = platform.primary_window() {
                    let appearance = if is_dark {
                        crate::android::window::WindowAppearance::Dark
                    } else {
                        crate::android::window::WindowAppearance::Light
                    };
                    win.set_appearance(appearance);
                }
            }
        }

        // 5. Pause / background
        if PAUSE_PENDING.swap(false, Ordering::Relaxed) {
            log::info!("deferred: Pause (iter={})", iteration);
            if let Some(platform) = PLATFORM.get() {
                platform.did_enter_background();
                if let Some(win) = platform.primary_window() {
                    win.set_active(false);
                    // A backgrounded app gives its scroll-layer tiles back.
                    win.release_layer_tiles();
                }
            }
            // Pause platform views
            pause_platform_views();
        }

        // 6. Resume / foreground
        if RESUME_PENDING.swap(false, Ordering::Relaxed) {
            log::info!("deferred: Resume (iter={})", iteration);
            if let Some(platform) = PLATFORM.get() {
                platform.did_become_active();
                if let Some(win) = platform.primary_window() {
                    win.set_active(true);
                    request_choreographer_frame(win);
                }
            }
            // Resume platform views
            resume_platform_views();
        }

        // Track active/focused state.
        if let Some(platform) = PLATFORM.get() {
            if let Some(win) = platform.primary_window() {
                let is_active = win.is_active();
                if is_active != app_is_active {
                    log::info!(
                        "run_event_loop: active {} -> {} (iter={})",
                        app_is_active,
                        is_active,
                        iteration,
                    );
                    app_is_active = is_active;
                }
            }
        }

        // Process input events.
        process_input_events(app);

        // Deferred initialisation callbacks (runs once).
        if !INIT_WINDOW_DONE.load(Ordering::Relaxed) {
            if let Some(platform) = PLATFORM.get() {
                if platform.primary_window().is_some() {
                    if let Some(finish_cb) = platform.take_finish_launching_callback() {
                        log::info!("invoking finish_launching callback (iter={})", iteration);
                        finish_cb();
                    }

                    if let Some(init_cb) = platform.take_on_init_window_callback() {
                        let win = platform.primary_window().unwrap();
                        log::info!("invoking on_init_window callback (iter={})", iteration);
                        init_cb(win);
                    }

                    INIT_WINDOW_DONE.store(true, Ordering::Relaxed);
                    NATIVE_INITIALIZED.store(true, Ordering::Release);
                    log::info!("NATIVE_INITIALIZED = true");

                    // The first frame is paced by the next display callback.
                    platform.flush_main_thread_tasks();
                    if let Some(win) = platform.primary_window() {
                        request_choreographer_frame(win);
                    }
                }
            }
        }
    }

    log::info!("run_event_loop: exiting main loop");
}

/// Pause all platform views when the app goes to background.
#[cfg(feature = "android-native-activity")]
fn pause_platform_views() {
    let _ = with_env(|env| {
        if let Ok(helper_class) = find_app_class(env, "dev.gpux.mobile.PlatformViews") {
            let _ = env.call_static_method(
                &helper_class,
                jni::jni_str!("pauseAll"),
                jni::jni_sig!("()V"),
                &[],
            );
            env.exception_clear();
        }
        Ok(())
    });
}

/// Resume all platform views when the app returns to foreground.
#[cfg(feature = "android-native-activity")]
fn resume_platform_views() {
    let _ = with_env(|env| {
        if let Ok(helper_class) = find_app_class(env, "dev.gpux.mobile.PlatformViews") {
            let _ = env.call_static_method(
                &helper_class,
                jni::jni_str!("resumeAll"),
                jni::jni_sig!("()V"),
                &[],
            );
            env.exception_clear();
        }
        Ok(())
    });
}

/// Dispose all platform views during app shutdown.
#[cfg(feature = "android-native-activity")]
fn dispose_all_platform_views() {
    let _ = with_env(|env| {
        if let Ok(helper_class) = find_app_class(env, "dev.gpux.mobile.PlatformViews") {
            let _ = env.call_static_method(
                &helper_class,
                jni::jni_str!("disposeAll"),
                jni::jni_sig!("()V"),
                &[],
            );
            env.exception_clear();
        }
        Ok(())
    });
}

/// Handle a single `MainEvent` from `android-activity`.
#[cfg(feature = "android-native-activity")]
fn handle_main_event(_app: &AndroidApp, event: MainEvent<'_>) {
    match event {
        MainEvent::InitWindow { .. } => {
            log::info!("MainEvent::InitWindow");
            // Defer to after poll_events to avoid deadlock with state lock.
            INIT_WINDOW_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::TerminateWindow { .. } => {
            log::info!("MainEvent::TerminateWindow");
            // Defer to after poll_events to avoid deadlock with state lock.
            TERM_WINDOW_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::WindowResized { .. } => {
            log::debug!("MainEvent::WindowResized");
            // Defer to after poll_events to avoid deadlock with state lock.
            WINDOW_RESIZED_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::GainedFocus => {
            log::info!("MainEvent::GainedFocus");
            RESUME_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::LostFocus if crate::android::fast::composition::panel_holds_focus() => {
            log::info!("MainEvent::LostFocus (to a composition panel)");
        }

        MainEvent::LostFocus => {
            log::info!("MainEvent::LostFocus");
            PAUSE_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::Resume { .. } => {
            log::info!("MainEvent::Resume");
            RESUME_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::Pause => {
            log::info!("MainEvent::Pause");
            // set_active uses AtomicBool so it never blocks.
            PAUSE_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::ConfigChanged { .. } => {
            log::debug!("MainEvent::ConfigChanged");
            CONFIG_CHANGED_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::Start => {
            log::info!("MainEvent::Start");
        }

        MainEvent::Stop => {
            log::info!("MainEvent::Stop");
        }

        MainEvent::SaveState { .. } => {
            log::info!("MainEvent::SaveState");
        }

        MainEvent::LowMemory => {
            log::warn!("MainEvent::LowMemory — releasing scroll-layer tiles");
            if let Some(win) = PLATFORM
                .get()
                .and_then(|platform| platform.primary_window())
            {
                win.release_layer_tiles();
            }
        }

        MainEvent::Destroy => {
            log::info!("MainEvent::Destroy");

            if let Some(platform) = PLATFORM.get() {
                platform.quit();
            }
        }

        MainEvent::InsetsChanged { .. } => {
            log::debug!("MainEvent::InsetsChanged");
            WINDOW_RESIZED_PENDING.store(true, Ordering::Relaxed);
        }

        MainEvent::ContentRectChanged { .. } => {
            log::debug!("MainEvent::ContentRectChanged");
            WINDOW_RESIZED_PENDING.store(true, Ordering::Relaxed);
        }

        _ => {
            log::trace!("MainEvent: other variant");
        }
    }
}

// ── main loop helper (compat with existing code) ──────────────────────────────

/// Run one iteration of the event loop.
///
/// This is a compatibility wrapper for code that uses a manual poll loop.
/// Prefer `run_event_loop` for the standard event loop.
///
/// `timeout_ms` — how long to block waiting for events (milliseconds).
/// Pass `0` for non-blocking, `-1` to block indefinitely.
///
/// Returns `true` if the application should exit.
#[cfg(feature = "android-native-activity")]
pub fn poll_events(timeout_ms: i32) -> bool {
    if let Some(platform) = PLATFORM.get() {
        if platform.should_quit() {
            return true;
        }
        platform.tick();
    }

    let app = match ANDROID_APP.get() {
        Some(app) => app,
        None => return false,
    };

    let timeout = if timeout_ms < 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms as u64))
    };

    poll_main_looper(app, timeout);

    process_input_events(app);

    // Drive the GPUI rendering pipeline (same as run_event_loop).
    if let Some(platform) = PLATFORM.get() {
        platform.flush_main_thread_tasks();
        if let Some(win) = platform.primary_window() {
            win.request_frame();
        }
    }

    false
}

// ── public init / run helpers ─────────────────────────────────────────────────

/// Install a panic hook that routes panic messages to logcat.
///
/// Call this early in `android_main` so that any subsequent panic is
/// visible via `adb logcat`.  Safe to call multiple times — each call
/// replaces the previous hook.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Box<dyn Any>".to_string()
        };
        if let Some(loc) = info.location() {
            log::error!(
                "PANIC at {}:{}:{}: {}",
                loc.file(),
                loc.line(),
                loc.column(),
                payload
            );
        } else {
            log::error!("PANIC: {}", payload);
        }
    }));
}

/// Store the `AndroidApp` globally and create the `AndroidPlatform`.
///
/// Called by every `android_main` before `run_event_loop`, which may run
/// more than once in a process (see [`Current`]). Returns a reference to the
/// platform so the caller can register callbacks (e.g. `set_on_init_window`)
/// before entering the event loop.
#[cfg(feature = "android-native-activity")]
pub fn init_platform(app: &AndroidApp) -> &'static Arc<AndroidPlatform> {
    if PLATFORM.get().is_some() {
        log::info!("init_platform: the activity started again; replacing the previous platform");
    }
    ANDROID_APP.set(app.clone());
    log::info!("init_platform: stored AndroidApp");

    let platform = PLATFORM.set(Arc::new(AndroidPlatform::new(false)));
    log::info!("init_platform: AndroidPlatform created");
    register_trim_memory();
    platform
}

/// The helper activity whose `onTrimMemory` reaches the platform (`GpuxNativeActivity`).
#[cfg(feature = "android-native-activity")]
const TRIM_MEMORY_ACTIVITY: &str = "dev.gpux.mobile.GpuxNativeActivity";

/// `GpuxNativeActivity.nativeTrimMemory(int)`: the activity's `onTrimMemory`, on the UI thread.
/// The window gives its scroll-layer tiles back at any level; the release never waits.
#[cfg(feature = "android-native-activity")]
extern "system" fn native_trim_memory(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    level: jni::sys::jint,
) {
    log::info!("onTrimMemory({level}) — releasing scroll-layer tiles");
    if let Some(win) = PLATFORM
        .get()
        .and_then(|platform| platform.primary_window())
    {
        win.release_layer_tiles();
    }
}

/// Registers [`native_trim_memory`] when the host's activity is `GpuxNativeActivity`; a host with
/// another activity gets `MainEvent::LowMemory`, pause and surface loss.
#[cfg(feature = "android-native-activity")]
fn register_trim_memory() {
    let registered = with_env(|env| {
        let class = find_app_class(env, TRIM_MEMORY_ACTIVITY)?;
        // SAFETY: the function pointer matches the Java declaration.
        let methods = unsafe {
            [jni::NativeMethod::from_raw_parts(
                jni::jni_str!("nativeTrimMemory"),
                jni::jni_str!("(I)V"),
                native_trim_memory as *mut std::ffi::c_void,
            )]
        };
        // SAFETY: as above.
        unsafe { env.register_native_methods(&class, &methods) }
            .map_err(|error| format!("registering nativeTrimMemory: {error}"))?;
        env.set_static_field(
            &class,
            jni::jni_str!("nativesReady"),
            jni::jni_sig!("Z"),
            jni::JValue::Bool(true),
        )
        .map_err(|error| format!("setting nativesReady: {error}"))?;
        Ok(())
    });
    match registered {
        Ok(()) => log::info!("onTrimMemory reaches the platform through {TRIM_MEMORY_ACTIVITY}"),
        Err(error) => log::debug!("{TRIM_MEMORY_ACTIVITY} is not the activity: {error}"),
    }
}

// ── system chrome (status bar / navigation bar) ───────────────────────────────

/// Cached last-applied system chrome style.
///
/// `set_system_chrome` is called on every frame render.  The JNI calls it
/// makes (getWindow, setStatusBarColor, etc.) are View operations that can
/// contend with the Android UI thread and intermittently deadlock.
/// By caching the last applied style we skip the JNI calls entirely when
/// nothing changed — which is the common case.
#[allow(clippy::type_complexity)]
static LAST_CHROME_STYLE: std::sync::Mutex<
    Option<(Option<u32>, Option<u32>, crate::StatusBarContentStyle)>,
> = std::sync::Mutex::new(None);

/// Apply system chrome styling on Android.
///
/// Sets the status bar color, navigation bar color, and light/dark
/// status bar icons via JNI calls to `Window` and `WindowInsetsController`.
///
/// Must be called from the main (native) thread that has JNI access.
pub fn set_system_chrome(style: &crate::SystemChromeStyle) {
    let status_bar_color = style.status_bar_color;
    let navigation_bar_color = style.navigation_bar_color;
    let status_bar_style = style.status_bar_style;

    // Skip the (expensive, potentially deadlocking) JNI calls when nothing changed.
    {
        let key = (status_bar_color, navigation_bar_color, status_bar_style);
        let mut last = LAST_CHROME_STYLE.lock().unwrap();
        if *last == Some(key) {
            return;
        }
        *last = Some(key);
    }

    let result = with_env(|env| {
        let activity_obj = activity(env)?;

        // 1. Get the Window: activity.getWindow()
        let window = env
            .call_method(
                &activity_obj,
                jni::jni_str!("getWindow"),
                jni::jni_sig!("()Landroid/view/Window;"),
                &[],
            )
            .and_then(|v: jni::objects::JValueOwned| v.l())
            .map_err(|e| {
                env.exception_clear();
                e.to_string()
            })?;
        if window.is_null() {
            return Err("getWindow returned null".into());
        }

        // 2. Set status bar color if provided
        if let Some(color) = status_bar_color {
            let argb = (0xFF000000_u32 | color) as i32;
            let _ = env.call_method(
                &window,
                jni::jni_str!("setStatusBarColor"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(argb)],
            );
            env.exception_clear();
        }

        // 3. Set navigation bar color if provided
        if let Some(color) = navigation_bar_color {
            let argb = (0xFF000000_u32 | color) as i32;
            let _ = env.call_method(
                &window,
                jni::jni_str!("setNavigationBarColor"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(argb)],
            );
            env.exception_clear();
        }

        // 4. Set light/dark status bar icons via WindowInsetsController (API 30+)
        let insetsctl = env.call_method(
            &window,
            jni::jni_str!("getInsetsController"),
            jni::jni_sig!("()Landroid/view/WindowInsetsController;"),
            &[],
        );

        if let Ok(v) = insetsctl {
            if let Ok(ctl) = v.l() {
                if !ctl.is_null() {
                    let mask: i32 = 0x00000008;
                    let appearance: i32 = match status_bar_style {
                        crate::StatusBarContentStyle::Dark => 0x00000008,
                        crate::StatusBarContentStyle::Light => 0,
                    };
                    let _ = env.call_method(
                        &ctl,
                        jni::jni_str!("setSystemBarsAppearance"),
                        jni::jni_sig!("(II)V"),
                        &[JValue::Int(appearance), JValue::Int(mask)],
                    );
                    env.exception_clear();
                }
            }
        } else {
            env.exception_clear();

            if let Ok(decor) = env
                .call_method(
                    &window,
                    jni::jni_str!("getDecorView"),
                    jni::jni_sig!("()Landroid/view/View;"),
                    &[],
                )
                .and_then(|v: jni::objects::JValueOwned| v.l())
            {
                if !decor.is_null() {
                    if let Ok(current) = env
                        .call_method(
                            &decor,
                            jni::jni_str!("getSystemUiVisibility"),
                            jni::jni_sig!("()I"),
                            &[],
                        )
                        .and_then(|v: jni::objects::JValueOwned| v.i())
                    {
                        let new_flags = match status_bar_style {
                            crate::StatusBarContentStyle::Dark => current | 0x00002000,
                            crate::StatusBarContentStyle::Light => current & !0x00002000,
                        };
                        let _ = env.call_method(
                            &decor,
                            jni::jni_str!("setSystemUiVisibility"),
                            jni::jni_sig!("(I)V"),
                            &[JValue::Int(new_flags)],
                        );
                        env.exception_clear();
                    }
                }
            }
        }

        Ok(())
    });

    if let Err(e) = result {
        log::warn!("set_system_chrome: {e}");
    }

    log::info!(
        "set_system_chrome: status_bar_color={:?}, nav_bar_color={:?}, style={:?}",
        style.status_bar_color,
        style.navigation_bar_color,
        style.status_bar_style
    );
}

// ── software keyboard (IME) control ───────────────────────────────────────────

/// Show the software keyboard on Android with a specific keyboard type.
/// Show the software keyboard on Android.
///
/// Uses the NDK `ANativeActivity_showSoftInput` via `android-activity`.
/// The previous EditText/JNI approach silently failed with
/// `CalledFromWrongThreadException` because all JNI View operations
/// must run on the Android UI thread, not the native Rust thread.
/// The NDK function handles the UI-thread dispatch internally.
///
/// Text input arrives via `KeyEvent`s through `process_input_events()`.
pub fn show_keyboard_android(_keyboard_type: crate::KeyboardType) {
    #[cfg(feature = "android-native-activity")]
    if let Some(app) = android_app() {
        log::info!("show_keyboard_android: using NDK show_soft_input");
        app.show_soft_input(false);
    }
}

/// Hide the software keyboard on Android.
///
/// Uses the NDK `ANativeActivity_hideSoftInput` via `android-activity`.
pub fn hide_keyboard_android() {
    #[cfg(feature = "android-native-activity")]
    if let Some(app) = android_app() {
        log::info!("hide_keyboard_android: using NDK hide_soft_input");
        app.hide_soft_input(false);
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

// ── JNI export for RayXActivity splash screen ────────────────────────────────

/// Called from `RayXActivity.nativeIsInitialized()` to check whether the
/// native library has finished initializing (first frame rendered).
///
/// The AndroidX SplashScreen API calls this via `setKeepOnScreenCondition`
/// to hold the splash visible until GPUI is ready.
///
/// # Safety
/// Must only be called from the JVM on a valid JNI thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_dev_rayx_mobile_RayXActivity_nativeIsInitialized(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> u8 {
    if NATIVE_INITIALIZED.load(Ordering::Acquire) {
        1 // JNI_TRUE
    } else {
        0 // JNI_FALSE
    }
}

/// JNI bridge: receive a deeplink URL from `RayXActivity.onNewIntent()`.
///
/// When the app is already running and a deeplink is opened (e.g. via
/// `adb shell am start -d <scheme>://video_player`), the Java side calls
/// this to notify the Rust deeplink handler.
///
/// # Safety
/// Must only be called from the JVM on a valid JNI thread with a valid `url` jobject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_dev_rayx_mobile_RayXActivity_nativeOnDeepLink(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    url: *mut std::ffi::c_void,
) {
    // We already have a JVM attached on this thread (UI thread).
    // Use with_env to get a properly wrapped Env handle.
    let url_raw = url as jni::sys::jobject;
    let _ = with_env(|env| {
        let url_obj = unsafe { JObject::from_raw(env, url_raw) };
        let url_string = get_string(env, &url_obj);
        // Don't let the JObject be dropped (it's owned by the JNI call frame).
        if url_string.is_empty() {
            return Ok(());
        }
        log::info!("nativeOnDeepLink: {}", url_string);
        Ok(())
    });
}

#[cfg(all(test, feature = "android-native-activity"))]
mod tests {
    use super::*;

    #[test]
    fn poll_events_returns_false_when_no_platform() {
        // PLATFORM is not set in a unit-test context, so poll_events should
        // be a safe no-op and return false (don't quit).
        let result = poll_events(0);
        let _ = result;
    }

    #[test]
    fn java_vm_returns_null_before_init() {
        // Before android_main is called, java_vm() should return null.
        let vm = java_vm();
        assert!(vm.is_null());
    }

    #[test]
    fn activity_as_ptr_returns_null_before_init() {
        let ptr = activity_as_ptr();
        assert!(ptr.is_null());
    }

    #[test]
    fn android_app_returns_none_before_init() {
        assert!(android_app().is_none());
    }

    #[test]
    fn platform_returns_none_before_init() {
        assert!(platform().is_none());
    }
}
