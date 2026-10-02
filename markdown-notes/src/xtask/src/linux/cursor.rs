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

/// What `cursor:` shows over a window's bottom right corner where the window
/// can be dragged larger or smaller.
pub const CORNER: &str = "corner";

/// The names a `cursor:` step uses, and the files of the theme each one is
/// loaded from: the names the plugin's window asks the theme for, and for
/// the corner the names a window's frame asks for.
const NAMES: [(&str, &[&str]); 5] = [
    ("ibeam", &["text", "xterm"]),
    ("arrow", &["left_ptr"]),
    ("hand", &["hand2", "hand1"]),
    ("grabbing", &["closedhand", "grabbing"]),
    (CORNER, &["se-resize", "nwse-resize", "bottom_right_corner"]),
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
            pictures.extend(pictures_in(&bytes).into_iter().map(|picture| (name, picture)));
        }
    }
    pictures
}

/// The pictures a cursor file of the theme holds, one for each size.
fn pictures_in(file: &[u8]) -> Vec<Picture> {
    parse_xcursor(file)
        .unwrap_or_default()
        .into_iter()
        .map(|image| {
            // The bytes as the file has them, which the crate calls
            // `pixels_rgba`: a pixel is four of them, low byte first.
            let pixels = image
                .pixels_rgba
                .chunks_exact(4)
                .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
                .collect();
            Picture { width: image.width, height: image.height, pixels }
        })
        .collect()
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

    /// A cursor file holding one picture, two pixels wide and one high.
    fn cursor_file(pixels: [[u8; 4]; 2]) -> Vec<u8> {
        const IMAGE: u32 = 0xfffd_0002;
        const NOMINAL_SIZE: u32 = 24;
        let mut file = Vec::from(*b"Xcur");
        // The file's header: its own length, the format's version, and how
        // many entries the table of contents has.
        for word in [16u32, 0x0001_0000, 1] {
            file.extend(word.to_le_bytes());
        }
        // The one entry: an image, its nominal size, and where it starts.
        for word in [IMAGE, NOMINAL_SIZE, 28] {
            file.extend(word.to_le_bytes());
        }
        // The image: its header's length, type, nominal size and version,
        // then width, height, the hot spot, and the delay.
        for word in [36u32, IMAGE, NOMINAL_SIZE, 1, 2, 1, 0, 0, 0] {
            file.extend(word.to_le_bytes());
        }
        for pixel in pixels {
            file.extend(pixel);
        }
        file
    }

    /// A cursor file keeps a pixel as one number, alpha in its top byte,
    /// written low byte first, and that number is what the X server hands
    /// back for the cursor on screen.
    #[test]
    fn a_theme_file_is_read_as_the_pixels_the_server_shows() {
        // Blue, green, red, alpha: an opaque pixel and a half clear one.
        let file = cursor_file([[0x11, 0x22, 0x33, 0xff], [0x00, 0x00, 0x40, 0x80]]);

        assert_eq!(
            pictures_in(&file),
            vec![Picture { width: 2, height: 1, pixels: vec![0xff33_2211, 0x8040_0000] }]
        );
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
