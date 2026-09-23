//! Open a VST3 plugin in a window, the way a DAW does.
//!
//! This is the minimal host with a window attached: it loads the plugin
//! binary, creates an instance through the factory, asks the edit controller
//! for its view, and gives that view a real window to live in. What appears is
//! the plugin itself, not a copy of its drawing code.
//!
//! ```text
//! mini-host [path-to-plugin-or-bundle]
//! ```
//!
//! With no path, it asks for one in a file dialog.
//!
#[path = "app/chrome.rs"]
mod chrome;
#[path = "app/icon.rs"]
mod icon;
#[path = "app/place.rs"]
mod place;
mod presets;

use std::ffi::c_void;
use std::path::PathBuf;
use std::process::ExitCode;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use vst3_loader::{Module, Plugin};

/// What the command line asked for.
struct Args {
    plugin: Option<PathBuf>,
    /// Where to write the plugin's state when the window closes. This is what
    /// a scripted UI test reads back to see what the interaction did.
    state: Option<PathBuf>,
    /// Where to write what this window and the plugin's window inside it
    /// measure. The plugin's editor is a child window, and on macOS a child
    /// window is a subview, which nothing outside this process can measure.
    geometry: Option<PathBuf>,
    /// A preset to restore before the plugin's window is opened, which is the
    /// order a DAW opening a project does it in: `setState`, then the editor.
    preset: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args { plugin: None, state: None, geometry: None, preset: None };
    let mut rest = std::env::args().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--state" => args.state = rest.next().map(PathBuf::from),
            "--geometry" => args.geometry = rest.next().map(PathBuf::from),
            "--preset" => args.preset = rest.next().map(PathBuf::from),
            _ => args.plugin = Some(PathBuf::from(arg)),
        }
    }
    args
}

/// The plugin to open: the path given on the command line, or one chosen in a
/// file dialog when there was none.
///
/// Returns `None` only when the dialog was cancelled.
fn locate_plugin(args: &Args) -> Option<PathBuf> {
    args.plugin.clone().or_else(ask_for_plugin)
}

/// On Windows a `.vst3` is a file, and on macOS it is a bundle, which the
/// system reports as a package: an item of type `public.data` rather than a
/// folder. Both are files to a file dialog, so both are picked by one.
///
/// A directory dialog would not do on macOS. It can only choose directories,
/// and a package is not one, so the bundle is shown greyed out and cannot be
/// chosen at all.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn ask_for_plugin() -> Option<PathBuf> {
    dialog()
        .add_filter("VST3 plugin", &["vst3"])
        .pick_file()
}

/// Elsewhere a `.vst3` is an ordinary directory, so the directory dialog
/// picks it.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn ask_for_plugin() -> Option<PathBuf> {
    dialog().pick_folder()
}

/// Opens where the host was started from, rather than wherever the platform
/// last left a dialog.
fn dialog() -> rfd::FileDialog {
    let mut dialog = rfd::FileDialog::new().set_title("Open a VST3 plugin");
    if let Ok(cwd) = std::env::current_dir() {
        dialog = dialog.set_directory(cwd);
    }
    dialog
}

/// Keeps the plugin alive for as long as the window it is attached to.
struct Host {
    plugin: Plugin,
    /// Written when the window closes, for a scripted test to read.
    state_out: Option<PathBuf>,
    /// Written every frame, for a scripted test to read what this window and
    /// the plugin's window inside it measure.
    geometry_out: Option<PathBuf>,
    /// The host window's own handle, which the plugin's window is a child of.
    handle: *mut c_void,
    /// Which class was instantiated, so a preset cannot be loaded into a
    /// different plugin than the one that wrote it.
    cid: [u8; 16],
    /// The name shown in the strip when there is nothing else to say.
    name: String,
    /// What the strip and its dialogs are working on.
    state: chrome::State,
    /// The dialog's own scratch, while one is open.
    panel: Option<chrome::Panel>,
    /// Where the open dialog was put, worked out once when it opens: asking for
    /// the same place every frame is what lets it be dragged somewhere else.
    dialog_at: Option<egui::Pos2>,
    /// Whether that place has been corrected for the size of the window's own
    /// frame, which is only known once the window is there.
    dialog_placed: bool,
    /// The size the plugin's window was last given, so it is only moved when
    /// something has changed.
    editor: Option<(i32, i32)>,
    /// The plugin is handed the keyboard once, after its window exists.
    focused: bool,
    /// Whether the plugin has been told the window is going away.
    closed: bool,
}

