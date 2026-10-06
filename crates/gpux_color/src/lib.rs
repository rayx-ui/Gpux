//! Standards-focused, UI-neutral color engine.

mod color;

pub use color::*;

/// Lets renderer build scripts generate shader bindings from [`PaintColor`]'s source.
#[doc(hidden)]
pub static GPUX_COLOR_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
