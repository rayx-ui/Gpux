use std::{collections::HashMap, sync::Arc};

use crate::{
    Font, Pixels, Point, SharedString, Size, TextRun, WindowTextSystem, WrappedLineLayout, point,
    px, size,
};

use super::{
    EditableTextProjection, EditableTextStyleRun, EditableTextStyleRuns, LayoutStyleKey,
    RowLayoutKey, ShapeRunKey, TextBufferError, TextBufferSnapshot, TextLayoutCache,
    TextLayoutCacheStats, TextLayoutUpdate,
};

/// A byte-based point in the visual rows produced by soft wrapping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextDisplayPoint {
    /// Zero-based visual row.
    pub row: usize,
    /// UTF-8 byte column relative to the visual row.
    pub column: usize,
}

impl TextDisplayPoint {
    /// Creates a visual row and byte-column point.
    pub const fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }
}

/// Explicit text inputs that can affect wrapping or caret geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayoutStyle {
    /// Base font, including family, fallbacks, features, weight, and style.
    pub font: Font,
    /// Font size used by shaping.
    pub font_size: Pixels,
    /// Height of each visual row.
    pub line_height: Pixels,
    /// Additional cluster spacing used by shaping.
    pub letter_spacing: Pixels,
    /// Optional soft-wrap width. `None` disables wrapping.
    pub wrap_width: Option<Pixels>,
    /// Device scale factor associated with the installed geometry.
    pub device_scale_factor: f32,
    /// Tab-stop width in spaces for callers that expand or render tabs.
    pub tab_size: u8,
}

impl TextLayoutStyle {
    /// Creates an unwrapped style with neutral spacing at DPR 1.
    pub fn new(font: Font, font_size: Pixels, line_height: Pixels) -> Self {
        Self {
            font,
            font_size,
            line_height,
            letter_spacing: px(0.),
            wrap_width: None,
            device_scale_factor: 1.,
            tab_size: 4,
        }
    }

    fn cache_key(&self) -> LayoutStyleKey {
        LayoutStyleKey::new(
            self.font.clone(),
            self.font_size,
            self.line_height,
            self.letter_spacing,
            self.wrap_width,
            self.device_scale_factor,
            self.tab_size,
        )
    }
}

/// One shaping request issued for a single logical line.
pub struct TextLayoutShapeRequest<'a> {
    /// Logical-line text without its LF delimiter.
    pub text: &'a str,
    /// Font-only runs covering the logical line.
    pub runs: &'a [TextRun],
    /// Font size used by shaping.
    pub font_size: Pixels,
    /// Additional cluster spacing used by shaping.
    pub letter_spacing: Pixels,
    /// Optional soft-wrap width.
    pub wrap_width: Option<Pixels>,
}

/// A narrow shaping boundary used by deterministic integration tests.
#[doc(hidden)]
pub trait TextLayoutShaper {
    /// Shapes one logical line and returns its wrapping geometry.
    fn shape_layout(
        &self,
        request: TextLayoutShapeRequest<'_>,
    ) -> Result<Arc<WrappedLineLayout>, String>;
}

impl TextLayoutShaper for WindowTextSystem {
    fn shape_layout(
        &self,
        request: TextLayoutShapeRequest<'_>,
    ) -> Result<Arc<WrappedLineLayout>, String> {
        let lines = self
            .shape_text(
                SharedString::new(request.text),
                request.font_size,
                request.letter_spacing,
                request.runs,
                request.wrap_width,
                None,
            )
            .map_err(|error| error.to_string())?;
        lines
            .into_iter()
            .next()
            .map(|line| line.layout)
            .ok_or_else(|| "text shaper returned no logical line".to_owned())
    }
}

