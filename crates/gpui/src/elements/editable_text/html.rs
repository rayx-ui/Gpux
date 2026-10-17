//! Converts [`AttributedText`] to an HTML subset and back, for the rich clipboard.
//!
//! Serialization writes paragraphs (`<p>` with alignment, indents, spacing, line height and
//! letter spacing as inline CSS), lists (`<ul>`, `<ol start>` and `<li>` nested by level;
//! checklists marked `data-gpux-checklist` with `data-checked` items), character formatting
//! (`<b>`, `<i>`, `<u>`, `<s>`, `<sup>`, `<sub>`, `<a href>` and `<span style>`), inline objects
//! (their trusted HTML fallback, else their escaped text fallback) and escaped text.
//!
//! Parsing accepts that output, so a round trip is equal, and the common subset browsers and
//! office suites write. It never fetches, loads or runs anything: it tokenizes with html5ever and
//! maps known tags, dropping `script`, `style`, `head`, `title` and comments and decoding
//! entities. Input above [`MAX_HTML_LEN`] is refused, so callers paste the plain text instead.

use std::{cell::RefCell, fmt::Write as _};

use html5ever::{
    Attribute,
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
    },
};

use crate::{Color, FontStyle, FontWeight, Pixels, Rgba, StrikethroughStyle, UnderlineStyle, px};

use super::{
    AttributeSpan, AttributeSpans, AttributedText, OBJECT_REPLACEMENT_CHARACTER,
    ParagraphAlignment, ParagraphFormat, TextAttributes, TextListItem, TextListKind,
};

/// The largest HTML input [`attributed_text_from_html`] parses: 4 MiB.
pub const MAX_HTML_LEN: usize = 4 * 1024 * 1024;

/// Heading sizes `h1` to `h6`, as multiples of the base size.
const HEADING_SCALES: [f32; 6] = [2., 1.5, 1.17, 1., 0.83, 0.67];

/// Serializes `text` as HTML.
pub fn attributed_text_to_html(text: &AttributedText) -> String {
    let mut html = String::from("<div data-gpux style=\"white-space:pre-wrap\">");
    let spans = &text.spans;
    let mut open_lists: Vec<(ListTag, u8)> = Vec::new();
    let mut paragraph_start = 0;
    for (index, paragraph) in text.text.split('\n').enumerate() {
        let range = paragraph_start..paragraph_start + paragraph.len();
        paragraph_start = range.end + 1;
        let format = text.paragraphs.get(index).cloned().unwrap_or_default();
        let list = format.list.clone();
        // Close lists deeper than this item, or all of them for a plain paragraph.
        let depth = list.as_ref().map_or(0, |item| usize::from(item.level) + 1);
        while open_lists.len() > depth
            || open_lists.last().is_some_and(|(tag, level)| {
                list.as_ref().is_some_and(|item| {
                    usize::from(*level) + 1 == depth && *tag != ListTag::of(&item.kind)
                })
            })
        {
            let (tag, _) = open_lists.pop().unwrap();
            html.push_str(tag.close());
        }
        if let Some(item) = &list {
            while open_lists.len() < depth {
                let tag = ListTag::of(&item.kind);
                html.push_str(&tag.open(&item.kind));
                open_lists.push((tag, open_lists.len() as u8));
            }
            html.push_str("<li");
            if let TextListKind::Checkbox { checked } = item.kind {
                let _ = write!(html, " data-checked=\"{checked}\"");
            }
            if let TextListKind::Custom(marker) = &item.kind {
                let _ = write!(html, " data-gpux-marker=\"{}\"", escape(marker));
            }
        } else {
            html.push_str("<p");
        }
        let style = paragraph_style(&format);
        if !style.is_empty() {
            let _ = write!(html, " style=\"{}\"", escape_attribute(&style));
        }
        html.push('>');
        write_runs(&mut html, &text.text, range, spans);
        html.push_str(if list.is_some() { "</li>" } else { "</p>" });
    }
    while let Some((tag, _)) = open_lists.pop() {
        html.push_str(tag.close());
    }
    html.push_str("</div>");
    html
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListTag {
    Unordered,
    Ordered,
    Checklist,
}

impl ListTag {
    fn of(kind: &TextListKind) -> Self {
        match kind {
            TextListKind::Numbered { .. } => ListTag::Ordered,
            TextListKind::Checkbox { .. } => ListTag::Checklist,
            TextListKind::Bullet | TextListKind::Custom(_) => ListTag::Unordered,
        }
    }

    fn open(self, kind: &TextListKind) -> String {
        match (self, kind) {
            (ListTag::Ordered, TextListKind::Numbered { start }) if *start != 1 => {
                format!("<ol start=\"{start}\">")
            }
            (ListTag::Ordered, _) => "<ol>".into(),
            (ListTag::Checklist, _) => "<ul data-gpux-checklist>".into(),
            (ListTag::Unordered, _) => "<ul>".into(),
        }
    }

    fn close(self) -> &'static str {
        match self {
            ListTag::Ordered => "</ol>",
            ListTag::Unordered | ListTag::Checklist => "</ul>",
        }
    }
}

