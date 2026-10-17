use std::{ops::Range, sync::Arc};

use crate::{
    Color, FontStyle, FontWeight, Pixels, SharedString, StrikethroughStyle, UnderlineStyle,
};

/// Character formatting stored by [`super::EditableTextState`]. Every field is optional; an unset
/// field uses the element's text style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextAttributes {
    /// The font family.
    pub font_family: Option<SharedString>,
    /// The font weight.
    pub font_weight: Option<FontWeight>,
    /// The font style.
    pub font_style: Option<FontStyle>,
    /// The font size.
    pub font_size: Option<Pixels>,
    /// The spacing added after each glyph cluster.
    pub letter_spacing: Option<Pixels>,
    /// How far glyphs are raised above the baseline (negative lowers them).
    pub baseline_shift: Option<Pixels>,
    /// The text color.
    pub color: Option<Color>,
    /// The background color.
    pub background_color: Option<Color>,
    /// The underline.
    pub underline: Option<UnderlineStyle>,
    /// The strikethrough.
    pub strikethrough: Option<StrikethroughStyle>,
    /// A link target. Typing at a link's end does not extend it.
    pub link: Option<SharedString>,
    /// An opaque owner tag. Typing at a tag's end does not extend it.
    pub tag: Option<SharedString>,
}

impl TextAttributes {
    /// Whether no field is set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether any field that changes shaping is set: family, weight, style, size, letter
    /// spacing or baseline shift.
    pub fn affects_shaping(&self) -> bool {
        self.font_family.is_some()
            || self.font_weight.is_some()
            || self.font_style.is_some()
            || self.font_size.is_some()
            || self.letter_spacing.is_some()
            || self.baseline_shift.is_some()
    }

    /// These attributes without their link and tag, which typing never extends.
    pub(crate) fn without_exclusive(&self) -> Self {
        Self {
            link: None,
            tag: None,
            ..self.clone()
        }
    }
}

/// A change to one attribute field.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AttributeChange<T> {
    /// Leaves the field as it is.
    #[default]
    Keep,
    /// Sets the field.
    Set(T),
    /// Clears the field, so the element's text style applies.
    Clear,
}

impl<T: Clone> AttributeChange<T> {
    fn apply(&self, field: &mut Option<T>) {
        match self {
            AttributeChange::Keep => {}
            AttributeChange::Set(value) => *field = Some(value.clone()),
            AttributeChange::Clear => *field = None,
        }
    }

    fn is_keep(&self) -> bool {
        matches!(self, AttributeChange::Keep)
    }
}

/// Sets or clears individual [`TextAttributes`] fields over a range, leaving the others.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(missing_docs)]
pub struct TextAttributesPatch {
    pub font_family: AttributeChange<SharedString>,
    pub font_weight: AttributeChange<FontWeight>,
    pub font_style: AttributeChange<FontStyle>,
    pub font_size: AttributeChange<Pixels>,
    pub letter_spacing: AttributeChange<Pixels>,
    pub baseline_shift: AttributeChange<Pixels>,
    pub color: AttributeChange<Color>,
    pub background_color: AttributeChange<Color>,
    pub underline: AttributeChange<UnderlineStyle>,
    pub strikethrough: AttributeChange<StrikethroughStyle>,
    pub link: AttributeChange<SharedString>,
    pub tag: AttributeChange<SharedString>,
}

impl TextAttributesPatch {
    /// A patch that sets every field `attributes` sets.
    pub fn setting(attributes: &TextAttributes) -> Self {
        fn set<T: Clone>(field: &Option<T>) -> AttributeChange<T> {
            field
                .clone()
                .map_or(AttributeChange::Keep, AttributeChange::Set)
        }
        Self {
            font_family: set(&attributes.font_family),
            font_weight: set(&attributes.font_weight),
            font_style: set(&attributes.font_style),
            font_size: set(&attributes.font_size),
            letter_spacing: set(&attributes.letter_spacing),
            baseline_shift: set(&attributes.baseline_shift),
            color: set(&attributes.color),
            background_color: set(&attributes.background_color),
            underline: set(&attributes.underline),
            strikethrough: set(&attributes.strikethrough),
            link: set(&attributes.link),
            tag: set(&attributes.tag),
        }
    }

