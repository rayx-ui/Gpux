use gpux::{
    EditableTextStyleRun, EditableTextStyleRuns, FontFallbacks, FontFeatures, FontId, FontStyle,
    GlyphId, HighlightStyle, LineLayout, ShapedGlyph, ShapedRun, TextBuffer, TextDisplayPoint,
    TextEdit, TextLayoutMap, TextLayoutShapeRequest, TextLayoutShaper, TextLayoutStyle,
    WrapBoundary, WrappedLineLayout, font, point, px,
};
use std::{sync::Arc, time::Instant};

struct DeterministicShaper;

impl TextLayoutShaper for DeterministicShaper {
    fn shape_layout(
        &self,
        request: TextLayoutShapeRequest<'_>,
    ) -> Result<Arc<WrappedLineLayout>, String> {
        let glyphs = request
            .text
            .char_indices()
            .enumerate()
            .map(|(column, (index, _))| ShapedGlyph {
                id: GlyphId(0),
                position: point(px(column as f32 * 8.), px(0.)),
                index,
                is_emoji: false,
            })
            .collect::<Vec<_>>();
        let character_count = glyphs.len();
        let wrap_every = request.wrap_width.and_then(|width| {
            let columns = (f32::from(width) / 8.).floor() as usize;
            (columns > 0).then_some(columns)
        });
        let wrap_boundaries = wrap_every
            .into_iter()
            .flat_map(|columns| (columns..character_count).step_by(columns))
            .map(|glyph_ix| WrapBoundary {
                run_ix: 0,
                glyph_ix,
            })
            .collect();
        Ok(Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                font_size: request.font_size,
                letter_spacing: request.letter_spacing,
                width: px(character_count as f32 * 8.),
                ascent: px(12.),
                descent: px(4.),
                runs: (!glyphs.is_empty())
                    .then(|| ShapedRun {
                        font_id: FontId(0),
                        glyphs,
                    })
                    .into_iter()
                    .collect(),
                len: request.text.len(),
            }),
            wrap_boundaries,
            wrap_width: request.wrap_width,
        }))
    }
}

fn style(wrap_width: Option<f32>) -> TextLayoutStyle {
    TextLayoutStyle {
        font: font(".SystemUIFont"),
        font_size: px(16.),
        line_height: px(22.),
        letter_spacing: px(0.),
        wrap_width: wrap_width.map(px),
        device_scale_factor: 1.,
        tab_size: 4,
    }
}

