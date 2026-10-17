---
agentx:
  kind: state
  scope: knowledge/gpui/text
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-17T03:30:00+02:00
  topics:
    - text-system
    - line-layout-cache
    - text-layout
    - editable-text
    - selection
    - ime
  supersedes: []
  superseded_by: []
---
# Text State

## Purpose And Usage

Read before changing text shaping, line layout caches, `TextLayout`, or `editable_text`.

## Current Technical Shape

`TextSystem::shape_text` seeds each line's decoration vector from `min(input_text_run_count, 32)`. All four font-run scratch callers share a thread-local pool whose `RefCell` borrow covers only pop/push, so nested calls are safe and the old shared mutex is gone. `TextLayout` takes a pooled `LineWrapperHandle` only inside its truncation branch and keeps its local-size cache for repeated measurement at the same width.

The ordinary line-layout cache keeps two generations (current and previous frame) with exact unused-entry eviction. Each borrowed request computes one deterministic Fx hash over all identity fields; owned keys retain that scalar, so lookup, previous-frame removal, and promotion hash only the scalar while equality still compares the hash and every text/layout field.

Text measurement is `TextLayout::measure` (shared by the ordinary path and layout retention). Under layout retention (`crates/gpui/src/fast/text.rs`) a text leaf carries last frame's measurement at its key when it shapes the same way (decorations rewritten in place if only colours changed), replays its logged sizes with the new closure, or is measured afresh; and the line layout cache's rollover also keeps lines that carried text still holds (only under retention; lines nothing holds are evicted exactly).

Shaped lines paint their glyphs a run at a time through `fast::glyphs::LineGlyphPainter`: font, size, color, subpixel mode and paint color are worked out once per run, each glyph is quantized exactly as `Window::paint_glyph` does, and glyph bounding boxes are kept per frame (`GlyphBoundsCache`). Number shaping and gpui-fast's path tessellation cache were measured and reverted (no scenario was faster), so every line not in the layout cache is shaped by the platform.

Runs carry their own metrics (`TextRun.metrics: RunMetrics`: optional font size and letter spacing, and a baseline shift; the default inherits the line's). `FontRun.metrics` keeps runs that differ only in metrics apart through the four merge loops, so the metrics are part of every line-layout cache key, and `fast/text.rs` `shapes_as` compares them. Platform shapers shape each run at its own size and report it on `ShapedRun.font_size` (CoreText maps output glyph runs back to input runs by string index, which survives the `next_up` ligature-break sizes and fallback fonts; cosmic sets `Attrs::metrics` per span). One shared pass after shaping (`apply_run_metrics_to_layout`) applies per-run letter spacing and baseline shift, splitting shaped runs where the shift changes (`ShapedRun.baseline_shift`) and widening ascent and descent; lines whose runs space alike and shift nothing take the old letter-spacing path unchanged. Paint reads each run's size and strikes each run through at its own middle, never from a shaper's glyph y.

`editable_text` is GPUI's public, policy-neutral editing foundation: revisioned UTF-8 buffers and snapshots, selection, history, IME, single-line and multiline modes, and `TextLayoutMap`. The layout map maps buffer offsets, visual rows, and pixel geometry for soft-wrap and no-wrap text, retains unaffected shaped rows across incremental edits, and invalidates for text, width, DPR, font metrics/features/fallbacks, tabs, and font-affecting style runs; paint-only color changes reuse shaping. `EditableText` uses it for vertical and page movement, wrapped selection, horizontal and vertical viewport scrolling, drag auto-scroll, resize, and IME geometry. Selection paint intersects the normalized byte range with each visible visual row (including empty and newline rows). Platform `secondary()+Shift+Up/Down` extends to logical-line boundaries independent of soft wrap. The shared epoch-guarded drag loop keeps selecting and scrolling with a stationary pointer outside the viewport and stops after release.

`EditableTextState` stores character formatting (`TextAttributes`) as normalized `AttributeSpans` inside an `Arc<Formatting>` that is `None` for plain fields. Every buffer mutation goes through `apply_buffer_edit`, which remaps spans and gives inserted text its insertion attributes (explicit typing attributes, the first replaced character, the character before, the character after at a paragraph start, or the break before an empty paragraph; links and tags end-exclusive; IME compositions keep theirs). History entries share the formatting `Arc`; `EditKind::Format` steps never merge; formatting-only changes (including their undo) keep the buffer revision, bump `format_revision` and emit `EditableTextFormatEvent`. `TextLayoutMap::set_formatting` feeds stored shaping fields into `shape_runs_for_line` (fonts and `RunMetrics`, adjacent equal runs merged so paint-only spans never split shaping) and stored paint fields into `decoration_runs`, under the owner's `EditableTextStyleRuns`; formatted fields skip the single-row splice.

`TextLayoutMap` keeps one `TextVisualRow` per visual row (top, height, shared baseline, `x_offset`, width). Logical rows whose runs all sit at the field's size and baseline keep the uniform geometry exactly (tops `line_height * index`); mixed rows are as tall as their tallest run's scaled line height and center a shared baseline from `TextLayoutShaper::font_ascent_descent`. Every geometry consumer reads the table, and `WrappedLine::paint_rows` paints each row at its own geometry. Each row's alignment offset is computed as paint computes it and stored relative to the field's alignment offset, so `EditableTextGeometry::alignment_offset` keeps its meaning while multi-row centered/right text no longer uses the widest row's offset. A single-line field asks layout for its row height and re-lays out next frame only when that height differs from what it last requested (laid-out bounds are pixel-snapped).

Paragraph formats (`ParagraphFormat`: alignment incl. justify, first-line/leading/trailing indents, spacing before/after, line-height multiplier, default letter spacing) live in `Formatting.paragraphs` (empty when all default, else one shared `Arc` per paragraph) and are remapped in `apply_buffer_edit` from the edit's starting paragraph and its deleted/inserted break counts. Layout wraps indented paragraphs with `LineLayout::wrap_boundaries_for_widths` (first-row and other-row widths, part of `RowLayoutKey`), adds spacing to the row table, and stores each row's `justify` advance; `paint_line` widens every space a justified row passes, and the map's positions and justified hit testing read the same value.

List items are `ParagraphFormat.list` (`TextListItem`: `TextListKind` bullet/numbered/checkbox/custom and a level). Each level moves the leading indent by the state's `list_indent_step` (24 px default, part of the installed-layout identity); `TextLayoutMap::rebuild_list_markers` computes markers (numbering counts consecutive items at a level, deeper items don't break it, a non-list paragraph or another kind restarts it), and `list_marker_x`/`list_checkbox_bounds` place them for both paint and checkbox hit testing, clamped at the field's leading edge. List editing defaults are public state methods behind `set_list_editing`; markers are never text. Layout files must avoid the `text_layout_map` source guard's tokens, including iterator `fold`.

