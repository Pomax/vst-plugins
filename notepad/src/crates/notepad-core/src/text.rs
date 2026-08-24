//! Byte-offset text utilities.
//!
//! The document keeps markdown source in a `String` and addresses positions as
//! byte offsets. Every helper here is UTF-8 safe: offsets handed back always sit
//! on a char boundary.

/// Zero-based index of the line containing `pos`.
pub fn line_index(text: &str, pos: usize) -> usize {
    text[..pos].bytes().filter(|b| *b == b'\n').count()
}

/// Byte range of line `index`, excluding the trailing newline.
pub fn line_range(text: &str, index: usize) -> Option<(usize, usize)> {
    let mut start = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        let end = start + line.len();
        if i == index {
            return Some((start, end));
        }
        start = end + 1;
    }
    None
}

/// Number of lines. A trailing newline yields a final empty line, matching editors.
pub fn line_count(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count() + 1
}

/// Next char boundary after `pos`, or `pos` if already at the end.
pub fn next_boundary(text: &str, pos: usize) -> usize {
    if pos >= text.len() {
        return text.len();
    }
    let mut i = pos + 1;
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Clamp an arbitrary offset onto the nearest char boundary at or below it.
pub fn clamp_boundary(text: &str, pos: usize) -> usize {
    let mut i = pos.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_helpers() {
        let t = "alpha\nbeta\ngamma";
        assert_eq!(line_index(t, 7), 1);
        assert_eq!(line_range(t, 2), Some((11, 16)));
        assert_eq!(line_count(t), 3);
    }

    #[test]
    fn trailing_newline_makes_an_empty_last_line() {
        let t = "one\n";
        assert_eq!(line_count(t), 2);
        assert_eq!(line_range(t, 1), Some((4, 4)));
    }

    #[test]
    fn boundaries_are_utf8_safe() {
        let t = "aé漢";
        assert_eq!(next_boundary(t, 0), 1);
        assert_eq!(next_boundary(t, 1), 3);
        assert_eq!(next_boundary(t, 3), 6);
        assert_eq!(clamp_boundary(t, 2), 1);
    }
}
