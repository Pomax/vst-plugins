//! Anything that takes a click turns the pointer into a pointing hand.
//!
//! The pointer is moved over each thing in turn and the cursor the window asks
//! for is read back, which is what the host is handed to show.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::{By, Queryable};
use egui_kittest::Harness;
use markdown_notes_core::{Editor, Theme};
use markdown_notes_plugin::gui::CLOSE_DIALOG;

/// Tall enough for the whole settings dialog.
const SIZE: (f32, f32) = (900.0, 800.0);

/// Two sections, so there is a section to delete, and a task to tick.
const SAMPLE: &str = "# One\n\n- [ ] a task\n\n# Two\n\nsecond";

const THEME: &str = "Theme: light";

fn open() -> Harness<'static> {
    let editor = Arc::new(Mutex::new(Editor::new()));
    if let Ok(mut e) = editor.lock() {
        e.set_document_text(SAMPLE);
        e.theme = Theme::Light;
        e.title = "cursors".to_string();
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(SIZE.0, SIZE.1))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    harness
}

/// The cursor asked for with the pointer resting at `at`.
fn cursor_at(harness: &mut Harness<'static>, at: egui::Pos2) -> egui::CursorIcon {
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.run_steps(3);
    harness.output().platform_output.cursor_icon
}

fn cursor_over(harness: &mut Harness<'static>, label: &str) -> egui::CursorIcon {
    let at = harness.get_by_label(label).rect().center();
    cursor_at(harness, at)
}

fn press(harness: &mut Harness<'static>, at: egui::Pos2, button: egui::PointerButton) {
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.run_steps(2);
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(2);
    }
}

/// The gear has no label to find it by. It is the last thing in the toolbar,
/// so it is what lies between the theme button and the window's edge.
fn gear(harness: &Harness<'static>) -> egui::Pos2 {
    let theme = harness.get_by_label(THEME).rect();
    egui::pos2((theme.right() + SIZE.0) / 2.0, theme.center().y)
}

fn open_settings(harness: &mut Harness<'static>) {
    let at = gear(harness);
    press(harness, at, egui::PointerButton::Primary);
}

#[test]
fn the_pointer_points_at_the_toolbar_buttons() {
    let mut harness = open();
    for label in ["Open…", "Save", "Save As…", "Markdown source", THEME] {
        assert_eq!(
            cursor_over(&mut harness, label),
            egui::CursorIcon::PointingHand,
            "over {label}"
        );
    }
}

#[test]
fn the_pointer_points_at_the_gear() {
    let mut harness = open();
    let at = gear(&harness);
    assert_eq!(cursor_at(&mut harness, at), egui::CursorIcon::PointingHand);
}

#[test]
fn the_pointer_points_at_the_new_section_button() {
    let mut harness = open();
    assert_eq!(cursor_over(&mut harness, "+"), egui::CursorIcon::PointingHand);
}

#[test]
fn the_pointer_points_at_delete_section() {
    let mut harness = open();
    let tab = harness.get_by_label("Two").rect().center();
    press(&mut harness, tab, egui::PointerButton::Secondary);
    assert_eq!(
        cursor_over(&mut harness, "Delete section"),
        egui::CursorIcon::PointingHand
    );
}

/// The item is the whole of its menu, not a box set inside it: the space the
/// menu keeps round its contents is part of the item.
#[test]
fn delete_section_fills_its_menu() {
    let mut harness = open();
    let tab = harness.get_by_label("Two").rect().center();
    press(&mut harness, tab, egui::PointerButton::Secondary);

    let style = harness.ctx.global_style();
    let margin = style.spacing.menu_margin;
    let text = egui::TextStyle::Button.resolve(&style).size;
    let item = harness.get_by_label("Delete section").rect();
    cursor_at(&mut harness, item.center());
    saved(&harness.render().expect("rendering failed"), "delete-section-menu");

    assert!(
        item.height() >= text + f32::from(margin.top) + f32::from(margin.bottom),
        "the item is {} tall, a box inside the menu's margin",
        item.height()
    );
}

