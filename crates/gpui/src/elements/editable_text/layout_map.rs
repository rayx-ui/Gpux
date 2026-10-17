use std::{collections::HashMap, sync::Arc};

use crate::{
    Font, FontId, LineLayout, Pixels, Point, RunMetrics, ShapedRun, SharedString, Size, TextAlign,
    TextRun, WindowTextSystem, WrappedLineLayout, point, px, size,
};

use super::{
    AttributeSpan, EditableTextProjection, EditableTextStyleRun, EditableTextStyleRuns, Formatting,
    InlineObject, LayoutStyleKey, OBJECT_REPLACEMENT_CHARACTER, ParagraphAlignment,
    ParagraphFormat, RowLayoutKey, ShapeRunKey, TextBufferError, TextBufferSnapshot, TextEdit,
    TextLayoutCache, TextLayoutCacheStats, TextLayoutUpdate, TextListKind,
    editable_text_alignment_offset,
};

/// A byte-based point in the visual rows produced by soft wrapping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextDisplayPoint {
    /// Zero-based visual row.
    pub row: usize,
    /// UTF-8 byte column relative to the visual row.
    pub column: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct TextLayoutEdit {
    expected_revision: u64,
    range: std::ops::Range<usize>,
    replacement_len: usize,
    replacement_has_newline: bool,
}

impl From<&TextEdit> for TextLayoutEdit {
    fn from(edit: &TextEdit) -> Self {
        Self {
            expected_revision: edit.expected_revision(),
            range: edit.range().clone(),
            replacement_len: edit.replacement().len(),
            replacement_has_newline: edit.replacement().contains('\n'),
        }
    }
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

    /// The ascent and descent (both positive) of a font at a size, for rows that mix sizes.
    fn font_ascent_descent(&self, _font_id: FontId, font_size: Pixels) -> (Pixels, Pixels) {
        (font_size * 0.8, font_size * 0.2)
    }
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

    fn font_ascent_descent(&self, font_id: FontId, font_size: Pixels) -> (Pixels, Pixels) {
        (
            self.ascent(font_id, font_size),
            -self.descent(font_id, font_size),
        )
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
    /// One entry per visual row of the logical row.
    shapes: Arc<[RowShape]>,
    /// The paragraph's format, when it is not the default.
    paragraph: Option<Arc<ParagraphFormat>>,
    /// The row's inline objects, with each one's offset in the row.
    objects: Arc<[(usize, InlineObject)]>,
}

/// A visual row's own height and baseline, and where its glyphs start and end in the logical
/// row's layout.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RowShape {
    height: Pixels,
    baseline: Pixels,
    start_x: Pixels,
    width: Pixels,
    /// The width without trailing spaces, and the spaces before it, for justification.
    trimmed_width: Pixels,
    spaces: usize,
}

/// The geometry of one visual row of a [`TextLayoutMap`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextVisualRow {
    /// The row's top, from the top of the text.
    pub top: Pixels,
    /// The row's height.
    pub height: Pixels,
    /// The distance from the row's top to its baseline, shared by every run in it.
    pub baseline: Pixels,
    /// How far the row's start is moved from the field's alignment offset
    /// ([`super::EditableTextGeometry::alignment_offset`]) by its own alignment; positions the
    /// map reports include it.
    pub x_offset: Pixels,
    /// The width of the row's glyphs.
    pub width: Pixels,
    /// The extra advance after each space of a justified row.
    pub justify: Pixels,
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
    /// Stored formatting the next update lays out, and the one installed.
    formatting: Option<Arc<Formatting>>,
    installed_formatting: Option<Arc<Formatting>>,
    /// The installed formatting's spans in display coordinates.
    projected_attributes: Vec<AttributeSpan>,
    /// Every visual row's geometry, in order.
    visual_rows: Vec<TextVisualRow>,
    alignment: TextAlign,
    align_width: Option<Pixels>,
    /// How far each list level moves a list item's leading indent, and the step installed.
    list_indent_step: Pixels,
    installed_list_indent_step: Pixels,
    /// The markers of list items, in order.
    list_markers: Vec<TextListMarker>,
    /// The field's font size, which list markers of unformatted items take.
    font_size: Pixels,
    /// The field's alignment offset the rows' offsets are relative to.
    alignment_offset: Pixels,
}

/// The marker of one list item, painted in the space before its first row.
#[derive(Clone, Debug, PartialEq)]
pub struct TextListMarker {
    /// The item's first visual row.
    pub visual_row: usize,
    /// The buffer offset where the item's paragraph starts.
    pub paragraph_start: usize,
    /// The marker's text; empty for a checkbox.
    pub text: SharedString,
    /// A checkbox item's state.
    pub checkbox: Option<bool>,
    /// The size of the item's first character, which the marker takes.
    pub font_size: Pixels,
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

    /// The geometry of one visual row.
    pub fn visual_row(&self, row: usize) -> Option<TextVisualRow> {
        self.visual_rows.get(row).copied()
    }

    /// The visual row at `y` (from the top of the text), or the row count past the last row.
    pub fn visual_row_at_y(&self, y: Pixels) -> usize {
        if y >= self.size.height {
            return self.visual_rows.len();
        }
        self.visual_rows
            .partition_point(|row| row.top <= y)
            .saturating_sub(1)
    }

