use super::Script;

pub const SANS_FAMILY: &str = "Helvetica";
pub const SANS_REGULAR: &str = "Helvetica";
pub const SANS_BOLD: &str = "Helvetica-Bold";

pub const MONO_FAMILY: &str = "Menlo";
pub const MONO_REGULAR: &str = "Menlo-Regular";
pub const MONO_BOLD: &str = "Menlo-Bold";

pub const DIAGRAM_FAMILY: &str = "Arial";
pub const DIAGRAM_REGULAR: &str = "ArialMT";

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
