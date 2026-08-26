//! Photograph or record what is on the screen, for the UI tests.
//!
//! macOS grants the right to read the screen to an application, not to a
//! command, and it grants it wholesale: whatever holds it can photograph
//! anything on screen at any time. That is far more than a UI test needs, and
//! it is not something to hand to a general purpose tool. So this application
//! holds it instead: it photographs a region, or films one while a test
//! drags a window's corner, and that is all it can be asked for. Both are
//! `screencapture` stills, run from here so the screen is read under this
//! application's right; a film is stills taken thirty a second, made into a
//! movie by ffmpeg afterwards.
//!
//! ```text
//! window-shot shot <x> <y> <width> <height> <output.png>
//! window-shot film <x> <y> <width> <height> <output.mp4> <stop-file>
//! ```
//!
//! A film says it is rolling by writing `<output>.started`, runs until the
//! stop file appears, and says the movie is saved with `<output>.done`.
//!
//! It is started through the launcher, so nobody reads its output: it reports
//! by writing files next to what it was asked to produce, `<output>.error`
//! among them.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
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
             window-shot film <x> <y> <width> <height> <output.mp4> <stop-file>"
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

    if let Err(e) = permission() {
        return complain(e);
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

/// Record a region of the screen until the stop file appears.
///
/// There is no video recorder in this: the region is photographed over and
/// over, aiming at thirty a second and taking what `screencapture` actually
/// manages, and the stills become a movie afterwards. ffmpeg does that
/// assembly, which is reading files, the part of it that works everywhere;
/// its own screen input does not work on this macOS.
///
/// `<out>.started` says the recording is rolling, so the test does not drag
/// the window before there is anything watching, `<out>.done` says the movie
/// is saved, and `<out>.log` holds whatever the assembler had to say.
fn film(x: i32, y: i32, width: i32, height: i32, out: &Path, stop: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(out);
    let log = out.with_extension("log");
    let _ = std::fs::remove_file(&log);
    let frames = out.with_extension("frames");
    let _ = std::fs::remove_dir_all(&frames);
    std::fs::create_dir_all(&frames)
        .map_err(|e| format!("creating {}: {e}", frames.display()))?;

    // The first photograph before the rolling mark, so a missing permission
    // fails the film here rather than half way through a test.
    shot(x, y, width, height, &frames.join("frame-000000.png"))?;
    std::fs::write(out.with_extension("started"), "")
        .map_err(|e| format!("writing the started mark: {e}"))?;

    let target = Duration::from_millis(33);
    let began = Instant::now();
    let mut taken: u64 = 1;
    while !stop.exists() && began.elapsed() < Duration::from_secs(300) {
        let tick = Instant::now();
        shot(x, y, width, height, &frames.join(format!("frame-{taken:06}.png")))?;
        taken += 1;
        if let Some(rest) = target.checked_sub(tick.elapsed()) {
            sleep(rest);
        }
    }

    // The stills play back at the rate they were really taken at, so the
    // movie lasts as long as the recording did.
    let seconds = began.elapsed().as_secs_f64().max(0.001);
    let rate = (taken as f64 / seconds).max(1.0);
    let complaints = File::create(&log).map_err(|e| format!("creating {}: {e}", log.display()))?;
    let assembled = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y"])
        .args(["-framerate", &format!("{rate:.3}")])
        .args(["-i", &frames.join("frame-%06d.png").display().to_string()])
        // The encoder wants even sides and its own pixel format, whatever
        // shape the region was.
        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2,format=yuv420p"])
        .arg(out)
        .stderr(Stdio::from(complaints))
        .status()
        .map_err(|e| format!("running ffmpeg: {e}"))?;
    if !assembled.success() || movie_size(out) < 1024 {
        return Err(format!(
            "no movie was assembled from {taken} stills; ffmpeg said: {}",
            said(&log)
        ));
    }
    let _ = std::fs::remove_dir_all(&frames);
    std::fs::write(out.with_extension("done"), "")
        .map_err(|e| format!("writing the done mark: {e}"))?;
    Ok(())
}

/// How much movie is on disk, with nothing there counting as none.
fn movie_size(out: &Path) -> u64 {
    std::fs::metadata(out).map(|m| m.len()).unwrap_or(0)
}

/// What the log holds, for an error message.
fn said(log: &Path) -> String {
    match std::fs::read_to_string(log) {
        Ok(text) if !text.trim().is_empty() => text.trim().to_string(),
        _ => "nothing".to_string(),
    }
}

