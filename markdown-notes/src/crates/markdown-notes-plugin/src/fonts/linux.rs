use super::Script;

pub const SANS: &str = "DejaVu Sans";

pub const MONO: &str = "DejaVu Sans Mono";

pub const DIAGRAM: &str = "Liberation Sans";

pub fn families_for(script: Script) -> &'static [&'static str] {
    match script {
        Script::Japanese | Script::Korean | Script::Han => {
            &["Noto Sans CJK JP", "Noto Sans CJK SC", "Source Han Sans"]
        }
        Script::Indic => &["Noto Sans Devanagari", "Lohit Devanagari"],
        Script::Thai => &["Noto Sans Thai", "Garuda"],
        Script::Ethiopic => &["Noto Sans Ethiopic"],
        Script::Emoji => &["Noto Color Emoji", "Noto Emoji"],
        Script::Other => &["Noto Sans", "DejaVu Sans"],
    }
}

pub fn name_of(font: &font_kit::font::Font) -> String {
    font.postscript_name().unwrap_or_else(|| font.full_name())
}
