//! Text measurements carried from one frame to the next under layout
//! retention.
//!
//! Adapted from gpui-fast's `fast/text.rs` (`TextMeasureInputs`,
//! `layout_text`, `adopt_measurement`, `update_decoration_runs`,
//! `carry_measurement`). Only text owns this: a text element tells, from its
//! text, runs and text style, whether last frame's element at the same key
//! measured text shaped the same way, and then takes that measurement over.
//! Its measurement closure is gpux's own (`TextLayout::layout`), so with
//! nothing carried it measures exactly as gpux does without fast mode.
//! Closures are still taken when the frame is cleared: what is kept is the
//! inputs and the measurement, never the closure.

use std::{borrow::Cow, cmp, hash::Hash, mem, rc::Rc, sync::Arc};

use collections::FxHashMap;

use crate::{
    App, AvailableSpace, Color, DecorationRun, FrameCache, LayoutId, Pixels, SharedString, Size,
    StrikethroughStyle, TextLayout, TextLayoutInner, TextMetrics, TextRun, TextStyle,
    UnderlineStyle, Window, WrappedLine,
};

/// Everything a text element's measurement is taken from, and the layout it
/// keeps the measurement in. The next frame's element at the same key
/// compares its own inputs with these.
pub(crate) struct TextMeasureInputs {
    text: SharedString,
    /// The runs the element was given, or none for plain text: one run in
    /// the text style.
    runs: Option<Vec<TextRun>>,
    text_style: TextStyle,
    font_size: Pixels,
    letter_spacing: Pixels,
    line_height: Pixels,
    /// The layout of the element whose inputs these are.
    pub(crate) layout: TextLayout,
}

/// What a run is painted with, which shaping also splits font runs on.
type Decoration<'a> = (
    &'a Color,
    Option<&'a Color>,
    Option<&'a UnderlineStyle>,
    Option<&'a StrikethroughStyle>,
);

fn decoration_of(run: &TextRun) -> Decoration<'_> {
    (
        &run.color,
        run.background_color.as_ref(),
        run.underline.as_ref(),
        run.strikethrough.as_ref(),
    )
}

/// What plain text in `style` is painted with, as [`decoration_of`] its run.
fn style_decoration(style: &TextStyle) -> Decoration<'_> {
    (
        &style.color,
        style.background_color.as_ref(),
        style.underline.as_ref(),
        style.strikethrough.as_ref(),
    )
}

/// Whether two text styles have the same font: `style.font() ==
/// other.font()`, without making either font.
fn same_font(style: &TextStyle, other: &TextStyle) -> bool {
    style.font_family == other.font_family
        && style.font_features == other.font_features
        && style.font_fallbacks == other.font_fallbacks
        && style.font_weight == other.font_weight
        && style.font_style == other.font_style
}

impl TextMeasureInputs {
    /// Whether the text truncates, in which case it is shaped from a
    /// rewritten string whose runs no longer line up with these.
    fn truncates(&self) -> bool {
        self.text_style.text_overflow.is_some()
    }

    /// The runs the text is shaped and painted with.
    fn runs(&self) -> Cow<'_, [TextRun]> {
        match &self.runs {
            Some(runs) => Cow::Borrowed(runs),
            None => Cow::Owned(vec![self.text_style.to_run(self.text.len())]),
        }
    }

    /// Whether `self` is shaped as `other` is: the same text, sizes, fonts,
    /// spacing and wrapping, and decoration changing in the same places,
    /// since shaping splits font runs wherever it changes. What it is painted
    /// with may differ; see [`Self::decorated_as`].
    pub(crate) fn shapes_as(&self, other: &Self) -> bool {
        fn runs(runs: &[TextRun]) -> impl Iterator<Item = &TextRun> {
            runs.iter().filter(|run| run.len > 0)
        }
        fn joins_previous<'a>(
            runs: impl Iterator<Item = &'a TextRun>,
        ) -> impl Iterator<Item = bool> {
            let mut previous = None;
            runs.map(move |run| {
                previous
                    .replace(decoration_of(run))
                    .is_some_and(|previous| previous == decoration_of(run))
            })
        }
        let (style, other_style) = (&self.text_style, &other.text_style);
        if !(self.text == other.text
            && self.font_size == other.font_size
            && self.letter_spacing == other.letter_spacing
            && self.line_height == other.line_height
            && style.white_space == other_style.white_space
            && style.line_clamp == other_style.line_clamp
            && style.text_overflow == other_style.text_overflow
            // Only truncation reads the style's own font.
            && (!self.truncates() || same_font(style, other_style)))
        {
            return false;
        }
        if self.runs.is_none() && other.runs.is_none() {
            // One run each, as long as the text, unless there is no text.
            return self.text.is_empty() || same_font(style, other_style);
        }
        let (self_runs, other_runs) = (self.runs(), other.runs());
        runs(&self_runs).count() == runs(&other_runs).count()
            && runs(&self_runs)
                .zip(runs(&other_runs))
                .all(|(run, other)| run.len == other.len && run.font == other.font)
            && joins_previous(runs(&self_runs)).eq(joins_previous(runs(&other_runs)))
    }

    /// Whether `self` is painted with what `other` is.
    pub(crate) fn decorated_as(&self, other: &Self) -> bool {
        if self.runs.is_none() && other.runs.is_none() {
            return self.text.is_empty()
                || style_decoration(&self.text_style) == style_decoration(&other.text_style);
        }
        let (self_runs, other_runs) = (self.runs(), other.runs());
        self_runs
            .iter()
            .filter(|run| run.len > 0)
            .map(decoration_of)
            .eq(other_runs
                .iter()
                .filter(|run| run.len > 0)
                .map(decoration_of))
    }
}

