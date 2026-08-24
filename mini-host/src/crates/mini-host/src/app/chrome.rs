//! The host's own strip above the plugin, and the two dialogs it opens.
//!
//! The strip is a fixed band across the top holding **Save preset** and
//! **Load preset**. Neither browses: saving asks for a name and puts the file
//! in the one directory this plugin's presets live in, and loading shows what
//! is in that directory. A plugin with no presets yet cannot be loaded into,
//! so the button is dead until there is something to pick.
//!
//! The dialogs are windows of their own: a real frame with a title bar, which
//! can be dragged anywhere on the desktop while the host carries on drawing
//! behind it. `place::open_frame` makes the frame and the dialog's egui view
//! is parented into it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mini_host::presets;

use crate::place;

/// How much room the strip takes off the top of the window: one row of
/// buttons and nothing more.
pub const HEIGHT: i32 = 26;

/// Which dialog wants to be open.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Want {
    #[default]
    Nothing,
    Name,
    Pick,
}

/// Shared between the strip, the dialogs and the thread that owns the plugin.
#[derive(Default)]
pub struct State {
    /// What the strip has asked for but has not been given yet.
    pub want: Want,
    /// Set by a dialog when the user commits; acted on and cleared by the host.
    pub save_to: Option<PathBuf>,
    pub load_from: Option<PathBuf>,
    /// The last preset saved or loaded, which the naming dialog starts from.
    pub last_name: String,
    /// What the strip reports back.
    pub message: Option<String>,
    /// Where this plugin's presets live.
    pub directory: PathBuf,
}

pub type Shared = Arc<Mutex<State>>;

/// The presets on disk, by name, in alphabetical order.
pub fn list(directory: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(presets::EXTENSION))
        })
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            Some((name, path))
        })
        .collect();
    found.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    found
}

/// Turn what was typed into a file inside the preset directory.
///
/// Only the last component is kept, so a name cannot climb out of the one
/// directory this plugin is allowed to write to.
pub fn path_for(directory: &Path, typed: &str) -> Option<PathBuf> {
    let name: String = typed
        .trim()
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let name = name.trim().trim_matches('.').to_string();
    if name.is_empty() {
        return None;
    }
    Some(directory.join(format!("{name}.{}", presets::EXTENSION)))
}

// ---- the strip -----------------------------------------------------------

struct Bar {
    shared: Shared,
    plugin: String,
}

/// Open the strip across the top of `parent`.
pub fn open_bar<P: raw_window_handle::HasWindowHandle>(
    parent: &P,
    plugin: String,
    shared: Shared,
    width: i32,
) -> baseview::WindowHandle {
    egui_baseview::EguiWindow::open_parented(
        parent,
        window_settings("Mini VST Host strip", width, HEIGHT),
        Bar { shared, plugin },
        |_c: &egui::Context, _o: &mut egui_baseview::ExtraOutputCommands, _s: &mut Bar| {},
        |_o: &egui::FullOutput, _v: &egui::ViewportOutput, _s: &mut Bar| {},
        |ui, cmds, bar| draw_bar(ui, cmds, bar),
    )
}

fn draw_bar(ui: &mut egui::Ui, cmds: &mut egui_baseview::ExtraOutputCommands, bar: &mut Bar) {
    let (message, directory) = match bar.shared.lock() {
        Ok(state) => (state.message.clone(), state.directory.clone()),
        Err(_) => return,
    };
    paint_background(ui, cmds);
    let saved = list(&directory);

    ui.horizontal_centered(|ui| {
        ui.add_space(4.0);
        if ui.button("Save preset").clicked() {
            ask(&bar.shared, Want::Name);
        }
        // Nothing to load means nothing to pick from, so the button is dead
        // rather than opening an empty list.
        let load = ui.add_enabled(!saved.is_empty(), egui::Button::new("Load preset"));
        if load.clicked() {
            ask(&bar.shared, Want::Pick);
        }
        if saved.is_empty() {
            load.on_hover_text("no presets saved for this plugin yet");
        }
        ui.separator();
        ui.label(message.unwrap_or_else(|| bar.plugin.clone()));
    });
}

fn ask(shared: &Shared, want: Want) {
    if let Ok(mut state) = shared.lock() {
        state.want = want;
    }
}

// ---- the dialogs ---------------------------------------------------------

/// How big each dialog's frame is, inside its title bar.
const NAME_SIZE: (i32, i32) = (330, 34);
const PICK_SIZE: (i32, i32) = (250, 240);

