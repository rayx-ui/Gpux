//! The browser transport: the page's `WebSocket`, on the main thread, reconnecting with bounded
//! backoff while no sidecar listens.

use std::{cell::RefCell, rc::Rc, time::Duration};

use wasm_bindgen::{JsCast, closure::Closure};

use super::{Backoff, Control, Event};

struct State {
    url: String,
    backoff: Backoff,
    delay: Duration,
    enabled: bool,
    halted: bool,
    socket: Option<web_sys::WebSocket>,
    events: async_channel::Sender<Event>,
    /// The socket's handlers, kept alive with it.
    handlers: Vec<Closure<dyn FnMut(wasm_bindgen::JsValue)>>,
}

/// The page's connection to the sidecar.
pub(crate) struct Transport {
    state: Rc<RefCell<State>>,
}

impl Transport {
    pub(crate) fn start(
        url: String,
        backoff: Backoff,
        enabled: bool,
        events: async_channel::Sender<Event>,
    ) -> Self {
        let state = Rc::new(RefCell::new(State {
            url,
            backoff,
            delay: backoff.min,
            enabled,
            halted: false,
            socket: None,
            events,
            handlers: Vec::new(),
        }));
        if enabled {
            connect(&state);
        }
        Self { state }
    }

    pub(crate) fn control(&self, control: Control) {
        match control {
            Control::Send(text) => {
                if let Some(socket) = &self.state.borrow().socket {
                    socket.send_with_str(&text).ok();
                }
            }
            Control::Close => {
                self.state.borrow_mut().halted = true;
                close(&self.state);
            }
            Control::SetEnabled(on) => {
                let was = {
                    let mut state = self.state.borrow_mut();
                    let was = state.enabled && !state.halted;
                    state.enabled = on;
                    state.halted = false;
                    state.delay = state.backoff.min;
                    was
                };
                if on && !was {
                    connect(&self.state);
                } else if !on {
                    close(&self.state);
                }
            }
            Control::Shutdown => {
                self.state.borrow_mut().enabled = false;
                close(&self.state);
            }
        }
    }
}

fn close(state: &Rc<RefCell<State>>) {
    let socket = state.borrow_mut().socket.take();
    if let Some(socket) = socket {
        socket.close().ok();
        state
            .borrow()
            .events
            .try_send(Event::Disconnected(None))
            .ok();
    }
}

fn connect(state: &Rc<RefCell<State>>) {
    let url = state.borrow().url.clone();
    let Ok(socket) = web_sys::WebSocket::new(&url) else {
        retry(state);
        return;
    };
    let on = |state: &Rc<RefCell<State>>, f: fn(&Rc<RefCell<State>>, wasm_bindgen::JsValue)| {
        let state = Rc::downgrade(state);
        Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(move |event| {
            if let Some(state) = state.upgrade() {
                f(&state, event);
            }
        })
    };
    let opened = on(state, |state, _| {
        let mut state = state.borrow_mut();
        state.delay = state.backoff.min;
        state.events.try_send(Event::Connected).ok();
    });
    let message = on(state, |state, event| {
        let event: web_sys::MessageEvent = event.unchecked_into();
        if let Some(text) = event.data().as_string() {
            state.borrow().events.try_send(Event::Text(text)).ok();
        }
    });
    let closed = on(state, |state, _| {
        let was_open = state.borrow_mut().socket.take().is_some();
        if was_open {
            state
                .borrow()
                .events
                .try_send(Event::Disconnected(None))
                .ok();
        }
        retry(state);
    });
    socket.set_onopen(Some(opened.as_ref().unchecked_ref()));
    socket.set_onmessage(Some(message.as_ref().unchecked_ref()));
    socket.set_onclose(Some(closed.as_ref().unchecked_ref()));
    let mut state = state.borrow_mut();
    state.socket = Some(socket);
    state.handlers = vec![opened, message, closed];
}

/// Tries again after the backoff, while the devtools are on and the app did not close them.
fn retry(state: &Rc<RefCell<State>>) {
    let delay = {
        let mut state = state.borrow_mut();
        if !state.enabled || state.halted {
            return;
        }
        let delay = state.delay;
        state.delay = (state.delay * 2).min(state.backoff.max);
        delay
    };
    let weak = Rc::downgrade(state);
    let again = Closure::once_into_js(move || {
        if let Some(state) = weak.upgrade() {
            let go = {
                let state = state.borrow();
                state.enabled && !state.halted && state.socket.is_none()
            };
            if go {
                connect(&state);
            }
        }
    });
    if let Some(window) = web_sys::window() {
        window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                again.unchecked_ref(),
                delay.as_millis() as i32,
            )
            .ok();
    }
}