fn paragraph_style(format: &ParagraphFormat) -> String {
    let mut style = String::new();
    if let Some(alignment) = format.alignment {
        let value = match alignment {
            ParagraphAlignment::Left => "left",
            ParagraphAlignment::Center => "center",
            ParagraphAlignment::Right => "right",
            ParagraphAlignment::Justify => "justify",
        };
        let _ = write!(style, "text-align:{value};");
    }
    for (property, value) in [
        ("text-indent", format.first_line_indent),
        ("margin-left", format.leading_indent),
        ("margin-right", format.trailing_indent),
        ("margin-top", format.space_before),
        ("margin-bottom", format.space_after),
    ] {
        if value != px(0.) {
            let _ = write!(style, "{property}:{}px;", f32::from(value));
        }
    }
    if let Some(multiplier) = format.line_height {
        let _ = write!(style, "line-height:{multiplier};");
    }
    if let Some(spacing) = format.letter_spacing {
        let _ = write!(style, "letter-spacing:{}px;", f32::from(spacing));
    }
    style
}

/// Writes the text of `range` with its formatting.
fn write_runs(
    html: &mut String,
    text: &str,
    range: std::ops::Range<usize>,
    spans: &[AttributeSpan],
) {
    let mut offset = range.start;
    let in_range = spans
        .iter()
        .filter(|span| span.range.end > range.start && span.range.start < range.end);
    for span in in_range {
        let start = span.range.start.max(range.start);
        let end = span.range.end.min(range.end);
        if offset < start {
            html.push_str(&escape(&text[offset..start]));
        }
        write_formatted(html, &text[start..end], &span.attributes);
        offset = end;
    }
    if offset < range.end {
        html.push_str(&escape(&text[offset..range.end]));
    }
}

fn write_formatted(html: &mut String, text: &str, attributes: &TextAttributes) {
    let mut closers: Vec<&'static str> = Vec::new();
    let mut open = |html: &mut String, tag: &'static str, attributes: String| {
        let _ = write!(html, "<{tag}{attributes}>");
        closers.push(tag);
    };
    if let Some(link) = &attributes.link {
        open(html, "a", format!(" href=\"{}\"", escape(link)));
    }
    if attributes.font_weight == Some(FontWeight::BOLD) {
        open(html, "b", String::new());
    }
    if attributes.font_style == Some(FontStyle::Italic) {
        open(html, "i", String::new());
    }
    if attributes
        .underline
        .as_ref()
        .is_some_and(|underline| *underline == default_underline())
    {
        open(html, "u", String::new());
    }
    if attributes
        .strikethrough
        .as_ref()
        .is_some_and(|strikethrough| *strikethrough == default_strikethrough())
    {
        open(html, "s", String::new());
    }
    match attributes.baseline_shift {
        Some(shift) if shift > px(0.) => open(
            html,
            "sup",
            format!(" style=\"vertical-align:{}px\"", f32::from(shift)),
        ),
        Some(shift) if shift < px(0.) => open(
            html,
            "sub",
            format!(" style=\"vertical-align:{}px\"", f32::from(shift)),
        ),
        _ => {}
    }
    let style = span_style(attributes);
    let mut span_attributes = String::new();
    if !style.is_empty() {
        let _ = write!(span_attributes, " style=\"{}\"", escape_attribute(&style));
    }
    if let Some(tag) = &attributes.tag {
        let _ = write!(span_attributes, " data-gpux-tag=\"{}\"", escape(tag));
    }
    if !span_attributes.is_empty() {
        open(html, "span", span_attributes);
    }
    match &attributes.object {
        Some(object) => {
            for _ in text.matches(OBJECT_REPLACEMENT_CHARACTER) {
                match &object.html_fallback {
                    Some(fallback) => html.push_str(fallback),
                    None => html.push_str(&escape(&object.text_fallback)),
                }
            }
        }
        None => html.push_str(&escape(text)),
    }
    for tag in closers.into_iter().rev() {
        let _ = write!(html, "</{tag}>");
    }
}