/// A dialog: a window of the desktop's own, and the view drawing inside it.
///
/// The view is listed first so that it is dropped first: it borrows the frame
/// it was opened into.
pub struct Dialog {
    view: baseview::WindowHandle,
    frame: place::Frame,
}

impl Dialog {
    /// False once the title bar's close button has been used.
    pub fn is_open(&self) -> bool {
        self.frame.is_open()
    }
}

impl Drop for Dialog {
    fn drop(&mut self) {
        self.view.close();
    }
}

/// What one dialog window is drawing.
struct Panel {
    shared: Shared,
    want: Want,
    directory: PathBuf,
    /// What is being typed into the name field.
    name: String,
    /// Whether the field has been given egui's focus. It does not happen by
    /// the window appearing.
    field_focused: bool,
    /// How far down the list is scrolled, in points.
    scrolled: f32,
}

/// Open the dialog `want` asks for, over the host's window.
pub fn open_dialog(want: Want, shared: Shared, owner: *mut std::ffi::c_void) -> Option<Dialog> {
    let (title, (width, height)) = match want {
        Want::Name => ("Save preset", NAME_SIZE),
        Want::Pick => ("Load preset", PICK_SIZE),
        Want::Nothing => return None,
    };

    let (name, directory) = match shared.lock() {
        Ok(state) => (state.last_name.clone(), state.directory.clone()),
        Err(_) => return None,
    };

    let frame = place::open_frame(title, width, height, owner)?;
    let panel = Panel {
        shared,
        want,
        directory,
        name,
        field_focused: false,
        scrolled: 0.0,
    };
    let view = egui_baseview::EguiWindow::open_parented(
        &frame,
        window_settings(title, width, height),
        panel,
        |_c: &egui::Context, _o: &mut egui_baseview::ExtraOutputCommands, _s: &mut Panel| {},
        |_o: &egui::FullOutput, _v: &egui::ViewportOutput, _s: &mut Panel| {},
        |ui, cmds, panel| draw_dialog(ui, cmds, panel),
    );
    frame.fit_contents();

    Some(Dialog { view, frame })
}

fn draw_dialog(ui: &mut egui::Ui, cmds: &mut egui_baseview::ExtraOutputCommands, panel: &mut Panel) {
    paint_background(ui, cmds);

    // Escape asks for nothing and gets nothing: the window goes and neither
    // `save_to` nor `load_from` is set, so the host has nothing to act on.
    //
    // Consumed rather than merely read, which is what egui's own modal does:
    // a key left in the queue is acted on again by the text field, which
    // answers Escape by giving up its focus.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        ask(&panel.shared, Want::Nothing);
        return;
    }

    match panel.want {
        Want::Name => draw_naming(ui, panel),
        Want::Pick => draw_picking(ui, panel),
        Want::Nothing => {}
    }
}

/// The name field and its Save button. Enter commits, as does the button.
fn draw_naming(ui: &mut egui::Ui, panel: &mut Panel) {
    let mut save = false;
    ui.horizontal_centered(|ui| {
        // The row is centred vertically and the button leaves a gap at the
        // right, so the left needs the same gap put there.
        ui.add_space(4.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut panel.name)
                .desired_width(ui.available_width() - 52.0)
                .hint_text("preset"),
        );
        if !panel.field_focused {
            field.request_focus();
            panel.field_focused = true;
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            save = true;
        }
        if ui.button("Save").clicked() {
            save = true;
        }
    });

    if save {
        if let Ok(mut shared) = panel.shared.lock() {
            // No asking whether to overwrite: this is a mini host.
            shared.save_to = path_for(&panel.directory, &panel.name);
            shared.last_name = panel.name.trim().to_string();
            shared.want = Want::Nothing;
        }
    }
}

/// The list of presets. One click on a row loads it; there is nothing else.
/// How wide the scrollbar is, and so how big its heads are: they are square.
const BAR: f32 = 16.0;

/// How far apart the rows of the preset list are, which is how far one click
/// on a scroll head moves it.
///
/// `tools/capture-window.ps1` has the same number, for working out which row
/// of a list a test means. A mismatch shows up as clicks landing a row off.
const ROW: f32 = 21.0;

