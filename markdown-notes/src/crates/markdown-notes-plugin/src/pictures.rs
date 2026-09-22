//! Pictures in a note: where they come from and how they are shown.
//!
//! One comes from a file dropped on the window or from the clipboard, and is
//! handed to the editor as bytes, which keeps it as a definition at the end of
//! the document (see `markdown_notes_core::images`). Showing one is decoding
//! those bytes into a texture, once, and keeping the texture for as long as
//! the picture is on screen.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use markdown_notes_core::images;

/// A picture on its way into the note: what to call it, and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoming {
    pub alt: String,
    pub bytes: Vec<u8>,
}

impl Incoming {
    /// A file dropped on the window, called by its name. Nothing for a file
    /// that cannot be read or is not a PNG or a JPEG.
    pub fn from_file(path: &Path) -> Option<Incoming> {
        let bytes = std::fs::read(path).ok()?;
        images::kind_of(&bytes)?;
        let alt = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_string());
        Some(Incoming { alt, bytes })
    }

    /// Whatever picture the clipboard holds, as a PNG. Nothing when it holds
    /// none.
    pub fn from_clipboard() -> Option<Incoming> {
        let mut clipboard = arboard::Clipboard::new().ok()?;
        let held = clipboard.get_image().ok()?;
        Incoming::from_rgba(held.width, held.height, &held.bytes)
    }

    /// Raw pixels encoded as a PNG.
    pub fn from_rgba(width: usize, height: usize, rgba: &[u8]) -> Option<Incoming> {
        let picture = image::RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec())?;
        let mut bytes = std::io::Cursor::new(Vec::new());
        picture.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
        Some(Incoming { alt: "image".to_string(), bytes: bytes.into_inner() })
    }
}

/// Decode a stored picture into pixels, and its size.
pub fn decode(image: &images::Image) -> Option<(egui::ColorImage, [usize; 2])> {
    let bytes = image.bytes()?;
    let decoded = image::load_from_memory(&bytes).ok()?.into_rgba8();
    let size = [decoded.width() as usize, decoded.height() as usize];
    Some((egui::ColorImage::from_rgba_unmultiplied(size, decoded.as_raw()), size))
}

/// A picture as egui shows it.
#[derive(Clone)]
pub struct Shown {
    pub texture: egui::TextureHandle,
    /// Its size in pixels, which is what it is drawn at on a 1:1 screen.
    pub size: egui::Vec2,
}

/// Every picture on screen, keyed by its definition, so one is decoded once
/// and not once a frame. A definition that cannot be decoded is remembered as
/// such, so it is tried once too.
#[derive(Default)]
pub struct Album {
    shown: HashMap<String, Option<Shown>>,
    asked_for: HashSet<String>,
}

impl Album {
    pub fn picture(&mut self, ctx: &egui::Context, image: &images::Image) -> Option<Shown> {
        let key = image.definition();
        self.asked_for.insert(key.clone());
        if let Some(known) = self.shown.get(&key) {
            return known.clone();
        }
        let shown = decode(image).map(|(pixels, size)| Shown {
            texture: ctx.load_texture("picture", pixels, egui::TextureOptions::LINEAR),
            size: egui::vec2(size[0] as f32, size[1] as f32),
        });
        self.shown.insert(key, shown.clone());
        shown
    }

    /// Forget whatever nothing asked for since the last call.
    pub fn end_frame(&mut self) {
        let asked_for = std::mem::take(&mut self.asked_for);
        self.shown.retain(|key, _| asked_for.contains(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2 by 2 PNG, red on the left and blue on the right.
    fn png() -> Vec<u8> {
        let rgba = [255, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 255, 255];
        Incoming::from_rgba(2, 2, &rgba).expect("nothing was encoded").bytes
    }

    #[test]
    fn pixels_become_a_png_that_decodes_back_to_the_same_pixels() {
        let bytes = png();
        assert_eq!(images::kind_of(&bytes), Some(images::PNG));

        let image = images::Image::from_bytes(1, images::PNG, &bytes);
        let (pixels, size) = decode(&image).expect("the PNG did not decode");
        assert_eq!(size, [2, 2]);
        assert_eq!(pixels.pixels[0], egui::Color32::from_rgb(255, 0, 0));
        assert_eq!(pixels.pixels[1], egui::Color32::from_rgb(0, 0, 255));
    }

    #[test]
    fn a_file_is_called_by_its_name_and_refused_when_it_is_not_a_picture() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.cache/pictures-test");
        std::fs::create_dir_all(&dir).unwrap();
        let picture = dir.join("cat.png");
        std::fs::write(&picture, png()).unwrap();
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"not a picture").unwrap();

        let incoming = Incoming::from_file(&picture).expect("the PNG was refused");
        assert_eq!(incoming.alt, "cat");
        assert_eq!(incoming.bytes, png());
        assert!(Incoming::from_file(&text).is_none());
        assert!(Incoming::from_file(&dir.join("missing.png")).is_none());
    }

    #[test]
    fn bytes_that_are_not_a_picture_do_not_decode() {
        let image = images::Image::from_bytes(1, images::PNG, b"not a picture");
        assert!(decode(&image).is_none());
    }
}