fn default_underline() -> UnderlineStyle {
    UnderlineStyle {
        thickness: px(1.),
        color: None,
        wavy: false,
    }
}

fn default_strikethrough() -> StrikethroughStyle {
    StrikethroughStyle {
        thickness: px(1.),
        color: None,
    }
}

fn span_style(attributes: &TextAttributes) -> String {
    let mut style = String::new();
    if let Some(family) = &attributes.font_family {
        let _ = write!(style, "font-family:'{}';", css_family(family));
    }
    if let Some(weight) = attributes.font_weight
        && weight != FontWeight::BOLD
    {
        let _ = write!(style, "font-weight:{};", weight.0);
    }
    if let Some(font_style) = attributes.font_style
        && font_style != FontStyle::Italic
    {
        let value = match font_style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        };
        let _ = write!(style, "font-style:{value};");
    }
    if let Some(size) = attributes.font_size {
        let _ = write!(style, "font-size:{}px;", f32::from(size));
    }
    if let Some(spacing) = attributes.letter_spacing {
        let _ = write!(style, "letter-spacing:{}px;", f32::from(spacing));
    }
    if attributes.baseline_shift == Some(px(0.)) {
        style.push_str("vertical-align:0px;");
    }
    if let Some(color) = &attributes.color {
        let _ = write!(style, "color:{};", css_color(color));
    }
    if let Some(color) = &attributes.background_color {
        let _ = write!(style, "background-color:{};", css_color(color));
    }
    if let Some(underline) = &attributes.underline
        && *underline != default_underline()
    {
        let _ = write!(
            style,
            "text-decoration-line:underline;text-decoration-thickness:{}px;",
            f32::from(underline.thickness)
        );
        if underline.wavy {
            style.push_str("text-decoration-style:wavy;");
        }
        if let Some(color) = &underline.color {
            let _ = write!(style, "text-decoration-color:{};", css_color(color));
        }
    }
    if let Some(strikethrough) = &attributes.strikethrough
        && *strikethrough != default_strikethrough()
    {
        let _ = write!(
            style,
            "--gpux-strikethrough-thickness:{}px;",
            f32::from(strikethrough.thickness)
        );
        if let Some(color) = &strikethrough.color {
            let _ = write!(style, "--gpux-strikethrough-color:{};", css_color(color));
        }
    }
    style
}

fn css_color(color: &Color) -> String {
    let rgba = Rgba::from(color.clone());
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    let (r, g, b, a) = (
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b),
        channel(rgba.a),
    );
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// [`escape`] for a double-quoted attribute value, keeping the single quotes CSS uses.
fn escape_attribute(text: &str) -> String {
    escape(text).replace("&#39;", "'")
}

/// A font family as a quoted CSS string can carry: no quotes, backslashes, declaration or
/// block delimiters, angle brackets or control characters.
fn css_family(family: &str) -> String {
    family
        .chars()
        .filter(|character| {
            !matches!(character, '\'' | '"' | '\\' | ';' | '{' | '}' | '<' | '>')
                && !character.is_control()
        })
        .collect()
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            character => escaped.push(character),
        }
    }
    escaped
}

