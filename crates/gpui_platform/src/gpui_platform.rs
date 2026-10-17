//! Convenience crate that re-exports gpux's platform traits and the
//! `current_platform` constructor so consumers don't need `#[cfg]` gating.

pub use gpux::Platform;

use std::rc::Rc;

#[cfg(target_family = "wasm")]
unsafe extern "C" {
    fn __wasm_call_ctors();
}

/// Returns a background executor for the current platform.
pub fn background_executor() -> gpux::BackgroundExecutor {
    current_platform(true).background_executor()
}

pub fn application() -> gpux::Application {
    #[cfg(target_family = "wasm")]
    {
        application_with_web_backend(gpui_web::WebBackendPreference::Auto)
    }

    #[cfg(not(target_family = "wasm"))]
    gpux::Application::with_platform(current_platform(false))
}

pub fn headless() -> gpux::Application {
    gpux::Application::with_platform(current_platform(true))
}

/// Returns a Linux app that may switch among `allowed_modes`.
///
/// It starts windowed in the process's own environment, or headless if that names no allowed
/// display server. Set another initial mode with [`gpux::Application::with_windowing`], and
/// switch later with [`gpux::App::request_windowing`].
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn linux(allowed_modes: gpux::WindowingModes) -> gpux::Application {
    gpux::Application::with_platform(gpui_linux::linux_platform(allowed_modes))
}

/// Unlike `application`, this function returns a single-threaded web application.
#[cfg(target_family = "wasm")]
pub fn single_threaded_web() -> gpux::Application {
    let platform = Rc::new(gpui_web::WebPlatform::new(false));
    let http_client = std::sync::Arc::new(platform.fetch_http_client());
    gpux::Application::with_platform(platform).with_http_client(http_client)
}

/// Returns a threaded web application with an optional background-worker maximum.
#[cfg(target_family = "wasm")]
pub fn web_application(max_background_workers: Option<usize>) -> gpux::Application {
    web_application_with_backend(max_background_workers, WebBackendPreference::Auto)
}

#[cfg(target_family = "wasm")]
pub fn web_application_with_backend(
    max_background_workers: Option<usize>,
    backend_preference: WebBackendPreference,
) -> gpux::Application {
    let platform = Rc::new(gpui_web::WebPlatform::new_with_backend_and_worker_limit(
        true,
        max_background_workers,
        backend_preference,
    ));
    let http_client = std::sync::Arc::new(platform.fetch_http_client());
    gpux::Application::with_platform(platform).with_http_client(http_client)
}

#[cfg(target_family = "wasm")]
pub use gpui_web::WebBackendPreference;

/// How web windows offer their screen-reader mirror, and the label of its enable button.
#[cfg(all(target_family = "wasm", feature = "accessibility"))]
pub use gpui_web::{
    DEFAULT_SCREEN_READER_LABEL, WebScreenReader, set_web_screen_reader,
    set_web_screen_reader_label,
};

#[cfg(target_family = "wasm")]
pub fn application_with_web_backend(backend_preference: WebBackendPreference) -> gpux::Application {
    let platform = Rc::new(gpui_web::WebPlatform::new_with_backend(
        true,
        backend_preference,
    ));
    let http_client = std::sync::Arc::new(platform.fetch_http_client());
    gpux::Application::with_platform(platform).with_http_client(http_client)
}

/// Initializes panic hooks and logging for the web platform.
/// Call this before running the application in a wasm_bindgen entrypoint.
#[cfg(target_family = "wasm")]
pub fn web_init() {
    // SAFETY: wasm-ld synthesizes this function from the module's constructor
    // list. Browser applications call web_init once before using GPUI.
    unsafe { __wasm_call_ctors() };
    console_error_panic_hook::set_once();
    gpui_web::init_logging();
}

/// Returns the default [`Platform`] for the current OS.
pub fn current_platform(headless: bool) -> Rc<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Rc::new(gpui_macos::MacPlatform::new(headless))
    }

    #[cfg(target_os = "windows")]
    {
        Rc::new(
            gpui_windows::WindowsPlatform::new(headless)
                .expect("failed to initialize Windows platform"),
        )
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        gpui_linux::current_platform(headless)
    }

    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        gpui_mobile::current_platform(headless)
    }

    #[cfg(target_family = "wasm")]
    {
        let _ = headless;
        Rc::new(gpui_web::WebPlatform::new(true))
    }
}

/// Returns the current platform's text system for benchmark contexts, built once per
/// thread and shared by every context on it.
///
/// `#[gpux::bench]` creates a context for every Criterion routine call. Building a whole
/// platform each time only to take its text system leaks the platform's threads on Linux
/// (a 20-worker dispatcher and a timer thread per call), which slows every later
/// benchmark in the process.
///
/// Benchmarks run on a worker thread with a larger stack ([`gpux::run_with_bench_stack`]),
/// where macOS cannot create its platform, so there the text system is built on its own.
#[cfg(feature = "bench-support")]
pub fn bench_text_system() -> std::sync::Arc<dyn gpux::PlatformTextSystem> {
    thread_local! {
        static TEXT_SYSTEM: std::sync::Arc<dyn gpux::PlatformTextSystem> = {
            #[cfg(all(target_os = "macos", feature = "font-kit"))]
            {
                std::sync::Arc::new(gpui_macos::MacTextSystem::new())
            }
            #[cfg(all(target_os = "macos", not(feature = "font-kit")))]
            {
                std::sync::Arc::new(gpux::NoopTextSystem::new())
            }
            #[cfg(not(target_os = "macos"))]
            {
                current_platform(true).text_system()
            }
        };
    }
    TEXT_SYSTEM.with(|text_system| text_system.clone())
}

