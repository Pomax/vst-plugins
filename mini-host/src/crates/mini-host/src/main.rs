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

use std::ffi::c_void;
use std::path::PathBuf;
use std::process::ExitCode;

use baseview::dpi::LogicalSize;
use baseview::{
    Event, EventStatus, Window, WindowContext, WindowEvent, WindowHandler, WindowOpenOptions,
    WindowScalePolicy, WindowSize,
};
use mini_host::{presets, Module, Plugin};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// What the command line asked for.
struct Args {
    plugin: Option<PathBuf>,
    /// Where to write the plugin's state when the window closes. This is what
    /// a scripted UI test reads back to see what the interaction did.
    state: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args { plugin: None, state: None };
    let mut rest = std::env::args().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--state" => args.state = rest.next().map(PathBuf::from),
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

/// On Windows a `.vst3` is a file, so the file dialog picks it.
#[cfg(target_os = "windows")]
fn ask_for_plugin() -> Option<PathBuf> {
    dialog()
        .add_filter("VST3 plugin", &["vst3"])
        .pick_file()
}

/// Elsewhere a `.vst3` is a bundle directory, so the directory dialog picks it.
///
/// A file dialog would not do: the Finder only shows a directory as a single
/// selectable item when its extension is one it knows, when an installed
/// application has claimed that extension as a package type, or when the
/// directory's package bit is set. `.vst3` is none of those on a machine with
/// no VST3 host installed, so the bundle appears as an ordinary folder and a
/// file dialog can only open it, not choose it.
#[cfg(not(target_os = "windows"))]
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
    /// The window's own handle, for reading back where it ended up.
    handle: *mut c_void,
    /// Last position seen, written out when the window closes. Reading it at
    /// close time is too late on Windows: the window is already gone.
    position: std::cell::Cell<Option<(i32, i32)>>,
    /// Which class was instantiated, so a preset cannot be loaded into a
    /// different plugin than the one that wrote it.
    cid: [u8; 16],
    /// Shared with the strip and its dialogs.
    shared: chrome::Shared,
    /// The strip across the top. Held only to keep it open: dropping the
    /// handle closes the window.
    _toolbar: Option<baseview::WindowHandle>,
    /// The plugin's window is moved down once, after it exists.
    inset: std::cell::Cell<bool>,
    /// The dialog window, while one is open.
    dialog: std::cell::RefCell<Option<chrome::Dialog>>,
}

impl Host {
    /// Carry out whatever a preset dialog decided, on the thread that owns the
    /// plugin. `getState` and `setState` are the calls a DAW makes to write and
    /// read its project file, and this is the same pair.
    fn apply_pending(&self) {
        let (save_to, load_from) = match self.shared.lock() {
            Ok(mut shared) => (shared.save_to.take(), shared.load_from.take()),
            Err(_) => return,
        };

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

        if let (Some(said), Ok(mut shared)) = (said, self.shared.lock()) {
            shared.message = Some(said);
        }
    }

    /// Open the dialog the strip asked for, and take it away again once it has
    /// been answered or its window has been closed.
    fn follow_dialog_requests(&self) {
        let want = match self.shared.lock() {
            Ok(shared) => shared.want,
            Err(_) => return,
        };
        let mut dialog = self.dialog.borrow_mut();

        // Closing the window from its title bar answers nothing, and leaves
        // the request standing unless it is withdrawn here.
        if dialog.as_ref().is_some_and(|open| !open.is_open()) {
            *dialog = None;
            if let Ok(mut shared) = self.shared.lock() {
                shared.want = chrome::Want::Nothing;
            }
            return;
        }

        match want {
            chrome::Want::Nothing => *dialog = None,
            want if dialog.is_none() => {
                *dialog =
                    chrome::open_dialog(want, std::sync::Arc::clone(&self.shared), self.handle);
            }
            _ => {}
        }
    }
}

/// A path as it should read in the strip: just the file's name.
fn name_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl WindowHandler for Host {
    fn on_frame(&self) {
        if let Some(position) = place::get(self.handle) {
            self.position.set(Some(position));
        }
        if !self.inset.get() {
            place::inset_editor(self.handle, chrome::HEIGHT);
            // The editor exists by now, so this is where the host hands it the
            // keyboard — once, the way a DAW does.
            place::focus_editor(self.handle);
            self.inset.set(true);
        }
        self.follow_dialog_requests();
        self.apply_pending();
    }

    fn resized(&self, size: WindowSize) {
        let logical: LogicalSize<f64> = size.into();
        let _ = self.plugin.resize(logical.width as i32, logical.height as i32);
    }

    fn on_event(&self, event: Event) -> EventStatus {
        match event {
            Event::Window(WindowEvent::WillClose) => {
                if let Some(position) = self.position.get() {
                    place::save(position);
                }
                if let Some(path) = &self.state_out {
                    match self.plugin.get_state() {
                        Ok(bytes) => {
                            let _ = std::fs::write(path, bytes);
                        }
                        Err(e) => eprintln!("could not read the plugin's state: {e}"),
                    }
                }
                let _ = self.plugin.detach();
                EventStatus::Captured
            }
            _ => EventStatus::Ignored,
        }
    }
}

/// The native handle behind a baseview window.
fn native_handle(window: &WindowContext) -> Option<*mut c_void> {
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
    if let Err(e) = plugin.open_editor() {
        eprintln!("the plugin has no editor: {e}");
        return ExitCode::FAILURE;
    }

    let (width, height) = plugin.view_size().unwrap_or((900, 620));
    println!("host:   {}", path.display());
    println!("vendor: {}", module.vendor());
    println!("editor: {width}x{height}");

    let options = WindowOpenOptions::new()
        .with_title("Mini VST Host")
        .with_size(LogicalSize::new(width as f64, (height + chrome::HEIGHT) as f64))
        .with_scale_policy(WindowScalePolicy::SystemScaleFactor);

    let cid = module
        .first_audio_class()
        .and_then(|index| module.class_info2(index))
        .map(|info| info.cid)
        .unwrap_or([0u8; 16]);
    let plugin_path = path.clone();

    Window::open_blocking(options, move |window| {
        let handle = native_handle(&window);
        let shared = chrome::Shared::default();
        if let Ok(mut state) = shared.lock() {
            state.directory = presets::directory_for(&plugin_path);
        }
        // The strip is opened after the plugin, so it sits above the editor
        // rather than behind it, and the editor is moved down to make room.
        let mut toolbar = None;
        match handle {
            // Safety: the window outlives the attachment — it is closed by
            // baseview only after this handler is dropped.
            Some(parent) => {
                if let Err(e) = unsafe { plugin.attach(parent) } {
                    eprintln!("could not attach the editor: {e}");
                }
                place::inset_editor(parent, chrome::HEIGHT);
                toolbar = Some(chrome::open_bar(
                    &window,
                    presets::plugin_name(&plugin_path),
                    std::sync::Arc::clone(&shared),
                    width,
                ));
                icon::set(parent);
                if let Some((x, y)) = place::load() {
                    place::set(parent, x, y);
                }
            }
            None => eprintln!("this window has no handle the plugin can use"),
        }
        Host {
            plugin,
            state_out: args.state.clone(),
            handle: handle.unwrap_or(std::ptr::null_mut()),
            position: std::cell::Cell::new(None),
            cid,
            shared,
            _toolbar: toolbar,
            inset: std::cell::Cell::new(false),
            dialog: std::cell::RefCell::new(None),
        }
    });

    ExitCode::SUCCESS
}