/// A scrollbar: a head at the top, a head at the bottom, a track between them,
/// and a thumb in the track. Returns how far down the list should be.
///
/// egui's own scrollbar is a track and a thumb with nothing else, so this is
/// painted here. When there is nothing to scroll the heads grey out and the
/// thumb is not there at all, which is what a disabled scrollbar looks like.
fn scrollbar(ui: &mut egui::Ui, rect: egui::Rect, content: f32, view: f32, at: f32) -> f32 {
    let range = (content - view).max(0.0);
    let live = range > 0.5;

    let head = egui::vec2(rect.width(), rect.width());
    let up = egui::Rect::from_min_size(rect.min, head);
    let down = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.bottom() - head.y),
        head,
    );
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.left(), up.bottom()),
        egui::pos2(rect.right(), down.top()),
    );

    let face = egui::Color32::from_gray(240);
    let edge = egui::Color32::from_gray(205);
    let glyph = if live {
        egui::Color32::from_gray(96)
    } else {
        egui::Color32::from_gray(193)
    };

    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, face);
    painter.rect_filled(track, 0.0, egui::Color32::from_gray(233));
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );

    // The arrow on each head, pointing the way that head scrolls.
    for (box_, pointing_up) in [(up, true), (down, false)] {
        let middle = box_.center();
        let reach = 3.0;
        let tip = if pointing_up { -reach } else { reach };
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(middle.x, middle.y + tip),
                egui::pos2(middle.x - reach, middle.y - tip),
                egui::pos2(middle.x + reach, middle.y - tip),
            ],
            glyph,
            egui::Stroke::NONE,
        ));
    }

    if !live {
        return 0.0;
    }

    // The thumb is as much of the track as the view is of the content, and sits
    // as far down the track as the view is down the content.
    let length = (view / content * track.height()).max(16.0);
    let travel = (track.height() - length).max(0.0);
    let top = track.top() + travel * (at / range);
    let thumb = egui::Rect::from_min_size(
        egui::pos2(track.left(), top),
        egui::vec2(track.width(), length),
    );
    ui.painter()
        .rect_filled(thumb, 0.0, egui::Color32::from_gray(205));

    let mut at = at;
    let id = ui.id().with("scrollbar");
    let dragged = ui.interact(thumb, id.with("thumb"), egui::Sense::drag());
    if dragged.dragged() && travel > 0.0 {
        at += dragged.drag_delta().y * range / travel;
    }
    // A head scrolls by exactly one row, the track by a screenful, the way
    // they do everywhere else.
    if ui.interact(up, id.with("up"), egui::Sense::click()).clicked() {
        at -= ROW;
    }
    if ui.interact(down, id.with("down"), egui::Sense::click()).clicked() {
        at += ROW;
    }
    let page = ui.interact(track, id.with("track"), egui::Sense::click());
    if page.clicked() {
        if let Some(pos) = page.interact_pointer_pos() {
            at += if pos.y < thumb.top() { -view } else { view };
        }
    }
    at.clamp(0.0, range)
}

fn draw_picking(ui: &mut egui::Ui, panel: &mut Panel) {
    let mut chosen: Option<(String, PathBuf)> = None;

    let rows = list(&panel.directory);

    // Sunk into the window on a page of its own, so it reads as a list to
    // scroll rather than as loose text: white ground, a border, and the same
    // gap to the window's edges that the naming dialog has.
    egui::Frame::new()
        .fill(egui::Color32::WHITE)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 96, 160)))
        // No margin inside the border: the scrollbar runs the full height of
        // the box and sits against its right edge, the way a list box does.
        // What needs the breathing room is the text, and that gets it below.
        .inner_margin(egui::Margin::ZERO)
        .outer_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            let visuals = ui.visuals_mut();
            visuals.override_text_color = Some(egui::Color32::from_gray(20));
            visuals.widgets.hovered.weak_bg_fill = HIGHLIGHT;
            visuals.selection.bg_fill = HIGHLIGHT;
            // The list takes everything but the bar's own column on the right.
            let whole = ui.available_rect_before_wrap();
            let bar = egui::Rect::from_min_max(
                egui::pos2(whole.right() - BAR, whole.top()),
                whole.max,
            );
            let page = egui::Rect::from_min_max(
                whole.min,
                egui::pos2(bar.left(), whole.bottom()),
            );

            let scrolled = panel.scrolled;
            let listed = ui.scope_builder(egui::UiBuilder::new().max_rect(page), |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                    )
                    .vertical_scroll_offset(scrolled)
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::same(2))
                            .show(ui, |ui| {
                                // Rows the width of the list, not the width of
                                // their text: anywhere on the line is the
                                // preset on it.
                                ui.with_layout(
                                    egui::Layout::top_down_justified(egui::Align::LEFT),
                                    |ui| {
                                        for (name, path) in &rows {
                                            if ui.selectable_label(false, name).clicked() {
                                                chosen = Some((name.clone(), path.clone()));
                                            }
                                        }
                                    },
                                );
                            });
                    })
            });
            let listed = listed.inner;

            // Where the wheel left it, unless the bar is about to move it.
            panel.scrolled = listed.state.offset.y;
            panel.scrolled = scrollbar(
                ui,
                bar,
                listed.content_size.y,
                listed.inner_rect.height(),
                panel.scrolled,
            );
        });

    if let Some((name, path)) = chosen {
        if let Ok(mut shared) = panel.shared.lock() {
            shared.load_from = Some(path);
            shared.last_name = name;
            shared.want = Want::Nothing;
        }
    }
}