/// Parses HTML into attributed text. `base_font_size` scales headings and relative sizes.
/// Returns `None` for input above [`MAX_HTML_LEN`], which callers paste as plain text.
pub fn attributed_text_from_html(html: &str, base_font_size: Pixels) -> Option<AttributedText> {
    if html.len() > MAX_HTML_LEN {
        return None;
    }
    let sink = HtmlSink {
        state: RefCell::new(ParseState::new(base_font_size)),
    };
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    Some(tokenizer.sink.state.into_inner().finish())
}

struct HtmlSink {
    state: RefCell<ParseState>,
}

impl TokenSink for HtmlSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        let mut state = self.state.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let name = tag.name.to_ascii_lowercase();
                match tag.kind {
                    TagKind::StartTag => {
                        let raw = state.start_tag(&name, &tag.attrs, tag.self_closing);
                        if let Some(kind) = raw {
                            return TokenSinkResult::RawData(kind);
                        }
                    }
                    TagKind::EndTag => state.end_tag(&name),
                }
            }
            Token::CharacterTokens(text) => state.characters(&text),
            Token::NullCharacterToken
            | Token::CommentToken(_)
            | Token::DoctypeToken(_)
            | Token::EOFToken
            | Token::ParseError(_) => {}
        }
        TokenSinkResult::Continue
    }
}

/// What a start tag pushed, so its end tag pops it.
struct OpenElement {
    name: String,
    attributes: TextAttributes,
    paragraph: Option<ParagraphFormat>,
    preserves_whitespace: bool,
}

struct ParseState {
    base_font_size: Pixels,
    text: String,
    spans: Vec<AttributeSpan>,
    paragraphs: Vec<ParagraphFormat>,
    stack: Vec<OpenElement>,
    /// Lists by depth: their tag, and the item counter of ordered lists.
    lists: Vec<(ListTag, u32)>,
    /// Inside dropped content (`script`, `style`, `head`, `title`): its tag.
    dropping: Option<String>,
    /// A paragraph break is due before the next text.
    pending_break: bool,
    /// The stack depth of the paragraph element (`p`, `li`, a heading) that asked for the
    /// pending break, which an empty element of its own still produces when it closes.
    pending_owner: Option<usize>,
    /// The format the next paragraph starts with.
    pending_paragraph: ParagraphFormat,
    paragraph_has_text: bool,
}

impl ParseState {
    fn new(base_font_size: Pixels) -> Self {
        Self {
            base_font_size,
            text: String::new(),
            spans: Vec::new(),
            paragraphs: vec![ParagraphFormat::default()],
            stack: Vec::new(),
            lists: Vec::new(),
            dropping: None,
            pending_break: false,
            pending_owner: None,
            pending_paragraph: ParagraphFormat::default(),
            paragraph_has_text: false,
        }
    }

    fn attributes(&self) -> TextAttributes {
        self.stack
            .last()
            .map(|element| element.attributes.clone())
            .unwrap_or_default()
    }

    fn preserves_whitespace(&self) -> bool {
        self.stack
            .iter()
            .any(|element| element.preserves_whitespace)
    }

    /// Starts a new paragraph with `format` before the next text, unless the current one is
    /// still empty, in which case it takes `format` itself.
    fn block(&mut self, format: ParagraphFormat) {
        if self.paragraph_has_text || self.pending_break {
            self.pending_break = true;
            self.pending_paragraph = format;
        } else if let Some(current) = self.paragraphs.last_mut() {
            *current = format;
        }
    }

