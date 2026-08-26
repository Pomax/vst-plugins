//! Photograph or record what is on the screen, for the UI tests.
//!
//! macOS grants the right to read the screen to an application, not to a
//! command, and it grants it wholesale: whatever holds it can photograph
//! anything on screen at any time. That is far more than a UI test needs, and
//! it is not something to hand to a general purpose tool. So this application
//! holds it instead: it photographs a region, or records one while a test
//! drags a window's corner, and that is all it can be asked for.
//!
//! ```text
//! window-shot shot <x> <y> <width> <height> <output.png>
//! window-shot film <x> <y> <width> <height> <output.mov> <stop-file>
//! ```
//!
//! A film runs until the stop file appears, and `<output>.done` says it is
//! finished.
//!
//! It is started through the launcher, so nobody reads its output: it reports
//! by writing files next to what it was asked to produce, `<output>.error`
//! among them.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread::sleep;
use std::time::{Duration, Instant};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

enum Task {
    Shot {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        out: PathBuf,
    },
    Film {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        out: PathBuf,
        stop: PathBuf,
    },
}

fn parse() -> Option<Task> {
    let mut raw = std::env::args().skip(1);
    match raw.next()?.as_str() {
        "shot" => Some(Task::Shot {
            x: raw.next()?.parse().ok()?,
            y: raw.next()?.parse().ok()?,
            width: raw.next()?.parse().ok()?,
            height: raw.next()?.parse().ok()?,
            out: PathBuf::from(raw.next()?),
        }),
        "film" => Some(Task::Film {
            x: raw.next()?.parse().ok()?,
            y: raw.next()?.parse().ok()?,
            width: raw.next()?.parse().ok()?,
            height: raw.next()?.parse().ok()?,
            out: PathBuf::from(raw.next()?),
            stop: PathBuf::from(raw.next()?),
        }),
        _ => None,
    }
}

fn main() -> ExitCode {
    let Some(task) = parse() else {
        eprintln!(
            "usage: window-shot shot <x> <y> <width> <height> <output.png>\n       \
             window-shot film <x> <y> <width> <height> <output.mov> <stop-file>"
        );
        return ExitCode::FAILURE;
    };

    let out = match &task {
        Task::Shot { out, .. } => out.clone(),
        Task::Film { out, .. } => out.clone(),
    };
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let complain = |message: String| -> ExitCode {
        eprintln!("{message}");
        let _ = std::fs::write(out.with_extension("error"), message);
        ExitCode::FAILURE
    };

    // Only photographing needs the screen recording right here: a film is
    // QuickTime Player's recording, made under QuickTime's own permission.
    if matches!(task, Task::Shot { .. }) {
        if let Err(e) = permission() {
            return complain(e);
        }
    }

    let result = match task {
        Task::Shot {
            x,
            y,
            width,
            height,
            out,
        } => shot(x, y, width, height, &out),
        Task::Film {
            x,
            y,
            width,
            height,
            out,
            stop,
        } => film(x, y, width, height, &out, &stop),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => complain(e),
    }
}

/// Make sure this application may read the screen, asking if it may not.
///
/// The request raises the system's prompt, and the answer only takes effect
/// for a process launched after it was given, so there is nothing to wait
/// for: without the grant this run is already lost, and the next one has it.
fn permission() -> Result<(), String> {
    if unsafe { CGPreflightScreenCaptureAccess() } {
        return Ok(());
    }
    unsafe { CGRequestScreenCaptureAccess() };
    sleep(Duration::from_secs(2));
    if unsafe { CGPreflightScreenCaptureAccess() } {
        return Ok(());
    }
    Err("Window Shot may not read the screen. Allow it in System Settings, \
         under Privacy & Security, \"Screen & System Audio Recording\"; the \
         grant takes effect on the next run."
        .to_string())
}

/// Photograph a region of the screen, exactly as it looks.
///
/// A region rather than a window: what is on screen at the window's place
/// includes whatever sits on top of it, and a file dialog does, from a
/// process of its own. Photographing the window alone leaves the dialog out
/// of the picture, which is how a test came to pass on a picture that did not
/// match the screen.
fn shot(x: i32, y: i32, width: i32, height: i32, out: &Path) -> Result<(), String> {
    let status = Command::new("screencapture")
        .arg("-x")
        .arg(format!("-R{x},{y},{width},{height}"))
        .arg(out)
        .status()
        .map_err(|e| format!("running screencapture: {e}"))?;
    if !status.success() || !out.exists() {
        return Err(format!("could not photograph {width}x{height} at {x},{y}"));
    }
    Ok(())
}

/// Record the screen until the stop file appears.
///
/// QuickTime Player makes the recording: it is the system's recorder and
/// holds its own permission to read the screen. This program only starts and
/// stops it, so the right to control QuickTime belongs to this program. The
/// movie is of the whole screen; the analysis crops the wanted region out of
/// every frame afterwards, which is why the region arguments go unused here.
///
/// `<out>.started` says the recording is rolling, so the test does not drag
/// the window before there is anything watching, and `<out>.done` says the
/// movie is saved.
fn film(
    _x: i32,
    _y: i32,
    _width: i32,
    _height: i32,
    out: &Path,
    stop: &Path,
) -> Result<(), String> {
    let _ = std::fs::remove_file(out);

    // Bring up the recording overlay and stand back. The region and the
    // Record button are the test driver's to set and press, because it is the
    // one holding the pointer; the recording is stopped with the system's
    // stop keystroke, also the driver's. What is left for this program is
    // saving the movie the recording opens in the player.
    quicktime("tell application \"QuickTime Player\" to new screen recording")?;
    sleep(Duration::from_millis(1500));
    std::fs::write(out.with_extension("overlay"), "")
        .map_err(|e| format!("writing the overlay mark: {e}"))?;

    let too_long = Instant::now() + Duration::from_secs(300);
    while !stop.exists() && Instant::now() < too_long {
        sleep(Duration::from_millis(100));
    }

    // The stopped recording opens as a document, but not instantly.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let saved = quicktime(&format!(
            "tell application \"QuickTime Player\"
                save front document in POSIX file \"{}\"
                close front document saving no
            end tell",
            out.display()
        ));
        if saved.is_ok() && out.exists() {
            break;
        }
        if Instant::now() > deadline {
            return Err(match saved {
                Err(e) => e,
                Ok(()) => "QuickTime never saved the recording".to_string(),
            });
        }
        sleep(Duration::from_millis(500));
    }
    std::fs::write(out.with_extension("done"), "")
        .map_err(|e| format!("writing the done mark: {e}"))?;
    Ok(())
}

/// Hand QuickTime Player a script and report what it said when it refused.
fn quicktime(script: &str) -> Result<(), String> {
    let out = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("running osascript: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "QuickTime would not do it: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}
