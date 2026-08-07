use gpux::{
    PlatformDispatcher, Priority, PriorityQueueReceiver, PriorityQueueSender, RunnableVariant,
};
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::time::Duration;
use wasm_bindgen::prelude::*;
use web_time::Instant;

#[cfg(feature = "multithreaded")]
const MIN_BACKGROUND_THREADS: usize = 2;

#[cfg(feature = "multithreaded")]
#[derive(Clone, Copy)]
struct SharedMemorySupport {
    has_shared_array_buffer: bool,
    has_atomics: bool,
    is_shared_buffer: bool,
}

#[cfg(feature = "multithreaded")]
impl SharedMemorySupport {
    fn supports_threads(self) -> bool {
        self.has_shared_array_buffer && self.has_atomics && self.is_shared_buffer
    }

    fn browser_primitives_available(self) -> bool {
        self.has_shared_array_buffer && self.has_atomics
    }

    fn dispatcher_fallback_message(self) -> &'static str {
        if !self.has_shared_array_buffer {
            "SharedArrayBuffer is not available; falling back to single-threaded dispatcher"
        } else if !self.has_atomics {
            "Atomics is not available; falling back to single-threaded dispatcher"
        } else {
            "SharedArrayBuffer is available, but wasm module memory is not shared; using single-threaded dispatcher"
        }
    }

    fn waker_disabled_message(self) -> &'static str {
        if !self.has_shared_array_buffer {
            "SharedArrayBuffer is not available; main thread mailbox waker loop disabled"
        } else if !self.has_atomics {
            "Atomics is not available; main thread mailbox waker loop disabled"
        } else {
            "SharedArrayBuffer is available, but wasm module memory is not shared; main thread mailbox waker loop disabled"
        }
    }

    fn log_dispatcher_fallback(self) {
        if self.browser_primitives_available() {
            log::info!("{}", self.dispatcher_fallback_message());
        } else {
            log::warn!("{}", self.dispatcher_fallback_message());
        }
    }

    fn log_waker_disabled(self) {
        if self.browser_primitives_available() {
            log::info!("{}", self.waker_disabled_message());
        } else {
            log::warn!("{}", self.waker_disabled_message());
        }
    }
}

#[cfg(feature = "multithreaded")]
fn shared_memory_support() -> SharedMemorySupport {
    let global = js_sys::global();
    let has_shared_array_buffer =
        js_sys::Reflect::has(&global, &JsValue::from_str("SharedArrayBuffer")).unwrap_or(false);
    let has_atomics = js_sys::Reflect::has(&global, &JsValue::from_str("Atomics")).unwrap_or(false);
    let memory = js_sys::WebAssembly::Memory::from(wasm_bindgen::memory());
    let buffer = memory.buffer();
    let is_shared_buffer = buffer.is_instance_of::<js_sys::SharedArrayBuffer>();
    SharedMemorySupport {
        has_shared_array_buffer,
        has_atomics,
        is_shared_buffer,
    }
}

enum MainThreadItem {
    Runnable(RunnableVariant),
    Delayed {
        runnable: RunnableVariant,
        millis: i32,
    },
    Function(Box<dyn FnOnce() + Send>),
    // TODO-Wasm: Shouldn't these run on their own dedicated thread?
    RealtimeFunction(Box<dyn FnOnce() + Send>),
}

struct MainThreadMailbox {
    sender: PriorityQueueSender<MainThreadItem>,
    receiver: parking_lot::Mutex<PriorityQueueReceiver<MainThreadItem>>,
    signal: AtomicI32,
}

impl MainThreadMailbox {
    fn new() -> Self {
        let (sender, receiver) = PriorityQueueReceiver::new();
        Self {
            sender,
            receiver: parking_lot::Mutex::new(receiver),
            signal: AtomicI32::new(0),
        }
    }

    fn post(&self, priority: Priority, item: MainThreadItem) {
        if self.sender.spin_send(priority, item).is_err() {
            log::error!("MainThreadMailbox::send failed: receiver disconnected");
        }

        // TODO-Wasm: Verify this lock-free protocol
        let view = self.signal_view();
        js_sys::Atomics::store(&view, 0, 1).ok();
        js_sys::Atomics::notify(&view, 0).ok();
    }

