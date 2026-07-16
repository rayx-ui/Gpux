use gpux::{
    EditableTextStyleRun, EditableTextStyleRuns, HighlightStyle, Point, TextBuffer,
    TextBufferError, TextEdit, hsla, point,
};

#[test]
fn text_buffer_applies_revision_checked_edits_atomically() {
    let mut buffer = TextBuffer::new("alpha 中文");
    let original = buffer.snapshot();

    let snapshot = buffer
        .apply(TextEdit::new(0, 6..12, "🎉"))
        .expect("valid edit should apply");

    assert_eq!(snapshot.revision(), 1);
    assert_eq!(snapshot.text(), "alpha 🎉");
    assert_eq!(original.revision(), 0);
    assert_eq!(original.text(), "alpha 中文");

    let before_rejected_edits = buffer.snapshot();
    assert_eq!(
        buffer.apply(TextEdit::new(0, 0..5, "stale")),
        Err(TextBufferError::StaleRevision {
            expected: 0,
            actual: 1,
        })
    );
    assert_eq!(buffer.snapshot(), before_rejected_edits);

    assert_eq!(
        buffer.apply(TextEdit::new(1, 7..8, "invalid")),
        Err(TextBufferError::NotUtf8Boundary { offset: 7 })
    );
    assert_eq!(buffer.snapshot(), before_rejected_edits);
}

#[test]
fn text_buffer_converts_bytes_points_and_utf16_without_clipping_invalid_input() {
    let snapshot = TextBuffer::new("A\r\n中🎉e\u{301}\n").snapshot();

    assert_eq!(snapshot.line_count(), 3);
    assert_eq!(snapshot.byte_to_point(0), Ok(point(0, 0)));
    assert_eq!(snapshot.byte_to_point(3), Ok(point(0, 1)));
    assert_eq!(snapshot.byte_to_point("中".len() + 3), Ok(point(3, 1)));
    assert_eq!(snapshot.point_to_byte(point(3, 1)), Ok(6));
    assert_eq!(snapshot.point_to_byte(point(0, 2)), Ok(snapshot.len()));

    let emoji_byte = "A\r\n中".len();
    let emoji_utf16 = snapshot.byte_to_utf16(emoji_byte).unwrap();
    assert_eq!(snapshot.utf16_to_byte(emoji_utf16), Ok(emoji_byte));
    assert_eq!(
        snapshot.utf16_to_byte(emoji_utf16 + 1),
        Err(TextBufferError::Utf16InsideScalar {
            offset: emoji_utf16 + 1,
        })
    );
    assert_eq!(
        snapshot.byte_to_point(emoji_byte + 1),
        Err(TextBufferError::NotUtf8Boundary {
            offset: emoji_byte + 1,
        })
    );
    assert!(matches!(
        snapshot.point_to_byte(Point::new(99, 1)),
        Err(TextBufferError::PointOutOfBounds { .. })
    ));
    assert!(matches!(
        snapshot.point_to_byte(Point::new(0, 99)),
        Err(TextBufferError::PointOutOfBounds { .. })
    ));
}

#[test]
fn text_buffer_validates_ranges_and_preserves_crlf_and_trailing_newline() {
    let snapshot = TextBuffer::new("one\r\ntwo\n").snapshot();

    assert_eq!(snapshot.text(), "one\r\ntwo\n");
    assert_eq!(snapshot.line_count(), 3);
    assert_eq!(snapshot.slice(0..5).unwrap(), "one\r\n");
    assert!(matches!(
        snapshot.slice(5..4),
        Err(TextBufferError::InvalidRange { start: 5, end: 4 })
    ));
    assert!(matches!(
        snapshot.slice(0..99),
        Err(TextBufferError::RangeOutOfBounds { .. })
    ));
}

#[test]
fn text_buffer_style_runs_are_revision_and_range_validated_then_clipped_stably() {
    let snapshot = TextBuffer::new("abcdef").snapshot();
    let red = HighlightStyle {
        color: Some(hsla(0.0, 1.0, 0.5, 1.0).into()),
        ..Default::default()
    };
    let faded = HighlightStyle {
        fade_out: Some(0.5),
        ..Default::default()
    };
    let runs = EditableTextStyleRuns::new(
        snapshot.revision(),
        [
            EditableTextStyleRun::new(3..6, faded.clone()),
            EditableTextStyleRun::new(0..4, red.clone()),
        ],
    );

    let clipped = runs.clipped_to(&snapshot, 2..5).unwrap();
    assert_eq!(
        clipped
            .iter()
            .map(|run| run.range.clone())
            .collect::<Vec<_>>(),
        vec![2..4, 3..5]
    );
    assert_eq!(clipped[0].style, red);
    assert_eq!(clipped[1].style, faded);

    let mut newer_buffer = TextBuffer::new("abcdef");
    let newer_snapshot = newer_buffer
        .apply(TextEdit::new(0, 0..0, "!"))
        .expect("valid edit should advance the revision");
    assert_eq!(
        runs.clipped_to(&newer_snapshot, 0..1),
        Err(TextBufferError::StaleRevision {
            expected: 0,
            actual: 1,
        })
    );

    let invalid_runs = EditableTextStyleRuns::new(
        snapshot.revision(),
        [EditableTextStyleRun::new(1..99, HighlightStyle::default())],
    );
    assert!(matches!(
        invalid_runs.clipped_to(&snapshot, 0..1),
        Err(TextBufferError::RangeOutOfBounds { .. })
    ));

    let unicode_snapshot = TextBuffer::new("éx").snapshot();
    let non_boundary_runs = EditableTextStyleRuns::new(
        unicode_snapshot.revision(),
        [EditableTextStyleRun::new(1..2, HighlightStyle::default())],
    );
    assert_eq!(
        non_boundary_runs.clipped_to(&unicode_snapshot, 0..2),
        Err(TextBufferError::NotUtf8Boundary { offset: 1 })
    );
}
