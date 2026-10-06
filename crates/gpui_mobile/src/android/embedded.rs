//! Embedded Android surface host for managed Activity integrations.
//!
//! This path is used when Android owns a normal managed Activity and hands a
//! `SurfaceView` surface to GPUI. It deliberately avoids `android-activity`
//! NativeActivity glue and drives GPUI from RayX-provided lifecycle events.

#![allow(unsafe_code)]

use std::{
    collections::VecDeque,
    ffi::c_void,
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use gpui::Platform;
use ndk::native_window::NativeWindow;

use super::{
    AndroidPlatform, SharedPlatform, TouchPoint,
    jni::{clear_embedded_context, set_embedded_context},
};

const FRAME_INTERVAL: Duration = Duration::from_micros(16_666);
const SURFACE_STABILITY_DELAY: Duration = Duration::from_millis(50);

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static SURFACE_ALIVE: AtomicBool = AtomicBool::new(false);
static SURFACE_GENERATION: AtomicU64 = AtomicU64::new(0);
static PLATFORM: OnceLock<Arc<AndroidPlatform>> = OnceLock::new();
static EVENTS: OnceLock<EventQueue> = OnceLock::new();

struct EventQueue {
    events: Mutex<VecDeque<EmbeddedEvent>>,
    ready: Condvar,
}

impl EventQueue {
    fn global() -> &'static Self {
        EVENTS.get_or_init(|| Self {
            events: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        })
    }

    fn push(&self, event: EmbeddedEvent) {
        let mut events = self
            .events
            .lock()
            .expect("embedded Android event queue poisoned");
        events.push_back(event);
        self.ready.notify_one();
    }

    fn drain(&self) -> Vec<EmbeddedEvent> {
        let mut events = self
            .events
            .lock()
            .expect("embedded Android event queue poisoned");
        events.drain(..).collect()
    }

    fn wait_timeout(&self, timeout: Duration) {
        if timeout.is_zero() {
            return;
        }

        let events = self
            .events
            .lock()
            .expect("embedded Android event queue poisoned");
        if events.is_empty() {
            let _ = self.ready.wait_timeout(events, timeout);
        }
    }
}

enum EmbeddedEvent {
    SurfaceAvailable {
        native_window: NativeWindow,
        scale_factor: f32,
        generation: u64,
    },
    SurfaceDestroyed,
    Active(bool),
    Touch(TouchPoint),
    Quit,
}

/// Install the managed Activity JNI context used by embedded Android hosts.
pub fn initialize(java_vm: *mut c_void, activity: *mut c_void) -> bool {
    super::init_logger();
    if !set_embedded_context(java_vm, activity) {
        log::error!("embedded Android initialize failed: missing JavaVM or Activity");
        return false;
    }

    INITIALIZED.store(true, Ordering::Release);
    log::info!("embedded Android runtime initialized");
    true
}

/// Clear the embedded Activity handles.
pub fn shutdown() {
    INITIALIZED.store(false, Ordering::Release);
    clear_embedded_context();
    EventQueue::global().push(EmbeddedEvent::Quit);
}

pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Ensure the current native thread has an ALooper before GPUI constructs the dispatcher.
pub fn prepare_current_thread() {
    unsafe {
        if ndk_sys::ALooper_forThread().is_null() {
            let looper = ndk_sys::ALooper_prepare(0);
            if looper.is_null() {
                log::error!("embedded Android failed to prepare an ALooper for the GPUI thread");
            } else {
                log::info!("embedded Android prepared a GPUI-thread ALooper");
            }
        }
    }
}

pub fn current_platform(headless: bool) -> Rc<dyn Platform> {
    if headless {
        return Rc::new(AndroidPlatform::new(true));
    }

    let platform = PLATFORM.get_or_init(|| Arc::new(AndroidPlatform::new(false)));
    SharedPlatform::new(Arc::clone(platform)).into_rc()
}

pub fn surface_created(env: *mut c_void, surface: *mut c_void, scale_factor: f32) -> bool {
    if env.is_null() || surface.is_null() {
        log::error!("embedded Android surface_created failed: missing JNI env or Surface");
        return false;
    }

    let Some(native_window) =
        (unsafe { NativeWindow::from_surface(env.cast::<jni_sys_03::JNIEnv>(), surface.cast()) })
    else {
        log::error!(
            "embedded Android surface_created failed: ANativeWindow_fromSurface returned null"
        );
        return false;
    };

    SURFACE_ALIVE.store(true, Ordering::Release);
    let generation = SURFACE_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    EventQueue::global().push(EmbeddedEvent::SurfaceAvailable {
        native_window,
        scale_factor: scale_factor.max(1.0),
        generation,
    });
    true
}

pub fn surface_destroyed() {
    SURFACE_ALIVE.store(false, Ordering::Release);
    SURFACE_GENERATION.fetch_add(1, Ordering::AcqRel);
    EventQueue::global().push(EmbeddedEvent::SurfaceDestroyed);
}