/// Requests the layout of a text element under layout retention, keeping its
/// measurement in `layout`: what `TextLayout::layout` does, for a node whose
/// measurement can be carried over from last frame's element at its key.
pub(crate) fn layout_text(
    layout: &TextLayout,
    text: SharedString,
    runs: Option<Vec<TextRun>>,
    window: &mut Window,
    cx: &mut App,
) -> LayoutId {
    let text_style = window.text_style();
    let font_size = text_style.font_size.to_pixels(window.rem_size());
    let letter_spacing = text_style.letter_spacing.to_pixels(window.rem_size());
    let line_height = window.pixel_snap(
        text_style
            .line_height
            .to_pixels(font_size.into(), window.rem_size()),
    );
    let inputs = TextMeasureInputs {
        text,
        runs,
        text_style,
        font_size,
        letter_spacing,
        line_height,
        layout: layout.clone(),
    };
    crate::fast::layout::request_text_node(window, Rc::new(inputs), cx)
}

/// Takes over the measurement `previous` left, if it stands for `inputs`:
/// shaped the same way, and painted with the same decorations or, for text
/// that does not truncate, repainted with this one's. Returns whether it did.
pub(crate) fn adopt_measurement(inputs: &TextMeasureInputs, previous: &TextMeasureInputs) -> bool {
    if !previous.shapes_as(inputs) {
        return false;
    }
    let recolored = !previous.decorated_as(inputs);
    if recolored && inputs.truncates() {
        return false;
    }
    let Some(mut inner) = carry_measurement(&previous.layout) else {
        return false;
    };
    if recolored {
        update_decoration_runs(&mut inner.lines, &inputs.runs());
    }
    *inputs.layout.0.borrow_mut() = Some(inner);
    true
}

/// Drops whatever measurement `inputs`' layout holds, for text measured
/// afresh: [`measure_text`] answers from a measurement it holds.
pub(crate) fn forget_measurement(inputs: &TextMeasureInputs) {
    inputs.layout.0.borrow_mut().take();
}

/// Measures text under the constraints Taffy offers, keeping the result in
/// its layout: gpux's text measurement (`TextLayout::measure`), reading its
/// inputs from `inputs`.
pub(crate) fn measure_text(
    inputs: &TextMeasureInputs,
    known_dimensions: Size<Option<Pixels>>,
    available_space: Size<AvailableSpace>,
    window: &mut Window,
    cx: &mut App,
) -> Size<Pixels> {
    inputs.layout.measure(
        &inputs.text,
        &inputs.runs(),
        &inputs.text_style,
        TextMetrics {
            font_size: inputs.font_size,
            letter_spacing: inputs.letter_spacing,
            line_height: inputs.line_height,
        },
        known_dimensions,
        available_space,
        window,
        cx,
    )
}

