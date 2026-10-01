//! Naming the cursor that is on screen.
//!
//! A window says what it wants the cursor to be by loading a picture from the
//! desktop's cursor theme, and what can be read back from outside is the
//! picture. So the cursor on screen is named by finding its picture among the
//! theme's own.

use xcursor::parser::parse_xcursor;
use xcursor::CursorTheme;

/// A cursor's picture: its size, and each pixel as alpha, red, green, blue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

/// The names a `cursor:` step uses, and the files of the theme each one is
/// loaded from: the names the plugin's window asks the theme for.
const NAMES: [(&str, &[&str]); 4] = [
    ("ibeam", &["text", "xterm"]),
    ("arrow", &["left_ptr"]),
    ("hand", &["hand2", "hand1"]),
    ("grabbing", &["closedhand", "grabbing"]),
];

/// Every picture the theme has for each of the named cursors, at every size
/// it comes in. A window picks the size, so all of them are candidates.
pub fn stock(theme: &str) -> Vec<(&'static str, Picture)> {
    let theme = CursorTheme::load(theme);
    let mut pictures = Vec::new();
    for (name, files) in NAMES {
        for file in files {
            let Some(path) = theme.load_icon(file) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            for image in parse_xcursor(&bytes).unwrap_or_default() {
                // The file keeps a pixel as four bytes, blue first.
                let pixels = image
                    .pixels_argb
                    .chunks_exact(4)
                    .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
                    .collect();
                pictures.push((
                    name,
                    Picture { width: image.width, height: image.height, pixels },
                ));
            }
        }
    }
    pictures
}

/// What a `cursor:` step calls the cursor with this picture: the name of the
/// stock cursor it is the picture of, or `other`.
pub fn name_of(shown: &Picture, stock: &[(&'static str, Picture)]) -> &'static str {
    stock
        .iter()
        .find(|(_, picture)| picture == shown)
        .map(|(name, _)| *name)
        .unwrap_or("other")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, fill: u32) -> Picture {
        Picture { width, height, pixels: vec![fill; (width * height) as usize] }
    }

    fn theme() -> Vec<(&'static str, Picture)> {
        vec![
            ("ibeam", picture(24, 24, 0xff00_0000)),
            ("ibeam", picture(32, 32, 0xff00_0000)),
            ("arrow", picture(24, 24, 0xffff_ffff)),
            ("grabbing", picture(24, 24, 0x8000_0000)),
        ]
    }

    #[test]
    fn the_shown_cursor_is_named_by_its_picture() {
        let stock = theme();
        assert_eq!(name_of(&picture(24, 24, 0xffff_ffff), &stock), "arrow");
        assert_eq!(name_of(&picture(24, 24, 0x8000_0000), &stock), "grabbing");
        // Either size of a cursor is that cursor.
        assert_eq!(name_of(&picture(24, 24, 0xff00_0000), &stock), "ibeam");
        assert_eq!(name_of(&picture(32, 32, 0xff00_0000), &stock), "ibeam");
    }

    #[test]
    fn a_cursor_no_stock_picture_matches_is_other() {
        let stock = theme();
        // Another picture altogether, and a stock picture at a size the theme
        // does not have.
        assert_eq!(name_of(&picture(24, 24, 0xff12_3456), &stock), "other");
        assert_eq!(name_of(&picture(48, 48, 0xffff_ffff), &stock), "other");
        assert_eq!(name_of(&picture(24, 24, 0xffff_ffff), &[]), "other");
    }
}
