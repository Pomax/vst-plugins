//! Print egui's built-in light and dark colours, for baking into the defaults.
fn main() {
    for (name, v) in [("light", egui::Visuals::light()), ("dark", egui::Visuals::dark())] {
        println!("== {name} ==");
        let c = |label: &str, c: egui::Color32| {
            println!("{label}: {} {} {} {}", c.r(), c.g(), c.b(), c.a())
        };
        c("inactive_bg_stroke", v.widgets.inactive.bg_stroke.color);
        c("hovered_bg_stroke", v.widgets.hovered.bg_stroke.color);
        c("hovered_fg", v.widgets.hovered.fg_stroke.color);
        c("active_fg", v.widgets.active.fg_stroke.color);
        c("noninteractive_bg_fill", v.widgets.noninteractive.bg_fill);
    }
}