/// Errors produced while atomically preparing a text layout update.
#[derive(Debug, thiserror::Error)]
pub enum TextLayoutError {
    /// Snapshot or style-run ranges are invalid or stale.
    #[error(transparent)]
    Buffer(#[from] TextBufferError),
    /// The platform text system rejected a logical line.
    #[error("text shaping failed: {0}")]
    Shaping(String),
}

struct LayoutRow {
    text: SharedString,
    buffer_start: usize,
    buffer_len: usize,
    display_start: usize,
    display_len: usize,
    visual_start: usize,
    visual_len: usize,
    character_boundaries: Arc<[usize]>,
    layout: Arc<WrappedLineLayout>,
}

/// Revision-tagged wrapping and geometry for neutral editable text.
#[derive(Default)]
pub struct TextLayoutMap {
    rows: Vec<LayoutRow>,
    installed_style_runs: Option<EditableTextStyleRuns>,
    projected_style_runs: Vec<EditableTextStyleRun>,
    cache: TextLayoutCache,
    line_height: Pixels,
    visual_line_count: usize,
    size: Size<Pixels>,
    projection: Option<EditableTextProjection>,
}

impl TextLayoutMap {
    /// Creates an empty layout map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the installed buffer revision.
    pub fn revision(&self) -> Option<u64> {
        self.cache.revision()
    }

    /// Returns cumulative cache counters.
    pub fn cache_stats(&self) -> TextLayoutCacheStats {
        self.cache.stats()
    }

    /// Returns the number of visual rows, including trailing empty rows.
    pub fn visual_line_count(&self) -> usize {
        self.visual_line_count
    }

    /// Returns the maximum row width and total visual height.
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }

    /// Atomically installs wrapping geometry for a snapshot using GPUI's text system.
    pub fn update(
        &mut self,
        snapshot: &TextBufferSnapshot,
        style_runs: &EditableTextStyleRuns,
        style: &TextLayoutStyle,
        text_system: &WindowTextSystem,
    ) -> Result<TextLayoutUpdate, TextLayoutError> {
        self.update_with_projection(snapshot, None, style_runs, style, text_system)
    }

    pub(crate) fn update_with_projection(
        &mut self,
        snapshot: &TextBufferSnapshot,
        projection: Option<&EditableTextProjection>,
        style_runs: &EditableTextStyleRuns,
        style: &TextLayoutStyle,
        text_system: &WindowTextSystem,
    ) -> Result<TextLayoutUpdate, TextLayoutError> {
        self.update_with_shaper_and_projection(snapshot, projection, style_runs, style, text_system)
    }

    /// Atomically installs wrapping geometry through an injected deterministic shaper.
    #[doc(hidden)]
    pub fn update_with_shaper(
        &mut self,
        snapshot: &TextBufferSnapshot,
        style_runs: &EditableTextStyleRuns,
        style: &TextLayoutStyle,
        shaper: &impl TextLayoutShaper,
    ) -> Result<TextLayoutUpdate, TextLayoutError> {
        self.update_with_shaper_and_projection(snapshot, None, style_runs, style, shaper)
    }