    fn start_tag(
        &mut self,
        name: &str,
        attributes: &[Attribute],
        self_closing: bool,
    ) -> Option<html5ever::tokenizer::states::RawKind> {
        use html5ever::tokenizer::states::RawKind;
        let raw_kind = |name: &str| match name {
            "script" => Some(RawKind::ScriptData),
            "style" | "noscript" | "iframe" => Some(RawKind::Rawtext),
            "title" => Some(RawKind::Rcdata),
            _ => None,
        };
        if let Some(dropping) = &self.dropping {
            // The head ends where body content starts, whether or not `</head>` came; inside
            // it, scripts and styles stay raw text so their content cannot end it.
            let head_content = matches!(
                name,
                "meta" | "link" | "title" | "style" | "script" | "base" | "noscript" | "template"
            );
            if dropping != "head" || head_content {
                return if self_closing { None } else { raw_kind(name) };
            }
            self.dropping = None;
        }
        let attribute = |key: &str| {
            attributes
                .iter()
                .find(|attribute| &*attribute.name.local == key)
                .map(|attribute| attribute.value.to_string())
        };
        match name {
            "script" | "style" | "head" | "title" | "template" | "noscript" | "iframe" => {
                if !self_closing {
                    self.dropping = Some(name.to_owned());
                }
                return raw_kind(name);
            }
            "br" => {
                self.newline();
                return None;
            }
            "img" => {
                if let Some(alt) = attribute("alt") {
                    self.push_text(&alt, true);
                }
                return None;
            }
            _ => {}
        }
        if self_closing {
            return None;
        }
        let style = attribute("style").unwrap_or_default();
        let mut text_attributes = self.attributes();
        let mut paragraph = None;
        let base = self.base_font_size;
        match name {
            "b" | "strong" => text_attributes.font_weight = Some(FontWeight::BOLD),
            "i" | "em" => text_attributes.font_style = Some(FontStyle::Italic),
            "u" => text_attributes.underline = Some(default_underline()),
            "s" | "strike" | "del" => text_attributes.strikethrough = Some(default_strikethrough()),
            "sup" => text_attributes.baseline_shift = Some(base * 0.33),
            "sub" => text_attributes.baseline_shift = Some(-(base * 0.2)),
            "a" => {
                if let Some(href) = attribute("href") {
                    text_attributes.link = Some(href.into());
                }
            }
            "font" => {
                if let Some(color) = attribute("color").and_then(|value| parse_color(&value)) {
                    text_attributes.color = Some(color);
                }
                if let Some(face) = attribute("face") {
                    text_attributes.font_family = Some(first_family(&face).into());
                }
                if let Some(size) =
                    attribute("size").and_then(|size| size.trim().parse::<usize>().ok())
                {
                    let scale = [0.625, 0.8125, 1., 1.125, 1.5, 2., 3.][size.clamp(1, 7) - 1];
                    text_attributes.font_size = Some(base * scale);
                }
            }
            "p" | "div" | "blockquote" | "pre" => {
                paragraph = Some(self.paragraph_format(&style));
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = usize::from(name.as_bytes()[1] - b'1');
                text_attributes.font_weight = Some(FontWeight::BOLD);
                text_attributes.font_size = Some(base * HEADING_SCALES[level]);
                paragraph = Some(self.paragraph_format(&style));
            }
            "ul" | "ol" => {
                let tag = if name == "ol" {
                    ListTag::Ordered
                } else if attribute("data-gpux-checklist").is_some() {
                    ListTag::Checklist
                } else {
                    ListTag::Unordered
                };
                let start = attribute("start")
                    .and_then(|start| start.trim().parse().ok())
                    .unwrap_or(1);
                self.lists.push((tag, start));
            }
            "li" => {
                let level = self.lists.len().saturating_sub(1).min(u8::MAX as usize) as u8;
                let (tag, start) = self
                    .lists
                    .last()
                    .copied()
                    .unwrap_or((ListTag::Unordered, 1));
                let kind = match tag {
                    ListTag::Ordered => TextListKind::Numbered { start },
                    ListTag::Checklist => TextListKind::Checkbox {
                        checked: attribute("data-checked").is_some_and(|value| value == "true"),
                    },
                    ListTag::Unordered => match attribute("data-gpux-marker") {
                        Some(marker) => TextListKind::Custom(marker.into()),
                        None => TextListKind::Bullet,
                    },
                };
                // Only the first item of a list keeps its start; later ones continue it.
                if let Some((_, start)) = self.lists.last_mut() {
                    *start = 1;
                }
                let mut format = self.paragraph_format(&style);
                format.list = Some(TextListItem { kind, level });
                paragraph = Some(format);
            }
            _ => {}
        }
        // A block's letter spacing is its paragraph's default, not its characters'.
        let character_style = if paragraph.is_some() {
            declarations(&style)
                .filter(|(property, _)| property != "letter-spacing")
                .map(|(property, value)| format!("{property}:{value}"))
                .collect::<Vec<_>>()
                .join(";")
        } else {
            style.clone()
        };
        apply_span_style(&mut text_attributes, &character_style, base);
        if let Some(tag) = attribute("data-gpux-tag") {
            text_attributes.tag = Some(tag.into());
        }
        let preserves_whitespace = name == "pre"
            || style
                .split(';')
                .any(|declaration| declaration.replace(' ', "").starts_with("white-space:pre"));
        if let Some(format) = &paragraph {
            self.block(format.clone());
            let leaf = matches!(name, "p" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
            self.pending_owner = (leaf && self.pending_break).then_some(self.stack.len());
        }
        self.stack.push(OpenElement {
            name: name.to_owned(),
            attributes: text_attributes,
            paragraph,
            preserves_whitespace,
        });
        None
    }

    fn end_tag(&mut self, name: &str) {
        if let Some(dropping) = &self.dropping {
            if dropping == name {
                self.dropping = None;
            }
            return;
        }
        if name == "ul" || name == "ol" {
            self.lists.pop();
        }
        let Some(index) = self.stack.iter().rposition(|element| element.name == name) else {
            return;
        };
        // An empty paragraph element is still a paragraph.
        if self.pending_break && self.pending_owner == Some(index) {
            self.start_paragraph();
        }
        let closed = self.stack.split_off(index);
        // A closed paragraph element occupies its paragraph, even when empty.
        if closed.iter().any(|element| {
            matches!(
                element.name.as_str(),
                "p" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            )
        }) {
            self.paragraph_has_text = true;
        }
        if closed.iter().any(|element| element.paragraph.is_some()) {
            // Text after a block starts a new paragraph in the enclosing block's format.
            let enclosing = self
                .stack
                .iter()
                .rev()
                .find_map(|element| element.paragraph.clone())
                .unwrap_or_default();
            self.block(enclosing);
        }
    }

    fn paragraph_format(&self, style: &str) -> ParagraphFormat {
        let mut format = ParagraphFormat::default();
        for (property, value) in declarations(style) {
            match property.as_str() {
                "text-align" => {
                    format.alignment = match value.as_str() {
                        "left" | "start" => Some(ParagraphAlignment::Left),
                        "center" => Some(ParagraphAlignment::Center),
                        "right" | "end" => Some(ParagraphAlignment::Right),
                        "justify" => Some(ParagraphAlignment::Justify),
                        _ => None,
                    }
                }
                "text-indent" => format.first_line_indent = length(&value, self.base_font_size),
                "margin-left" | "padding-left" => {
                    format.leading_indent = length(&value, self.base_font_size)
                }
                "margin-right" => format.trailing_indent = length(&value, self.base_font_size),
                "margin-top" => format.space_before = length(&value, self.base_font_size),
                "margin-bottom" => format.space_after = length(&value, self.base_font_size),
                "line-height" => {
                    format.line_height = value.trim().parse::<f32>().ok().filter(|m| *m > 0.)
                }
                "letter-spacing" => {
                    format.letter_spacing = Some(length(&value, self.base_font_size))
                }
                _ => {}
            }
        }
        format
    }

    fn newline(&mut self) {
        if self.pending_break {
            self.start_paragraph();
        }
        self.text.push('\n');
        let format = self.paragraphs.last().cloned().unwrap_or_default();
        self.paragraphs.push(format);
        self.paragraph_has_text = false;
    }

    fn start_paragraph(&mut self) {
        self.pending_break = false;
        self.pending_owner = None;
        self.text.push('\n');
        self.paragraphs
            .push(std::mem::take(&mut self.pending_paragraph));
        self.paragraph_has_text = false;
    }

    fn characters(&mut self, text: &str) {
        if self.dropping.is_some() {
            return;
        }
        if self.preserves_whitespace() {
            self.push_text(text, false);
            return;
        }
        // Outside preformatted content, runs of whitespace are one space, and a paragraph
        // starts at its first word.
        let mut collapsed = String::with_capacity(text.len());
        let mut last_space =
            self.pending_break || self.text.ends_with(' ') || !self.paragraph_has_text;
        for character in text.chars() {
            // HTML whitespace; a no-break space is text.
            if matches!(character, ' ' | '\t' | '\n' | '\r' | '\x0c') {
                if !last_space {
                    collapsed.push(' ');
                    last_space = true;
                }
            } else {
                collapsed.push(character);
                last_space = false;
            }
        }
        if !collapsed.is_empty() {
            self.push_text(&collapsed, false);
        }
    }

    fn push_text(&mut self, text: &str, _replaced: bool) {
        if text.is_empty() {
            return;
        }
        if self.pending_break {
            self.start_paragraph();
        }
        let start = self.text.len();
        self.text.push_str(&text.replace(['\r', '\n'], " "));
        let attributes = self.attributes();
        if !attributes.is_empty() {
            self.spans
                .push(AttributeSpan::new(start..self.text.len(), attributes));
        }
        self.paragraph_has_text = true;
    }

    fn finish(self) -> AttributedText {
        let len = self.text.len();
        let spans = AttributeSpans::from_spans(self.spans, len).spans().to_vec();
        let paragraphs = if self.paragraphs.iter().all(ParagraphFormat::is_default) {
            Vec::new()
        } else {
            self.paragraphs
        };
        AttributedText {
            text: self.text,
            spans,
            paragraphs,
        }
    }
}

/// The `property: value` declarations of an inline style, lowercased.
fn declarations(style: &str) -> impl Iterator<Item = (String, String)> + '_ {
    style.split(';').filter_map(|declaration| {
        let (property, value) = declaration.split_once(':')?;
        Some((
            property.trim().to_ascii_lowercase(),
            value
                .trim()
                .trim_end_matches("!important")
                .trim()
                .to_owned(),
        ))
    })
}

