//! Text cut to the room there is for it.

/// What replaces the tail of text too long to fit.
pub const ELLIPSIS: &str = "...";

/// `text` as it is shown in `room`: whole if it fits, and otherwise as much of
/// its start as fits with [`ELLIPSIS`] after it.
///
/// `width` measures a string in the same units as `room`, and has to grow with
/// the string. The ellipsis is counted: the result is never wider than `room`
/// unless the ellipsis alone is.
pub fn fitted(text: &str, room: f32, width: impl Fn(&str) -> f32) -> String {
    if width(text) <= room {
        return text.to_string();
    }

    let ends: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    let cut = |chars: usize| {
        let end = ends.get(chars).copied().unwrap_or(text.len());
        format!("{}{ELLIPSIS}", text[..end].trim_end())
    };

    // The most characters that still fit, found by halving: `fits` holds for
    // every count up to the answer and for none after it.
    let (mut fits, mut too_many) = (0, ends.len());
    while too_many - fits > 1 {
        let middle = (fits + too_many) / 2;
        if width(&cut(middle)) <= room {
            fits = middle;
        } else {
            too_many = middle;
        }
    }
    cut(fits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ten units a character, so a room of 150 holds fifteen of them.
    fn width(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    #[test]
    fn text_that_fits_is_shown_whole() {
        assert_eq!(fitted("funky cake", 100.0, width), "funky cake");
    }

    #[test]
    fn text_that_does_not_fit_keeps_its_start_and_ends_in_three_dots() {
        assert_eq!(fitted("This is a long title", 150.0, width), "This is a lo...");
    }

    #[test]
    fn what_is_shown_is_never_wider_than_the_room() {
        let title = "This is a long title";
        for room in 30..=200 {
            let shown = fitted(title, room as f32, width);
            assert!(width(&shown) <= room as f32, "{shown:?} in {room}");
        }
    }

    #[test]
    fn a_space_is_not_left_hanging_before_the_dots() {
        assert_eq!(fitted("This is a long title", 110.0, width), "This is...");
    }

    #[test]
    fn room_for_nothing_still_shows_the_dots() {
        assert_eq!(fitted("This is a long title", 10.0, width), "...");
    }

    #[test]
    fn characters_of_more_than_one_byte_are_cut_between_and_not_through() {
        assert_eq!(fitted("日本語のタイトルです", 70.0, width), "日本語の...");
    }
}