/// Rewrites the decorations of lines already shaped, leaving the shaping
/// alone, as `TextSystem::shape_text` would have split them. `runs` must
/// split the lines as the runs they were shaped with did; see
/// [`TextMeasureInputs::shapes_as`].
pub(crate) fn update_decoration_runs(lines: &mut [WrappedLine], runs: &[TextRun]) {
    let mut runs = runs.iter().filter(|run| run.len > 0).cloned().peekable();
    for line in lines.iter_mut() {
        let line_len = line.text.len();
        line.decoration_runs.clear();
        let mut offset = 0;
        while offset < line_len {
            let Some(run) = runs.peek_mut() else {
                log::warn!("`TextRun`s do not cover the entire shaped text");
                break;
            };
            let len_within_line = cmp::min(line_len - offset, run.len);
            if let Some(last_run) = line.decoration_runs.last_mut()
                && last_run.color == run.color
                && last_run.underline == run.underline
                && last_run.strikethrough == run.strikethrough
                && last_run.background_color == run.background_color
            {
                last_run.len += len_within_line as u32;
            } else {
                line.decoration_runs.push(DecorationRun {
                    len: len_within_line as u32,
                    color: run.color.clone(),
                    background_color: run.background_color.clone(),
                    underline: run.underline.clone(),
                    strikethrough: run.strikethrough.clone(),
                });
            }
            run.len -= len_within_line;
            if run.len == 0 {
                runs.next();
            }
            offset += len_within_line;
        }
        // Skip the `\n` that separated this line from the next.
        if let Some(run) = runs.peek_mut() {
            run.len -= 1;
            if run.len == 0 {
                runs.next();
            }
        }
    }
}

/// What the measurement kept in `layout` left, without where it was last
/// painted, for another element's layout to take over: moved out when
/// nothing else holds the layout, copied otherwise (an element a cached view
/// still shows, or a caller's clone of a `StyledText`'s layout).
fn carry_measurement(layout: &TextLayout) -> Option<TextLayoutInner> {
    if Rc::strong_count(&layout.0) == 1 {
        let mut inner = layout.0.borrow_mut().take()?;
        inner.bounds = None;
        Some(inner)
    } else {
        layout.0.borrow().as_ref().map(|inner| TextLayoutInner {
            len: inner.len,
            lines: inner
                .lines
                .iter()
                .map(|line| WrappedLine {
                    layout: line.layout.clone(),
                    text: line.text.clone(),
                    decoration_runs: line.decoration_runs.clone(),
                })
                .collect(),
            line_height: inner.line_height,
            wrap_width: inner.wrap_width,
            truncate_width: inner.truncate_width,
            size: inner.size,
            bounds: None,
        })
    }
}

/// Leaves in `previous` everything the next frame may ask for: what this
/// frame asked for, in `current`, and what it did not but something still
/// holds. `current` is left empty. Whichever of the two is larger is kept and
/// the other moved into it.
fn carry_over<K: Eq + Hash, V>(
    previous: &mut FxHashMap<Arc<K>, Arc<V>>,
    current: &mut FxHashMap<Arc<K>, Arc<V>>,
) {
    previous.retain(|_, layout| Arc::strong_count(layout) > 1);
    if previous.len() < current.len() {
        mem::swap(previous, current);
    }
    previous.extend(current.drain());
}

/// Ends a frame of the line layout cache under layout retention: what it
/// laid out, in `current`, becomes what the next frame can reuse, in
/// `previous`, as it does without fast mode, and so do the lines something
/// still holds.
///
/// Without fast mode every line the frame did not ask for is dropped. Text
/// that carries last frame's measurement holds its lines without asking the
/// cache for them, so they would be dropped, and shaped again the next time
/// that text is measured. A line nothing holds goes as before.
pub(crate) fn carry_over_line_layouts(previous: &mut FrameCache, current: &mut FrameCache) {
    // Wrapped lines hold the lines they were wrapped from, so they are swept
    // first, letting a line they were the last to hold go with them.
    carry_over(&mut previous.wrapped_lines, &mut current.wrapped_lines);
    carry_over(
        &mut previous.wrapped_lines_by_hash,
        &mut current.wrapped_lines_by_hash,
    );
    carry_over(&mut previous.lines, &mut current.lines);
    carry_over(&mut previous.lines_by_hash, &mut current.lines_by_hash);

    // The used lists index what this frame laid out.
    mem::swap(&mut previous.used_lines, &mut current.used_lines);
    mem::swap(
        &mut previous.used_wrapped_lines,
        &mut current.used_wrapped_lines,
    );
    mem::swap(
        &mut previous.used_lines_by_hash,
        &mut current.used_lines_by_hash,
    );
    mem::swap(
        &mut previous.used_wrapped_lines_by_hash,
        &mut current.used_wrapped_lines_by_hash,
    );
    current.used_lines.clear();
    current.used_wrapped_lines.clear();
    current.used_lines_by_hash.clear();
    current.used_wrapped_lines_by_hash.clear();
}
