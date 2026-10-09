//! Scroll layers: a scroll container's content painted once into cached tiles
//! and composited at the scroll offset on frames where only the offset
//! changed. Adapted from gpui-fast's `fast/layers/`.

/// Whether a renderer on this target implements scroll-layer tiles. Where it
/// does not, [`FastMode::normalized`](crate::fast::mode::FastMode::normalized)
/// keeps scroll layers off and every layer entry point is a no-op.
pub(crate) const COMPILED: bool = cfg!(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos",
    target_os = "windows"
));
