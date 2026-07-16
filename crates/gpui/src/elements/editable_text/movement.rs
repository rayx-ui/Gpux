use unicode_segmentation::UnicodeSegmentation as _;

pub(crate) fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .rev()
        .find_map(|(index, _)| (index < offset).then_some(index))
        .unwrap_or(0)
}

pub(crate) fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .find_map(|(index, _)| (index > offset).then_some(index))
        .unwrap_or(text.len())
}

pub(crate) fn grapheme_boundary_at_or_before(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset == text.len() {
        return text.len();
    }
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index <= offset)
        .last()
        .unwrap_or(0)
}

pub(crate) fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let offset = grapheme_boundary_at_or_before(text, offset);
    text.split_word_bound_indices()
        .filter(|(_, segment)| !segment.chars().all(char::is_whitespace))
        .map(|(index, _)| index)
        .take_while(|index| *index < offset)
        .last()
        .unwrap_or(0)
}

pub(crate) fn next_word_boundary(text: &str, offset: usize) -> usize {
    let offset = grapheme_boundary_at_or_before(text, offset);
    text.split_word_bound_indices()
        .filter(|(_, segment)| !segment.chars().all(char::is_whitespace))
        .map(|(index, _)| index)
        .find(|index| *index > offset)
        .unwrap_or(text.len())
}

pub(crate) fn normalize_single_line(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push(' ');
            }
            '\n' => normalized.push(' '),
            _ => normalized.push(character),
        }
    }
    normalized
}

pub(crate) fn normalize_multiline(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_boundaries_preserve_graphemes_and_unicode_segments() {
        let text = "one, cafe\u{301} 👩‍💻 العربية";
        let combining_end = "one, cafe\u{301}".len();
        assert_eq!(previous_grapheme_boundary(text, combining_end), 8);
        assert_eq!(next_grapheme_boundary(text, 8), combining_end);
        assert_eq!(next_word_boundary(text, 0), 3);
        assert_eq!(next_word_boundary(text, 3), 5);
        assert_eq!(previous_word_boundary(text, text.len()), 24);
    }

    #[test]
    fn normalization_floors_offsets_inside_graphemes() {
        let text = "a👩‍💻e\u{301}";
        assert_eq!(grapheme_boundary_at_or_before(text, 2), 1);
        assert_eq!(grapheme_boundary_at_or_before(text, text.len()), text.len());
    }
}
