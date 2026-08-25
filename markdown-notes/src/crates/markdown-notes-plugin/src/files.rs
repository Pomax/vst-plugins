//! Open / Save / Save As, with native dialogs.
//!
//! Shared by the key handler and the GUI's toolbar buttons so both behave
//! identically. Every entry point takes the shared editor and returns an error
//! message rather than logging, leaving presentation to the caller.

use std::sync::{Arc, Mutex};

use markdown_notes_core::Command;

use crate::Shared;

/// Where a dialog running off the drawing thread leaves its error message.
pub type Report = Arc<Mutex<Option<String>>>;

/// Run a host-level command on a thread of its own.
///
/// A native dialog is modal: it spins its own event loop until dismissed.
/// Opening one from inside a frame re-enters the drawing code — the window's
/// timer keeps firing, egui is asked to lay out a frame while it is already
/// laying one out, and the plugin comes down. So the dialog goes on its own
/// thread and the drawing thread returns immediately.
///
/// Anything to report lands in `report`, which the GUI shows on a later frame.
pub fn perform_off_thread(editor: &Shared, report: &Report, command: Command) {
    let editor = Arc::clone(editor);
    let report = Arc::clone(report);
    std::thread::spawn(move || {
        let message = perform(&editor, command);
        if let Ok(mut slot) = report.lock() {
            *slot = message;
        }
    });
}

/// Run a host-level command. Returns an error message if it failed; `None`
/// means it succeeded or the user cancelled the dialog.
///
/// The editor mutex is never held while a dialog is open: a modal dialog spins
/// its own event loop, and holding the lock across it would block every other
/// thread that touches the document.
pub fn perform(editor: &Shared, command: Command) -> Option<String> {
    match command {
        Command::Open => {
            let path = dialog().pick_file()?;
            match editor.lock() {
                Ok(mut ed) => ed
                    .open_path(&path)
                    .err()
                    .map(|e| format!("could not open {}: {e}", path.display())),
                Err(_) => Some("editor is unavailable".into()),
            }
        }
        Command::Save => {
            let has_path = editor.lock().map(|e| e.file.is_some()).unwrap_or(false);
            if !has_path {
                // Nothing to save over yet, so Save behaves as Save As.
                return perform(editor, Command::SaveAs);
            }
            match editor.lock() {
                Ok(mut ed) => ed.save().err().map(|e| format!("could not save: {e}")),
                Err(_) => Some("editor is unavailable".into()),
            }
        }
        Command::SaveAs => {
            let suggested = editor
                .lock()
                .ok()
                .and_then(|e| {
                    e.file
                        .as_ref()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                })
                .unwrap_or_else(|| "Untitled.md".to_string());

            let path = dialog().set_file_name(suggested).save_file()?;
            match editor.lock() {
                Ok(mut ed) => ed
                    .save_as(&path)
                    .err()
                    .map(|e| format!("could not save {}: {e}", path.display())),
                Err(_) => Some("editor is unavailable".into()),
            }
        }
    }
}

fn dialog() -> rfd::FileDialog {
    rfd::FileDialog::new()
        .set_title("Markdown Notes")
        .add_filter("Markdown", markdown_notes_core::file::MARKDOWN_EXTENSIONS)
        .add_filter("All files", &["*"])
}