    fn drain(&self, window: &web_sys::Window) {
        let mut receiver = self.receiver.lock();
        loop {
            // We need these `spin` variants because we can't acquire a lock on the main thread.
            // TODO-WASM: Should we do something different?
            match receiver.spin_try_pop() {
                Ok(Some(item)) => execute_on_main_thread(window, item),
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    fn signal_view(&self) -> js_sys::Int32Array {
        let byte_offset = self.signal.as_ptr() as u32;
        let memory = js_sys::WebAssembly::Memory::from(wasm_bindgen::memory());
        js_sys::Int32Array::new_with_byte_offset_and_length(&memory.buffer(), byte_offset, 1)
    }

    #[cfg(feature = "multithreaded")]
    fn run_waker_loop(self: &Arc<Self>, window: web_sys::Window) {
        let support = shared_memory_support();
        if !support.supports_threads() {
            support.log_waker_disabled();
            return;
        }

        let mailbox = Arc::clone(self);
        wasm_bindgen_futures::spawn_local(async move {
            let view = mailbox.signal_view();
            loop {
                js_sys::Atomics::store(&view, 0, 0).expect("Atomics.store failed");

                let result = match js_sys::Atomics::wait_async(&view, 0, 0) {
                    Ok(result) => result,
                    Err(error) => {
                        log::error!("Atomics.waitAsync failed: {error:?}");
                        break;
                    }
                };

                let is_async = js_sys::Reflect::get(&result, &JsValue::from_str("async"))
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if !is_async {
                    // The signal may change after the store and before waitAsync observes it.
                    // In that expected "not-equal" case, drain the queued work and re-arm.
                    mailbox.drain(&window);
                    continue;
                }

                let promise: js_sys::Promise =
                    js_sys::Reflect::get(&result, &JsValue::from_str("value"))
                        .expect("waitAsync result missing 'value'")
                        .unchecked_into();

                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

                mailbox.drain(&window);
            }
        });
    }
}

pub struct WebDispatcher {
    main_thread_id: std::thread::ThreadId,
    browser_window: web_sys::Window,
    background_sender: PriorityQueueSender<RunnableVariant>,
    main_thread_mailbox: Arc<MainThreadMailbox>,
    supports_threads: bool,
    #[cfg(feature = "multithreaded")]
    _background_threads: Vec<wasm_thread::JoinHandle<()>>,
}

// Safety: `web_sys::Window` is only accessed from the main thread
// All other fields are `Send + Sync` by construction.
unsafe impl Send for WebDispatcher {}
unsafe impl Sync for WebDispatcher {}

impl WebDispatcher {
    pub fn new(browser_window: web_sys::Window, allow_threads: bool) -> Self {
        Self::with_background_worker_limit(browser_window, allow_threads, None)
    }

    pub fn with_background_worker_limit(
        browser_window: web_sys::Window,
        allow_threads: bool,
        max_background_workers: Option<usize>,
    ) -> Self {
        #[cfg(feature = "multithreaded")]
        let (background_sender, background_receiver) = PriorityQueueReceiver::new();
        #[cfg(not(feature = "multithreaded"))]
        let (background_sender, _) = PriorityQueueReceiver::new();

        let main_thread_mailbox = Arc::new(MainThreadMailbox::new());

        #[cfg(feature = "multithreaded")]
        let shared_memory = shared_memory_support();
        #[cfg(feature = "multithreaded")]
        let supports_threads = allow_threads && shared_memory.supports_threads();
        #[cfg(not(feature = "multithreaded"))]
        let supports_threads = false;
        #[cfg(not(feature = "multithreaded"))]
        let _ = allow_threads;
        #[cfg(not(feature = "multithreaded"))]
        let _ = max_background_workers;

        #[cfg(feature = "multithreaded")]
        {
            if supports_threads {
                main_thread_mailbox.run_waker_loop(browser_window.clone());
            } else {
                if allow_threads {
                    shared_memory.log_dispatcher_fallback();
                } else {
                    log::info!(
                        "Threaded wasm dispatcher disabled; using single-threaded dispatcher"
                    );
                }
            }
        }

        #[cfg(not(feature = "multithreaded"))]
        {
            log::info!(
                "Threaded wasm dispatcher feature disabled; using single-threaded dispatcher"
            );
        }

        #[cfg(feature = "multithreaded")]
        let background_threads = if supports_threads {
            let thread_count = background_worker_count(&browser_window, max_background_workers);
            log::info!("Threaded wasm dispatcher enabled with {thread_count} background workers");

            // TODO-Wasm: Is it bad to have web workers blocking for a long time like this?
            (0..thread_count)
                .map(|i| {
                    let mut receiver = background_receiver.clone();
                    wasm_thread::Builder::new()
                        .name(format!("background-worker-{i}"))
                        .spawn(move || {
                            loop {
                                let runnable: RunnableVariant = match receiver.pop() {
                                    Ok(runnable) => runnable,
                                    Err(_) => {
                                        log::info!(
                                            "background-worker-{i}: channel disconnected, exiting"
                                        );
                                        break;
                                    }
                                };

                                runnable.run();
                            }
                        })
                        .expect("failed to spawn background worker thread")
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        Self {
            main_thread_id: std::thread::current().id(),
            browser_window,
            background_sender,
            main_thread_mailbox,
            supports_threads,
            #[cfg(feature = "multithreaded")]
            _background_threads: background_threads,
        }
    }

    fn on_main_thread(&self) -> bool {
        std::thread::current().id() == self.main_thread_id
    }

    pub(crate) fn dispatch_function_on_main_thread(
        &self,
        function: impl FnOnce() + Send + 'static,
    ) {
        if self.on_main_thread() {
            let callback = Closure::once_into_js(function);
            self.browser_window
                .queue_microtask(callback.unchecked_ref());
        } else {
            self.main_thread_mailbox
                .post(Priority::High, MainThreadItem::Function(Box::new(function)));
        }
    }
}

#[cfg(feature = "multithreaded")]
fn background_worker_count(
    browser_window: &web_sys::Window,
    max_background_workers: Option<usize>,
) -> usize {
    let uncapped =
        (browser_window.navigator().hardware_concurrency() as usize).max(MIN_BACKGROUND_THREADS);
    max_background_workers.map_or(uncapped, |maximum| uncapped.min(maximum))
}

impl PlatformDispatcher for WebDispatcher {
    fn is_main_thread(&self) -> bool {
        self.on_main_thread()
    }

    fn dispatch(&self, runnable: RunnableVariant, priority: Priority) {
        if !self.supports_threads {
            self.dispatch_on_main_thread(runnable, priority);
            return;
        }

        let result = if self.on_main_thread() {
            self.background_sender.spin_send(priority, runnable)
        } else {
            self.background_sender.send(priority, runnable)
        };

        if let Err(error) = result {
            log::error!("dispatch: failed to send to background queue: {error:?}");
        }
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, priority: Priority) {
        if self.on_main_thread() {
            schedule_runnable(&self.browser_window, runnable, priority);
        } else {
            self.main_thread_mailbox
                .post(priority, MainThreadItem::Runnable(runnable));
        }
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        let millis = duration.as_millis().min(i32::MAX as u128) as i32;
        if self.on_main_thread() {
            let callback = Closure::once_into_js(move || {
                runnable.run();
            });
            self.browser_window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.unchecked_ref(),
                    millis,
                )
                .ok();
        } else {
            self.main_thread_mailbox
                .post(Priority::High, MainThreadItem::Delayed { runnable, millis });
        }
    }

    fn spawn_realtime(&self, function: Box<dyn FnOnce() + Send>) {
        if self.on_main_thread() {
            let callback = Closure::once_into_js(move || {
                function();
            });
            self.browser_window
                .queue_microtask(callback.unchecked_ref());
        } else {
            self.main_thread_mailbox
                .post(Priority::High, MainThreadItem::RealtimeFunction(function));
        }
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

fn execute_on_main_thread(window: &web_sys::Window, item: MainThreadItem) {
    match item {
        MainThreadItem::Runnable(runnable) => {
            runnable.run();
        }
        MainThreadItem::Delayed { runnable, millis } => {
            let callback = Closure::once_into_js(move || {
                runnable.run();
            });
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.unchecked_ref(),
                    millis,
                )
                .ok();
        }
        MainThreadItem::Function(function) => {
            function();
        }
        MainThreadItem::RealtimeFunction(function) => {
            function();
        }
    }
}

fn schedule_runnable(window: &web_sys::Window, runnable: RunnableVariant, priority: Priority) {
    let callback = Closure::once_into_js(move || {
        runnable.run();
    });
    let callback: &js_sys::Function = callback.unchecked_ref();

    match priority {
        Priority::RealtimeAudio => {
            window.queue_microtask(callback);
        }
        _ => {
            // TODO-Wasm: this ought to enqueue so we can dequeue with proper priority
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(callback, 0)
                .ok();
        }
    }
}
