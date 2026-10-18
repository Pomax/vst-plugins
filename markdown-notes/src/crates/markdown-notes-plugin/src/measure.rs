//! Measuring text for merman, with the fonts the diagram is drawn in.
//!
//! merman's own measurer carries a table of character widths copied from a
//! browser, because upstream Mermaid measures text by asking the browser and
//! merman has none. This plug-in does: the same font database resvg draws the
//! SVG with can shape the text and report what it actually measures.
//!
//! Measuring with the font that will draw the words means a label never
//! overflows the box laid out for it, which a borrowed table cannot promise on
//! a machine whose fonts differ from the one the table came from.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use merman::render::TextMeasurer;
use resvg::usvg::fontdb;

/// How tall a line is as a multiple of the font size. Mermaid's own line
/// height, which its layout assumes.
const LINE_HEIGHT: f64 = 1.1;

pub struct SystemFontMeasurer {
    fonts: Arc<fontdb::Database>,
    /// Shaping the same string twice is common during wrapping, and shaping is
    /// the expensive part of a layout.
    widths: Mutex<HashMap<(String, u64, bool), f64>>,
}

impl std::fmt::Debug for SystemFontMeasurer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SystemFontMeasurer")
    }
}

/// The one measurer, over the machine's fonts.
///
/// Loading the font database takes long enough to be worth doing once, and
/// every diagram in the document measures against the same fonts anyway.
pub fn shared() -> Arc<SystemFontMeasurer> {
    static ONE: std::sync::OnceLock<Arc<SystemFontMeasurer>> = std::sync::OnceLock::new();
    Arc::clone(ONE.get_or_init(|| Arc::new(SystemFontMeasurer::new(crate::diagram::system_fonts()))))
}

impl SystemFontMeasurer {
    pub fn new(fonts: Arc<fontdb::Database>) -> Self {
        Self {
            fonts,
            widths: Mutex::new(HashMap::new()),
        }
    }

    /// The advance width of one line, shaped, in pixels.
    fn shaped_width_px(&self, line: &str, size: f64, bold: bool) -> Option<f64> {
        let id = face_for(&self.fonts, bold)?;
        self.fonts.with_face_data(id, |data, index| {
            let font = harfrust::FontRef::from_index(data, index).ok()?;
            let shaping = harfrust::ShaperData::new(&font);
            let shaper = shaping.shaper(&font).build();
            let mut buffer = harfrust::UnicodeBuffer::new();
            buffer.push_str(line);
            // HarfBuzz refuses to shape without a direction, where the older
            // shaper guessed one. The script the text is in decides it.
            buffer.guess_segment_properties();
            let shaped = shaper.shape(buffer, harfrust::ShapeOptions::new());
            let advance: i32 = shaped
                .glyph_positions()
                .iter()
                .map(|position| position.x_advance)
                .sum();
            let per_em = f64::from(shaper.units_per_em());
            Some(f64::from(advance) / per_em * size)
        })?
    }

    fn width_px(&self, line: &str, style: &merman_style::Style) -> f64 {
        if line.is_empty() {
            return 0.0;
        }
        let key = (line.to_string(), style.size.to_bits(), style.bold);
        if let Ok(cache) = self.widths.lock() {
            if let Some(width) = cache.get(&key) {
                return *width;
            }
        }
        let width = self
            .shaped_width_px(line, style.size, style.bold)
            // No font on the machine answered, so fall back to the rough
            // half-em-per-character rule rather than reporting nothing.
            .unwrap_or_else(|| line.chars().count() as f64 * style.size * 0.5);
        if let Ok(mut cache) = self.widths.lock() {
            cache.insert(key, width);
        }
        width
    }
}

/// The Arial face for regular or bold text.
pub fn face_for(fonts: &fontdb::Database, bold: bool) -> Option<fontdb::ID> {
    fonts.query(&fontdb::Query {
        families: &[fontdb::Family::Name(crate::diagram::FONT_FAMILY)],
        weight: if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        },
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    })
}

/// What merman's `TextStyle` says, in the terms this module needs.
mod merman_style {
    pub struct Style {
        pub size: f64,
        pub bold: bool,
    }
}

fn style_of(style: &merman::render::TextStyle) -> merman_style::Style {
    merman_style::Style {
        size: style.font_size.max(1.0),
        bold: style
            .font_weight
            .as_deref()
            .is_some_and(|weight| weight == "bold" || weight.parse::<u16>().is_ok_and(|w| w >= 600)),
    }
}

/// Mermaid breaks a label on `<br>` as well as on a newline.
fn lines_of(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let br = ["<br>", "<br/>", "<br />", "\n"]
            .iter()
            .filter_map(|tag| rest.find(tag).map(|at| (at, tag.len())))
            .min_by_key(|(at, _)| *at);
        match br {
            Some((at, len)) => {
                out.push(&rest[..at]);
                rest = &rest[at + len..];
            }
            None => {
                out.push(rest);
                return out;
            }
        }
    }
}

impl TextMeasurer for SystemFontMeasurer {
    fn measure(
        &self,
        text: &str,
        style: &merman::render::TextStyle,
    ) -> merman::render::TextMetrics {
        let style = style_of(style);
        let lines = lines_of(text);
        let width = lines
            .iter()
            .map(|line| self.width_px(line, &style))
            .fold(0.0_f64, f64::max);
        merman::render::TextMetrics {
            width,
            height: lines.len() as f64 * style.size * LINE_HEIGHT,
            line_count: lines.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_arial_is_loaded() {
        let fonts = crate::diagram::system_fonts();
        assert!(fonts.len() > 0, "this machine has no Arial");
        for face in fonts.faces() {
            assert!(
                face.families.iter().any(|(name, _)| name == crate::diagram::FONT_FAMILY),
                "{:?} was loaded",
                face.families
            );
        }
    }

    #[test]
    fn arial_is_found_for_regular_and_bold_text() {
        let fonts = crate::diagram::system_fonts();
        assert!(face_for(&fonts, false).is_some(), "no regular Arial");
        assert!(face_for(&fonts, true).is_some(), "no bold Arial");
    }
}
