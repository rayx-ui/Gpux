use std::array;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::color::{ColorSpace, EncodedColor};

#[derive(Debug)]
pub(crate) struct ColorConversionCache {
    values: [OnceLock<EncodedColor>; ColorSpace::COUNT],
    initializations: [AtomicUsize; ColorSpace::COUNT],
}

impl ColorConversionCache {
    pub(crate) fn new(source: EncodedColor) -> Self {
        let cache = Self {
            values: array::from_fn(|_| OnceLock::new()),
            initializations: array::from_fn(|_| AtomicUsize::new(0)),
        };
        let _ = cache.values[source.space().index()].set(source);
        cache
    }

    pub(crate) fn get_or_init(
        &self,
        space: ColorSpace,
        initialize: impl FnOnce() -> EncodedColor,
    ) -> EncodedColor {
        *self.values[space.index()].get_or_init(|| {
            self.initializations[space.index()].fetch_add(1, Ordering::Relaxed);
            initialize()
        })
    }

    pub(crate) fn initialization_count(&self, space: ColorSpace) -> usize {
        self.initializations[space.index()].load(Ordering::Relaxed)
    }

    pub(crate) fn total_initialization_count(&self) -> usize {
        self.initializations
            .iter()
            .map(|count| count.load(Ordering::Relaxed))
            .sum()
    }
}
