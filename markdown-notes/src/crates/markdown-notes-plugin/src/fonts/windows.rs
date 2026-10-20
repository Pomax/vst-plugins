use super::Script;

pub const SANS_FAMILY: &str = "Segoe UI";
pub const SANS_REGULAR: &str = "SegoeUI";
pub const SANS_BOLD: &str = "SegoeUI-Bold";

pub const MONO_FAMILY: &str = "Consolas";
pub const MONO_REGULAR: &str = "Consolas";
pub const MONO_BOLD: &str = "Consolas-Bold";

pub const DIAGRAM_FAMILY: &str = "Arial";
pub const DIAGRAM_REGULAR: &str = "ArialMT";

pub fn families_for(script: Script) -> &'static [&'static str] {
    match script {
        Script::Japanese => &["Yu Gothic UI", "Yu Gothic", "Meiryo", "MS Gothic"],
        Script::Korean => &["Malgun Gothic", "Gulim"],
        Script::Han => &["Microsoft YaHei UI", "Microsoft YaHei", "Microsoft JhengHei UI", "SimSun"],
        Script::Indic => &["Nirmala UI", "Mangal"],
        Script::Thai => &["Leelawadee UI", "Tahoma"],
        Script::Ethiopic => &["Ebrima", "Nyala"],
        Script::Emoji => &["Segoe UI Emoji", "Segoe UI Symbol"],
        Script::Other => &["Segoe UI Historic", "Segoe UI Symbol", "Arial Unicode MS"],
    }
}

pub fn name_of(font: &font_kit::font::Font) -> String {
    font.full_name()
}