    /// The caret's rectangle at a buffer offset, from the top of the text: as tall as the
    /// line height of the run at the caret, around the row's baseline.
    pub fn caret_bounds(&self, offset: usize) -> Option<crate::Bounds<Pixels>> {
        let position = self.position_for_index(offset)?;
        let display = self.buffer_to_display(offset)?;
        let row = self.visual_rows.get(display.row)?;
        let logical = self.row_for_buffer_offset(offset)?;
        let local = self
            .buffer_offset_to_display(offset)
            .saturating_sub(logical.display_start);
        let line = &logical.layout.unwrapped_layout;
        // The run of the character before the caret within its row, else the one after it.
        let row_start = row_boundary_index(&logical.layout, display.row - logical.visual_start);
        let mut glyphs = line.runs.iter().flat_map(|run| {
            run.glyphs
                .iter()
                .map(move |glyph| (glyph.index, run.font_size))
        });
        let run_size = glyphs
            .clone()
            .filter(|(index, _)| *index >= row_start)
            .take_while(|(index, _)| *index < local.max(row_start + 1))
            .last()
            .or_else(|| glyphs.find(|(index, _)| *index >= local))
            .or_else(|| line.runs.first().map(|run| (0, run.font_size)))
            .map(|(_, size)| size);
        let height = match run_size {
            Some(size) if line.font_size > px(0.) && size != line.font_size => {
                (self.line_height * (size / line.font_size)).min(row.height)
            }
            _ => self.line_height.min(row.height),
        };
        let top = if height >= row.height {
            row.top
        } else {
            (row.top + row.baseline - height * (row.baseline / row.height)).max(row.top)
        };
        Some(crate::Bounds::new(
            point(position.x, top),
            size(px(0.), height),
        ))
    }

    /// Aligns each visual row within `width` as paint does; a row's offset from the field's
    /// alignment offset then enters every position the map reports.
    pub(crate) fn set_alignment(&mut self, alignment: TextAlign, width: Option<Pixels>) {
        if self.alignment != alignment || self.align_width != width {
            self.alignment = alignment;
            self.align_width = width;
            self.rebuild_visual_rows();
        }
    }

    /// Each list item's marker: bullets and numbers by level, numbers counting consecutive
    /// numbered items at a level (deeper items do not break the count; a paragraph that is not
    /// a list item, or another kind at that level, restarts it).
    fn rebuild_list_markers(&mut self) {
        self.list_markers.clear();
        // Per level: the kind's discriminant, its first number, and the items counted.
        let mut counters: Vec<Option<(std::mem::Discriminant<TextListKind>, u32, u32)>> =
            Vec::new();
        for row in &self.rows {
            let Some(item) = row
                .paragraph
                .as_deref()
                .and_then(|format| format.list.as_ref())
            else {
                counters.clear();
                continue;
            };
            let level = usize::from(item.level);
            counters.truncate(level + 1);
            counters.resize(level + 1, None);
            let kind = std::mem::discriminant(&item.kind);
            let start = match item.kind {
                TextListKind::Numbered { start } => start,
                _ => 0,
            };
            let count = match &mut counters[level] {
                Some((counted, _, count)) if *counted == kind => {
                    *count = count.saturating_add(1);
                    *count
                }
                slot => {
                    *slot = Some((kind, start, 0));
                    0
                }
            };
            let first = counters[level].map_or(start, |(_, first, _)| first);
            let (text, checkbox) = match &item.kind {
                TextListKind::Bullet => (bullet_marker(level), None),
                TextListKind::Numbered { .. } => {
                    (number_marker(level, first.saturating_add(count)), None)
                }
                TextListKind::Checkbox { checked } => (SharedString::default(), Some(*checked)),
                TextListKind::Custom(text) => (text.clone(), None),
            };
            let font_size = self
                .projected_attributes
                .iter()
                .find(|span| {
                    span.range.start <= row.display_start && span.range.end > row.display_start
                })
                .and_then(|span| span.attributes.font_size)
                .unwrap_or(self.font_size);
            self.list_markers.push(TextListMarker {
                visual_row: row.visual_start,
                paragraph_start: row.buffer_start,
                text,
                checkbox,
                font_size,
            });
        }
    }

    fn rebuild_visual_rows(&mut self) {
        self.visual_rows.clear();
        let global = self.align_width.map_or(px(0.), |width| {
            editable_text_alignment_offset(self.alignment, width, self.size.width)
        });
        self.alignment_offset = global;
        let line_height = self.line_height;
        let mut uniform = true;
        let mut next_top = px(0.);
        for row in &self.rows {
            let paragraph = row.paragraph.as_deref();
            let last_row = row.shapes.len().saturating_sub(1);
            for (row_index, shape) in row.shapes.iter().enumerate() {
                let index = self.visual_rows.len();
                let space_before = paragraph
                    .filter(|_| row_index == 0)
                    .map_or(px(0.), |format| format.space_before);
                let space_after = paragraph
                    .filter(|_| row_index == last_row)
                    .map_or(px(0.), |format| format.space_after);
                // Rows of the configured height sit exactly where uniform rows always did.
                let starts_uniform = uniform && space_before == px(0.);
                let top = if starts_uniform {
                    line_height * index
                } else {
                    next_top + space_before
                };
                uniform = starts_uniform && shape.height == line_height && space_after == px(0.);
                next_top = top + shape.height + space_after;
                let (x, justify) = match paragraph {
                    Some(format) => paragraph_row_x(
                        format,
                        self.list_indent_step,
                        self.alignment,
                        self.align_width,
                        shape,
                        row_index == 0,
                        row_index == last_row,
                    ),
                    None => (
                        self.align_width.map_or(px(0.), |width| {
                            editable_text_alignment_offset(self.alignment, width, shape.width)
                        }),
                        px(0.),
                    ),
                };
                let x_offset = if self.align_width.is_some() {
                    x - global
                } else {
                    x
                };
                self.visual_rows.push(TextVisualRow {
                    top,
                    height: shape.height,
                    baseline: shape.baseline,
                    x_offset,
                    width: shape.width,
                    justify,
                });
            }
        }
        self.size.height = if uniform {
            line_height * self.visual_rows.len()
        } else {
            next_top
        };
    }

    /// Sets the stored formatting the next update lays out with the text.
    pub(crate) fn set_formatting(&mut self, formatting: Option<Arc<Formatting>>) {
        self.formatting = formatting;
    }

    /// Sets how far each list level moves a list item's leading indent.
    pub(crate) fn set_list_indent_step(&mut self, step: Pixels) {
        self.list_indent_step = step;
    }

