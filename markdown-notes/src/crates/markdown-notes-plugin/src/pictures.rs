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
    /// that cannot be read, or whose name and bytes do not agree on one kind
    /// of picture: `.png` with a PNG's magic number, `.jpg` or `.jpeg` with a
    /// JPEG's, the extension in any case.
    pub fn from_file(path: &Path) -> Option<Incoming> {
        let named = kind_named(path)?;
        let bytes = std::fs::read(path).ok()?;
        if images::kind_of(&bytes) != Some(named) {
            return None;
        }
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

/// The kind of picture a file's name says it is: PNG for `.png`, JPEG for
/// `.jpg` or `.jpeg`, the extension in any case. Nothing for any other name.
fn kind_named(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some(images::PNG),
        "jpg" | "jpeg" => Some(images::JPEG),
        _ => None,
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

    /// A 2 by 2 JPEG.
    fn jpeg() -> Vec<u8> {
        let rgb = [255, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 255];
        let picture = image::RgbImage::from_raw(2, 2, rgb.to_vec()).expect("not a picture");
        let mut bytes = std::io::Cursor::new(Vec::new());
        picture.write_to(&mut bytes, image::ImageFormat::Jpeg).expect("nothing was encoded");
        bytes.into_inner()
    }

    /// `bytes` written under `name`, in a directory of this test's own, and
    /// what the file is taken for.
    fn dropped(dir: &Path, name: &str, bytes: &[u8]) -> Option<Incoming> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        Incoming::from_file(&path)
    }

    #[test]
    fn a_file_is_a_picture_when_its_name_and_its_bytes_agree() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.cache/pictures-test");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(images::kind_of(&jpeg()), Some(images::JPEG));

        let cat = dropped(&dir, "cat.png", &png()).expect("the PNG was refused");
        assert_eq!(cat.alt, "cat");
        assert_eq!(cat.bytes, png());
        assert!(dropped(&dir, "CAT.PNG", &png()).is_some());
        assert!(dropped(&dir, "dog.jpg", &jpeg()).is_some());
        assert!(dropped(&dir, "dog.jpeg", &jpeg()).is_some());
        assert!(dropped(&dir, "DOG.JPEG", &jpeg()).is_some());

        // The name says one kind and the bytes another.
        assert!(dropped(&dir, "cat.jpg", &png()).is_none());
        assert!(dropped(&dir, "dog.png", &jpeg()).is_none());
        // The name says no kind at all, whatever the bytes are.
        assert!(dropped(&dir, "cat.gif", &png()).is_none());
        assert!(dropped(&dir, "cat", &png()).is_none());
        assert!(dropped(&dir, "notes.txt", b"not a picture").is_none());
        // The name says a kind and the bytes are nothing.
        assert!(dropped(&dir, "empty.png", b"").is_none());
        assert!(Incoming::from_file(&dir.join("missing.png")).is_none());
    }

    #[test]
    fn bytes_that_are_not_a_picture_do_not_decode() {
        let image = images::Image::from_bytes(1, images::PNG, b"not a picture");
        assert!(decode(&image).is_none());
    }
}
