//! Fast mode: the performance and composition work of gpui-fast
//! (longbridge/gpui-fast, v0.1.6, `b8f0c02`), adapted to gpux.
//!
//! Every mechanism fast mode adds lives under this module, one file per topic,
//! named after the gpui-fast file it adapts so that a later gpui-fast fix can
//! be read beside the gpux file it concerns (see `docs/upstream/gpui-fast.md`).
//! The rest of the crate only calls into it: a field typed by a struct from
//! here, a call written by its full path (`crate::fast::mode::...`), or a
//! visibility change. Nothing here is glob-imported or glob-re-exported; the
//! few public items are exported one by one from `gpui.rs`.
//!
//! Each mechanism is switched per window through [`mode::FastMode`]. With
//! every switch off, a hook costs a branch on a `bool` and the frame is the
//! one gpux draws without fast mode.

pub(crate) mod bounds_tree;
pub(crate) mod dependencies;
pub(crate) mod glyphs;
pub(crate) mod layers;
pub(crate) mod layout;
pub(crate) mod layout_key;
pub(crate) mod mode;
pub(crate) mod retained;
#[cfg(any(test, feature = "test-support", feature = "bench-support"))]
pub mod scenarios;
pub(crate) mod scene;
#[cfg(any(
    test,
    feature = "test-support",
    feature = "bench-support",
    feature = "diagnostics"
))]
pub(crate) mod stats;
pub(crate) mod text;
#[cfg(any(test, feature = "test-support", feature = "diagnostics"))]
pub(crate) mod verify;

#[cfg(test)]
mod tests;

/// Adds to a [`stats::FastStats`] counter where the counters are compiled
/// (`test-support`, `bench-support`, `diagnostics`), and does nothing
/// elsewhere: `fast_stat!(window.fast_stats.frame, views_built += 1)`.
/// Production builds compile neither the counters nor the increments.
macro_rules! fast_stat {
    (atomic $counter:expr, += $count:expr) => {
        #[cfg(any(
            test,
            feature = "test-support",
            feature = "bench-support",
            feature = "diagnostics"
        ))]
        {
            $counter.fetch_add($count as u64, std::sync::atomic::Ordering::Relaxed);
        }
    };
    ($stats:expr, $field:ident += $count:expr) => {
        #[cfg(any(
            test,
            feature = "test-support",
            feature = "bench-support",
            feature = "diagnostics"
        ))]
        {
            let count = $count;
            $stats.$field += count as u64;
        }
    };
}
pub(crate) use fast_stat;