fn apply_span_style(attributes: &mut TextAttributes, style: &str, base: Pixels) {
    for (property, value) in declarations(style) {
        let lower = value.to_ascii_lowercase();
        match property.as_str() {
            "font-family" => attributes.font_family = Some(first_family(&value).into()),
            "font-weight" => {
                attributes.font_weight = match lower.as_str() {
                    "bold" | "bolder" => Some(FontWeight::BOLD),
                    "normal" | "lighter" => Some(FontWeight::NORMAL),
                    number => number.parse::<f32>().ok().map(FontWeight),
                }
            }
            "font-style" => {
                attributes.font_style = match lower.as_str() {
                    "italic" => Some(FontStyle::Italic),
                    "oblique" => Some(FontStyle::Oblique),
                    "normal" => Some(FontStyle::Normal),
                    _ => attributes.font_style,
                }
            }
            "font-size" => {
                if let Some(size) = font_size(&lower, base) {
                    attributes.font_size = Some(size);
                }
            }
            "letter-spacing" => attributes.letter_spacing = Some(length(&lower, base)),
            "vertical-align" => match lower.as_str() {
                "super" => attributes.baseline_shift = Some(base * 0.33),
                "sub" => attributes.baseline_shift = Some(-(base * 0.2)),
                "baseline" => attributes.baseline_shift = None,
                value => {
                    if value.ends_with("px") {
                        attributes.baseline_shift = Some(length(value, base));
                    }
                }
            },
            "color" => {
                if let Some(color) = parse_color(&lower) {
                    attributes.color = Some(color);
                }
            }
            "background-color" | "background" => {
                if let Some(color) = parse_color(&lower) {
                    attributes.background_color = Some(color);
                }
            }
            "text-decoration" | "text-decoration-line" => {
                if lower.contains("underline") {
                    attributes.underline.get_or_insert_with(default_underline);
                }
                if lower.contains("line-through") {
                    attributes
                        .strikethrough
                        .get_or_insert_with(default_strikethrough);
                }
            }
            "text-decoration-thickness" => {
                if let Some(underline) = &mut attributes.underline {
                    underline.thickness = length(&lower, base);
                }
            }
            "text-decoration-style" => {
                if let Some(underline) = &mut attributes.underline {
                    underline.wavy = lower == "wavy";
                }
            }
            "text-decoration-color" => {
                if let Some(underline) = &mut attributes.underline {
                    underline.color = parse_color(&lower);
                }
            }
            "--gpux-strikethrough-thickness" => {
                attributes
                    .strikethrough
                    .get_or_insert_with(default_strikethrough)
                    .thickness = length(&lower, base);
            }
            "--gpux-strikethrough-color" => {
                attributes
                    .strikethrough
                    .get_or_insert_with(default_strikethrough)
                    .color = parse_color(&lower);
            }
            _ => {}
        }
    }
}