    fn update_with_shaper_and_projection(
        &mut self,
        snapshot: &TextBufferSnapshot,
        projection: Option<&EditableTextProjection>,
        style_runs: &EditableTextStyleRuns,
        style: &TextLayoutStyle,
        shaper: &impl TextLayoutShaper,
    ) -> Result<TextLayoutUpdate, TextLayoutError> {
        let style_key = style.cache_key();
        if self
            .cache
            .matches_installed(snapshot.revision(), &style_key)
            && self.installed_style_runs.as_ref() == Some(style_runs)
            && self.projection.as_ref() == projection
        {
            return Ok(TextLayoutUpdate::default());
        }

        let clipped_runs = style_runs.clipped_to(snapshot, 0..snapshot.len())?;
        let projected_runs = project_style_runs(&clipped_runs, projection);
        let style_matches = self.cache.style_matches(&style_key);
        let reusable = self.cache.reusable_rows(&style_key);
        let source_text = snapshot.text();
        let text = projection.map_or(source_text.as_str(), EditableTextProjection::text);
        let logical_lines = logical_lines(&text);
        let mut next_rows = Vec::with_capacity(logical_lines.len());
        let mut next_cache = HashMap::with_capacity(logical_lines.len());
        let mut visual_start = 0;
        let mut update = TextLayoutUpdate::default();
        let mut max_width = px(0.);
        let mut style_run_start = 0;

        for (display_start, line) in logical_lines {
            let display_end = display_start + line.len();
            let buffer_start =
                projection.map_or(display_start, |map| map.display_to_buffer(display_start));
            let buffer_end =
                projection.map_or(display_end, |map| map.display_to_buffer(display_end));
            while style_run_start < projected_runs.len()
                && projected_runs[style_run_start].range.end <= display_start
            {
                style_run_start += 1;
            }
            let style_run_end = style_run_start
                + projected_runs[style_run_start..]
                    .partition_point(|run| run.range.start < display_end);
            let (shape_runs, run_keys) = shape_runs_for_line(
                display_start,
                display_end,
                &projected_runs[style_run_start..style_run_end],
                style.font.clone(),
            );
            let line_text: Arc<str> = Arc::from(line);
            let row_key = RowLayoutKey::new(line_text.clone(), run_keys);
            let layout = if let Some(layout) = reusable.and_then(|rows| rows.get(&row_key)) {
                update.reused_rows += 1;
                layout.clone()
            } else {
                update.shaped_rows += 1;
                update.invalidated_rows += 1;
                shaper
                    .shape_layout(TextLayoutShapeRequest {
                        text: &line_text,
                        runs: &shape_runs,
                        font_size: style.font_size,
                        letter_spacing: style.letter_spacing,
                        wrap_width: style.wrap_width,
                    })
                    .map_err(TextLayoutError::Shaping)?
            };
            let visual_len = layout.wrap_boundaries().len() + 1;
            max_width = max_width.max(layout.width());
            let character_boundaries = line
                .char_indices()
                .map(|(offset, _)| offset)
                .chain(std::iter::once(line.len()))
                .collect::<Vec<_>>()
                .into();
            next_rows.push(LayoutRow {
                text: SharedString::new(line),
                buffer_start,
                buffer_len: buffer_end.saturating_sub(buffer_start),
                display_start,
                display_len: line.len(),
                visual_start,
                visual_len,
                character_boundaries,
                layout: layout.clone(),
            });
            next_cache.insert(row_key, layout);
            visual_start += visual_len;
        }

        if !style_matches {
            update.invalidated_rows = update
                .invalidated_rows
                .max(self.cache.prior_row_count())
                .max(next_rows.len());
        }

        self.rows = next_rows;
        self.installed_style_runs = Some(style_runs.clone());
        self.projected_style_runs = projected_runs;
        self.line_height = style.line_height;
        self.visual_line_count = visual_start;
        self.size = size(max_width, style.line_height * visual_start);
        self.projection = projection.cloned();
        self.cache
            .install(snapshot.revision(), style_key, next_cache, update);
        Ok(update)
    }

    /// Converts a valid buffer byte offset to a visual row and byte column.
    pub fn buffer_to_display(&self, offset: usize) -> Option<TextDisplayPoint> {
        let row = self.row_for_buffer_offset(offset)?;
        let display_offset = self.buffer_offset_to_display(offset);
        let local_offset = display_offset.saturating_sub(row.display_start);
        let position = row
            .layout
            .position_for_index(local_offset, self.line_height)?;
        let local_visual_row = visual_row(position.y, self.line_height).min(row.visual_len - 1);
        let visual_start = row_boundary_index(&row.layout, local_visual_row, self.line_height);
        Some(TextDisplayPoint::new(
            row.visual_start + local_visual_row,
            local_offset.saturating_sub(visual_start),
        ))
    }

    /// Converts a visual row and byte column back to a valid buffer offset.
    pub fn display_to_buffer(&self, point: TextDisplayPoint) -> Option<usize> {
        let row = self.row_for_visual_row(point.row)?;
        let local_visual_row = point.row - row.visual_start;
        let start = row_boundary_index(&row.layout, local_visual_row, self.line_height);
        let end = row_boundary_end(&row.layout, local_visual_row, self.line_height);
        let requested = start + point.column.min(end.saturating_sub(start));
        let boundary = row
            .character_boundaries
            .partition_point(|boundary| *boundary <= requested)
            .saturating_sub(1);
        let display_offset = row.display_start + row.character_boundaries[boundary];
        Some(self.display_offset_to_buffer(display_offset))
    }