    /// Whether the patch changes nothing.
    pub fn is_empty(&self) -> bool {
        self.font_family.is_keep()
            && self.font_weight.is_keep()
            && self.font_style.is_keep()
            && self.font_size.is_keep()
            && self.letter_spacing.is_keep()
            && self.baseline_shift.is_keep()
            && self.color.is_keep()
            && self.background_color.is_keep()
            && self.underline.is_keep()
            && self.strikethrough.is_keep()
            && self.link.is_keep()
            && self.tag.is_keep()
    }

    /// Applies the patch to `attributes`.
    pub fn apply(&self, attributes: &mut TextAttributes) {
        self.font_family.apply(&mut attributes.font_family);
        self.font_weight.apply(&mut attributes.font_weight);
        self.font_style.apply(&mut attributes.font_style);
        self.font_size.apply(&mut attributes.font_size);
        self.letter_spacing.apply(&mut attributes.letter_spacing);
        self.baseline_shift.apply(&mut attributes.baseline_shift);
        self.color.apply(&mut attributes.color);
        self.background_color
            .apply(&mut attributes.background_color);
        self.underline.apply(&mut attributes.underline);
        self.strikethrough.apply(&mut attributes.strikethrough);
        self.link.apply(&mut attributes.link);
        self.tag.apply(&mut attributes.tag);
    }
}

/// Whether a field is on across a whole range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeState {
    /// Every character in the range has it.
    On,
    /// No character in the range has it.
    Off,
    /// Some characters have it and some do not, or they differ.
    Mixed,
}

/// One stored formatting span over a UTF-8 byte range.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeSpan {
    /// The byte range.
    pub range: Range<usize>,
    /// The attributes of every character in the range.
    pub attributes: Arc<TextAttributes>,
}

impl AttributeSpan {
    /// Creates a span.
    pub fn new(range: Range<usize>, attributes: TextAttributes) -> Self {
        Self {
            range,
            attributes: Arc::new(attributes),
        }
    }
}

/// Sorted, non-overlapping, normalized attribute spans: no empty spans, no spans without
/// attributes, and no two adjacent spans with equal attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributeSpans {
    spans: Vec<AttributeSpan>,
}

impl AttributeSpans {
    /// Builds a normalized set from arbitrary spans; later spans win where they overlap.
    pub fn from_spans(spans: impl IntoIterator<Item = AttributeSpan>, len: usize) -> Self {
        let mut set = Self::default();
        for span in spans {
            let range = span.range.start.min(len)..span.range.end.min(len);
            set.assign(range, span.attributes);
        }
        set
    }

    /// The spans, in order.
    pub fn spans(&self) -> &[AttributeSpan] {
        &self.spans
    }

    /// Whether no character has attributes.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The attributes of the character starting at `offset`, if any are stored.
    pub fn at(&self, offset: usize) -> Option<&Arc<TextAttributes>> {
        let index = self.spans.partition_point(|span| span.range.end <= offset);
        self.spans
            .get(index)
            .filter(|span| span.range.start <= offset)
            .map(|span| &span.attributes)
    }

