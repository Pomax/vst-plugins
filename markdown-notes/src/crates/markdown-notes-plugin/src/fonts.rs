//! System fonts, loaded at runtime.
//!
//! Nothing is bundled. The editor starts with the operating system's UI font,
//! which covers Latin, Greek, Cyrillic, Hebrew and Arabic. When text arrives
//! that needs something else, the font for that script is loaded from the
//! system and added as a fallback.

use std::collections::HashSet;
use std::sync::Arc;

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::FontFamily;
use font_kit::family_name::FamilyName;
use font_kit::properties::{Properties, Weight};
use font_kit::source::SystemSource;

/// A group of characters served by one font.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Script {
    Japanese,
    Korean,
    Han,
    Indic,
    Thai,
    Ethiopic,
    Emoji,
    /// Anything else the base font does not cover.
    Other,
}

/// Which script a character belongs to, or `None` when the base UI font is
/// expected to have it.
fn script_of(c: char) -> Option<Script> {
    let c = c as u32;
    match c {
        // Latin, Greek, Cyrillic, Hebrew, Arabic, punctuation, symbols.
        0x0000..=0x07FF | 0x2000..=0x25FF | 0xFB00..=0xFB4F => None,

        0x0900..=0x0DFF => Some(Script::Indic),
        0x0E00..=0x0E7F => Some(Script::Thai),
        0x1200..=0x139F => Some(Script::Ethiopic),
        0x3040..=0x30FF | 0x31F0..=0x31FF => Some(Script::Japanese),
        0x1100..=0x11FF | 0xA960..=0xA97F | 0xAC00..=0xD7FF => Some(Script::Korean),
        0x2E80..=0x2FDF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF => {
            Some(Script::Han)
        }
        0x1F000..=0x1FAFF | 0x2600..=0x27BF => Some(Script::Emoji),
        _ => Some(Script::Other),
    }
}

#[cfg_attr(target_os = "windows", path = "fonts/windows.rs")]
#[cfg_attr(target_os = "macos", path = "fonts/macos.rs")]
#[cfg_attr(target_os = "linux", path = "fonts/linux.rs")]
pub(crate) mod platform;

/// Ask the system for a font by family name and hand back its bytes.
fn load_family(name: &str) -> Option<(String, Vec<u8>)> {
    let handle = SystemSource::new()
        .select_best_match(
            &[FamilyName::Title(name.to_string())],
            &Properties::new(),
        )
        .ok()?;
    let font = handle.load().ok()?;
    let data = font.copy_font_data()?;
    Some((platform::name_of(&font), data.as_ref().clone()))
}

/// The face of `family` at `weight`, with its index in the file the face
/// lives in: a collection holds several faces, and the first of them is not
/// necessarily the one matched.
///
/// Panics when the machine has no such face. The editor is drawn in nothing
/// else, so it cannot run without it.
fn load_face(family: &str, weight: Weight) -> (String, egui::FontData) {
    let mut properties = Properties::new();
    properties.weight = weight;
    let handle = SystemSource::new()
        .select_best_match(&[FamilyName::Title(family.to_string())], &properties)
        .unwrap_or_else(|_| panic!("the font {family} is not installed"));
    let index = match &handle {
        font_kit::handle::Handle::Path { font_index, .. }
        | font_kit::handle::Handle::Memory { font_index, .. } => *font_index,
    };
    let font = handle
        .load()
        .unwrap_or_else(|e| panic!("the font {family} could not be loaded: {e}"));
    let found = font.properties().weight;
    if found != weight {
        panic!("the font {family} has no face of weight {}, only {}", weight.0, found.0);
    }
    let data = font
        .copy_font_data()
        .unwrap_or_else(|| panic!("the font {family} could not be read"));
    let mut face = egui::FontData::from_owned(data.as_ref().clone());
    face.index = index;
    (platform::name_of(&font), face)
}

/// The family the bold face is installed under.
pub const BOLD: &str = "bold";

/// The family the bold monospace face is installed under.
pub const MONO_BOLD: &str = "mono-bold";