fn first_family(value: &str) -> String {
    css_family(value.split(',').next().unwrap_or_default().trim())
}

/// A CSS length in pixels: `px`, `pt`, `em` or `rem` (of `base`), or a bare number of pixels.
fn length(value: &str, base: Pixels) -> Pixels {
    let value = value.trim();
    let number = |suffix: &str| value.strip_suffix(suffix)?.trim().parse::<f32>().ok();
    if let Some(pixels) = number("px") {
        px(pixels)
    } else if let Some(points) = number("pt") {
        px(points * 4. / 3.)
    } else if let Some(rems) = number("rem") {
        base * rems
    } else if let Some(ems) = number("em") {
        base * ems
    } else {
        px(value.parse::<f32>().unwrap_or(0.))
    }
}

fn font_size(value: &str, base: Pixels) -> Option<Pixels> {
    let scale = match value {
        "xx-small" => 0.5625,
        "x-small" => 0.625,
        "small" => 0.8125,
        "medium" => 1.,
        "large" => 1.125,
        "x-large" => 1.5,
        "xx-large" => 2.,
        value if value.ends_with('%') => value.trim_end_matches('%').parse::<f32>().ok()? / 100.,
        value => {
            let size = length(value, base);
            return (size > px(0.)).then_some(size);
        }
    };
    Some(base * scale)
}

fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim().to_ascii_lowercase();
    let rgba = |r: f32, g: f32, b: f32, a: f32| -> Color {
        Rgba {
            r: r / 255.,
            g: g / 255.,
            b: b / 255.,
            a,
        }
        .into()
    };
    if let Some(hex) = value.strip_prefix('#') {
        let digit = |index: usize, width: usize| {
            u8::from_str_radix(hex.get(index..index + width)?, 16)
                .ok()
                .map(|value| {
                    if width == 1 {
                        f32::from(value * 17)
                    } else {
                        f32::from(value)
                    }
                })
        };
        return match hex.len() {
            3 => Some(rgba(digit(0, 1)?, digit(1, 1)?, digit(2, 1)?, 1.)),
            6 => Some(rgba(digit(0, 2)?, digit(2, 2)?, digit(4, 2)?, 1.)),
            8 => Some(rgba(
                digit(0, 2)?,
                digit(2, 2)?,
                digit(4, 2)?,
                digit(6, 2)? / 255.,
            )),
            _ => None,
        };
    }
    if let Some(arguments) = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let parts: Vec<f32> = arguments
            .split([',', ' ', '/'])
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.trim().parse().ok())
            .collect();
        return match parts.as_slice() {
            [r, g, b] => Some(rgba(*r, *g, *b, 1.)),
            [r, g, b, a] => Some(rgba(*r, *g, *b, a.clamp(0., 1.))),
            _ => None,
        };
    }
    let named = match value.as_str() {
        "black" => (0., 0., 0.),
        "white" => (255., 255., 255.),
        "red" => (255., 0., 0.),
        "green" => (0., 128., 0.),
        "lime" => (0., 255., 0.),
        "blue" => (0., 0., 255.),
        "yellow" => (255., 255., 0.),
        "gray" | "grey" => (128., 128., 128.),
        "orange" => (255., 165., 0.),
        "purple" => (128., 0., 128.),
        _ => return None,
    };
    Some(rgba(named.0, named.1, named.2, 1.))
}
