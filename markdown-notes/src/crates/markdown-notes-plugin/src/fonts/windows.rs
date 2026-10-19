use super::Script;

pub const SANS: &str = "Segoe UI";

pub const MONO: &str = "Consolas";

pub const DIAGRAM: &str = "Arial";

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