    /// Every inline object with its buffer offset and the box it reserves, from the top of the
    /// text: its width, standing `baseline_offset` below its row's baseline.
    pub fn inline_object_bounds(&self) -> Vec<(usize, InlineObject, crate::Bounds<Pixels>)> {
        let mut boxes = Vec::new();
        for row in &self.rows {
            for (index, object) in row.objects.iter() {
                let offset = self.display_offset_to_buffer(row.display_start + index);
                let (Some(position), Some(display)) = (
                    self.position_for_index(offset),
                    self.buffer_to_display(offset),
                ) else {
                    continue;
                };
                // An object starting a wrapped row reports the end of the row before it.
                let display = if display.column == 0 || offset == row.buffer_start {
                    display
                } else {
                    let row_range = self.visual_row_range(display.row);
                    match row_range {
                        Some(range) if range.end == offset => {
                            TextDisplayPoint::new(display.row + 1, 0)
                        }
                        _ => display,
                    }
                };
                let Some(visual) = self.visual_rows.get(display.row) else {
                    continue;
                };
                let x = if display.column == 0 {
                    self.position_for_display(display)
                        .map_or(position.x, |p| p.x)
                } else {
                    position.x
                };
                let top = visual.top + visual.baseline + object.baseline_offset - object.height;
                boxes.push((
                    offset,
                    object.clone(),
                    crate::Bounds::new(point(x, top), size(object.width, object.height)),
                ));
            }
        }
        boxes
    }

    /// The offsets, within logical row `row`, of its inline objects' characters.
    pub(crate) fn logical_row_object_offsets(&self, row: usize) -> Vec<usize> {
        self.rows.get(row).map_or_else(Vec::new, |row| {
            row.objects.iter().map(|(index, _)| *index).collect()
        })
    }

    /// The markers of list items, in order.
    pub fn list_markers(&self) -> &[TextListMarker] {
        &self.list_markers
    }

    /// Where a marker `width` wide starts, from the start of the text: right-aligned a gap of
    /// 0.4 of its font size before the item's first row, but never before the field's leading
    /// edge.
    pub fn list_marker_x(&self, marker: &TextListMarker, width: Pixels) -> Option<Pixels> {
        let start = self
            .position_for_display(TextDisplayPoint::new(marker.visual_row, 0))?
            .x;
        // The field's leading edge, in the coordinates positions use.
        let leading_edge = -self.alignment_offset;
        Some((start - marker.font_size * 0.4 - width).max(leading_edge))
    }

    /// Where a checkbox item's box sits, from the top of the text: a square of 0.8 of the
    /// item's font size placed as [`Self::list_marker_x`] places a marker, standing on the
    /// row's baseline.
    pub fn list_checkbox_bounds(&self, marker: &TextListMarker) -> Option<crate::Bounds<Pixels>> {
        marker.checkbox?;
        let row = self.visual_rows.get(marker.visual_row)?;
        let side = marker.font_size * 0.8;
        let x = self.list_marker_x(marker, side)?;
        Some(crate::Bounds::new(
            point(x, row.top + row.baseline - side),
            size(side, side),
        ))
    }

    /// The list item whose checkbox contains `position` (from the top of the text).
    pub fn list_checkbox_at(&self, position: Point<Pixels>) -> Option<&TextListMarker> {
        self.list_markers.iter().find(|marker| {
            self.list_checkbox_bounds(marker)
                .is_some_and(|bounds| bounds.contains(&position))
        })
    }

    fn formatting_installed(&self) -> bool {
        if self.list_indent_step != self.installed_list_indent_step {
            return false;
        }
        match (&self.formatting, &self.installed_formatting) {
            (None, None) => true,
            (Some(next), Some(installed)) => Arc::ptr_eq(next, installed) || next == installed,
            _ => false,
        }
    }

    /// The shaped layout of one logical row (a line between hard breaks).
    pub fn logical_row_layout(&self, row: usize) -> Option<Arc<WrappedLineLayout>> {
        self.rows.get(row).map(|row| row.layout.clone())
    }

