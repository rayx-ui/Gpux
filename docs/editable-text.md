# Rich editable text

gpux has one editable element: `EditableText` over an `EditableTextState` entity. It is a plain
text field until an owner stores formatting in it, and a rich text editor once one does. A field
that stores nothing allocates nothing for formatting and behaves exactly as before.

Formatting lives in the state, beside the text, and travels with it through edits, undo and
redo, and the clipboard. The element paints it; the owner decides what to store and when.

## What stays with the owner

gpux supplies the mechanics: storage, layout, paint, editing, history and clipboard exchange. It
holds no editing policy. The owner (RayX's editors, for example) keeps:

- toolbars, menus and shortcuts that apply formatting;
- documents, block structure and any model larger than one field;
- Markdown and other formats besides the HTML subset below;
- collaboration, and its own operation and undo services when it needs more than the field's;
- code-editor features. Gutters, folds, search highlights and diagnostics never enter the
  layout map; owners draw them from `TextLayoutMap` geometry, or paint them through
  `EditableTextStyleRuns`, which stay bound to one buffer revision and are dropped by the next
  edit.

## Character formatting

`TextAttributes` holds what one character can carry. Every field is optional; an unset field
takes the element's text style:

| Field | Effect |
|---|---|
| `font_family`, `font_weight`, `font_style` | The font. |
| `font_size` | The run's size; a larger run makes its row taller. |
| `letter_spacing` | Extra advance after each cluster of the run. |
| `baseline_shift` | Moves the run up (positive) or down (negative) from the baseline: superscript and subscript. |
| `color`, `background_color` | Text and background colors; a background covers the run's whole row. |
| `underline`, `strikethrough` | Decorations; one without a color of its own takes the run's. |
| `link`, `tag` | Data for the owner: a link target and a free-form tag. They change nothing on screen. |
| `object` | Makes the character an inline object ([below](#inline-objects)). |

The state stores sorted, non-overlapping spans of shared attributes:

- `set_attributes(range, &patch, cx)` changes a range as one undo step. A `TextAttributesPatch`
  says per field whether to keep, set or clear it (`AttributeChange`);
  `TextAttributesPatch::setting(&attributes)` sets every field the attributes set.
- `attributes_at(offset)` reads one character; `attribute_state(range, predicate)` answers
  `On`, `Off` or `Mixed` for a toolbar button.
- `set_attributes` over an empty range sets the typing attributes there instead.
  `typing_attributes` and `set_typing_attributes` hold what the next insertion takes until the
  caret moves or focus changes. Text typed over a selection takes the attributes of the first
  character it replaces. Without them, typed text takes the attributes of the character
  before the caret (or after it at a paragraph's start), never an inline object, and a link or
  tag only inside it.
- `format_revision()` counts formatting changes, separately from the buffer's `revision()`.
  Formatting changes emit `EditableTextFormatEvent::Changed { format_revision }`;
  `EditableTextEvent` is unchanged and still reports only text changes.

Text runs outside editable text carry the same per-run metrics: `TextRun.metrics`
(`RunMetrics`: font size, letter spacing, baseline shift). Build runs with
`TextRun { .., ..Default::default() }` so new fields keep their defaults.

## Rows

A row is as tall as its tallest run or object, and every run sits on the row's shared
baseline. `TextLayoutMap::visual_row(row)` returns a row's top, height and baseline;
`visual_row_at_y` hit-tests rows of different heights, and caret, selection and IME bounds
follow the row they are in.

## Paragraph formatting

`ParagraphFormat` belongs to a paragraph (the text between line breaks):

| Field | Effect |
|---|---|
| `alignment` | `ParagraphAlignment`: `Left`, `Center`, `Right` or `Justify`; unset takes the element's `TextAlign`. |
| `first_line_indent`, `leading_indent`, `trailing_indent` | Indents; the first line's is added to the leading one. |
| `space_before`, `space_after` | Space above and below the paragraph. |
| `line_height` | A multiplier of the element's line height. |
| `letter_spacing` | The paragraph's default letter spacing; runs can override it. |
| `list` | Makes the paragraph a list item ([below](#lists)). |

`set_paragraph_format(range, format, cx)`, `update_paragraph_format(range, update, cx)` and
`clear_paragraph_format(range, cx)` change every paragraph the range touches, as one undo step;
`paragraph_format(offset)` reads one. Splitting a paragraph gives both halves its format;
joining two keeps the first one's.

## Lists

A list item is a paragraph whose format has a `TextListItem`: a `TextListKind` (`Bullet`,
`Numbered { start }`, `Checkbox { checked }` or `Custom(marker)`) and a level. Each level moves
the item by `list_indent_step` (24 px by default). Markers are painted in the space before the
item, never stored as text; numbering counts consecutive items at a level.

With `set_list_editing(true)` (the default) the state edits lists the usual way:
`list_insert_newline` continues an item and ends the list on an empty one,
`list_delete_backward` at an item's start removes its marker first, `indent_list_item` and
`outdent_list_item` change its level, and `toggle_list_checkbox` (also a click on the box)
checks it.

## Inline objects

An inline object is one U+FFFC (`OBJECT_REPLACEMENT_CHARACTER`) whose attributes hold an
`InlineObject`: a key, a width, a height, a baseline offset, a text fallback and an optional
HTML fallback. `insert_inline_object(range, object, cx)` inserts one as one undo step and
`inline_objects()` lists them.

An object reserves exactly its size in its row and never splits; a row may break before or
after it. It edits as one character, appears as U+FFFC in `text()` and to the IME, and copies
as its text fallback. `EditableText::inline_object_renderer(|object, window, cx| ..)` renders
the owner's element for each visible object in its box (`TextLayoutMap::inline_object_bounds`);
the element sees pointer events over it first, and those it does not handle select text.
Without a renderer an object keeps its space and paints nothing.

## History

Undo and redo restore the text, the selection and every stored format together, as whole
states; nothing is remapped backwards. Typing and IME composition coalesce into one step as
before. A formatting change (`set_attributes`, a paragraph or list change) is always its own
step, as is every paste and every structured insertion (`insert_inline_object`,
`insert_attributed_text`, `set_attributed_text`). Undoing a formatting-only step emits
`EditableTextFormatEvent::Changed` and leaves the buffer revision alone; undoing a step that
changed the text emits `EditableTextEvent::Changed`.

## Snapshots

`AttributedText { text, spans, paragraphs }` is the text and everything stored with it, with
public fields so owners can serialize it. `attributed_text()` and `attributed_text_in(range)`
take one; `set_attributed_text(text, cx)` replaces everything and
`insert_attributed_text(range, text, cx)` inserts one, each as one undo step.
`AttributedText::plain_text()` writes objects as their text fallbacks.

## Clipboard

A `ClipboardItem` can carry HTML beside its plain text (`ClipboardItem::new_string_with_html`,
`html()`); `text()` never returns it. Every platform writes and reads both in one clipboard
transaction: macOS and iOS pasteboards, Windows' `HTML Format`, Wayland's and X11's
`text/html`, the browser's copy and paste events and `navigator.clipboard`, and Android's
`ClipboardManager`.

`attributed_text_to_html` and `attributed_text_from_html` convert `AttributedText` to an HTML
subset and back: paragraphs, lists, bold, italic, underline, strikethrough, superscript,
subscript, links and `span` styles, plus what browsers, Google Docs, Word and Pages commonly
write. Headings `h1` to `h6` paste as bold paragraphs at 2, 1.5, 1.17, 1, 0.83 and 0.67 times
the field's font size; `br` breaks the paragraph; `img` pastes as its `alt` text. Checkbox lists
are written as `<ul data-gpux-checklist>` with a `data-checked` attribute on each item, and
read back as checkbox items. The parser never fetches, loads or runs anything; it drops scripts, styles, heads and
comments, and refuses input above `MAX_HTML_LEN` (4 MiB).

In `EditableText`:

- Copy and cut write HTML beside the text when the selection holds formatting, a paragraph
  format, a list or an object; plain selections copy as plain text only. Objects write their
  HTML fallback, or their escaped text fallback.
- Paste inserts the HTML's formatting only with `set_rich_paste(true)`; rich paste is off by
  default, so plain fields stay plain. Otherwise, or when the HTML holds no text, paste inserts
  the plain text.
- Every paste is its own undo step, keyboard and platform pastes alike (`paste_item`, also
  behind `EntityInputHandler::paste`).
- Single-line fields flatten pasted paragraphs into one line.

## Tests

Behaviour is proved by groups in `crates/gpui/tests` named `editable_text_*`
(`editable_text_attributes`, `editable_text_typing_attributes`, `editable_text_format_history`,
`editable_text_variable_rows`, `editable_text_paragraphs`, `editable_text_lists`,
`editable_text_list_editing`, `editable_text_inline_objects`, `editable_text_html`,
`editable_text_rich_clipboard`, `editable_text_api_compat` and others), plus `text_run_metrics`
and `clipboard_html`. Pixels are proved by the conformance scenes `rich-text-runs`,
`editable-rich-attributes`, `editable-paragraphs`, `editable-lists` and
`editable-inline-objects`; platform clipboards by lane D's `platform-clipboard-html`, lane B's
`clipboard: html` specs and lanes E and F's `clipboard-android` and `clipboard-ios`.
[testing.md](testing.md) describes the lanes.