/// Returns a new deterministic text system for tests: Cosmic Text loaded with only the fonts
/// `gpux_fonts` bundles, falling back to IBM Plex Sans.
///
/// It never consults system fonts, so text measured with it is identical on every desktop OS.
/// `#[gpux::test(text = "bundled")]` builds its contexts with it; pass it to
/// [`gpux::TestAppContext::build_with_text_system`] to build one by hand.
#[cfg(all(
    feature = "test-support",
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "windows",
        target_os = "macos"
    )
))]
pub fn bundled_text_system() -> std::sync::Arc<dyn gpux::PlatformTextSystem> {
    let text_system = gpui_wgpu::CosmicTextSystem::new_without_system_fonts("IBM Plex Sans");
    gpux::PlatformTextSystem::add_fonts(
        &text_system,
        gpux_fonts::ALL
            .iter()
            .map(|font| std::borrow::Cow::Borrowed(*font))
            .collect(),
    )
    .expect("the bundled gpux fonts are valid font files");
    std::sync::Arc::new(text_system)
}

/// Returns a new [`HeadlessRenderer`] for the current platform, if available.
#[cfg(any(feature = "bench-support", feature = "test-support"))]
pub fn current_headless_renderer() -> anyhow::Result<Option<Box<dyn gpux::PlatformHeadlessRenderer>>>
{
    #[cfg(target_os = "macos")]
    {
        Ok(Some(Box::new(
            gpui_macos::metal_renderer::MetalHeadlessRenderer::new(),
        )))
    }

    // Linux and Windows render through wgpu (Vulkan|GL, or DX12 with WARP when there is no
    // GPU). `GPUX_WGPU_ADAPTER` picks the adapter: `software`, `hardware` or a name substring.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        gpui_wgpu::WgpuHeadlessRenderer::new()
            .map(|renderer| Some(Box::new(renderer) as Box<dyn gpux::PlatformHeadlessRenderer>))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Ok(None)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use gpux::{AppContext, Empty, VisualTestAppContext};
    use std::cell::RefCell;
    use std::time::Duration;

    // Note: All VisualTestAppContext tests are ignored by default because they require
    // the macOS main thread. Standard Rust tests run on worker threads, which causes
    // SIGABRT when interacting with macOS AppKit/Cocoa APIs.
    //
    // To run these tests, use:
    // cargo test -p gpux visual_test_context -- --ignored --test-threads=1

    #[test]
    #[ignore] // Requires macOS main thread
    fn test_foreground_tasks_run_with_run_until_parked() {
        let mut cx = VisualTestAppContext::new(current_platform(false));

        let task_ran = Rc::new(RefCell::new(false));

        // Spawn a foreground task via the App's spawn method
        // This should use our TestDispatcher, not the MacDispatcher
        {
            let task_ran = task_ran.clone();
            cx.update(|cx| {
                cx.spawn(async move |_| {
                    *task_ran.borrow_mut() = true;
                })
                .detach();
            });
        }

        // The task should not have run yet
        assert!(!*task_ran.borrow());

        // Run until parked should execute the foreground task
        cx.run_until_parked();

        // Now the task should have run
        assert!(*task_ran.borrow());
    }

    #[test]
    #[ignore] // Requires macOS main thread
    fn test_advance_clock_triggers_delayed_tasks() {
        let mut cx = VisualTestAppContext::new(current_platform(false));

        let task_ran = Rc::new(RefCell::new(false));

        // Spawn a task that waits for a timer
        {
            let task_ran = task_ran.clone();
            let executor = cx.background_executor.clone();
            cx.update(|cx| {
                cx.spawn(async move |_| {
                    executor.timer(Duration::from_millis(500)).await;
                    *task_ran.borrow_mut() = true;
                })
                .detach();
            });
        }

        // Run until parked - the task should be waiting on the timer
        cx.run_until_parked();
        assert!(!*task_ran.borrow());

        // Advance clock past the timer duration
        cx.advance_clock(Duration::from_millis(600));

        // Now the task should have completed
        assert!(*task_ran.borrow());
    }

    #[test]
    #[ignore] // Requires macOS main thread - window creation fails on test threads
    fn test_window_spawn_uses_test_dispatcher() {
        let mut cx = VisualTestAppContext::new(current_platform(false));

        let task_ran = Rc::new(RefCell::new(false));

        let window = cx
            .open_offscreen_window_default(|_, cx| cx.new(|_| Empty))
            .expect("Failed to open window");

        // Spawn a task via window.spawn - this is the critical test case
        // for tooltip behavior, as tooltips use window.spawn for delayed show
        {
            let task_ran = task_ran.clone();
            cx.update_window(window.into(), |_, window, cx| {
                window
                    .spawn(cx, async move |_| {
                        *task_ran.borrow_mut() = true;
                    })
                    .detach();
            })
            .ok();
        }

        // The task should not have run yet
        assert!(!*task_ran.borrow());

        // Run until parked should execute the foreground task spawned via window
        cx.run_until_parked();

        // Now the task should have run
        assert!(*task_ran.borrow());
    }
}
