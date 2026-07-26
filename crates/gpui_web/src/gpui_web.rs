#![cfg(target_family = "wasm")]

mod dispatcher;
mod display;
mod events;
mod html_element;
mod html_in_canvas;
mod html_island;
mod http_client;
mod keyboard;
mod logging;
#[cfg(feature = "rayx_diagnostics")]
mod media_query_diagnostics;
mod platform;
mod web_viewport_scheduler;
mod window;

pub use dispatcher::WebDispatcher;
pub use display::WebDisplay;
#[cfg(feature = "rayx_diagnostics")]
pub use html_in_canvas::{HtmlInCanvasDiagnostics, HtmlInCanvasProbes, HtmlInCanvasSupport};
pub use http_client::FetchHttpClient;
pub use keyboard::WebKeyboardLayout;
pub use logging::init_logging;
pub use platform::WebPlatform;
pub use window::WebWindow;