impl Host {
    /// Carry out whatever a preset dialog decided. `getState` and `setState`
    /// are the calls a DAW makes to write and read its project file, and this
    /// is the same pair.
    fn apply_pending(&mut self) {
        let (save_to, load_from) = (self.state.save_to.take(), self.state.load_from.take());

        let mut said = None;
        if let Some(path) = save_to {
            said = Some(match self.plugin.get_state() {
                Ok(state) => match presets::save(&path, self.cid, &state) {
                    Ok(()) => format!("saved {}", name_of(&path)),
                    Err(e) => format!("could not save: {e}"),
                },
                Err(e) => format!("the plugin would not give up its state: {e}"),
            });
        }
        if let Some(path) = load_from {
            said = Some(match presets::load(&path) {
                Ok((cid, _)) if cid != self.cid => {
                    "that preset belongs to a different plugin".to_string()
                }
                Ok((_, state)) => match self.plugin.set_state(&state) {
                    Ok(()) => format!("loaded {}", name_of(&path)),
                    Err(e) => format!("the plugin refused the preset: {e}"),
                },
                Err(e) => format!("could not read the preset: {e}"),
            });
        }

        if let Some(said) = said {
            self.state.message = Some(said);
        }
    }

    /// Show the dialog the strip asked for, in a window of its own.
    ///
    /// A viewport is a real window: it has a title bar, it can be dragged
    /// anywhere, and it lives for exactly as long as this keeps asking for it.
    /// While one is up the host's window stops taking input, which is what
    /// makes the dialog modal.
    fn follow_dialog_requests(&mut self, ctx: &egui::Context) {
        let Some((title, (width, height))) = chrome::dialog_window(self.state.want) else {
            if self.panel.take().is_some() {
                self.dialog_at = None;
                self.dialog_placed = false;
                place::set_enabled(self.handle, true);
            }
            return;
        };

        if self.panel.is_none() {
            self.panel = Some(chrome::Panel::new(&self.state));
            self.dialog_at = centred_over(ctx, width as f32, height as f32);
            self.dialog_placed = false;
            place::set_enabled(self.handle, false);
        }
        let host = ctx.input(|i| i.viewport().outer_rect);

        let mut builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([width as f32, height as f32])
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false);
        if let Some(at) = self.dialog_at {
            builder = builder.with_position(at);
        }

        // Borrowed apart, so the closure can have these without borrowing all
        // of the host.
        let Host { state, panel, dialog_at, dialog_placed, .. } = self;
        let Some(panel) = panel.as_mut() else { return };

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("mini-host-dialog"),
            builder,
            |ui, _class| {
                egui::CentralPanel::no_frame()
                    .frame(egui::Frame::new().fill(chrome::HIGHLIGHT))
                    .show(ui, |ui| chrome::draw_dialog(ui, panel, state));
                // The title bar's close button answers nothing.
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    state.want = chrome::Want::Nothing;
                }

                // A position is where the window's frame goes, and what should
                // land in the middle is the room inside the frame: a title bar
                // at the top and a border around the rest, which is the
                // platform's business and is only known once the window is
                // there. So the centring is put right here, once. After that
                // the window stays where it is, including wherever it has been
                // dragged to.
                if !*dialog_placed {
                    let mine = ui.ctx().input(|i| i.viewport().outer_rect);
                    if let (Some(host), Some(mine)) = (host, mine) {
                        let (inner_w, inner_h) = (width as f32, height as f32);
                        let border = (mine.width() - inner_w) / 2.0;
                        let title = mine.height() - inner_h - border;
                        *dialog_at = Some(egui::pos2(
                            host.center().x - inner_w / 2.0 - border,
                            host.center().y - inner_h / 2.0 - title,
                        ));
                        *dialog_placed = true;
                    }
                }
            },
        );
    }

    /// Tell the plugin what size it is now.
    ///
    /// Where its window goes is not decided here: it follows the host's window
    /// from inside the resize itself, which `place::track_editor` arranges once
    /// and for all. This is the plugin being told what happened, so it can lay
    /// its editor out and keep the size with the project. Being a frame late
    /// with that is fine; being a frame late with the window is not.
    fn tell_plugin_the_size(&mut self, ctx: &egui::Context) {
        let Some(rect) = ctx.input(|i| i.viewport().inner_rect) else {
            return;
        };
        let size = (rect.width() as i32, rect.height() as i32);
        if self.editor == Some(size) {
            return;
        }
        self.editor = Some(size);
        let _ = self.plugin.resize(size.0, size.1 - chrome::HEIGHT);
        // The view has already been stretched by this point; this makes it
        // take the new size now, in this frame, not whenever it next draws.
        place::follow_resize(self.handle);
        if !self.focused {
            // The editor exists by now, so this is where the host hands it the
            // keyboard, once, the way a DAW does.
            place::focus_editor(self.handle);
            self.focused = true;
        }
    }

    /// Hand back the plugin's state and let it go. Called once, whichever way
    /// the window closes.
    fn finish(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(path) = &self.state_out {
            match self.plugin.get_state() {
                Ok(bytes) => {
                    let _ = std::fs::write(path, bytes);
                }
                Err(e) => eprintln!("could not read the plugin's state: {e}"),
            }
        }
        let _ = self.plugin.detach();
    }
}