    pub(crate) fn projected_attributes(&self) -> &[AttributeSpan] {
        &self.projected_attributes
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

    #[doc(hidden)]
    pub fn update_after_edit_with_shaper(
        &mut self,
        snapshot: &TextBufferSnapshot,
        edit: &TextEdit,
        style_runs: &EditableTextStyleRuns,
        style: &TextLayoutStyle,
        shaper: &impl TextLayoutShaper,
    ) -> Result<TextLayoutUpdate, TextLayoutError> {
        self.update_after_layout_edit_with_shaper(
            snapshot,
            &TextLayoutEdit::from(edit),
            style_runs,
            style,
            shaper,
        )
    }

    pub(crate) fn update_after_layout_edit_with_shaper(
        &mut self,
        snapshot: &TextBufferSnapshot,
        edit: &TextLayoutEdit,
        style_runs: &EditableTextStyleRuns,
        style: &TextLayoutStyle,
        shaper: &impl TextLayoutShaper,
    ) -> Result<TextLayoutUpdate, TextLayoutError> {
        if let Some(update) =
            self.splice_single_line_edit(snapshot, edit, style_runs, style, shaper)?
        {
            return Ok(update);
        }
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
            && self.formatting_installed()
        {
            return Ok(TextLayoutUpdate::default());
        }

        let clipped_runs = style_runs.clipped_to(snapshot, 0..snapshot.len())?;
        let projected_runs = project_style_runs(&clipped_runs, projection);
        let projected_attributes = self
            .formatting
            .as_ref()
            .map(|formatting| project_attribute_spans(formatting.spans.spans(), projection))
            .unwrap_or_default();
        let mut attribute_start = 0;
        let style_matches = self.cache.style_matches(&style_key);
        let reusable = self.cache.reusable_rows(&style_key);
        let line_count = projection.map_or_else(
            || snapshot.line_count(),
            |map| map.text().split('\n').count(),
        );
        let mut next_rows = Vec::with_capacity(line_count);
        let mut next_cache = HashMap::with_capacity(line_count);
        let mut visual_start = 0;
        let mut update = TextLayoutUpdate::default();
        let mut max_width = px(0.);
        let mut style_run_start = 0;

        let formatting = self.formatting.clone();
        let list_step = self.list_indent_step;
        let mut layout_line = |display_start: usize,
                               line: &str,
                               buffer_start: usize,
                               buffer_end: usize,
                               paragraph_index: usize|
         -> Result<(), TextLayoutError> {
            let display_end = display_start + line.len();
            let paragraph = formatting
                .as_ref()
                .and_then(|formatting| formatting.paragraph(paragraph_index))
                .cloned();
            while style_run_start < projected_runs.len()
                && projected_runs[style_run_start].range.end <= display_start
            {
                style_run_start += 1;
            }
            let style_run_end = style_run_start
                + projected_runs[style_run_start..]
                    .partition_point(|run| run.range.start < display_end);
            while attribute_start < projected_attributes.len()
                && projected_attributes[attribute_start].range.end <= display_start
            {
                attribute_start += 1;
            }
            let attribute_end = attribute_start
                + projected_attributes[attribute_start..]
                    .partition_point(|span| span.range.start < display_end);
            let (shape_runs, run_keys) = shape_runs_for_line(
                display_start,
                display_end,
                &projected_runs[style_run_start..style_run_end],
                &projected_attributes[attribute_start..attribute_end],
                paragraph.as_ref().and_then(|format| format.letter_spacing),
                style.font.clone(),
            );
            let line_text: Arc<str> = Arc::from(line);
            let objects: Arc<[(usize, InlineObject)]> = projected_attributes
                [attribute_start..attribute_end]
                .iter()
                .filter_map(|span| Some((span, span.attributes.object.as_ref()?)))
                .flat_map(|(span, object)| {
                    let start = span.range.start.max(display_start);
                    let end = span.range.end.min(display_end);
                    line.get(start - display_start..end - display_start)
                        .unwrap_or_default()
                        .match_indices(OBJECT_REPLACEMENT_CHARACTER)
                        .map(move |(index, _)| (start - display_start + index, object.clone()))
                        .collect::<Vec<_>>()
                })
                .collect();
            let wrap_widths = paragraph
                .as_deref()
                .and_then(|format| paragraph_wrap_widths(format, style.wrap_width, list_step));
            let row_key = RowLayoutKey::new(line_text.clone(), run_keys)
                .with_wrap_widths(wrap_widths)
                .with_objects(
                    objects
                        .iter()
                        .map(|(index, object)| (*index, object.width))
                        .collect(),
                );
            let layout = if let Some(layout) = reusable.and_then(|rows| rows.get(&row_key)) {
                update.reused_rows += 1;
                layout.clone()
            } else {
                update.shaped_rows += 1;
                update.invalidated_rows += 1;
                let layout = shaper
                    .shape_layout(TextLayoutShapeRequest {
                        text: &line_text,
                        runs: &shape_runs,
                        font_size: style.font_size,
                        letter_spacing: style.letter_spacing,
                        wrap_width: if wrap_widths.is_some() || !objects.is_empty() {
                            None
                        } else {
                            style.wrap_width
                        },
                    })
                    .map_err(TextLayoutError::Shaping)?;
                // Inline objects reserve exactly their widths; a paragraph with indents wraps
                // its first row and the others at their own widths, from one shaping.
                let unwrapped = if objects.is_empty() {
                    layout.unwrapped_layout.clone()
                } else {
                    Arc::new(with_object_widths(&layout.unwrapped_layout, &objects))
                };
                match (wrap_widths, objects.is_empty()) {
                    (None, true) => layout,
                    (widths, _) => {
                        let widths = widths.or(style.wrap_width.map(|width| (width, width)));
                        Arc::new(WrappedLineLayout {
                            wrap_boundaries: widths
                                .map(|(first, other)| {
                                    unwrapped.wrap_boundaries_for_widths(&line_text, first, other)
                                })
                                .unwrap_or_default(),
                            wrap_width: widths.map(|(_, other)| other),
                            unwrapped_layout: unwrapped,
                        })
                    }
                }
            };
            let visual_len = layout.wrap_boundaries().len() + 1;
            let shapes = row_shapes(
                &layout,
                &line_text,
                style,
                paragraph.as_deref(),
                &objects,
                shaper,
            );
            // Content reaches past each row's own indent and its trailing indent.
            max_width = max_width.max(match paragraph.as_deref() {
                Some(format) => {
                    let (first_indent, other_indent) = paragraph_indents(format, list_step);
                    shapes
                        .iter()
                        .enumerate()
                        .map(|(row, shape)| {
                            let indent = if row == 0 { first_indent } else { other_indent };
                            indent + shape.width + format.trailing_indent.max(px(0.))
                        })
                        .reduce(Pixels::max)
                        .unwrap_or_default()
                }
                None => layout.width(),
            });
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
                shapes,
                paragraph,
                objects,
            });
            next_cache.insert(row_key, layout);
            visual_start += visual_len;
            Ok(())
        };

        if let Some(projection) = projection {
            let mut display_start = 0;
            for line in projection.text().split('\n') {
                let display_end = display_start + line.len();
                let buffer_start = projection.display_to_buffer(display_start);
                layout_line(
                    display_start,
                    line,
                    buffer_start,
                    projection.display_to_buffer(display_end),
                    snapshot.byte_to_point(buffer_start)?.y,
                )?;
                display_start = display_end + 1;
            }
        } else {
            for line_index in 0..snapshot.line_count() {
                let range = snapshot.logical_line_range(line_index)?;
                let line = snapshot.slice(range.clone())?;
                layout_line(range.start, &line, range.start, range.end, line_index)?;
            }
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
        self.installed_formatting = self.formatting.clone();
        self.installed_list_indent_step = self.list_indent_step;
        self.projected_attributes = projected_attributes;
        self.font_size = style.font_size;
        self.rebuild_visual_rows();
        self.rebuild_list_markers();
        self.cache
            .install(snapshot.revision(), style_key, next_cache, update);
        Ok(update)
    }

