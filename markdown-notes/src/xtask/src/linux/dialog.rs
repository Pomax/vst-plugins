//! Reading the desktop's file dialog off a picture of the host's window.
//!
//! The dialog sits over the host's window from just under the host's strip.
//! Boxes here are measured from the top left of that window, frame included,
//! which is how a picture of it is read.

use super::Rect;

/// What the button that accepts a dialog may say, for the word a step calls
/// the dialog by. One that saves says `Replace` while the name in it is the
/// name of a file that is there already, and one that opens says `Select`.
pub fn says(word: &str) -> Vec<&str> {
    if word.eq_ignore_ascii_case("Save") {
        vec![word, "Replace"]
    } else if word.eq_ignore_ascii_case("Open") {
        vec![word, "Select"]
    } else {
        vec![word]
    }
}

/// The button that accepts the dialog: the lowest line that says one of
/// `says` and nothing else, of those under the host's strip, which ends at
/// `below`. The strip has words of its own, and the dialog's button is on
/// its bottom row.
pub fn accept_button(lines: &[(Rect, String)], says: &[&str], below: i32) -> Option<Rect> {
    lines
        .iter()
        .filter(|(_, text)| says.iter().any(|word| text.trim().eq_ignore_ascii_case(word)))
        .map(|(line, _)| *line)
        .filter(|line| line.y >= below)
        .max_by_key(|line| line.y)
}

/// Whether two looks running found the button, and in one place. A dialog
/// that is still coming up shows what is under it, and that is somewhere
/// else or gone by the next look.
pub fn settled(first: Option<Rect>, second: Option<Rect>) -> bool {
    match (first, second) {
        (Some(first), Some(second)) => {
            (first.x - second.x).abs() <= 2 && (first.y - second.y).abs() <= 2
        }
        _ => false,
    }
}

/// The name a dialog that saves shows beside its accept button: the closest
/// thing to read on the button's row, to the left of it.
pub fn name_beside(lines: &[Rect], accept: Rect) -> Option<Rect> {
    let row = accept.y + accept.height / 2;
    lines
        .iter()
        .copied()
        .filter(|line| line.x + line.width <= accept.x)
        .filter(|line| {
            let apart = line.y + line.height / 2 - row;
            apart.abs() <= accept.height.max(line.height)
        })
        .max_by_key(|line| line.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP_ENDS: i32 = 63;

    fn at(x: i32, y: i32, width: i32) -> Rect {
        Rect { x, y, width, height: 12 }
    }

    fn line(area: Rect, text: &str) -> (Rect, String) {
        (area, text.to_string())
    }

    #[test]
    fn the_accept_button_is_the_lowest_line_under_the_strip_that_says_only_its_word() {
        let on_the_strip = line(at(8, 44, 30), "Save");
        let a_file_called_it = line(at(240, 280, 30), "save");
        let button = line(at(820, 576, 30), "Save");
        let lower_and_longer = line(at(40, 600, 90), "Save the notes");

        assert_eq!(
            accept_button(
                &[on_the_strip.clone(), button.clone(), a_file_called_it, lower_and_longer],
                &["Save"],
                STRIP_ENDS
            ),
            Some(button.0)
        );
        // The strip's own word is not a dialog's.
        assert_eq!(accept_button(&[on_the_strip], &["Save"], STRIP_ENDS), None);
        assert_eq!(accept_button(&[button], &["Open"], STRIP_ENDS), None);
        assert_eq!(accept_button(&[], &["Save"], STRIP_ENDS), None);
    }

    #[test]
    fn a_dialog_that_saves_may_say_replace() {
        let replace = line(at(796, 576, 54), "Replace");

        assert_eq!(says("Save"), vec!["Save", "Replace"]);
        assert_eq!(says("Load"), vec!["Load"]);
        assert_eq!(
            accept_button(&[replace.clone()], &says("Save"), STRIP_ENDS),
            Some(replace.0)
        );
        assert_eq!(accept_button(&[replace], &says("Open"), STRIP_ENDS), None);
    }

    #[test]
    fn a_dialog_that_opens_may_say_select() {
        let select = line(at(806, 576, 44), "Select");

        assert_eq!(says("Open"), vec!["Open", "Select"]);
        assert_eq!(
            accept_button(&[select.clone()], &says("Open"), STRIP_ENDS),
            Some(select.0)
        );
        assert_eq!(accept_button(&[select], &says("Save"), STRIP_ENDS), None);
    }

    #[test]
    fn a_button_is_settled_when_two_looks_find_it_in_one_place() {
        let button = at(820, 576, 30);
        let read_a_pixel_off = at(821, 575, 29);
        let on_the_toolbar = at(478, 76, 30);

        assert!(settled(Some(button), Some(button)));
        assert!(settled(Some(button), Some(read_a_pixel_off)));
        assert!(!settled(Some(on_the_toolbar), Some(button)));
        assert!(!settled(None, Some(button)));
        assert!(!settled(Some(button), None));
        assert!(!settled(None, None));
    }

    #[test]
    fn the_name_is_the_nearest_thing_left_of_the_button_on_its_row() {
        let button = at(820, 576, 30);
        let file_type = at(201, 576, 70);
        let name = at(499, 577, 82);
        let a_folder = at(499, 280, 40);
        let right_of_it = at(870, 576, 20);

        assert_eq!(
            name_beside(&[file_type, a_folder, name, button, right_of_it], button),
            Some(name)
        );
        // Nothing on the button's row but the button.
        assert_eq!(name_beside(&[a_folder, button], button), None);
    }
}