impl Host {
    /// Write down where this window is and what it and the plugin's window
    /// inside it measure.
    ///
    /// A test after a drag cannot know what size the window ended up, but it
    /// can say the plugin still fills it: `inset` is how much of the window
    /// the plugin does not cover, on each side.
    ///
    /// `window` is the whole window, frame and title bar included, where it
    /// sits on the desktop. Anything outside the process that has to aim at
    /// this window, a screen recorder among them, needs that and cannot get
    /// it as reliably from anywhere else.
    fn report_geometry(&self, ctx: &egui::Context) {
        let Some(path) = &self.geometry_out else {
            return;
        };
        let Some(rect) = ctx.input(|i| i.viewport().inner_rect) else {
            return;
        };
        let (width, height) = (rect.width().round() as i32, rect.height().round() as i32);
        let Some((x, y, editor_width, editor_height)) = place::editor_in_host(self.handle) else {
            return;
        };
        let mut text = String::new();
        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
            text.push_str(&format!(
                "window={},{},{}x{}\n",
                outer.min.x.round() as i32,
                outer.min.y.round() as i32,
                outer.width().round() as i32,
                outer.height().round() as i32,
            ));
        }
        text.push_str(&format!(
            "host={width}x{height}\n\
             editor={x},{y},{editor_width}x{editor_height}\n\
             inset={x},{y},{},{}\n",
            width - (x + editor_width),
            height - (y + editor_height),
        ));
        let _ = std::fs::write(path, text);
    }
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("strip")
            .exact_size(chrome::HEIGHT as f32)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(chrome::HIGHLIGHT))
            .show(ui, |ui| chrome::draw_bar(ui, &mut self.state, &self.name));

        let ctx = ui.ctx().clone();
        self.tell_plugin_the_size(&ctx);
        self.follow_dialog_requests(&ctx);
        self.apply_pending();
        self.report_geometry(&ctx);

        if ctx.input(|i| i.viewport().close_requested()) {
            self.finish();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.finish();
    }
}

/// A path as it should read in the strip: just the file's name.
fn name_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// What the strip calls the plug-in: the name it gives itself, then the file
/// it came from in brackets.
///
/// The two are not the same thing and both are worth seeing. The name is what
/// the plug-in reports over VST3, which is what a DAW lists and which carries
/// its version. The file is what was loaded off disk, which a rename can
/// change without the plug-in knowing.
fn strip_label(reported: Option<&str>, path: &std::path::Path) -> String {
    let file = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let reported = reported.map(str::trim).filter(|name| !name.is_empty());
    match (reported, file.is_empty()) {
        (Some(reported), false) => format!("{reported} ({file})"),
        (Some(reported), true) => reported.to_string(),
        (None, _) => file,
    }
}

/// Where a dialog of this size sits to be centred on the host's window.
///
/// A window of its own opens wherever the platform decides, which is not over
/// the window that asked for it. A dialog belongs to what it is asking about,
/// so it opens on top of it.
fn centred_over(ctx: &egui::Context, width: f32, height: f32) -> Option<egui::Pos2> {
    let host = ctx.input(|i| i.viewport().outer_rect)?;
    Some(egui::pos2(
        host.center().x - width / 2.0,
        host.center().y - height / 2.0,
    ))
}

/// The native handle behind the application's window.
fn native_handle<W: HasWindowHandle>(window: &W) -> Option<*mut c_void> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut c_void),
        RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()),
        RawWindowHandle::Xcb(h) => Some(h.window.get() as usize as *mut c_void),
        _ => None,
    }
}