    fn splice_single_line_edit(
        &mut self,
        snapshot: &TextBufferSnapshot,
        edit: &TextLayoutEdit,
        style_runs: &EditableTextStyleRuns,
        style: &TextLayoutStyle,
        shaper: &impl TextLayoutShaper,
    ) -> Result<Option<TextLayoutUpdate>, TextLayoutError> {
        if edit.expected_revision.checked_add(1) != Some(snapshot.revision())
            || edit.range.len() != edit.replacement_len
            || edit.replacement_has_newline
            || !style_runs.is_empty()
            || !self
                .installed_style_runs
                .as_ref()
                .is_some_and(EditableTextStyleRuns::is_empty)
            || self.projection.is_some()
            || self.formatting.is_some()
            || self.installed_formatting.is_some()
            || !self.cache.style_matches(&style.cache_key())
        {
            return Ok(None);
        }

        let row_index = self
            .rows
            .partition_point(|row| row.buffer_start + row.buffer_len < edit.range.start)
            .min(self.rows.len().saturating_sub(1));
        let Some(row) = self.rows.get(row_index) else {
            return Ok(None);
        };
        if edit.range.start < row.buffer_start
            || edit.range.end > row.buffer_start + row.buffer_len
            || self
                .rows
                .iter()
                .enumerate()
                .any(|(index, candidate)| index != row_index && candidate.text == row.text)
        {
            return Ok(None);
        }

        let text = snapshot.slice(row.buffer_start..row.buffer_start + row.buffer_len)?;
        if text.contains('\n') {
            return Ok(None);
        }
        let (runs, run_keys) =
            shape_runs_for_line(0, text.len(), &[], &[], None, style.font.clone());
        let text: Arc<str> = Arc::from(text);
        let layout = shaper
            .shape_layout(TextLayoutShapeRequest {
                text: &text,
                runs: &runs,
                font_size: style.font_size,
                letter_spacing: style.letter_spacing,
                wrap_width: style.wrap_width,
            })
            .map_err(TextLayoutError::Shaping)?;
        let visual_len = layout.wrap_boundaries().len() + 1;
        if layout.width() < row.layout.width() && row.layout.width() == self.size.width {
            return Ok(None);
        }

        let old_key = RowLayoutKey::new(Arc::from(row.text.as_ref()), Vec::new());
        let new_key = RowLayoutKey::new(text.clone(), run_keys);
        let visual_delta = visual_len as isize - row.visual_len as isize;
        let replacement = LayoutRow {
            text: SharedString::new(text.as_ref()),
            buffer_start: row.buffer_start,
            buffer_len: row.buffer_len,
            display_start: row.display_start,
            display_len: text.len(),
            visual_start: row.visual_start,
            visual_len,
            character_boundaries: text
                .char_indices()
                .map(|(offset, _)| offset)
                .chain(std::iter::once(text.len()))
                .collect::<Vec<_>>()
                .into(),
            layout: layout.clone(),
            shapes: row_shapes(&layout, &text, style, None, &[], shaper),
            paragraph: None,
            objects: Arc::from([]),
        };
        self.rows[row_index] = replacement;
        if visual_delta != 0 {
            for row in &mut self.rows[row_index + 1..] {
                row.visual_start = row.visual_start.saturating_add_signed(visual_delta);
            }
            self.visual_line_count = self.visual_line_count.saturating_add_signed(visual_delta);
        }
        self.size.width = self.size.width.max(layout.width());
        self.size.height = style.line_height * self.visual_line_count;
        self.installed_style_runs = Some(style_runs.clone());
        self.rebuild_visual_rows();
        let update = TextLayoutUpdate {
            shaped_rows: 1,
            reused_rows: self.rows.len().saturating_sub(1),
            invalidated_rows: 1,
        };
        self.cache
            .replace_row(snapshot.revision(), old_key, new_key, layout, update);
        Ok(Some(update))
    }

    /// Converts a valid buffer byte offset to a visual row and byte column.
    pub fn buffer_to_display(&self, offset: usize) -> Option<TextDisplayPoint> {
        let row = self.row_for_buffer_offset(offset)?;
        let display_offset = self.buffer_offset_to_display(offset);
        let local_offset = display_offset.saturating_sub(row.display_start);
        let local_visual_row = local_visual_row(&row.layout, local_offset)?.min(row.visual_len - 1);
        let visual_start = row_boundary_index(&row.layout, local_visual_row);
        Some(TextDisplayPoint::new(
            row.visual_start + local_visual_row,
            local_offset.saturating_sub(visual_start),
        ))
    }

    /// Converts a visual row and byte column back to a valid buffer offset.
    pub fn display_to_buffer(&self, point: TextDisplayPoint) -> Option<usize> {
        let row = self.row_for_visual_row(point.row)?;
        let local_visual_row = point.row - row.visual_start;
        let start = row_boundary_index(&row.layout, local_visual_row);
        let end = row_boundary_end(&row.layout, local_visual_row);
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
        let local = row
            .layout
            .position_for_index(display_offset.saturating_sub(row.display_start), UNIT_ROW)?;
        let local_row = f32::from(local.y) as usize;
        let visual = self.visual_rows.get(row.visual_start + local_row)?;
        let x = self.justified_x(
            row,
            visual,
            row_boundary_index(&row.layout, local_row),
            display_offset.saturating_sub(row.display_start),
            local.x,
        );
        Some(point(x + visual.x_offset, visual.top))
    }

    /// The field's font size from the last layout, or zero before it.
    pub fn font_size(&self) -> Pixels {
        self.font_size
    }

    /// Returns the configured height of one visual row.
    pub fn line_height(&self) -> Pixels {
        self.line_height
    }

    /// Returns the buffer range covered by one visual row.
    pub fn visual_row_range(&self, visual_row: usize) -> Option<std::ops::Range<usize>> {
        let row = self.row_for_visual_row(visual_row)?;
        let local_visual_row = visual_row - row.visual_start;
        let start = row_boundary_index(&row.layout, local_visual_row);
        let end = row_boundary_end(&row.layout, local_visual_row);
        Some(
            self.display_offset_to_buffer(row.display_start + start)
                ..self.display_offset_to_buffer(row.display_start + end),
        )
    }