    /// Returns the pixel position for a valid buffer byte offset.
    pub fn position_for_index(&self, offset: usize) -> Option<Point<Pixels>> {
        let row = self.row_for_buffer_offset(offset)?;
        let display_offset = self.buffer_offset_to_display(offset);
        let local = row.layout.position_for_index(
            display_offset.saturating_sub(row.display_start),
            self.line_height,
        )?;
        Some(local + point(px(0.), self.line_height * row.visual_start))
    }

    /// Returns the configured height of one visual row.
    pub fn line_height(&self) -> Pixels {
        self.line_height
    }

    /// Returns the buffer range covered by one visual row.
    pub fn visual_row_range(&self, visual_row: usize) -> Option<std::ops::Range<usize>> {
        let row = self.row_for_visual_row(visual_row)?;
        let local_visual_row = visual_row - row.visual_start;
        let start = row_boundary_index(&row.layout, local_visual_row, self.line_height);
        let end = row_boundary_end(&row.layout, local_visual_row, self.line_height);
        Some(
            self.display_offset_to_buffer(row.display_start + start)
                ..self.display_offset_to_buffer(row.display_start + end),
        )
    }

    /// Returns the pixel position of a visual-row byte column.
    pub fn position_for_display(&self, display: TextDisplayPoint) -> Option<Point<Pixels>> {
        let row = self.row_for_visual_row(display.row)?;
        let local_visual_row = display.row - row.visual_start;
        let start = row_boundary_index(&row.layout, local_visual_row, self.line_height);
        let end = row_boundary_end(&row.layout, local_visual_row, self.line_height);
        let index = start + display.column.min(end.saturating_sub(start));
        let x = if index == start {
            px(0.)
        } else {
            row.layout
                .position_for_index(index, self.line_height)
                .map(|position| position.x)?
        };
        Some(point(x, self.line_height * display.row))
    }