pub fn set_active(active: bool) {
    EventQueue::global().push(EmbeddedEvent::Active(active));
}

pub fn touch(pointer_id: i32, x: f32, y: f32, action: u32) {
    EventQueue::global().push(EmbeddedEvent::Touch(TouchPoint {
        id: pointer_id,
        x,
        y,
        action,
        // Stamped on arrival: the event loop drains the queue a frame later.
        timestamp: Some(Instant::now()),
    }));
}

pub fn run_event_loop() {
    let Some(platform) = PLATFORM.get() else {
        log::error!("embedded Android run_event_loop entered before platform initialization");
        return;
    };

    log::info!("embedded Android GPUI event loop entered");
    let mut pending_surface: Option<PendingSurface> = None;
    while !platform.should_quit() {
        let iteration_started = Instant::now();
        let queue = EventQueue::global();
        for event in queue.drain() {
            process_event(platform, event, &mut pending_surface);
        }

        if let Some(surface) = pending_surface.take() {
            if surface.is_current() && platform.is_active() {
                if surface.available_at.elapsed() >= SURFACE_STABILITY_DELAY {
                    activate_surface(platform, surface);
                } else {
                    pending_surface = Some(surface);
                }
            } else {
                log::info!(
                    "embedded Android dropped stale or inactive pending surface generation={}",
                    surface.generation
                );
            }
        }

        platform.tick();
        platform.flush_main_thread_tasks();

        let primary_window = platform.primary_window();
        let should_request_frame = platform.is_active()
            && primary_window
                .as_ref()
                .is_some_and(|window| window.is_active() && window.has_surface());

        if let Some(window) = primary_window.filter(|_| should_request_frame) {
            window.request_frame();
        }

        let wait_timeout = FRAME_INTERVAL.saturating_sub(iteration_started.elapsed());
        queue.wait_timeout(wait_timeout);
    }

    log::info!("embedded Android GPUI event loop exited");
}

struct PendingSurface {
    native_window: NativeWindow,
    scale_factor: f32,
    generation: u64,
    available_at: Instant,
}

impl PendingSurface {
    fn is_current(&self) -> bool {
        SURFACE_ALIVE.load(Ordering::Acquire)
            && SURFACE_GENERATION.load(Ordering::Acquire) == self.generation
    }
}

fn process_event(
    platform: &Arc<AndroidPlatform>,
    event: EmbeddedEvent,
    pending_surface: &mut Option<PendingSurface>,
) {
    match event {
        EmbeddedEvent::SurfaceAvailable {
            native_window,
            scale_factor,
            generation,
        } => {
            if !SURFACE_ALIVE.load(Ordering::Acquire)
                || SURFACE_GENERATION.load(Ordering::Acquire) != generation
            {
                log::info!(
                    "embedded Android ignored stale surface generation={}",
                    generation
                );
                return;
            }

            *pending_surface = Some(PendingSurface {
                native_window,
                scale_factor,
                generation,
                available_at: Instant::now(),
            });
        }
        EmbeddedEvent::SurfaceDestroyed => {
            *pending_surface = None;
            if let Some(window) = platform.primary_window() {
                window.term_window();
                window.set_active(false);
            }
            platform.did_enter_background();
        }
        EmbeddedEvent::Active(active) => {
            if active {
                platform.did_become_active();
            } else {
                *pending_surface = None;
                platform.did_enter_background();
            }
            if let Some(window) = platform.primary_window() {
                window.set_active(active);
            }
        }
        EmbeddedEvent::Touch(point) => {
            if let Some(window) = platform.primary_window() {
                window.handle_touch(point);
            }
        }
        EmbeddedEvent::Quit => platform.quit(),
    }
}

fn activate_surface(platform: &Arc<AndroidPlatform>, surface: PendingSurface) {
    if !surface.is_current() || !platform.is_active() {
        log::info!(
            "embedded Android skipped stale or inactive surface activation generation={}",
            surface.generation
        );
        return;
    }

    platform.update_primary_display_with_scale(&surface.native_window, surface.scale_factor);
    if let Some(window) = platform.primary_window() {
        if let Err(error) = window.init_window(surface.native_window, platform.gpu_context()) {
            log::error!("embedded Android failed to reinitialize GPUI surface: {error:#}");
            return;
        }
        window.set_active(true);
    } else {
        match platform.open_window(surface.native_window, surface.scale_factor, false) {
            Ok(window) => {
                log::info!("embedded Android opened primary GPUI window");
                if let Some(callback) = platform.take_on_init_window_callback() {
                    callback(window);
                }
                if let Some(callback) = platform.take_finish_launching_callback() {
                    callback();
                }
            }
            Err(error) => {
                log::error!("embedded Android failed to open primary GPUI window: {error:#}");
                return;
            }
        }
    }
    platform.did_become_active();
}