/// The family to draw bold text in.
pub fn bold_family(ui: &egui::Ui) -> FontFamily {
    installed_or(ui, BOLD, FontFamily::Proportional)
}

/// The family to draw the source view in.
pub fn mono_bold_family(ui: &egui::Ui) -> FontFamily {
    installed_or(ui, MONO_BOLD, FontFamily::Monospace)
}

/// The family named `name`, or `fallback` while there is none.
///
/// [`install_base`] binds [`BOLD`] and [`MONO_BOLD`] to the machine's bold
/// faces, but a font change only takes effect on the pass after the one that
/// made it, so the very first pass of a context has neither. Asking which
/// families exist costs a lookup and means never reaching for one that does
/// not.
fn installed_or(ui: &egui::Ui, name: &str, fallback: FontFamily) -> FontFamily {
    let family = FontFamily::Name(name.into());
    if ui.fonts(|fonts| fonts.families().contains(&family)) {
        family
    } else {
        fallback
    }
}

/// Install the base UI and monospace faces, regular and bold.
pub fn install_base(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    let faces = [
        (platform::SANS, Weight::NORMAL, FontFamily::Proportional),
        (platform::MONO, Weight::NORMAL, FontFamily::Monospace),
        (platform::SANS, Weight::BOLD, FontFamily::Name(BOLD.into())),
        (platform::MONO, Weight::BOLD, FontFamily::Name(MONO_BOLD.into())),
    ];
    for (family, weight, target) in faces {
        let (name, face) = load_face(family, weight);
        fonts.font_data.insert(name.clone(), Arc::new(face));
        fonts.families.insert(target, vec![name]);
    }
    ctx.set_fonts(fonts);
}

/// Load whatever `text` needs and the base font does not provide.
///
/// `loaded` records the scripts already dealt with, including those where no
/// font could be found, so a document full of unsupported characters does not
/// re-query the system every frame. Returns true when a font was added, which
/// means the frame should be drawn again.
pub fn ensure_coverage(ctx: &egui::Context, text: &str, loaded: &mut HashSet<Script>) -> bool {
    let mut wanted: Vec<Script> = Vec::new();
    for c in text.chars() {
        if let Some(script) = script_of(c) {
            if !loaded.contains(&script) && !wanted.contains(&script) {
                wanted.push(script);
            }
        }
    }
    if wanted.is_empty() {
        return false;
    }

    let mut added = false;
    for script in wanted {
        loaded.insert(script);
        let Some((name, bytes)) = platform::families_for(script).iter().find_map(|f| load_family(f))
        else {
            continue;
        };
        ctx.add_font(FontInsert::new(
            &name,
            egui::FontData::from_owned(bytes),
            vec![
                InsertFontFamily {
                    family: FontFamily::Proportional,
                    priority: FontPriority::Lowest,
                },
                InsertFontFamily {
                    family: FontFamily::Monospace,
                    priority: FontPriority::Lowest,
                },
            ],
        ));
        added = true;
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bold is a face, not a darker colour.
    ///
    /// A bold face is wider than the regular one at the same size, so measuring
    /// the same words in both says whether a real face was installed. Colour
    /// would not shift a single pixel.
    #[test]
    fn bold_is_a_different_face_from_the_regular_one() {
        let ctx = egui::Context::default();
        install_base(&ctx);

        let width_in = |family: FontFamily| {
            let mut width = 0.0;
            let mut output = ctx.run_ui(Default::default(), |ui| {
                width = ui
                    .painter()
                    .layout_no_wrap(
                        "Handgloves in the morning".to_string(),
                        egui::FontId::new(15.0, family.clone()),
                        egui::Color32::WHITE,
                    )
                    .size()
                    .x;
            });
            // Laying out text fills the font atlas, and nothing here paints it.
            // egui refuses to drop textures nobody applied.
            output.textures_delta.clear();
            width
        };

        let regular = width_in(FontFamily::Proportional);
        let bold = width_in(FontFamily::Name(BOLD.into()));
        assert!(regular > 0.0, "no regular face was installed");
        assert!(
            bold > regular,
            "bold measured {bold} against a regular {regular}: \
             the same width means the same face, drawn in another colour"
        );
    }
}
