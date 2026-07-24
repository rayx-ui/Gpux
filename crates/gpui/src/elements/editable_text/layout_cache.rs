use std::{collections::HashMap, sync::Arc};

use crate::{Font, Pixels, WrappedLineLayout};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct LayoutStyleKey {
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
    letter_spacing: Pixels,
    wrap_width: Option<Pixels>,
    device_scale_factor_bits: u32,
    tab_size: u8,
}

impl LayoutStyleKey {
    pub(super) fn new(
        font: Font,
        font_size: Pixels,
        line_height: Pixels,
        letter_spacing: Pixels,
        wrap_width: Option<Pixels>,
        device_scale_factor: f32,
        tab_size: u8,
    ) -> Self {
        Self {
            font,
            font_size,
            line_height,
            letter_spacing,
            wrap_width,
            device_scale_factor_bits: device_scale_factor.to_bits(),
            tab_size,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ShapeRunKey {
    pub(super) len: usize,
    pub(super) font: Font,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct RowLayoutKey {
    text: Arc<str>,
    runs: Arc<[ShapeRunKey]>,
}

impl RowLayoutKey {
    pub(super) fn new(text: Arc<str>, runs: Vec<ShapeRunKey>) -> Self {
        Self {
            text,
            runs: runs.into(),
        }
    }
}

/// Cumulative cache counters for the current layout-map instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextLayoutCacheStats {
    /// Number of logical rows sent to a text shaper.
    pub shaped_rows: usize,
    /// Number of logical rows reused from the prior installed cache.
    pub cache_hits: usize,
    /// Number of prior or requested logical rows invalidated by an update.
    pub invalidated_rows: usize,
}

#[derive(Default)]
pub(super) struct TextLayoutCache {
    revision: Option<u64>,
    style: Option<LayoutStyleKey>,
    rows: HashMap<RowLayoutKey, Arc<WrappedLineLayout>>,
    stats: TextLayoutCacheStats,
}

impl TextLayoutCache {
    pub(super) fn revision(&self) -> Option<u64> {
        self.revision
    }

    pub(super) fn stats(&self) -> TextLayoutCacheStats {
        self.stats
    }

    pub(super) fn reusable_rows(
        &self,
        style: &LayoutStyleKey,
    ) -> HashMap<RowLayoutKey, Arc<WrappedLineLayout>> {
        if self.style.as_ref() == Some(style) {
            self.rows.clone()
        } else {
            HashMap::new()
        }
    }

    pub(super) fn prior_row_count(&self) -> usize {
        self.rows.len()
    }

    pub(super) fn style_matches(&self, style: &LayoutStyleKey) -> bool {
        self.style.as_ref() == Some(style)
    }

    pub(super) fn matches_installed(&self, revision: u64, style: &LayoutStyleKey) -> bool {
        self.revision == Some(revision) && self.style_matches(style)
    }

    pub(super) fn install(
        &mut self,
        revision: u64,
        style: LayoutStyleKey,
        rows: HashMap<RowLayoutKey, Arc<WrappedLineLayout>>,
        update: TextLayoutUpdate,
    ) {
        self.revision = Some(revision);
        self.style = Some(style);
        self.rows = rows;
        self.stats.shaped_rows += update.shaped_rows;
        self.stats.cache_hits += update.reused_rows;
        self.stats.invalidated_rows += update.invalidated_rows;
    }
}

/// Per-update layout-cache counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextLayoutUpdate {
    /// Logical rows sent to the text shaper during this update.
    pub shaped_rows: usize,
    /// Logical rows reused from the prior installed cache.
    pub reused_rows: usize,
    /// Logical rows whose prior layout could not be reused.
    pub invalidated_rows: usize,
}

impl TextLayoutUpdate {
    /// Returns the fraction of requested rows reused by this update.
    pub fn reuse_ratio(self) -> f32 {
        let total = self.shaped_rows + self.reused_rows;
        if total == 0 {
            1.0
        } else {
            self.reused_rows as f32 / total as f32
        }
    }
}