    /// Returns the pixel position of a visual-row byte column.
    pub fn position_for_display(&self, display: TextDisplayPoint) -> Option<Point<Pixels>> {
        let row = self.row_for_visual_row(display.row)?;
        let local_visual_row = display.row - row.visual_start;
        let start = row_boundary_index(&row.layout, local_visual_row);
        let end = row_boundary_end(&row.layout, local_visual_row);
        let index = start + display.column.min(end.saturating_sub(start));
        let x = if index == start {
            px(0.)
        } else {
            row.layout
                .position_for_index(index, UNIT_ROW)
                .map(|position| position.x)?
        };
        let visual = self.visual_rows.get(display.row)?;
        let x = self.justified_x(row, visual, start, index, x);
        Some(point(x + visual.x_offset, visual.top))
    }

    pub(crate) fn logical_rows_for_visual_range(
        &self,
        visual_rows: std::ops::Range<usize>,
    ) -> std::ops::Range<usize> {
        let first = self
            .rows
            .partition_point(|row| row.visual_start + row.visual_len <= visual_rows.start);
        let end = self
            .rows
            .partition_point(|row| row.visual_start < visual_rows.end);
        first..end
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
        let visual_row = self.visual_row_at_y(position.y);
        let row = self.row_for_visual_row(visual_row)?;
        let visual = self.visual_rows.get(visual_row)?;
        let local_row = visual_row - row.visual_start;
        if visual.justify > px(0.) {
            // A justified row's boundaries sit at their widened positions.
            let start = row_boundary_index(&row.layout, local_row);
            let end = row_boundary_end(&row.layout, local_row);
            let row_start_x = row.layout.unwrapped_layout.x_for_index(start);
            let target = position.x - visual.x_offset;
            let index = row
                .character_boundaries
                .iter()
                .copied()
                .filter(|boundary| (start..=end).contains(boundary))
                .min_by(|a, b| {
                    let distance = |index: usize| {
                        let x = row.layout.unwrapped_layout.x_for_index(index) - row_start_x;
                        (self.justified_x(row, visual, start, index, x) - target).abs()
                    };
                    f32::from(distance(*a)).total_cmp(&f32::from(distance(*b)))
                })
                .unwrap_or(start);
            return Some(self.display_offset_to_buffer(row.display_start + index));
        }
        let local = point(
            position.x - visual.x_offset,
            UNIT_ROW * local_row + UNIT_ROW / 2.,
        );
        let index = match row.layout.closest_index_for_position(local, UNIT_ROW) {
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
        let index = self
            .rows
            .partition_point(|row| row.buffer_start <= offset)
            .checked_sub(1)?;
        let row = self.rows.get(index)?;
        let is_last = index + 1 == self.rows.len();
        (offset >= row.buffer_start
            && (offset <= row.buffer_start + row.buffer_len)
            && (is_last || offset < row.buffer_start + row.buffer_len + 1))
            .then_some(row)
    }

    /// `x` within a row moved by the justification of the spaces before `index`.
    fn justified_x(
        &self,
        row: &LayoutRow,
        visual: &TextVisualRow,
        row_start: usize,
        index: usize,
        x: Pixels,
    ) -> Pixels {
        if visual.justify == px(0.) {
            return x;
        }
        let spaces = row
            .text
            .get(row_start.min(index)..index)
            .map_or(0, |text| text.matches(' ').count());
        x + visual.justify * spaces as f32
    }

    fn row_for_visual_row(&self, visual_row: usize) -> Option<&LayoutRow> {
        let index = self
            .rows
            .partition_point(|row| row.visual_start <= visual_row)
            .checked_sub(1)?;
        let row = self.rows.get(index)?;
        (visual_row < row.visual_start + row.visual_len).then_some(row)
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

fn project_attribute_spans(
    spans: &[AttributeSpan],
    projection: Option<&EditableTextProjection>,
) -> Vec<AttributeSpan> {
    let Some(projection) = projection else {
        return spans.to_vec();
    };
    spans
        .iter()
        .filter_map(|span| {
            let start = projection.buffer_to_display(span.range.start);
            let end = projection.buffer_to_display(span.range.end);
            (start < end).then(|| AttributeSpan {
                range: start..end,
                attributes: span.attributes.clone(),
            })
        })
        .collect()
}

/// Shaping runs for one line: stored family, weight, style, size, letter spacing and baseline
/// shift, then the owner's style runs' weight and style over them.
fn shape_runs_for_line(
    line_start: usize,
    line_end: usize,
    style_runs: &[EditableTextStyleRun],
    attributes: &[AttributeSpan],
    paragraph_letter_spacing: Option<Pixels>,
    base_font: Font,
) -> (Vec<TextRun>, Vec<ShapeRunKey>) {
    let mut boundaries = vec![line_start, line_end];
    for span in attributes
        .iter()
        .filter(|span| span.attributes.affects_shaping())
    {
        let start = span.range.start.max(line_start);
        let end = span.range.end.min(line_end);
        if start < end {
            boundaries.push(start);
            boundaries.push(end);
        }
    }
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

    let mut text_runs: Vec<TextRun> = Vec::new();
    let mut keys: Vec<ShapeRunKey> = Vec::new();
    for range in boundaries.windows(2) {
        let start = range[0];
        let end = range[1];
        if start == end {
            continue;
        }
        let mut font = base_font.clone();
        let mut metrics = RunMetrics {
            letter_spacing: paragraph_letter_spacing,
            ..Default::default()
        };
        if let Some(span) = attributes.iter().find(|span| {
            span.range.start <= start && span.range.end > start && span.attributes.affects_shaping()
        }) {
            let stored = &span.attributes;
            if let Some(family) = &stored.font_family {
                font.family = family.clone();
            }
            if let Some(weight) = stored.font_weight {
                font.weight = weight;
            }
            if let Some(font_style) = stored.font_style {
                font.style = font_style;
            }
            metrics = RunMetrics {
                font_size: stored.font_size,
                letter_spacing: stored.letter_spacing.or(paragraph_letter_spacing),
                baseline_shift: stored.baseline_shift.unwrap_or_default(),
            };
        }
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
        // Spans that differ only in paint shape as one run, so paint changes never reshape.
        if let (Some(last_run), Some(last_key)) = (text_runs.last_mut(), keys.last_mut())
            && last_run.font == font
            && last_run.metrics == metrics
        {
            last_run.len += len;
            last_key.len += len;
            continue;
        }
        text_runs.push(TextRun {
            len,
            font: font.clone(),
            color: crate::black().into(),
            background_color: None,
            underline: None,
            strikethrough: None,
            metrics,
        });
        keys.push(ShapeRunKey { len, font, metrics });
    }

    if line_start == line_end {
        keys.push(ShapeRunKey {
            len: 0,
            font: base_font,
            metrics: RunMetrics::default(),
        });
    }
    (text_runs, keys)
}

/// Wrapped-layout queries take a row height; the map asks them in rows of one pixel and keeps
/// each visual row's own height and top itself.
const UNIT_ROW: Pixels = px(1.);

fn local_visual_row(layout: &WrappedLineLayout, local_offset: usize) -> Option<usize> {
    layout
        .position_for_index(local_offset, UNIT_ROW)
        .map(|position| f32::from(position.y) as usize)
}

/// The first-row and other-row wrap widths of a paragraph with indents, when the field wraps.
fn paragraph_wrap_widths(
    format: &ParagraphFormat,
    wrap_width: Option<Pixels>,
    list_step: Pixels,
) -> Option<(Pixels, Pixels)> {
    let wrap_width = wrap_width?;
    if format.leading_indent == px(0.)
        && format.first_line_indent == px(0.)
        && format.trailing_indent == px(0.)
        && format.list.is_none()
    {
        return None;
    }
    let (first_indent, other_indent) = paragraph_indents(format, list_step);
    let available = |indent: Pixels| (wrap_width - indent - format.trailing_indent).max(px(1.));
    Some((available(first_indent), available(other_indent)))
}

/// Where the first row and the other rows start: the leading indent (moved one list step per
/// list level, counting the first), plus the first-line indent on the first row; a hanging
/// indent stops at the field's edge.
fn paragraph_indents(format: &ParagraphFormat, list_step: Pixels) -> (Pixels, Pixels) {
    let list = format
        .list
        .as_ref()
        .map_or(px(0.), |item| list_step * (f32::from(item.level) + 1.));
    let leading = format.leading_indent.max(px(0.)) + list;
    ((leading + format.first_line_indent).max(px(0.)), leading)
}

/// A paragraph row's start and justification: its indent, then its alignment between the
/// indents (or the element's, when the paragraph sets none). Justified rows widen their spaces
/// to reach the trailing indent, except the paragraph's last row and single-word rows.
fn paragraph_row_x(
    format: &ParagraphFormat,
    list_step: Pixels,
    element_alignment: TextAlign,
    width: Option<Pixels>,
    shape: &RowShape,
    first: bool,
    last: bool,
) -> (Pixels, Pixels) {
    let (first_indent, other_indent) = paragraph_indents(format, list_step);
    let indent = if first { first_indent } else { other_indent };
    let Some(width) = width else {
        return (indent, px(0.));
    };
    let available = width - indent - format.trailing_indent;
    let alignment = match format.alignment {
        Some(ParagraphAlignment::Left) => TextAlign::Left,
        Some(ParagraphAlignment::Center) => TextAlign::Center,
        Some(ParagraphAlignment::Right) => TextAlign::Right,
        Some(ParagraphAlignment::Justify) => {
            let justify = if last || shape.spaces == 0 || shape.trimmed_width >= available {
                px(0.)
            } else {
                (available - shape.trimmed_width) / shape.spaces as f32
            };
            return (indent, justify);
        }
        None => element_alignment,
    };
    // A row wider than its space starts at its indent rather than before it.
    (
        indent + editable_text_alignment_offset(alignment, available, shape.width).max(px(0.)),
        px(0.),
    )
}

/// Each visual row's height, baseline and horizontal extent. Logical rows whose runs all sit
/// at the field's size and baseline keep exactly the uniform geometry; rows mixing sizes or
/// shifts are as tall as their tallest run's line height and share one baseline. A paragraph's
/// line-height multiplier scales its rows.
fn row_shapes(
    layout: &WrappedLineLayout,
    text: &str,
    style: &TextLayoutStyle,
    paragraph: Option<&ParagraphFormat>,
    objects: &[(usize, InlineObject)],
    shaper: &impl TextLayoutShaper,
) -> Arc<[RowShape]> {
    let line = &layout.unwrapped_layout;
    let multiplier = paragraph
        .and_then(|format| format.line_height)
        .unwrap_or(1.);
    let line_height = if multiplier == 1. {
        style.line_height
    } else {
        style.line_height * multiplier
    };
    let justifying =
        paragraph.is_some_and(|format| format.alignment == Some(ParagraphAlignment::Justify));
    let mut extents = Vec::with_capacity(layout.wrap_boundaries().len() + 1);
    let mut start_x = px(0.);
    let mut start_index = 0;
    for boundary in layout.wrap_boundaries() {
        let glyph = &line.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
        extents.push((
            start_x,
            glyph.position.x - start_x,
            start_index..glyph.index,
        ));
        start_x = glyph.position.x;
        start_index = glyph.index;
    }
    extents.push((start_x, line.width - start_x, start_index..text.len()));
    // Justification needs each row's inner spaces and its width without trailing spaces.
    let justification = |start_x: Pixels, width: Pixels, range: std::ops::Range<usize>| {
        if !justifying {
            return (width, 0);
        }
        let row_text = text.get(range.clone()).unwrap_or_default();
        let trimmed = row_text.trim_end_matches(' ');
        let trimmed_end = range.start + trimmed.len();
        let trimmed_width = if trimmed_end >= range.end {
            width
        } else {
            line.x_for_index(trimmed_end) - start_x
        };
        (trimmed_width, trimmed.matches(' ').count())
    };
    let uniform_baseline = (line_height - line.ascent - line.descent) / 2. + line.ascent;
    let uniform = line
        .runs
        .iter()
        .all(|run| run.font_size == style.font_size && run.baseline_shift == Pixels::ZERO);
    if (uniform && objects.is_empty()) || style.font_size <= px(0.) {
        return extents
            .into_iter()
            .map(|(start_x, width, range)| {
                let (trimmed_width, spaces) = justification(start_x, width, range);
                RowShape {
                    height: line_height,
                    baseline: uniform_baseline,
                    start_x,
                    width,
                    trimmed_width,
                    spaces,
                }
            })
            .collect();
    }

    // The distinct (size, shift, font) of the runs in each visual row.
    let mut row_runs: Vec<Vec<(Pixels, Pixels, FontId)>> = vec![Vec::new(); extents.len()];
    let mut boundaries = layout.wrap_boundaries().iter().peekable();
    let mut row = 0;
    for (run_ix, run) in line.runs.iter().enumerate() {
        for glyph_ix in 0..run.glyphs.len() {
            while boundaries
                .peek()
                .is_some_and(|boundary| (boundary.run_ix, boundary.glyph_ix) <= (run_ix, glyph_ix))
            {
                boundaries.next();
                row = (row + 1).min(row_runs.len() - 1);
            }
            let entry = (run.font_size, run.baseline_shift, run.font_id);
            if !row_runs[row].contains(&entry) {
                row_runs[row].push(entry);
            }
        }
    }
    extents
        .into_iter()
        .zip(row_runs)
        .map(|((start_x, width, range), runs)| {
            // Objects count by their own height and baseline offset.
            let row_objects = objects
                .iter()
                .filter(|(index, _)| range.contains(index))
                .map(|(_, object)| object)
                .collect::<Vec<_>>();
            let (trimmed_width, spaces) = justification(start_x, width, range);
            if runs.is_empty() && row_objects.is_empty() {
                return RowShape {
                    height: line_height,
                    baseline: uniform_baseline,
                    start_x,
                    width,
                    trimmed_width,
                    spaces,
                };
            }
            let mut height = line_height;
            let mut ascent = px(0.);
            let mut descent = px(0.);
            for (size, shift, font_id) in runs {
                height = height.max(line_height * (size / style.font_size));
                let (run_ascent, run_descent) = shaper.font_ascent_descent(font_id, size);
                ascent = ascent.max(run_ascent + shift);
                descent = descent.max(run_descent - shift);
            }
            for object in row_objects {
                ascent = ascent.max(object.height - object.baseline_offset);
                descent = descent.max(object.baseline_offset);
                height = height.max(ascent + descent);
            }
            RowShape {
                height,
                baseline: (height - ascent - descent) / 2. + ascent,
                start_x,
                width,
                trimmed_width,
                spaces,
            }
        })
        .collect()
}

fn row_boundary_index(layout: &WrappedLineLayout, visual_row: usize) -> usize {
    let position = point(px(0.), UNIT_ROW * visual_row);
    match layout.closest_index_for_position(position, UNIT_ROW) {
        Ok(index) | Err(index) => index,
    }
}

fn row_boundary_end(layout: &WrappedLineLayout, visual_row: usize) -> usize {
    if visual_row + 1 > layout.wrap_boundaries().len() {
        layout.len()
    } else {
        row_boundary_index(layout, visual_row + 1)
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

fn bullet_marker(level: usize) -> SharedString {
    SharedString::new_static(["\u{2022}", "\u{25e6}", "\u{25aa}"][level % 3])
}

/// 1., a. or i. by level.
fn number_marker(level: usize, number: u32) -> SharedString {
    let label = match level % 3 {
        0 => number.to_string(),
        1 => {
            let mut label = String::new();
            let mut number = number.max(1);
            while number > 0 {
                number -= 1;
                label.insert(0, char::from(b'a' + (number % 26) as u8));
                number /= 26;
            }
            label
        }
        // Roman numerals stop at 3999; larger numbers stay decimal, which bounds the label.
        _ if number > 3999 => number.to_string(),
        _ => {
            let mut number = number.max(1);
            let mut label = String::new();
            for (value, numeral) in [
                (1000, "m"),
                (900, "cm"),
                (500, "d"),
                (400, "cd"),
                (100, "c"),
                (90, "xc"),
                (50, "l"),
                (40, "xl"),
                (10, "x"),
                (9, "ix"),
                (5, "v"),
                (4, "iv"),
                (1, "i"),
            ] {
                while number >= value {
                    label.push_str(numeral);
                    number -= value;
                }
            }
            label
        }
    };
    format!("{label}.").into()
}

/// The layout with each inline object's character advancing exactly the object's width.
fn with_object_widths(line: &LineLayout, objects: &[(usize, InlineObject)]) -> LineLayout {
    let positions = line
        .runs
        .iter()
        .flat_map(|run| {
            run.glyphs
                .iter()
                .map(|glyph| (glyph.index, glyph.position.x))
        })
        .collect::<Vec<_>>();
    // The shift each glyph moves by: the sum of the object widths before it less the advances
    // their characters were shaped with.
    let mut shifts = vec![px(0.); positions.len() + 1];
    for (glyph_ix, (index, x)) in positions.iter().enumerate() {
        let Some((_, object)) = objects.iter().find(|(offset, _)| offset == index) else {
            continue;
        };
        let next_x = positions
            .get(glyph_ix + 1)
            .map_or(line.width, |(_, next)| *next);
        let delta = object.width - (next_x - *x);
        for shift in &mut shifts[glyph_ix + 1..] {
            *shift += delta;
        }
    }
    let mut glyph_ix = 0;
    let runs = line
        .runs
        .iter()
        .map(|run| ShapedRun {
            glyphs: run
                .glyphs
                .iter()
                .map(|glyph| {
                    let mut glyph = glyph.clone();
                    glyph.position.x += shifts[glyph_ix];
                    glyph_ix += 1;
                    glyph
                })
                .collect(),
            ..run.clone()
        })
        .collect();
    LineLayout {
        font_size: line.font_size,
        letter_spacing: line.letter_spacing,
        width: line.width + shifts[positions.len()],
        ascent: line.ascent,
        descent: line.descent,
        runs,
        len: line.len,
    }
}