fn main() -> ExitCode {
    let args = parse_args();
    let Some(path) = locate_plugin(&args) else {
        return ExitCode::SUCCESS;
    };

    let module = match Module::load(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let mut plugin = match module.create_plugin() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("could not create the plugin: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(preset) = &args.preset {
        let restored = presets::load(preset)
            .map_err(|e| format!("could not read {}: {e}", preset.display()))
            .and_then(|(_, state)| {
                plugin
                    .set_state(&state)
                    .map_err(|e| format!("the plugin refused {}: {e}", preset.display()))
            });
        if let Err(e) = restored {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = plugin.open_editor() {
        eprintln!("the plugin has no editor: {e}");
        return ExitCode::FAILURE;
    }

    let (width, height) = plugin.view_size().unwrap_or((900, 620));
    println!("host:   {}", path.display());
    println!("vendor: {}", module.vendor());
    println!("editor: {width}x{height}");

    let cid = module
        .first_audio_class()
        .and_then(|index| module.class_info2(index))
        .map(|info| info.cid)
        .unwrap_or([0u8; 16]);
    let label = strip_label(
        module
            .first_audio_class()
            .and_then(|index| module.class_info(index))
            .map(|(name, _)| name)
            .as_deref(),
        &path,
    );
    let plugin_path = path.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mini VST Host")
            .with_inner_size([width as f32, (height + chrome::HEIGHT) as f32])
            .with_icon(icon::image()),
        // Where the window was left is the application's to remember, and
        // eframe already does it.
        persist_window: true,
        // Under test, which is what --geometry or --state marks, the memory is
        // a scratch file beside the test's own files instead of the
        // application's: a remembered size would start every test at whatever
        // size the last run left the window, and a test's clicks are written
        // against the size the plugin asked for. eframe restores a stored
        // window whether or not it may store one, so the store itself has to
        // be the fresh thing.
        persistence_path: args
            .geometry
            .as_ref()
            .or(args.state.as_ref())
            .map(|p| p.with_extension("memory")),
        ..Default::default()
    };

    let run = eframe::run_native(
        "mini-host",
        options,
        Box::new(move |cc| {
            let handle = native_handle(cc);
            if let Err(e) = plugin.attach(cc) {
                eprintln!("could not attach the editor: {e}");
            }
            match handle {
                Some(parent) => {
                    // Put it under the strip, and keep it there for every
                    // resize from here on without anything having to watch.
                    place::inset_editor(parent, chrome::HEIGHT);
                    place::track_editor(parent, chrome::HEIGHT);
                }
                None => eprintln!("this window has no handle the plugin can use"),
            }
            let mut state = chrome::State::default();
            state.directory = presets::directory_for(&plugin_path);
            Ok(Box::new(Host {
                plugin,
                state_out: args.state.clone(),
                geometry_out: args.geometry.clone(),
                handle: handle.unwrap_or(std::ptr::null_mut()),
                cid,
                name: label,
                state,
                panel: None,
                dialog_at: None,
                dialog_placed: false,
                editor: None,
                focused: false,
                closed: false,
            }))
        }),
    );

    if let Err(e) = run {
        eprintln!("could not open a window: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::strip_label;
    use std::path::Path;

    #[test]
    fn the_strip_shows_the_reported_name_then_the_file() {
        assert_eq!(
            strip_label(
                Some("Markdown Notes 11.0.0"),
                Path::new("/plugins/Markdown Notes.vst3")
            ),
            "Markdown Notes 11.0.0 (Markdown Notes.vst3)"
        );
    }

    /// The path is often the binary inside the bundle, and the strip has room
    /// for a file name, not for a path.
    #[test]
    fn only_the_file_name_is_shown_never_the_path() {
        let inside = Path::new("/plugins/Markdown Notes.vst3/Contents/x86_64-win/Markdown Notes.vst3");
        assert_eq!(
            strip_label(Some("Markdown Notes 11.0.0"), inside),
            "Markdown Notes 11.0.0 (Markdown Notes.vst3)"
        );
    }

    /// A plug-in that reports no name, or a factory that refuses to say,
    /// leaves the file as the only thing to call it.
    #[test]
    fn a_plugin_that_names_itself_nothing_is_called_by_its_file() {
        let path = Path::new("/plugins/Whatever.vst3");
        assert_eq!(strip_label(None, path), "Whatever.vst3");
        assert_eq!(strip_label(Some("   "), path), "Whatever.vst3");
    }
}