Inline objects are U+FFFC characters whose `TextAttributes.object` holds an `InlineObject`; typing never inherits it. Layout shapes such rows unwrapped, widens each object character to its width (`with_object_widths`, part of `RowLayoutKey`), then wraps (a row may break before or after U+FFFC), and `row_shapes` counts object heights. `EditableText::inline_object_renderer` lays out and paints owner elements at `TextLayoutMap::inline_object_bounds`, and `paint_rows` hides only those object glyphs.

Rich formatting follows [Editable state owns rich formatting](../../../decisions/gpui/text/2026-10-17-gpui-text-editable-state-owns-formatting.md); [docs/editable-text.md](../../../../../docs/editable-text.md) describes the API. Pasted HTML is bounded (non-finite numbers dropped, lengths, sizes and list starts capped, nesting capped at 256) and parses in linear time.

Clipboard items carry HTML as `ClipboardEntry::Html` beside the string entry; every backend writes and reads both in one transaction (Android through `dev.gpux.mobile.Clipboard.read`/`write` on the UI thread, iOS reading by length). The codec (`editable_text/html.rs`, html5ever tokenizer only) maps `AttributedText` to an HTML subset and back, escapes style attribute values, sanitizes font families, ends an unclosed head at body content and refuses input above `MAX_HTML_LEN`. `EditableTextState::copy` adds HTML only for formatted selections; `paste_item` (also the `EntityInputHandler::paste` override) inserts parsed formatting through `insert_attributed_text` only with `set_rich_paste(true)` and a non-empty parse, always as its own undo step.

`EditableText` registers its backing state entity with `Window::set_view_id` during prepaint, so state notifications invalidate cached ancestors. `EditableTextState::cancel_pointer_selection` ends only GPUI's native pointer-drag session and auto-scroll epoch for an aggregate selection owner that claimed the drag. `EditableTextDisplayTransform::Projected` gives owners such as code editors a buffer/display projection without fold policy; projected `EditableTextStyleRuns` keep font-affecting attributes in shaping while color, background, underline, strikethrough, and fade flow through paint-time decoration runs, and layout keys restrict each row to its overlapping style spans. Manual multi-line wheel scroll persists across ordinary redraws; only initial layout, viewport changes, edits, and explicit selection movement reveal the caret.

## Backlinks

- [Text system](../../../../../crates/gpui/src/text_system.rs)
- [Line layout cache](../../../../../crates/gpui/src/text_system/line_layout.rs)
- [Line wrapper](../../../../../crates/gpui/src/text_system/line_wrapper.rs)
- [Text element](../../../../../crates/gpui/src/elements/text.rs)
- [Glyph runs](../../../../../crates/gpui/src/fast/glyphs.rs)
- [Editable text](../../../../../crates/gpui/src/elements/editable_text/mod.rs)
- [Editable text element](../../../../../crates/gpui/src/elements/editable_text/element.rs)