    /// The spans overlapping `range`, clipped to it.
    pub fn clipped(&self, range: Range<usize>) -> impl Iterator<Item = AttributeSpan> + '_ {
        let first = self
            .spans
            .partition_point(|span| span.range.end <= range.start);
        self.spans[first..]
            .iter()
            .take_while(move |span| span.range.start < range.end)
            .filter_map(move |span| {
                let start = span.range.start.max(range.start);
                let end = span.range.end.min(range.end);
                (start < end).then(|| AttributeSpan {
                    range: start..end,
                    attributes: span.attributes.clone(),
                })
            })
    }

    /// Replaces the attributes of `range` with `attributes`.
    pub fn assign(&mut self, range: Range<usize>, attributes: Arc<TextAttributes>) {
        if range.is_empty() {
            return;
        }
        self.cut(range.clone());
        if !attributes.is_empty() {
            let index = self
                .spans
                .partition_point(|span| span.range.end <= range.start);
            self.spans
                .insert(index, AttributeSpan { range, attributes });
        }
        self.normalize();
    }

    /// Applies `patch` to every character in `range`, including those without attributes.
    pub fn patch(&mut self, range: Range<usize>, patch: &TextAttributesPatch) {
        if range.is_empty() || patch.is_empty() {
            return;
        }
        let mut pieces = Vec::new();
        let mut cursor = range.start;
        for span in self.clipped(range.clone()).collect::<Vec<_>>() {
            if cursor < span.range.start {
                let mut attributes = TextAttributes::default();
                patch.apply(&mut attributes);
                pieces.push((cursor..span.range.start, attributes));
            }
            let mut attributes = (*span.attributes).clone();
            patch.apply(&mut attributes);
            cursor = span.range.end;
            pieces.push((span.range, attributes));
        }
        if cursor < range.end {
            let mut attributes = TextAttributes::default();
            patch.apply(&mut attributes);
            pieces.push((cursor..range.end, attributes));
        }
        self.cut(range);
        for (range, attributes) in pieces {
            if !attributes.is_empty() {
                let index = self
                    .spans
                    .partition_point(|span| span.range.end <= range.start);
                self.spans.insert(
                    index,
                    AttributeSpan {
                        range,
                        attributes: Arc::new(attributes),
                    },
                );
            }
        }
        self.normalize();
    }

    /// Moves spans through a replacement of `range` by `replacement_len` bytes: spans before it
    /// stay, spans after it shift, spans it covers shrink or vanish, and a span containing it
    /// grows with it. The inserted bytes then take `inserted` (no attributes clears them).
    pub fn remap(
        &mut self,
        range: Range<usize>,
        replacement_len: usize,
        inserted: &Arc<TextAttributes>,
    ) {
        let (start, end) = (range.start, range.end);
        let shift = |offset: usize| offset - (end - start) + replacement_len;
        for span in &mut self.spans {
            let (a, b) = (span.range.start, span.range.end);
            if b <= start {
                continue;
            }
            if a >= end {
                span.range = shift(a)..shift(b);
                continue;
            }
            let new_start = if a < start {
                a
            } else {
                start + replacement_len
            };
            let new_end = if b > end { shift(b) } else { start };
            span.range = new_start..new_end.max(new_start);
        }
        self.spans.retain(|span| !span.range.is_empty());
        self.assign(start..start + replacement_len, inserted.clone());
        self.normalize();
    }

    fn cut(&mut self, range: Range<usize>) {
        let mut kept = Vec::with_capacity(self.spans.len() + 1);
        for span in self.spans.drain(..) {
            if span.range.end <= range.start || span.range.start >= range.end {
                kept.push(span);
                continue;
            }
            if span.range.start < range.start {
                kept.push(AttributeSpan {
                    range: span.range.start..range.start,
                    attributes: span.attributes.clone(),
                });
            }
            if span.range.end > range.end {
                kept.push(AttributeSpan {
                    range: range.end..span.range.end,
                    attributes: span.attributes,
                });
            }
        }
        self.spans = kept;
    }

    fn normalize(&mut self) {
        self.spans
            .retain(|span| !span.range.is_empty() && !span.attributes.is_empty());
        let mut merged: Vec<AttributeSpan> = Vec::with_capacity(self.spans.len());
        for span in self.spans.drain(..) {
            if let Some(last) = merged.last_mut()
                && last.range.end == span.range.start
                && (Arc::ptr_eq(&last.attributes, &span.attributes)
                    || last.attributes == span.attributes)
            {
                last.range.end = span.range.end;
                continue;
            }
            merged.push(span);
        }
        self.spans = merged;
    }
}

/// A snapshot of an editable field's text and everything stored with it, for owners to
/// serialize and for the clipboard codec.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributedText {
    /// The text.
    pub text: String,
    /// Character formatting over byte ranges of `text`, sorted and non-overlapping.
    pub spans: Vec<AttributeSpan>,
}

impl AttributedText {
    /// Text without any formatting.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }
}

/// Everything an editable field stores beside its text. History entries share it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Formatting {
    pub spans: AttributeSpans,
}

impl Formatting {
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}