fn font_style_run(revision: u64, range: std::ops::Range<usize>) -> EditableTextStyleRuns {
    EditableTextStyleRuns::new(
        revision,
        [EditableTextStyleRun::new(
            range,
            HighlightStyle {
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
        )],
    )
}

#[test]
fn wrap_and_no_wrap_round_trip_mixed_script_offsets_and_pixels() {
    let text = "Latin 中 👨‍👩‍👧‍👦 e\u{301} العربية עברית\nsecond mixed 中 line\n";
    let buffer = TextBuffer::new(text);
    let snapshot = buffer.snapshot();
    let runs = EditableTextStyleRuns::empty(snapshot.revision());

    let mut unwrapped = TextLayoutMap::new();
    unwrapped
        .update_with_shaper(&snapshot, &runs, &style(None), &DeterministicShaper)
        .unwrap();
    assert_eq!(unwrapped.visual_line_count(), snapshot.line_count());

    let mut wrapped = TextLayoutMap::new();
    wrapped
        .update_with_shaper(&snapshot, &runs, &style(Some(92.)), &DeterministicShaper)
        .unwrap();
    assert!(wrapped.visual_line_count() > snapshot.line_count());

    for offset in text
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
    {
        let display = wrapped.buffer_to_display(offset).unwrap();
        assert_eq!(wrapped.display_to_buffer(display), Some(offset));
    }

    for needle in [
        "Latin",
        "中",
        "👨",
        "e\u{301}",
        "العربية",
        "עברית",
        "second",
    ] {
        let offset = text.find(needle).unwrap();
        let position = wrapped.position_for_index(offset).unwrap();
        assert_eq!(wrapped.index_for_position(position), Some(offset));
    }

    let eof_position = wrapped.position_for_index(text.len()).unwrap();
    assert_eq!(wrapped.index_for_position(eof_position), Some(text.len()));

    let zero_width = wrapped
        .update_with_shaper(&snapshot, &runs, &style(Some(0.)), &DeterministicShaper)
        .unwrap();
    assert_eq!(zero_width.shaped_rows, snapshot.line_count());
    assert_eq!(wrapped.size().width, px(0.));
    assert_eq!(
        wrapped.display_to_buffer(wrapped.buffer_to_display(text.len()).unwrap()),
        Some(text.len())
    );
}

#[test]
fn layout_cache_tracks_every_font_geometry_input_but_reuses_color_only_changes() {
    let text = "alpha\nbeta 中\ngamma 👨‍👩‍👧‍👦";
    let buffer = TextBuffer::new(text);
    let snapshot = buffer.snapshot();
    let empty = EditableTextStyleRuns::empty(snapshot.revision());
    let mut map = TextLayoutMap::new();
    let mut layout_style = style(Some(120.));

    let initial = map
        .update_with_shaper(&snapshot, &empty, &layout_style, &DeterministicShaper)
        .unwrap();
    assert_eq!(initial.shaped_rows, snapshot.line_count());

    let colored = EditableTextStyleRuns::new(
        snapshot.revision(),
        [EditableTextStyleRun::new(
            0..5,
            HighlightStyle {
                color: Some(gpux::red().into()),
                ..Default::default()
            },
        )],
    );
    let color_only = map
        .update_with_shaper(&snapshot, &colored, &layout_style, &DeterministicShaper)
        .unwrap();
    assert_eq!(color_only.shaped_rows, 0);
    assert_eq!(color_only.reused_rows, snapshot.line_count());

    let font_run = font_style_run(snapshot.revision(), 6.."alpha\nbeta 中".len());
    let font_change = map
        .update_with_shaper(&snapshot, &font_run, &layout_style, &DeterministicShaper)
        .unwrap();
    assert_eq!(font_change.shaped_rows, 1);
    assert_eq!(font_change.reused_rows, snapshot.line_count() - 1);

    let mutations: [fn(&mut TextLayoutStyle); 9] = [
        |style: &mut TextLayoutStyle| style.wrap_width = Some(px(96.)),
        |style: &mut TextLayoutStyle| style.device_scale_factor = 2.,
        |style: &mut TextLayoutStyle| style.font = font("Serif"),
        |style: &mut TextLayoutStyle| {
            style.font.fallbacks = Some(FontFallbacks::from_fonts(vec!["Noto Sans".into()]))
        },
        |style: &mut TextLayoutStyle| {
            style.font.features = FontFeatures(Arc::new(vec![("calt".into(), 0)]))
        },
        |style: &mut TextLayoutStyle| style.font_size = px(18.),
        |style: &mut TextLayoutStyle| style.line_height = px(26.),
        |style: &mut TextLayoutStyle| style.letter_spacing = px(0.5),
        |style: &mut TextLayoutStyle| style.tab_size = 8,
    ];
    for mutate in mutations {
        mutate(&mut layout_style);
        let changed = map
            .update_with_shaper(&snapshot, &font_run, &layout_style, &DeterministicShaper)
            .unwrap();
        assert_eq!(changed.shaped_rows, snapshot.line_count());
        assert_eq!(changed.invalidated_rows, snapshot.line_count());
    }
}

#[test]
fn revision_updates_invalidate_only_changed_rows_and_stale_runs_are_atomic() {
    let mut buffer = TextBuffer::new("first\nsecond\nthird");
    let before = buffer.snapshot();
    let before_runs = EditableTextStyleRuns::empty(before.revision());
    let mut map = TextLayoutMap::new();
    map.update_with_shaper(
        &before,
        &before_runs,
        &style(Some(90.)),
        &DeterministicShaper,
    )
    .unwrap();

    let after = buffer
        .apply(TextEdit::new(before.revision(), 6..12, "SECOND"))
        .unwrap();
    let after_runs = EditableTextStyleRuns::empty(after.revision());
    let changed = map
        .update_with_shaper(&after, &after_runs, &style(Some(90.)), &DeterministicShaper)
        .unwrap();
    assert_eq!(changed.shaped_rows, 1);
    assert_eq!(changed.reused_rows, 2);
    assert_eq!(map.revision(), Some(after.revision()));

    let stats_before_error = map.cache_stats();
    assert!(
        map.update_with_shaper(
            &after,
            &before_runs,
            &style(Some(90.)),
            &DeterministicShaper,
        )
        .is_err()
    );
    assert_eq!(map.revision(), Some(after.revision()));
    assert_eq!(map.cache_stats(), stats_before_error);
}

#[test]
fn incremental_layout_reuses_over_ninety_nine_percent_of_unchanged_rows() {
    let text = (0..256)
        .map(|row| format!("row {row:03} Latin 中 emoji 👨‍👩‍👧‍👦"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut buffer = TextBuffer::new(text);
    let before = buffer.snapshot();
    let mut map = TextLayoutMap::new();
    map.update_with_shaper(
        &before,
        &EditableTextStyleRuns::empty(before.revision()),
        &style(Some(280.)),
        &DeterministicShaper,
    )
    .unwrap();

    let changed_start = before.text().find("row 128").unwrap();
    let after = buffer
        .apply(TextEdit::new(
            before.revision(),
            changed_start..changed_start + "row 128".len(),
            "ROW 128",
        ))
        .unwrap();
    let started = Instant::now();
    let update = map
        .update_with_shaper(
            &after,
            &EditableTextStyleRuns::empty(after.revision()),
            &style(Some(280.)),
            &DeterministicShaper,
        )
        .unwrap();
    let elapsed = started.elapsed();

    eprintln!(
        "text-layout incremental rows={} reused={} ratio={:.4} elapsed_us={}",
        update.shaped_rows,
        update.reused_rows,
        update.reuse_ratio(),
        elapsed.as_micros()
    );

    assert_eq!(update.shaped_rows, 1);
    assert_eq!(update.reused_rows, 255);
    assert!(update.reuse_ratio() > 0.99);
    assert!(
        elapsed.as_secs_f32() < 1.0,
        "incremental update took {elapsed:?}"
    );
}

#[test]
fn same_length_single_line_edit_splices_layout_without_geometry_drift() {
    let text = (0..2_000)
        .map(|row| format!("row {row:04} Latin 中 emoji 👨‍👩‍👧‍👦"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut buffer = TextBuffer::new(text);
    let before = buffer.snapshot();
    let before_runs = EditableTextStyleRuns::empty(before.revision());
    let layout_style = style(Some(280.));
    let mut incremental = TextLayoutMap::new();
    let mut full = TextLayoutMap::new();
    incremental
        .update_with_shaper(&before, &before_runs, &layout_style, &DeterministicShaper)
        .unwrap();
    full.update_with_shaper(&before, &before_runs, &layout_style, &DeterministicShaper)
        .unwrap();

    let start = before.text().find("row 1000").unwrap();
    let edit = TextEdit::new(before.revision(), start..start + 8, "ROW 1000");
    let after = buffer.apply(edit.clone()).unwrap();
    let after_runs = EditableTextStyleRuns::empty(after.revision());
    let update = incremental
        .update_after_edit_with_shaper(
            &after,
            &edit,
            &after_runs,
            &layout_style,
            &DeterministicShaper,
        )
        .unwrap();
    full.update_with_shaper(&after, &after_runs, &layout_style, &DeterministicShaper)
        .unwrap();

    assert_eq!(update.shaped_rows, 1);
    assert_eq!(update.reused_rows, 1_999);
    assert_eq!(incremental.visual_line_count(), full.visual_line_count());
    for offset in [0, start, start + 4, after.len() / 2, after.len()] {
        assert_eq!(
            incremental.buffer_to_display(offset),
            full.buffer_to_display(offset)
        );
        assert_eq!(
            incremental.position_for_index(offset),
            full.position_for_index(offset)
        );
    }
}

#[test]
fn text_layout_map_source_stays_free_of_editor_and_form_policy() {
    let map = include_str!("../src/elements/editable_text/layout_map.rs").to_lowercase();
    let cache = include_str!("../src/elements/editable_text/layout_cache.rs").to_lowercase();
    for forbidden in [
        "fold",
        "search",
        "gutter",
        "diagnostic",
        "runtime_theme",
        "form",
    ] {
        let contains = |source: &str| {
            source
                .split(|character: char| !character.is_alphanumeric() && character != '_')
                .any(|token| token == forbidden)
        };
        assert!(!contains(&map), "layout_map contains {forbidden}");
        assert!(!contains(&cache), "layout_cache contains {forbidden}");
    }
}

#[test]
fn display_point_constructor_is_row_column_only() {
    assert_eq!(
        TextDisplayPoint::new(3, 7),
        TextDisplayPoint { row: 3, column: 7 }
    );
}

#[cfg(feature = "test-support")]
#[gpux::test]
fn real_text_system_round_trips_mixed_script_pixel_geometry(cx: &mut gpux::TestAppContext) {
    let text = "Latin 中 👨‍👩‍👧‍👦 e\u{301} العربية עברית\n";
    let buffer = TextBuffer::new(text);
    let snapshot = buffer.snapshot();
    let mut map = TextLayoutMap::new();
    let text_system = gpux::WindowTextSystem::new(cx.text_system().clone());
    map.update(
        &snapshot,
        &EditableTextStyleRuns::empty(snapshot.revision()),
        &style(Some(112.)),
        &text_system,
    )
    .unwrap();

    assert!(map.visual_line_count() > 1);
    for needle in ["Latin", "中", "👨", "e\u{301}", "العربية", "עברית"] {
        let offset = text.find(needle).unwrap();
        let position = map.position_for_index(offset).unwrap();
        assert_eq!(map.index_for_position(position), Some(offset));
    }
    assert_eq!(
        map.display_to_buffer(map.buffer_to_display(text.len()).unwrap()),
        Some(text.len())
    );

    map.update(
        &snapshot,
        &EditableTextStyleRuns::empty(snapshot.revision()),
        &style(Some(0.)),
        &text_system,
    )
    .unwrap();
    assert_eq!(map.size().width, px(0.));
}