    pub(crate) fn logical_row_count(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn projected_style_runs(&self) -> &[EditableTextStyleRun] {
        &self.projected_style_runs
    }

    pub(crate) fn paint_row(
        &self,
        row: usize,
    ) -> Option<(usize, usize, SharedString, Arc<WrappedLineLayout>)> {
        let row = self.rows.get(row)?;
        Some((
            row.display_start,
            row.visual_start,
            row.text.clone(),
            row.layout.clone(),
        ))
    }

    /// Returns the closest valid buffer offset to a pixel position.
    pub fn index_for_position(&self, position: Point<Pixels>) -> Option<usize> {
        if position.y < px(0.) || self.line_height <= px(0.) {
            return None;
        }
        let visual_row = visual_row(position.y, self.line_height);
        let row = self.row_for_visual_row(visual_row)?;
        let local = point(position.x, position.y - self.line_height * row.visual_start);
        let index = match row
            .layout
            .closest_index_for_position(local, self.line_height)
        {
            Ok(index) | Err(index) => index,
        };
        Some(self.display_offset_to_buffer(row.display_start + index.min(row.display_len)))
    }

    pub(crate) fn buffer_range_to_display(
        &self,
        range: std::ops::Range<usize>,
    ) -> std::ops::Range<usize> {
        self.buffer_offset_to_display(range.start)..self.buffer_offset_to_display(range.end)
    }

    fn buffer_offset_to_display(&self, offset: usize) -> usize {
        self.projection
            .as_ref()
            .map_or(offset, |projection| projection.buffer_to_display(offset))
    }

    fn display_offset_to_buffer(&self, offset: usize) -> usize {
        self.projection
            .as_ref()
            .map_or(offset, |projection| projection.display_to_buffer(offset))
    }

    fn row_for_buffer_offset(&self, offset: usize) -> Option<&LayoutRow> {
        self.rows.iter().enumerate().find_map(|(index, row)| {
            let is_last = index + 1 == self.rows.len();
            (offset >= row.buffer_start
                && (offset <= row.buffer_start + row.buffer_len)
                && (is_last || offset < row.buffer_start + row.buffer_len + 1))
                .then_some(row)
        })
    }

    fn row_for_visual_row(&self, visual_row: usize) -> Option<&LayoutRow> {
        self.rows.iter().find(|row| {
            visual_row >= row.visual_start && visual_row < row.visual_start + row.visual_len
        })
    }
}

fn project_style_runs(
    style_runs: &[EditableTextStyleRun],
    projection: Option<&EditableTextProjection>,
) -> Vec<EditableTextStyleRun> {
    let Some(projection) = projection else {
        return style_runs.to_vec();
    };
    style_runs
        .iter()
        .filter_map(|run| {
            let start = projection.buffer_to_display(run.range.start);
            let end = projection.buffer_to_display(run.range.end);
            (start < end).then(|| EditableTextStyleRun::new(start..end, run.style.clone()))
        })
        .collect()
}

fn logical_lines(text: &str) -> Vec<(usize, &str)> {
    let mut start = 0;
    let mut lines = Vec::new();
    for line in text.split('\n') {
        lines.push((start, line));
        start += line.len() + 1;
    }
    lines
}

fn shape_runs_for_line(
    line_start: usize,
    line_end: usize,
    style_runs: &[EditableTextStyleRun],
    base_font: Font,
) -> (Vec<TextRun>, Vec<ShapeRunKey>) {
    let mut boundaries = vec![line_start, line_end];
    for run in style_runs {
        let start = run.range.start.max(line_start);
        let end = run.range.end.min(line_end);
        if start < end {
            boundaries.push(start);
            boundaries.push(end);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut text_runs = Vec::new();
    let mut keys = Vec::new();
    for range in boundaries.windows(2) {
        let start = range[0];
        let end = range[1];
        if start == end {
            continue;
        }
        let mut font = base_font.clone();
        for run in style_runs
            .iter()
            .filter(|run| run.range.start <= start && run.range.end >= end)
        {
            if let Some(weight) = run.style.font_weight {
                font.weight = weight;
            }
            if let Some(font_style) = run.style.font_style {
                font.style = font_style;
            }
        }
        let len = end - start;
        text_runs.push(TextRun {
            len,
            font: font.clone(),
            color: crate::black().into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        keys.push(ShapeRunKey { len, font });
    }

    if line_start == line_end {
        keys.push(ShapeRunKey {
            len: 0,
            font: base_font,
        });
    }
    (text_runs, keys)
}

fn visual_row(y: Pixels, line_height: Pixels) -> usize {
    if line_height <= px(0.) {
        0
    } else {
        (f32::from(y) / f32::from(line_height)).max(0.) as usize
    }
}

fn row_boundary_index(layout: &WrappedLineLayout, visual_row: usize, line_height: Pixels) -> usize {
    let position = point(px(0.), line_height * visual_row);
    match layout.closest_index_for_position(position, line_height) {
        Ok(index) | Err(index) => index,
    }
}

fn row_boundary_end(layout: &WrappedLineLayout, visual_row: usize, line_height: Pixels) -> usize {
    if visual_row + 1 > layout.wrap_boundaries().len() {
        layout.len()
    } else {
        row_boundary_index(layout, visual_row + 1, line_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditableTextDisplayTransform, FontStyle, HighlightStyle};

    #[test]
    fn projected_style_runs_follow_visible_display_offsets() {
        let source = "header\nhidden\nvisible";
        let hidden_start = source.find("hidden").unwrap();
        let visible_start = source.find("visible").unwrap();
        let projection =
            EditableTextDisplayTransform::omitting_ranges(source, [hidden_start..visible_start])
                .unwrap()
                .project(source)
                .unwrap();
        let runs = [EditableTextStyleRun::new(
            visible_start..source.len(),
            HighlightStyle {
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
        )];

        let projected = project_style_runs(&runs, Some(&projection));

        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].range, hidden_start..projection.text().len());
        assert_eq!(projected[0].style.font_style, Some(FontStyle::Italic));
    }
}