// ---- shared bits ---------------------------------------------------------

fn window_settings(title: &str, width: i32, height: i32) -> egui_baseview::EguiWindowSettings {
    egui_baseview::EguiWindowSettings::new()
        .with_tile(title)
        .with_size(baseview::dpi::Size::Logical(baseview::dpi::LogicalSize {
            width: width as f64,
            height: height as f64,
        }))
        .with_scale_policy(baseview::WindowScalePolicy::SystemScaleFactor)
}

/// The host's own colour: the same blue the notepad plugin highlights with.
///
/// It marks everything here as the host rather than the plugin, which matters
/// when the two are stacked in one window.
const HIGHLIGHT: egui::Color32 = egui::Color32::from_rgb(0, 155, 255);

/// `run_ui` hands over a bare root with no panel behind it, so each of these
/// windows paints its own background and clears to the same colour.
fn paint_background(ui: &mut egui::Ui, cmds: &mut egui_baseview::ExtraOutputCommands) {
    // A frame is only drawn when something changes, and the surface keeps
    // whatever was on it in between, which showed as the chrome flickering
    // away whenever the pointer moved off it.
    ui.ctx().request_repaint();

    cmds.clear_color(egui::Rgba::from(HIGHLIGHT));
    ui.painter().rect_filled(ui.max_rect(), 0.0, HIGHLIGHT);

    // Widgets have to read against it, so they are given a face of their own
    // rather than the near-black egui starts with.
    let visuals = ui.visuals_mut();
    visuals.panel_fill = HIGHLIGHT;
    visuals.override_text_color = Some(egui::Color32::WHITE);
    visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(0, 122, 204);
    visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(64, 178, 255);
    visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(0, 96, 160);
    visuals.widgets.noninteractive.bg_stroke.color = egui::Color32::from_rgb(0, 122, 204);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0, 96, 160);
    visuals.extreme_bg_color = egui::Color32::from_rgb(0, 122, 204);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mini-host-chrome-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_name_becomes_a_file_in_the_preset_directory() {
        let dir = Path::new("/presets/Notepad");
        assert_eq!(
            path_for(dir, "mix 3"),
            Some(dir.join("mix 3.preset"))
        );
    }

    #[test]
    fn a_name_cannot_climb_out_of_the_directory() {
        let dir = Path::new("/presets/Notepad");
        let escaped = path_for(dir, "../../etc/passwd").unwrap();
        assert_eq!(escaped, dir.join("etcpasswd.preset"));
        assert!(escaped.starts_with(dir));
    }

    #[test]
    fn a_name_of_nothing_is_refused() {
        let dir = Path::new("/presets/Notepad");
        assert_eq!(path_for(dir, "   "), None);
        assert_eq!(path_for(dir, "..."), None);
        assert_eq!(path_for(dir, "/"), None);
    }

    #[test]
    fn an_empty_directory_lists_nothing() {
        assert!(list(&temp("empty")).is_empty());
    }

    #[test]
    fn a_directory_that_is_not_there_lists_nothing() {
        assert!(list(Path::new("/no/such/directory")).is_empty());
    }

    #[test]
    fn presets_are_listed_by_name_in_order_and_nothing_else_is() {
        let dir = temp("listing");
        for name in ["vocal-chain.preset", "Drums.preset", "notes.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let found = list(&dir);
        let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["Drums", "vocal-chain"]);
    }
}