#[test]
fn the_pointer_points_at_a_task_checkbox() {
    let mut harness = open();
    let at = harness.get(By::new().role(accesskit::Role::CheckBox)).rect().center();
    assert_eq!(cursor_at(&mut harness, at), egui::CursorIcon::PointingHand);
}

#[test]
fn the_pointer_points_at_the_theme_choices_in_settings() {
    let mut harness = open();
    open_settings(&mut harness);
    for label in ["Auto", "Light", "Dark"] {
        assert_eq!(
            cursor_over(&mut harness, label),
            egui::CursorIcon::PointingHand,
            "over {label}"
        );
    }
}

#[test]
fn the_pointer_points_at_a_colour_swatch_in_settings() {
    let mut harness = open();
    open_settings(&mut harness);
    let at = harness
        .get_all(By::new().role(accesskit::Role::ColorWell))
        .next()
        .expect("the settings dialog shows no colour swatch")
        .rect()
        .center();
    assert_eq!(cursor_at(&mut harness, at), egui::CursorIcon::PointingHand);
}

#[test]
fn the_pointer_points_at_reset_this_scheme_in_settings() {
    let mut harness = open();
    open_settings(&mut harness);
    assert_eq!(
        cursor_over(&mut harness, "Reset this scheme"),
        egui::CursorIcon::PointingHand
    );
}

/// The settings dialog, open, and where its close button is.
fn settings() -> (Harness<'static>, egui::Rect) {
    let mut harness = open();
    open_settings(&mut harness);
    let close = harness.get_by_label(CLOSE_DIALOG).rect();
    (harness, close)
}

/// Somewhere in the window that nothing is drawn at.
fn out_of_the_way() -> egui::Pos2 {
    egui::pos2(10.0, SIZE.1 - 10.0)
}

fn pixel(image: &image::RgbaImage, at: egui::Pos2) -> [u8; 4] {
    image.get_pixel(at.x as u32, at.y as u32).0
}

/// Keep the frame a test judged, beside the UI tests' screenshots.
fn saved(image: &image::RgbaImage, name: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/uitests")
        .join(format!("{name}.png"));
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = image.save(&path);
}

#[test]
fn the_pointer_points_at_the_settings_close_button() {
    let (mut harness, close) = settings();
    assert_eq!(
        cursor_at(&mut harness, close.center()),
        egui::CursorIcon::PointingHand
    );
}

#[test]
fn the_settings_close_button_closes_the_dialog() {
    let (mut harness, close) = settings();
    press(&mut harness, close.center(), egui::PointerButton::Primary);
    assert!(
        harness.query_by_label("Reset this scheme").is_none(),
        "the dialog is still open"
    );
}

/// The button shows the operating system's own close-window icon: the system
/// has one to give, and something is drawn in the button.
#[test]
fn the_settings_close_button_is_the_systems_own_icon() {
    use native_theme::theme::{system_icon_set, IconRole};
    assert!(
        native_theme::icons::load_icon(IconRole::WindowClose, system_icon_set()).is_some(),
        "the system gave no close-window icon"
    );

    let (mut harness, close) = settings();
    cursor_at(&mut harness, out_of_the_way());
    let image = harness.render().expect("rendering failed");
    saved(&image, "settings-close");

    let bar = pixel(&image, close.left_top() - egui::vec2(4.0, 0.0));
    let mut marked = 0;
    for y in close.top() as u32..close.bottom() as u32 {
        for x in close.left() as u32..close.right() as u32 {
            if image.get_pixel(x, y).0 != bar {
                marked += 1;
            }
        }
    }
    assert!(marked > 0, "nothing is drawn in the close button");

    cursor_at(&mut harness, close.center());
    saved(&harness.render().expect("rendering failed"), "settings-close-hovered");
}
