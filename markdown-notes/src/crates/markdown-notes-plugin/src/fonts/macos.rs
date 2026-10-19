use super::Script;

pub const SANS: &str = "Helvetica";

pub const MONO: &str = "Menlo";

pub const DIAGRAM: &str = "Arial";

pub fn families_for(script: Script) -> &'static [&'static str] {
    match script {
        Script::Japanese => &["Hiragino Sans", "Hiragino Kaku Gothic ProN"],
        Script::Korean => &["Apple SD Gothic Neo", "AppleGothic"],
        Script::Han => &["PingFang SC", "PingFang TC", "Heiti SC"],
        Script::Indic => &["Kohinoor Devanagari", "Devanagari Sangam MN"],
        Script::Thai => &["Thonburi", "Ayuthaya"],
        Script::Ethiopic => &["Kefa"],
        Script::Emoji => &["Apple Color Emoji"],
        Script::Other => &["Arial Unicode MS", "Apple Symbols"],
    }
}

pub fn name_of(font: &font_kit::font::Font) -> String {
    font.full_name()
}
