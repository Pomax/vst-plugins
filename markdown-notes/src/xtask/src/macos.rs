//! The window driver for macOS.
//!
//! Windows has `tools/capture-window.ps1`, a program to shell out to. macOS
//! has no equivalent, so the same job is done here: launch the host, find its
//! window, deliver real clicks and keystrokes to it, photograph it, and close
//! it so the program writes its state on the way out.
//!
//! Input goes through `CGEvent`, which is what a keyboard and mouse produce.
//! Window geometry and closing go through System Events, which is the only way
//! to ask another process about its windows.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

use crate::drawn::Place;
use crate::locate::{self, Finder, Named};

/// A window's place on screen, in points, top left origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    fn right(&self) -> i32 {
        self.x + self.width
    }

    fn bottom(&self) -> i32 {
        self.y + self.height
    }
}

/// Somewhere on the screen for a window `width` by `height` that overlaps
/// `taken` nowhere: to its right, to its left, under it or above it, the
/// first of those with the room. A file carried from one window to another
/// must cross no part of the window it came from on the way.
fn room_beside(taken: Rect, screen: Rect, width: i32, height: i32) -> Result<(i32, i32), String> {
    let tries = [
        (taken.right(), taken.y),
        (taken.x - width, taken.y),
        (taken.x, taken.bottom()),
        (taken.x, taken.y - height),
    ];
    for (x, y) in tries {
        let x = x.clamp(screen.x, (screen.right() - width).max(screen.x));
        let y = y.clamp(screen.y, (screen.bottom() - height).max(screen.y));
        let clear = x >= taken.right()
            || x + width <= taken.x
            || y >= taken.bottom()
            || y + height <= taken.y;
        if clear {
            return Ok((x, y));
        }
    }
    Err(format!(
        "no room on the screen for a {width}x{height} window beside the one under test"
    ))
}

fn source() -> Result<CGEventSource, String> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "could not open an event source; grant Accessibility permission".to_string())
}

fn post(event: CGEvent) {
    event.post(CGEventTapLocation::HID);
}

fn mouse(kind: CGEventType, at: CGPoint) -> Result<(), String> {
    mouse_with(kind, at, CGMouseButton::Left, 1)
}

/// A mouse event for `button`, `clicks` deep: the second press of a double
/// click is a click of two.
fn mouse_with(
    kind: CGEventType,
    at: CGPoint,
    button: CGMouseButton,
    clicks: i64,
) -> Result<(), String> {
    let event = CGEvent::new_mouse_event(source()?, kind, at, button)
        .map_err(|_| "could not make a mouse event".to_string())?;
    // A press with no click state is not a click. Windows work out what was
    // clicked, and how many times, from this field, and a zero means the event
    // is delivered and then ignored: the pointer moves, and nothing else
    // happens.
    let pressing = matches!(
        kind,
        CGEventType::LeftMouseDown
            | CGEventType::LeftMouseUp
            | CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDown
            | CGEventType::RightMouseUp
    );
    if pressing {
        let number = match button {
            CGMouseButton::Right => 1,
            _ => 0,
        };
        event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, clicks);
        event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, number);
    }
    post(event);
    Ok(())
}

/// Two clicks where a finger makes them, the second marked as the second,
/// which is how a window here knows a double click.
fn double_click(x: i32, y: i32) -> Result<(), String> {
    click(x, y)?;
    let at = point(x, y);
    mouse_with(CGEventType::LeftMouseDown, at, CGMouseButton::Left, 2)?;
    sleep(Duration::from_millis(120));
    mouse_with(CGEventType::LeftMouseUp, at, CGMouseButton::Left, 2)?;
    sleep(Duration::from_millis(120));
    Ok(())
}

/// A click and then a press that stays held, until `let_go`: the start of a
/// drag that selects by words.
fn double_hold(x: i32, y: i32) -> Result<(), String> {
    click(x, y)?;
    mouse_with(CGEventType::LeftMouseDown, point(x, y), CGMouseButton::Left, 2)?;
    sleep(Duration::from_millis(150));
    Ok(())
}

/// A click with the right button.
fn right_click(x: i32, y: i32) -> Result<(), String> {
    let at = point(x, y);
    mouse(CGEventType::MouseMoved, at)?;
    sleep(Duration::from_millis(120));
    mouse_with(CGEventType::RightMouseDown, at, CGMouseButton::Right, 1)?;
    sleep(Duration::from_millis(120));
    mouse_with(CGEventType::RightMouseUp, at, CGMouseButton::Right, 1)?;
    sleep(Duration::from_millis(120));
    Ok(())
}

fn point(x: i32, y: i32) -> CGPoint {
    CGPoint::new(x as f64, y as f64)
}

/// Where the pointer is now.
fn pointer() -> Result<CGPoint, String> {
    let event = CGEvent::new(source()?).map_err(|_| "could not read the pointer".to_string())?;
    Ok(event.location())
}

/// Move the pointer there the way a hand does, across rather than by jumping.
///
/// A window being dragged reads where the pointer is over and over. Put it
/// somewhere in one step and the window is asked to go straight from one place
/// to another, which is nothing like a drag and tests none of the drawing.
fn glide(from: CGPoint, to: CGPoint, over: Duration, dragging: bool) -> Result<(), String> {
    let steps = (over.as_millis() / 16).max(1) as i32;
    let kind = if dragging {
        CGEventType::LeftMouseDragged
    } else {
        CGEventType::MouseMoved
    };
    for step in 1..=steps {
        let at = CGPoint::new(
            from.x + (to.x - from.x) * step as f64 / steps as f64,
            from.y + (to.y - from.y) * step as f64 / steps as f64,
        );
        mouse(kind, at)?;
        sleep(Duration::from_millis(16));
    }
    Ok(())
}

/// A click the window can see: the pointer moves, settles, presses, and only
/// then releases. Sent back to back, the press and release land in one frame
/// and are missed.
fn click(x: i32, y: i32) -> Result<(), String> {
    let at = point(x, y);
    mouse(CGEventType::MouseMoved, at)?;
    sleep(Duration::from_millis(120));
    mouse(CGEventType::LeftMouseDown, at)?;
    sleep(Duration::from_millis(120));
    mouse(CGEventType::LeftMouseUp, at)?;
    sleep(Duration::from_millis(120));
    Ok(())
}

/// Press where the pointer is, having glided there, and keep holding.
fn take_hold(x: i32, y: i32) -> Result<(), String> {
    let to = point(x, y);
    glide(pointer()?, to, Duration::from_millis(200), false)?;
    sleep(Duration::from_millis(150));
    mouse(CGEventType::LeftMouseDown, to)?;
    sleep(Duration::from_millis(150));
    Ok(())
}

/// Move the pointer somewhere in the time a hand would take, button or no
/// button. Whether the button is down decides which event the window gets:
/// a moved event during a drag is dropped by AppKit.
fn move_to(x: i32, y: i32, dragging: bool) -> Result<(), String> {
    let to = point(x, y);
    glide(pointer()?, to, Duration::from_millis(400), dragging)?;
    sleep(Duration::from_millis(150));
    Ok(())
}

fn let_go(x: i32, y: i32) -> Result<(), String> {
    move_to(x, y, true)?;
    mouse(CGEventType::LeftMouseUp, point(x, y))?;
    sleep(Duration::from_millis(200));
    Ok(())
}

/// A key going down and coming back up, with whatever modifiers are held,
/// taking the time one keystroke takes.
fn tap(code: u16, flags: CGEventFlags) -> Result<(), String> {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(source()?, code, down)
            .map_err(|_| format!("could not make a key event for {code}"))?;
        event.set_flags(flags);
        post(event);
        sleep(Duration::from_millis(15));
    }
    sleep(KEYSTROKE.saturating_sub(Duration::from_millis(30)));
    Ok(())
}

/// Hold a modifier, tap a key, let go, as a keyboard does it.
///
/// Setting the flag on the key event alone is not enough. A window that
/// watches the modifier keys themselves never sees one go down, so the
/// keystroke arrives as the plain letter and the shortcut does nothing.
fn shortcut(modifier: u16, flags: CGEventFlags, key: u16) -> Result<(), String> {
    let event = CGEvent::new_keyboard_event(source()?, modifier, true)
        .map_err(|_| "could not make a key event".to_string())?;
    event.set_flags(flags);
    post(event);
    sleep(Duration::from_millis(60));

    tap(key, flags)?;

    let event = CGEvent::new_keyboard_event(source()?, modifier, false)
        .map_err(|_| "could not make a key event".to_string())?;
    event.set_flags(CGEventFlags::empty());
    post(event);
    sleep(Duration::from_millis(60));
    Ok(())
}

const CONTROL: u16 = 59;
const SHIFT: u16 = 56;
const OPTION: u16 = 58;
const COMMAND: u16 = 55;

/// Give a native file dialog a path, the way this platform's dialogs take one.
///
/// Typing a `/` into a file dialog here does not put a slash in the name
/// field: it opens the "go to" sheet. So a typed path has to go through that
/// sheet deliberately. A path that exists is given whole, which leaves it
/// selected; one that does not exist yet is a save, so the sheet gets its
/// directory and the name field gets its name. Either way the test's own
/// Enter still confirms the dialog, the same keystroke it is on Windows.
fn dialog_path(finder: &Finder, area: Rect, work: &Path, path: &str) -> Result<(), String> {
    // Numbered pictures of every stage, because none of this is visible in
    // the test's own output when it goes wrong.
    let stage = {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DIALOGS: AtomicUsize = AtomicUsize::new(0);
        DIALOGS.fetch_add(1, Ordering::Relaxed)
    };
    let picture = |step: &str| -> PathBuf {
        work.join(format!("dialog-{stage}-{step}.png"))
    };

    // Precondition: a file dialog is actually open. Every open and save
    // panel has a Cancel button; until that is on screen, every keystroke
    // below would land in the editor instead of a dialog. So it is looked
    // for, not assumed, and nothing is typed until it is seen.
    let opened = picture("opened");
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if find_on_screen(finder, area, "Cancel", &opened)?.is_some() {
            break;
        }
        if Instant::now() > deadline {
            return Err(format!(
                "no file dialog is open, so there is nowhere to type a path; see {}",
                opened.display()
            ));
        }
        sleep(Duration::from_millis(300));
    }

    let both = CGEventFlags::CGEventFlagCommand | CGEventFlags::CGEventFlagShift;
    for (code, down) in [(COMMAND, true), (SHIFT, true)] {
        let event = CGEvent::new_keyboard_event(source()?, code, down)
            .map_err(|_| "could not make a key event".to_string())?;
        event.set_flags(both);
        post(event);
        sleep(Duration::from_millis(40));
    }
    tap(code_for('g').unwrap_or(5), both)?;
    for code in [SHIFT, COMMAND] {
        let event = CGEvent::new_keyboard_event(source()?, code, false)
            .map_err(|_| "could not make a key event".to_string())?;
        event.set_flags(CGEventFlags::empty());
        post(event);
        sleep(Duration::from_millis(40));
    }
    sleep(Duration::from_millis(800));
    let _ = shot(area, &picture("go-to-sheet"));

    if Path::new(path).exists() {
        write(path)?;
        sleep(Duration::from_millis(400));
        let typed = picture("path-typed");
        let tail = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if find_on_screen(finder, area, &tail, &typed)?.is_none() {
            return Err(format!(
                "the path never appeared in the dialog; see {}",
                typed.display()
            ));
        }
        tap(RETURN, CGEventFlags::empty())?;
        sleep(Duration::from_millis(800));
        let _ = shot(area, &picture("confirmed"));
        return Ok(());
    }

    let split = Path::new(path);
    let directory = split
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/".to_string());
    // The bare name: the panel owns the extension. It proposes a name with
    // the extension handled per its file type, and appends that type to
    // whatever the field holds, so a name given as `x.md` is saved as
    // `x.md.md`.
    let name = split
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    write(&directory)?;
    sleep(Duration::from_millis(400));
    let typed = picture("directory-typed");
    let tail = Path::new(&directory)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if find_on_screen(finder, area, &tail, &typed)?.is_none() {
        return Err(format!(
            "the directory never appeared in the dialog; see {}",
            typed.display()
        ));
    }
    tap(RETURN, CGEventFlags::empty())?;
    sleep(Duration::from_millis(800));
    let _ = shot(area, &picture("directory-confirmed"));
    // The name field holds the dialog's proposed name; replace it.
    let select_all = code_for('a').ok_or("no key code for the letter a")?;
    shortcut(COMMAND, CGEventFlags::CGEventFlagCommand, select_all)?;
    write(&name)?;
    sleep(Duration::from_millis(200));
    let named = picture("named");
    if find_on_screen(finder, area, &name, &named)?.is_none() {
        return Err(format!(
            "the name never appeared in the dialog; see {}",
            named.display()
        ));
    }
    Ok(())
}

/// How long one keystroke takes: distinct keystrokes rather than a machine
/// flooding the queue, fast enough that a run does not crawl.
const KEYSTROKE: Duration = Duration::from_millis(20);

/// Type text as text rather than as keys, at a human pace.
///
/// A key event carries the characters it produced, and setting them directly
/// is what makes the layout somebody happens to be using irrelevant: the
/// window receives what the test said to type. One character per keystroke:
/// a whole line arriving in a single event is nothing like typing, and the
/// editor under test is allowed to care about the difference.
fn write(text: &str) -> Result<(), String> {
    for character in text.chars() {
        let mut buffer = [0u8; 4];
        let typed = character.encode_utf8(&mut buffer);
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(source()?, 0, down)
                .map_err(|_| "could not make a key event".to_string())?;
            event.set_string(typed);
            post(event);
            sleep(Duration::from_millis(10));
        }
        sleep(KEYSTROKE.saturating_sub(Duration::from_millis(20)));
    }
    Ok(())
}

const RETURN: u16 = 36;
const ESCAPE: u16 = 53;
const BACKSPACE: u16 = 51;
const END: u16 = 119;
const HOME: u16 = 115;
const LEFT: u16 = 123;
const UP: u16 = 126;
const DOWN: u16 = 125;
const PGUP: u16 = 116;
const TAB: u16 = 48;

/// The virtual key code of a key a `type:` step names in braces, or a
/// `shortcut:` step names after its modifier.
fn named(name: &str) -> Option<u16> {
    Some(match name.to_ascii_uppercase().as_str() {
        "ENTER" => RETURN,
        "ESC" => ESCAPE,
        "BS" | "BACKSPACE" => BACKSPACE,
        "END" => END,
        "HOME" => HOME,
        "LEFT" => LEFT,
        "UP" => UP,
        "DOWN" => DOWN,
        "PGUP" => PGUP,
        "TAB" => TAB,
        _ => return None,
    })
}

/// The virtual key code for a letter or digit on the ANSI layout.
///
/// Only needed for shortcuts: a modifier and a key, where the character the key
/// produces is not what the window is being told about.
fn code_for(key: char) -> Option<u16> {
    Some(match key {
        'a' => 0, 's' => 1, 'd' => 2, 'f' => 3, 'h' => 4, 'g' => 5, 'z' => 6,
        'x' => 7, 'c' => 8, 'v' => 9, 'b' => 11, 'q' => 12, 'w' => 13, 'e' => 14,
        'r' => 15, 'y' => 16, 't' => 17, 'o' => 31, 'u' => 32, 'i' => 34,
        'p' => 35, 'l' => 37, 'j' => 38, 'k' => 40, 'n' => 45, 'm' => 46,
        '1' => 18, '2' => 19, '3' => 20, '4' => 21, '5' => 23, '6' => 22,
        '7' => 26, '8' => 28, '9' => 25, '0' => 29,
        _ => return None,
    })
}

/// Send one `type:` step, which is text with named keys written in braces.
///
/// `{ENTER}`, `{ESC}`, `{HOME}`, `{END}`, `{LEFT}`, `{UP}`, `{DOWN}`, `{PGUP}`
/// and `{TAB}` are those keys. `{BS 40}` is backspace forty times. `{(}` and
/// `{)}` are the brackets themselves, which would otherwise be read as the
/// start of a name. This is the notation the tests are already written in,
/// and it is Windows `SendKeys`.
fn send_keys(step: &str) -> Result<(), String> {
    let mut literal = String::new();
    let mut rest = step;
    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("no closing brace: {step}"))?;
        let name = &after[..close];
        rest = &after[close + 1..];

        if name == "(" || name == ")" {
            literal.push_str(name);
            continue;
        }

        write(&literal)?;
        literal.clear();

        let (name, times) = match name.split_once(' ') {
            Some((name, count)) => (
                name,
                count
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| format!("not a repeat count: {name} {count}"))?,
            ),
            None => (name, 1),
        };
        let code = named(name).ok_or_else(|| format!("unknown key: {name}"))?;
        for _ in 0..times {
            tap(code, CGEventFlags::empty())?;
        }
    }
    literal.push_str(rest);
    write(&literal)
}

/// Ask System Events about a process's front window.
fn osascript(script: &str) -> Result<String, String> {
    let out = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("running osascript: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Where a window of this process sits, by title.
///
/// An empty title means whichever window is at the front, which is what the
/// window under test is until a dialog opens on top of it.
fn window_rect(pid: u32, title: &str) -> Result<Rect, String> {
    let which = if title.is_empty() {
        "front window".to_string()
    } else {
        format!("(first window whose name is {title:?})")
    };
    placed(&osascript(&format!(
        "tell application \"System Events\" to tell (first process whose unix id is {pid}) \
         to get {{position, size}} of {which}"
    ))?)
}

/// A window's place as System Events answers it: position, then size.
fn placed(answer: &str) -> Result<Rect, String> {
    let numbers: Vec<i32> = answer
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    if numbers.len() != 4 {
        return Err(format!("could not read the window's place: {answer:?}"));
    }
    Ok(Rect {
        x: numbers[0],
        y: numbers[1],
        width: numbers[2],
        height: numbers[3],
    })
}

/// Where Finder's window on a folder is. Finder titles the window with the
/// folder's name.
fn finder_window(folder: &str) -> Result<Rect, String> {
    placed(&osascript(&format!(
        "tell application \"System Events\" to tell process \"Finder\" \
         to get {{position, size}} of (first window whose name is {folder:?})"
    ))?)
}

/// Put Finder's window on a folder at a place on the screen, at a size.
fn place_finder_window(folder: &str, x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
    osascript(&format!(
        "tell application \"System Events\" to tell process \"Finder\" \
         to tell (first window whose name is {folder:?})\n\
         set position to {{{x}, {y}}}\n\
         set size to {{{width}, {height}}}\n\
         end tell"
    ))
    .map(|_| ())
}

/// Close Finder's windows on a folder.
fn close_finder_window(folder: &str) -> Result<(), String> {
    osascript(&format!(
        "tell application \"Finder\" to close (every window whose name is {folder:?})"
    ))
    .map(|_| ())
}

/// The screen, as Finder measures it: left, top, right, bottom.
fn screen_rect() -> Result<Rect, String> {
    let answer = osascript("tell application \"Finder\" to get bounds of window of desktop")?;
    let numbers: Vec<i32> = answer
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    if numbers.len() != 4 {
        return Err(format!("could not read the screen's size: {answer:?}"));
    }
    Ok(Rect {
        x: numbers[0],
        y: numbers[1],
        width: numbers[2] - numbers[0],
        height: numbers[3] - numbers[1],
    })
}

/// Wait for a window to exist, and say where it is.
fn wait_for_window(pid: u32, title: &str, patience: Duration) -> Result<Rect, String> {
    let deadline = Instant::now() + patience;
    let mut last = String::new();
    while Instant::now() < deadline {
        match window_rect(pid, title) {
            Ok(rect) if rect.width > 0 && rect.height > 0 => return Ok(rect),
            Ok(_) => {}
            Err(e) => last = e,
        }
        sleep(Duration::from_millis(200));
    }
    Err(if last.is_empty() {
        format!("no window appeared for process {pid}")
    } else {
        format!("no window appeared for process {pid}: {last}")
    })
}

/// Bring a process to the front, so keystrokes reach it rather than whatever
/// the person at the machine last clicked on.
fn front(pid: u32) -> Result<(), String> {
    osascript(&format!(
        "tell application \"System Events\" to set frontmost of \
         (first process whose unix id is {pid}) to true"
    ))
    .map(|_| ())
}

/// Give a window a drawable area of exactly this size.
fn set_size(pid: u32, width: i32, height: i32) -> Result<(), String> {
    osascript(&format!(
        "tell application \"System Events\" to tell (first process whose unix id is {pid}) \
         to set size of front window to {{{width}, {height}}}"
    ))
    .map(|_| ())
}

/// Where the host itself says its window is, from its geometry file.
///
/// The host measures its own window and writes it down every frame, so this
/// is where the window is now rather than where it was when the run started.
/// The numbers are the same ones System Events gives, and cost no permission.
fn reported_window(reported: &Path) -> Option<Rect> {
    let text = std::fs::read_to_string(reported).ok()?;
    let line = text.lines().find(|line| line.starts_with("window="))?;
    let (_, place) = line.split_once('=')?;
    let (position, size) = place.rsplit_once(',')?;
    let (x, y) = position.split_once(',')?;
    let (width, height) = size.split_once('x')?;
    Some(Rect {
        x: x.trim().parse().ok()?,
        y: y.trim().parse().ok()?,
        width: width.trim().parse().ok()?,
        height: height.trim().parse().ok()?,
    })
}

/// The drawable size the host itself reports, from its geometry file.
fn reported_host_size(reported: &Path) -> Option<(i32, i32)> {
    let text = std::fs::read_to_string(reported).ok()?;
    let line = text.lines().find(|line| line.starts_with("host="))?;
    let (_, size) = line.split_once('=')?;
    let (width, height) = size.split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

/// The drawable height the host itself reports, from its geometry file.
fn reported_host_height(reported: &Path) -> Option<i32> {
    reported_host_size(reported).map(|(_, height)| height)
}

/// Ask a window to close, so the program runs its shutdown.
///
/// The state a UI test asserts on is written on the way out, so killing the
/// process instead would leave nothing to assert on.
fn ask_to_close(pid: u32) -> Result<(), String> {
    osascript(&format!(
        "tell application \"System Events\" to tell (first process whose unix id is {pid}) \
         to click (first button of front window whose subrole is \"AXCloseButton\")"
    ))
    .map(|_| ())
}

/// Start the window photographer on a task and wait for what it writes.
///
/// Reading the screen is a permission macOS grants to one application,
/// wholesale. `binaries/Window Shot.app` is the application that holds it for
/// these tests: it can photograph a window of a named process or record a
/// region during a drag, and nothing else, so nothing running the tests can
/// read the screen in general. It is started through the launcher so the
/// permission is its own rather than inherited, which means nobody reads its
/// output: it reports by writing files. `wait_for` names the file that says
/// the task is under way or done.
///
/// The first ever run raises the system's permission prompt and then waits
/// for the answer, so the deadline is generous.
fn photograph(args: &[&str], wait_for: &Path, out: &Path) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let complaint = out.with_extension("error");
    let _ = std::fs::remove_file(out);
    let _ = std::fs::remove_file(&complaint);

    let status = Command::new("open")
        .arg("-n")
        .arg("-a")
        .arg(photographer()?)
        .arg("--args")
        .args(args)
        .status()
        .map_err(|e| format!("running the window photographer: {e}"))?;
    if !status.success() {
        return Err("the window photographer would not start".to_string());
    }

    let deadline = Instant::now() + Duration::from_secs(200);
    let mut said_waiting = false;
    while Instant::now() < deadline {
        if wait_for.exists() {
            return Ok(());
        }
        if complaint.exists() {
            sleep(Duration::from_millis(200));
            return Err(std::fs::read_to_string(&complaint)
                .unwrap_or_else(|_| "the window photographer failed".to_string()));
        }
        if !said_waiting && Instant::now() > deadline - Duration::from_secs(190) {
            said_waiting = true;
            println!(
                "waiting: if \"Window Shot\" is asking to read the screen, allow it; \
                 the run continues on its own"
            );
        }
        sleep(Duration::from_millis(100));
    }
    Err(format!("the window photographer wrote nothing at {}", wait_for.display()))
}

/// Photograph the screen where a window is, dialogs on top of it included.
///
/// A region rather than the window itself: a file dialog belongs to a system
/// process of its own, so a picture of just the window leaves out the dialog
/// the test opened. The screen is what the user sees, and the picture is of
/// the screen.
fn shot(area: Rect, to: &Path) -> Result<(), String> {
    photograph(
        &[
            "shot",
            &area.x.to_string(),
            &area.y.to_string(),
            &area.width.to_string(),
            &area.height.to_string(),
            &to.display().to_string(),
        ],
        to,
        to,
    )
}

/// Where the window photographer is, built if it is not there yet.
fn photographer() -> Result<PathBuf, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or("no directory above this project")?
        .to_path_buf();
    let app = root.join("binaries").join("Window Shot.app");
    if app.exists() {
        return Ok(app);
    }
    let project = root.join("tools").join("window-shot");
    let status = Command::new("sh")
        .arg("build.sh")
        .current_dir(&project)
        .status()
        .map_err(|e| format!("building the window photographer: {e}"))?;
    if !status.success() {
        return Err("building the window photographer failed".to_string());
    }
    app.exists()
        .then_some(app)
        .ok_or_else(|| "the window photographer did not build".to_string())
}

/// The colour a test picture is filled with, the same one
/// `tools/capture-window.ps1` fills its pictures with.
const PICTURE_COLOUR: [u8; 3] = [200, 40, 160];

/// Write a picture for a test to paste or drop.
fn write_picture(path: &Path, width: u32, height: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(path, crate::png::solid(width, height, PICTURE_COLOUR))
        .map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Put a picture on the clipboard, as a screenshot tool leaves one.
///
/// Under both the types it may be asked for: the PNG as it is on disk, and the
/// TIFF AppKit makes of it. Whatever reads the clipboard decides which one it
/// looks for, and a pasteboard holds as many representations as are put on it.
/// Nothing is put there as text: a paste of a picture is the key alone.
fn put_on_clipboard(path: &Path) -> Result<(), String> {
    use objc2::AnyThread;
    use objc2_app_kit::{NSImage, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF};
    use objc2_foundation::NSData;

    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let held = NSData::with_bytes(&bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &held)
        .ok_or_else(|| format!("{} is not a picture", path.display()))?;
    let tiff = image
        .TIFFRepresentation()
        .ok_or_else(|| format!("{} cannot be held as a TIFF", path.display()))?;

    let board = NSPasteboard::generalPasteboard();
    unsafe {
        board.clearContents();
        if !board.setData_forType(Some(&held), NSPasteboardTypePNG) {
            return Err("the clipboard would not take the picture".to_string());
        }
        board.setData_forType(Some(&tiff), NSPasteboardTypeTIFF);
    }
    Ok(())
}

/// A path written in a test file, as this platform spells it.
///
/// The tests were written on Windows and separate directories with a
/// backslash. Left alone, `%CACHE%\shot.png` names one file with a backslash
/// in it rather than a file in that directory.
fn path_of(value: &str) -> PathBuf {
    PathBuf::from(value.replace('\\', "/"))
}

/// The centre of the first place `text` is on screen inside `area`, in
/// screen points, or nothing when it is not on screen.
fn find_on_screen(finder: &Finder, area: Rect, text: &str, probe: &Path) -> Result<Option<(i32, i32)>, String> {
    Ok(find_box_on_screen(finder, area, text, probe)?.map(|found| {
        (found.x + found.width / 2, found.y + found.height / 2)
    }))
}

/// The box of the first place `text` is on screen inside `area`.
fn find_box_on_screen(finder: &Finder, area: Rect, text: &str, probe: &Path) -> Result<Option<Rect>, String> {
    shot(area, probe)?;
    let Some(first) = finder.all(probe, text, None)?.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(on_screen(area, locate::width_of(probe)?, first)))
}

/// The display's scale: the width of a picture of `area`, in the display's
/// pixels, over the area's width in points.
fn scale_of(area: Rect, picture_width: u32) -> f64 {
    picture_width as f64 / area.width.max(1) as f64
}

/// A box in a picture of `area`, on the screen.
fn on_screen(area: Rect, picture_width: u32, place: Place) -> Rect {
    let scale = scale_of(area, picture_width);
    Rect {
        x: area.x + (place.x as f64 / scale) as i32,
        y: area.y + (place.y as f64 / scale) as i32,
        width: (place.width as f64 / scale) as i32,
        height: (place.height as f64 / scale) as i32,
    }
}

/// A point in a picture of `area`, on the screen.
fn point_on_screen(area: Rect, picture_width: u32, point: (i32, i32)) -> (i32, i32) {
    let scale = scale_of(area, picture_width);
    (
        area.x + (point.0 as f64 / scale) as i32,
        area.y + (point.1 as f64 / scale) as i32,
    )
}

/// How long a place is looked for before a step gives up on it.
const PATIENCE: Duration = Duration::from_secs(5);

/// The text on the clipboard, line ends as `\n`.
fn clipboard_text() -> String {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};

    let board = NSPasteboard::generalPasteboard();
    let held = unsafe { board.stringForType(NSPasteboardTypeString) };
    held.map(|text| text.to_string()).unwrap_or_default().replace("\r\n", "\n")
}

/// Where the text finder is, built if it is not there yet.
fn finder() -> Result<PathBuf, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or("no directory above this project")?
        .to_path_buf();
    let tool = root.join("binaries").join("find-text");
    if tool.exists() {
        return Ok(tool);
    }
    let status = Command::new("sh")
        .arg(root.join("tools").join("find-text").join("build.sh"))
        .status()
        .map_err(|e| format!("building the text finder: {e}"))?;
    if !status.success() {
        return Err("building the text finder failed".to_string());
    }
    tool.exists()
        .then_some(tool)
        .ok_or_else(|| "the text finder did not build".to_string())
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateScrollWheelEvent(
        source: *const std::ffi::c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
    ) -> *mut std::ffi::c_void;
    fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
    fn CFRelease(value: *mut std::ffi::c_void);
    fn CGPreflightPostEventAccess() -> u8;
    fn CGRequestPostEventAccess() -> u8;
}

/// Whether this process may post clicks and keystrokes, asking macOS for the
/// permission when it may not.
///
/// An event posted without it is dropped without a word: the window opens,
/// nothing is typed, and the test fails a few seconds later on what it could
/// not find. Asking first puts the system's own prompt on screen instead,
/// naming the application the tests were run from, and the run stops here
/// until that has been allowed.
fn may_post_input() -> Result<(), String> {
    if unsafe { CGPreflightPostEventAccess() } != 0 {
        return Ok(());
    }
    if unsafe { CGRequestPostEventAccess() } != 0 {
        return Ok(());
    }
    Err("this application may not post keystrokes or clicks, so no test can \
         run. macOS has just been asked to allow it. If a prompt came up, \
         allow it and run the tests again. If none did, macOS already holds \
         an entry for this application and never asks twice: remove it with \
         `tccutil reset Accessibility <the terminal's bundle id>`, and the \
         next run will ask"
        .to_string())
}

/// Scroll the list under the pointer by a few lines, the way a wheel does.
///
/// The crate wraps no scroll events, so this one comes straight from the
/// framework: unit 1 is lines, tap 0 is where hardware input arrives, and a
/// negative wheel turns towards the bottom of the list.
fn scroll_down(area: Rect) -> Result<(), String> {
    mouse(
        CGEventType::MouseMoved,
        point(area.x + area.width / 2, area.y + area.height / 2),
    )?;
    sleep(Duration::from_millis(120));
    unsafe {
        let event = CGEventCreateScrollWheelEvent(std::ptr::null(), 1, 1, -3);
        if event.is_null() {
            return Err("could not make a scroll event".to_string());
        }
        CGEventPost(0, event);
        CFRelease(event);
    }
    sleep(Duration::from_millis(400));
    Ok(())
}

/// The name of the cursor currently on screen, as far as the stock set goes.
///
/// A window says what it wants the cursor to be, and the only way to check it
/// from outside is to ask the system what is on screen and compare it with the
/// cursors everything draws from. They are compared by their pictures: two
/// `NSCursor` objects for the same cursor are not the same object.
// `currentSystemCursor` is deprecated in favour of asking ScreenCaptureKit to
// draw the cursor into a capture. That answers a different question: this needs
// the cursor's identity, not a picture of the screen with it in.
#[allow(deprecated)]
fn cursor_now() -> String {
    use objc2_app_kit::{NSApplication, NSCursor};

    // Asking for the application object is what brings AppKit up. Without it
    // every stock cursor is NULL.
    let Some(main) = objc2::MainThreadMarker::new() else {
        return "unreadable".to_string();
    };
    let _ = NSApplication::sharedApplication(main);

    let picture = |cursor: &NSCursor| -> Option<Vec<u8>> {
        Some(cursor.image().TIFFRepresentation()?.to_vec())
    };

    let Some(shown) = NSCursor::currentSystemCursor() else {
        return "unreadable".to_string();
    };
    let Some(shown) = picture(&shown) else {
        return "unreadable".to_string();
    };

    let stock: [(&str, objc2::rc::Retained<NSCursor>); 4] = [
        ("ibeam", NSCursor::IBeamCursor()),
        ("arrow", NSCursor::arrowCursor()),
        ("hand", NSCursor::pointingHandCursor()),
        ("grabbing", NSCursor::closedHandCursor()),
    ];
    for (name, cursor) in stock {
        if picture(&cursor).is_some_and(|known| known == shown) {
            return name.to_string();
        }
    }
    "other".to_string()
}

/// Start the host on a plugin and wait for its window to be up and in front.
fn launch(
    program: &Path,
    plugin: &Path,
    state: &Path,
    reported: &Path,
    extra: &[String],
    settle: Duration,
) -> Result<(Child, u32, Rect), String> {
    let child = Command::new(program)
        .arg(plugin)
        .arg("--state")
        .arg(state)
        .arg("--geometry")
        .arg(reported)
        .args(extra)
        .spawn()
        .map_err(|e| format!("running {}: {e}", program.display()))?;
    let pid = child.id();
    wait_for_window(pid, "", Duration::from_secs(30)).map_err(|e| {
        format!("{e}\n      (System Events needs Accessibility permission for this terminal)")
    })?;
    front(pid)?;
    sleep(settle);
    let rect = window_rect(pid, "")?;
    println!(
        "window: {},{} {}x{}",
        rect.x, rect.y, rect.width, rect.height
    );
    Ok((child, pid, rect))
}



/// How many frames the movie holds.
///
/// Counting distinct frames throws away every frame identical to the one
/// before it: a settling window draws a few distinct frames, a flickering one
/// draws a new frame every time it is looked at. The movie is already of the
/// window's region and nothing else, because the recording selection was
/// drawn to exactly that region.
fn film_frames(video: &Path, distinct_only: bool) -> Result<u64, String> {
    let filter = if distinct_only { "mpdecimate" } else { "null" };
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-i"])
        .arg(video)
        .args(["-vf", filter, "-f", "null", "-"])
        .output()
        .map_err(|e| format!("running ffmpeg: {e}"))?;
    let said = String::from_utf8_lossy(&out.stderr);
    let mut seen = 0;
    for line in said.lines() {
        let Some(at) = line.find("frame=") else {
            continue;
        };
        let rest = line[at + 6..].trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(count) = digits.parse() {
            seen = count;
        }
    }
    Ok(seen)
}

/// A recording in progress, started by `film:` and ended by `endfilm:`.
///
/// QuickTime Player records it: a real movie of the screen, which is analysed
/// after the fact by cropping the window's region out of every frame and
/// counting how many differ.
struct Film {
    to: PathBuf,
    video: PathBuf,
    stop: PathBuf,
    done: PathBuf,
}

struct Run {
    child: Child,
    pid: u32,
    /// The text finder, which reads the pictures the steps take.
    finder: Finder,
    /// How many times `restart:` has started the host again.
    restarts: usize,
    /// What the run was started with, so `restart:` can do it again.
    program: PathBuf,
    plugin: PathBuf,
    state: PathBuf,
    settle: Duration,
    /// Where the host writes what it and the plugin inside it measure.
    reported: PathBuf,
    /// The window under test, which is what is photographed at the end
    /// whichever window the steps were last addressing.
    host: Rect,
    /// The window the steps are addressing now.
    rect: Rect,
    /// Whether the mouse button is currently held down by a `hold:` step.
    holding: bool,
    /// The title of the window the steps are addressing, empty for the one
    /// under test.
    title: String,
    /// Whether a `kill:` step force quit the host, which is the one way it
    /// may be gone without that being a failure.
    killed: bool,
    film: Option<Film>,
}

impl Run {
    fn start(
        program: &Path,
        plugin: &Path,
        state: &Path,
        reported: &Path,
        settle: Duration,
    ) -> Result<Run, String> {
        let finder = Finder::new(finder()?);
        let (child, pid, rect) = launch(program, plugin, state, reported, &[], settle)?;
        Ok(Run {
            child,
            pid,
            finder,
            restarts: 0,
            program: program.to_path_buf(),
            plugin: plugin.to_path_buf(),
            state: state.to_path_buf(),
            settle,
            reported: reported.to_path_buf(),
            host: rect,
            rect,
            holding: false,
            title: String::new(),
            killed: false,
            film: None,
        })
    }

    /// Close the host and start it again.
    ///
    /// What a preset has to survive is the host going away, so a test that
    /// only ever loads one back into the process that saved it is not testing
    /// the thing.
    ///
    /// The state the old run wrote is put aside first, as `STATE.1` for the
    /// first run and `STATE.2` for the second, where the runner checks it
    /// against the expectations written before the step. It is moved rather
    /// than left, so whatever is asserted afterwards can only have come from
    /// the new run.
    ///
    /// `extra` is `ARG|ARG`, arguments the host is started with after its own,
    /// for a host that is to come back up differently from how it first
    /// started. They are paths as a test file writes them.
    fn restart(&mut self, extra: &str) -> Result<(), String> {
        self.finish()?;
        self.restarts += 1;
        if self.state.exists() {
            let mut aside = self.state.clone().into_os_string();
            aside.push(format!(".{}", self.restarts));
            std::fs::rename(&self.state, &aside)
                .map_err(|e| format!("putting {} aside: {e}", self.state.display()))?;
        }
        let extra: Vec<String> = extra
            .split('|')
            .map(str::trim)
            .filter(|arg| !arg.is_empty())
            .map(|arg| arg.replace('\\', "/"))
            .collect();
        let (child, pid, rect) = launch(
            &self.program,
            &self.plugin,
            &self.state,
            &self.reported,
            &extra,
            self.settle,
        )?;
        self.child = child;
        self.pid = pid;
        self.host = rect;
        self.rect = rect;
        self.holding = false;
        self.title = String::new();
        self.killed = false;
        Ok(())
    }

    /// Where the window the steps are addressing is now.
    fn area(&self) -> Rect {
        window_rect(self.pid, &self.title).unwrap_or(self.rect)
    }

    /// A file beside the test's own for a look at the window.
    fn probe(&self, name: &str) -> Result<PathBuf, String> {
        self.state
            .parent()
            .map(|work| work.join(name))
            .ok_or_else(|| "nowhere to put the look".to_string())
    }

    /// The point on screen a WHERE phrase names, looked for in fresh
    /// pictures of the window until it is there. A failure says what the
    /// window reads instead.
    fn resolve(&self, named: &Named, step: &str) -> Result<(i32, i32), String> {
        let probe = self.probe("point.png")?;
        let deadline = Instant::now() + PATIENCE;
        loop {
            let area = self.area();
            shot(area, &probe)?;
            if let Some(point) = self.finder.point_in(&probe, named)? {
                let width = locate::width_of(&probe)?;
                let _ = std::fs::remove_file(&probe);
                return Ok(point_on_screen(area, width, point));
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "nothing is at \"{named}\" for {step}; the window reads: {}; see {}",
                    self.finder.reads(&probe),
                    probe.display()
                ));
            }
        }
    }

    fn resolve_point(&self, value: &str, step: &str) -> Result<(i32, i32), String> {
        self.resolve(&locate::split_named(value), step)
    }

    /// The box on screen of the named words, looked for in fresh pictures of
    /// the window until it is there. A failure says what the window reads
    /// instead.
    fn find_words(&self, named: &Named, step: &str) -> Result<Rect, String> {
        let probe = self.probe("words.png")?;
        let deadline = Instant::now() + PATIENCE;
        loop {
            let area = self.area();
            shot(area, &probe)?;
            if let Some(words) = self.finder.first(&probe, named)? {
                let width = locate::width_of(&probe)?;
                let _ = std::fs::remove_file(&probe);
                return Ok(on_screen(area, width, words));
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "\"{named}\" is not on screen for {step}; the window reads: {}; see {}",
                    self.finder.reads(&probe),
                    probe.display()
                ));
            }
        }
    }

    /// Close the host and hold it to a clean exit.
    ///
    /// A crash on the way out is a failure like any other: the checks all
    /// passing and the program then dying is not a pass.
    fn finish(&mut self) -> Result<(), String> {
        if let Ok(Some(status)) = self.child.try_wait() {
            if self.killed || status.success() {
                return Ok(());
            }
            return Err(format!("the host quit on its own: {status}"));
        }
        let _ = ask_to_close(self.pid);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                if status.success() {
                    return Ok(());
                }
                return Err(format!("the host crashed while closing: {status}"));
            }
            sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        Err("the window would not close".to_string())
    }
}

/// Run one test's steps against a freshly launched host.
pub fn drive(
    _root: &Path,
    host: &Path,
    plugin: &Path,
    steps: &Path,
    state: &Path,
    shot_to: &Path,
) -> Result<(), String> {
    let settle = Duration::from_millis(4000);
    let text = std::fs::read_to_string(steps)
        .map_err(|e| format!("reading {}: {e}", steps.display()))?;
    let reported = steps.with_extension("geometry");

    may_post_input()?;
    let mut run = Run::start(host, plugin, state, &reported, settle)?;
    let result = play(&mut run, &text, shot_to);
    // A test that failed halfway can leave the recorder rolling, and it keeps
    // rolling until it is told to stop.
    if let Some(film) = run.film.take() {
        let _ = std::fs::write(&film.stop, "");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !film.done.exists() && Instant::now() < deadline {
            sleep(Duration::from_millis(200));
        }
    }
    let closed = run.finish();
    result.and(closed)
}

fn play(run: &mut Run, text: &str, shot_to: &Path) -> Result<(), String> {
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (kind, value) = match line.split_once(':') {
            Some((kind, value)) => (kind, value),
            None => (line, ""),
        };
        step(run, kind, value)?;
    }

    // Always the window under test, whichever window the steps were last
    // addressing. A killed host has no window left to photograph.
    if run.child.try_wait().map(|s| s.is_some()).unwrap_or(false) {
        return Ok(());
    }
    let area = window_rect(run.pid, "").unwrap_or(run.host);
    shot(area, shot_to)
}

fn pair(value: &str, what: &str) -> Result<(i32, i32), String> {
    let (x, y) = value
        .split_once(',')
        .ok_or_else(|| format!("cannot read {what}: {value}"))?;
    let x = x
        .trim()
        .parse()
        .map_err(|_| format!("cannot read {what}: {value}"))?;
    let y = y
        .trim()
        .parse()
        .map_err(|_| format!("cannot read {what}: {value}"))?;
    Ok((x, y))
}

fn step(run: &mut Run, kind: &str, value: &str) -> Result<(), String> {
    match kind {
        // A typed absolute path is dialog input: no test types a path as
        // document text, and this platform's dialogs do not take a path
        // through their name field. The tests spell paths with backslashes,
        // which here are ordinary characters, so they are swapped first.
        "type" if value.starts_with('/') => {
            let work = run
                .state
                .parent()
                .map(Path::to_path_buf)
                .ok_or("nowhere to put the dialog pictures")?;
            dialog_path(&run.finder, run.rect, &work, &value.replace('\\', "/"))
        }
        "type" => send_keys(value),
        "wait" => {
            let ms: u64 = value
                .trim()
                .parse()
                .map_err(|_| format!("not a number of milliseconds: {value}"))?;
            sleep(Duration::from_millis(ms));
            Ok(())
        }
        "click" => {
            // `click:WHERE` clicks the point WHERE names.
            let (x, y) = run.resolve_point(value, "click")?;
            let area = run.area();
            click(x, y)?;
            sleep(Duration::from_millis(500));
            println!("click:  {value} at {},{}", x - area.x, y - area.y);
            Ok(())
        }
        "hold" => {
            // `hold:WHERE` presses there and keeps holding, so the steps
            // after it happen mid-drag.
            let (x, y) = run.resolve_point(value, "hold")?;
            run.holding = true;
            take_hold(x, y)?;
            println!("hold:   {value}");
            Ok(())
        }
        "moveto" => {
            // `moveto:WHERE` moves the pointer there, button or no button.
            let (x, y) = run.resolve_point(value, "moveto")?;
            move_to(x, y, run.holding)?;
            println!("moveto: {value}");
            Ok(())
        }
        "letgo" => {
            // `letgo:` releases the button where the pointer is;
            // `letgo:WHERE` moves there first.
            if value.trim().is_empty() {
                mouse(CGEventType::LeftMouseUp, pointer()?)?;
            } else {
                let (x, y) = run.resolve_point(value, "letgo")?;
                let_go(x, y)?;
            }
            run.holding = false;
            sleep(Duration::from_millis(400));
            println!("letgo:  {value}");
            Ok(())
        }
        "shortcut" => {
            let (modifier, key) = value
                .trim()
                .to_ascii_lowercase()
                .split_once('+')
                .map(|(m, k)| (m.to_string(), k.to_string()))
                .ok_or_else(|| format!("cannot read shortcut: {value}"))?;
            // A letter is its own key; a named key is looked up.
            let mut letters = key.chars();
            let code = match (letters.next(), letters.next()) {
                (Some(letter), None) => code_for(letter),
                _ => named(&key),
            }
            .ok_or_else(|| format!("unknown key: {key}"))?;

            // Cut, copy and paste belong to the platform, not to the plugin:
            // it claims the letter so it is not typed and waits for the
            // clipboard event the platform sends. Here the key that produces
            // that event is Cmd, so Ctrl over one of those letters would
            // reach the document as nothing at all. The plugin's own
            // shortcuts, select-all among them, stay on Ctrl.
            let clipboard = matches!(key.as_str(), "c" | "x" | "v");
            let (held, flags) = match modifier.as_str() {
                "ctrl" if clipboard => (COMMAND, CGEventFlags::CGEventFlagCommand),
                "ctrl" => (CONTROL, CGEventFlags::CGEventFlagControl),
                "shift" => (SHIFT, CGEventFlags::CGEventFlagShift),
                "alt" => (OPTION, CGEventFlags::CGEventFlagAlternate),
                "cmd" | "command" => (COMMAND, CGEventFlags::CGEventFlagCommand),
                other => return Err(format!("unknown modifier: {other}")),
            };
            shortcut(held, flags, code)?;
            sleep(Duration::from_millis(300));
            Ok(())
        }
        "window" => {
            let wanted = value.trim();
            let rect = wait_for_window(run.pid, wanted, Duration::from_secs(5))?;
            front(run.pid)?;
            run.rect = rect;
            run.title = wanted.to_string();
            println!(
                "window: {} at {},{} {}x{}",
                if wanted.is_empty() { "under test" } else { wanted },
                rect.x,
                rect.y,
                rect.width,
                rect.height
            );
            Ok(())
        }
        "nowindow" => {
            // `nowindow:NAME` fails while a window of that name is still
            // there. Closing one is an action like any other: something has
            // to say it happened, and the next `window:` cannot, since with
            // no name it takes whatever is in front, dialog included.
            let wanted = value.trim();
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if window_rect(run.pid, wanted).is_err() {
                    println!("nowindow: {wanted}");
                    return Ok(());
                }
                if Instant::now() > deadline {
                    return Err(format!("the {wanted} window is still open"));
                }
                sleep(Duration::from_millis(200));
            }
        }
        "resize" => {
            // `resize:W,H` gives the window a drawable area of exactly that
            // size, matching what dragging its corner sets. The size a window
            // is set to counts its title bar, and how tall that is belongs to
            // the platform, so the host's own report of its drawable area says
            // how far off the first attempt was and the second corrects it.
            let (width, height) = pair(value, "resize")?;
            set_size(run.pid, width, height)?;
            sleep(Duration::from_millis(700));
            if let Some(reported) = reported_host_height(&run.reported) {
                let off = height - reported;
                if off != 0 {
                    set_size(run.pid, width, height + off)?;
                    sleep(Duration::from_millis(700));
                }
            }
            run.rect = window_rect(run.pid, "")?;
            run.host = run.rect;
            println!("resize: {width}x{height}");
            Ok(())
        }
        "press" => {
            // `press:TEXT` looks first and clicks second: the window is
            // photographed, the words are found in the picture, and the
            // click goes where they are. Words the window does not show are
            // a failure, not a blind click.
            let named = locate::split_named(value);
            if named.text.is_empty() {
                return Err("press: needs words".to_string());
            }
            let found = run.find_words(&named, "press")?;
            let area = run.area();
            let (x, y) = (found.x + found.width / 2, found.y + found.height / 2);
            mouse(CGEventType::MouseMoved, point(x, y))?;
            click(x, y)?;
            sleep(Duration::from_millis(500));
            println!("press:  {value} at {},{}", x - area.x, y - area.y);
            Ok(())
        }
        "dialog" | "nodialog" => {
            // `dialog:` fails unless a file dialog is open, `nodialog:` fails
            // if one is. The panel is a window of another process, so it is
            // found by looking rather than asked for: every open and save
            // panel carries a Cancel button, and its own name, given as the
            // value, is on it too.
            let want = value.trim();
            let probe = run
                .state
                .parent()
                .map(|work| work.join("dialog.png"))
                .ok_or("nowhere to put the look")?;
            // The panel is a window of the host's own process, so it is found
            // as a window rather than hunted for across the screen: the
            // frontmost window being one other than the window under test is
            // a dialog being up. Either way the window is given time to be as
            // the step says: a panel takes a moment to come up, and a moment
            // to go away after the keystroke that closed it.
            let deadline = Instant::now() + Duration::from_secs(3);
            let front = loop {
                let front = window_rect(run.pid, "").ok();
                let other = front.filter(|rect| *rect != run.host);
                let settled = if kind == "dialog" { other.is_some() } else { other.is_none() };
                if settled || Instant::now() > deadline {
                    break other;
                }
                sleep(Duration::from_millis(200));
            };
            match (kind, front) {
                ("dialog", None) => Err(format!(
                    "no {want} dialog is open, so the button did nothing"
                )),
                ("dialog", Some(rect)) => {
                    // Which dialog, read off the panel itself. A picture of
                    // one window is small enough for text recognition to
                    // read; a picture of the whole screen is not.
                    let found = find_on_screen(&run.finder, rect, want, &probe)?;
                    if found.is_none() {
                        return Err(format!(
                            "a dialog is open, but nothing on it says {want:?}; see {}",
                            probe.display()
                        ));
                    }
                    // The picture stays behind on a failure and goes on a
                    // pass: what was on screen is the whole of the evidence,
                    // and a step that throws it away leaves nothing to work
                    // from.
                    let _ = std::fs::remove_file(&probe);
                    println!("dialog: {want}");
                    Ok(())
                }
                ("nodialog", Some(_)) => {
                    Err(format!("a {want} dialog is still open"))
                }
                _ => {
                    println!("{kind}: {want}");
                    Ok(())
                }
            }
        }
        "showing" | "hidden" => {
            // `showing:TEXT` fails unless the window shows TEXT; `hidden:`
            // fails if it does. The window is read until it is as the step
            // says or the wait runs out.
            let named = locate::split_named(value);
            if named.text.is_empty() {
                return Err(format!("cannot read {kind}: {value}"));
            }
            let probe = run.probe("showing.png")?;
            let wanted = kind == "showing";
            let deadline = Instant::now() + PATIENCE;
            let found = loop {
                shot(run.area(), &probe)?;
                let found = run.finder.first(&probe, &named)?.is_some();
                if found == wanted || Instant::now() >= deadline {
                    break found;
                }
            };
            if found != wanted {
                let complaint = if wanted { "does not show" } else { "still shows" };
                return Err(format!(
                    "the window {complaint} {value:?}; it reads: {}; see {}",
                    run.finder.reads(&probe),
                    probe.display()
                ));
            }
            let _ = std::fs::remove_file(&probe);
            println!("{kind}: {value}");
            Ok(())
        }
        "dragtext" | "dragwords" => {
            // `dragtext:TEXT|to WHERE` drags from TEXT's first letter to
            // WHERE. `dragwords:TEXT|to WHERE` double-clicks the middle of
            // TEXT and drags the second press along TEXT's row to WHERE.
            let (named, target) = locate::split_to(value, kind)?;
            let found = run.find_words(&named, kind)?;
            let row = found.y + found.height / 2;
            let to = run.resolve(&target, kind)?;
            if kind == "dragtext" {
                take_hold(found.x, row)?;
                let_go(to.0, to.1)?;
            } else {
                double_hold(found.x + found.width / 2, row)?;
                let_go(to.0, row)?;
            }
            sleep(Duration::from_millis(500));
            println!("{kind}: {value}");
            Ok(())
        }
        "dbltext" | "righttext" => {
            // `dbltext:TEXT` double-clicks the middle of TEXT;
            // `righttext:TEXT` clicks it with the right button.
            let found = run.find_words(&locate::split_named(value), kind)?;
            let (x, y) = (found.x + found.width / 2, found.y + found.height / 2);
            if kind == "dbltext" {
                double_click(x, y)?;
            } else {
                right_click(x, y)?;
            }
            sleep(Duration::from_millis(500));
            println!("{kind}: {value}");
            Ok(())
        }
        "mark" => {
            // `mark:before A` and `mark:after A` fail unless the drop mark,
            // an upright line in the accent colour, is drawn left or right
            // of the words A on their row.
            let named = locate::split_named(value);
            let probe = run.probe("mark.png")?;
            let deadline = Instant::now() + PATIENCE;
            loop {
                let area = run.area();
                shot(area, &probe)?;
                let width = locate::width_of(&probe)?;
                if let Some(found) = run.finder.mark_in(&probe, &named, width as i32)? {
                    let (x, y) = point_on_screen(area, width, found);
                    let _ = std::fs::remove_file(&probe);
                    println!("mark:   {value} at {},{}", x - area.x, y - area.y);
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(format!("no drop mark is drawn {value}; see {}", probe.display()));
                }
            }
        }
        "copied" => {
            // `copied:TEXT` fails unless the text on the clipboard holds
            // TEXT, `\n` for a newline. The clipboard is read until it does
            // or the wait runs out.
            let wanted = value.replace("\\n", "\n");
            if wanted.is_empty() {
                return Err(format!("cannot read copied: {value}"));
            }
            let deadline = Instant::now() + PATIENCE;
            let held = loop {
                let held = clipboard_text();
                if held.contains(&wanted) || Instant::now() >= deadline {
                    break held;
                }
                sleep(Duration::from_millis(25));
            };
            if !held.contains(&wanted) {
                return Err(format!("the clipboard does not hold {value:?}; it holds: {held}"));
            }
            println!("copied: {value}");
            Ok(())
        }
        "dragto" => {
            // `dragto:W,H,MS` drags the window's bottom right corner until
            // the drawable area is exactly W by H, whatever size the window
            // started at, so the expectations afterwards can name exact
            // numbers without a size being set by anything but the mouse.
            let numbers: Vec<i32> = value
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            if numbers.len() != 3 {
                return Err(format!("cannot read dragto: {value}"));
            }
            let (width, height) = reported_host_size(&run.reported)
                .ok_or("the host has not reported its size")?;
            let (dx, dy) = (numbers[0] - width, numbers[1] - height);
            let corner = (run.host.right() - 3, run.host.bottom() - 3);
            take_hold(corner.0, corner.1)?;
            glide(
                point(corner.0, corner.1),
                point(corner.0 + dx, corner.1 + dy),
                Duration::from_millis(numbers[2] as u64),
                true,
            )?;
            let_go(corner.0 + dx, corner.1 + dy)?;
            sleep(Duration::from_millis(500));
            run.rect = window_rect(run.pid, "")?;
            run.host = run.rect;
            println!("dragto: {}x{} over {}ms", numbers[0], numbers[1], numbers[2]);
            Ok(())
        }
        "remove" => {
            let path = path_of(value);
            if path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("removing {}: {e}", path.display()))?;
            }
            Ok(())
        }
        "picture" => {
            // `picture:PATH|W,H` writes a PNG of that size, all one colour,
            // for a test to paste or drop.
            let (path, size) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read picture: {value}"))?;
            let (width, height) = pair(size, "picture size")?;
            if width <= 0 || height <= 0 {
                return Err(format!("a picture cannot be {width} by {height}"));
            }
            write_picture(&path_of(path), width as u32, height as u32)
        }
        "clipboard" => put_on_clipboard(&path_of(value)),
        "write" => {
            // `write:PATH|TEXT` writes TEXT to a file, `\n` for a newline,
            // for a test to drag or open.
            let (path, text) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read write: {value}"))?;
            let path = path_of(path.trim());
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("creating {}: {e}", parent.display()))?;
            }
            std::fs::write(&path, text.replace("\\n", "\n"))
                .map_err(|e| format!("writing {}: {e}", path.display()))
        }
        "dragfile" => {
            // `dragfile:PATH|WHERE` drags a file out of Finder and drops it
            // at WHERE in the window: the file is revealed in Finder, its
            // name is found in that window by reading it, the mouse presses
            // on the name, carries it across the screen to WHERE and lets go
            // there. Finder is the source of the drag, as it is when a person
            // does this, and nothing here tells the plugin what is being
            // dragged. The Finder window is closed afterwards. The drop's
            // place is found before Finder comes up over anything.
            let (path, at) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read dragfile: {value}"))?;
            let path = path_of(path.trim());
            if at.trim().is_empty() {
                return Err(format!("cannot read dragfile: {value}"));
            }
            if !path.exists() {
                return Err(format!("nothing to drag at {}", path.display()));
            }
            let (to_x, to_y) = run.resolve_point(at.trim(), "dragfile")?;
            let folder = path
                .parent()
                .and_then(|folder| folder.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .ok_or_else(|| format!("{} is in no folder Finder can show", path.display()))?;
            // Finder hides the extension of a file type it knows, so the name
            // is looked for without it.
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let revealed = Command::new("open")
                .arg("-R")
                .arg(&path)
                .status()
                .map_err(|e| format!("running open: {e}"))?;
            if !revealed.success() {
                return Err(format!("Finder would not show {}", path.display()));
            }
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut opened = finder_window(&folder);
            while opened.is_err() && Instant::now() < deadline {
                sleep(Duration::from_millis(200));
                opened = finder_window(&folder);
            }
            opened.map_err(|e| format!("Finder never opened {folder:?}: {e}"))?;
            // Beside the window under test, so the file's trip to X,Y crosses
            // no part of Finder: a file let go over Finder is dropped there.
            let (px, py) = room_beside(run.host, screen_rect()?, 640, 480)?;
            place_finder_window(&folder, px, py, 640, 480)?;
            osascript("tell application \"Finder\" to activate")?;
            sleep(Duration::from_millis(500));
            let area = finder_window(&folder)?;
            let probe = run.probe("dragfile.png")?;
            // The leftmost place the name is read, which is the file list.
            let deadline = Instant::now() + Duration::from_secs(10);
            let found = loop {
                shot(area, &probe)?;
                let leftmost = run
                    .finder
                    .all(&probe, &stem, None)?
                    .into_iter()
                    .min_by_key(|found| found.x);
                if let Some(leftmost) = leftmost {
                    break Some(on_screen(area, locate::width_of(&probe)?, leftmost));
                }
                if Instant::now() >= deadline {
                    break None;
                }
                sleep(Duration::from_millis(200));
            };
            let Some(found) = found else {
                return Err(format!(
                    "Finder shows no file called {stem:?}; it reads: {}; see {}",
                    run.finder.reads(&probe),
                    probe.display()
                ));
            };
            let _ = std::fs::remove_file(&probe);
            let (from_x, from_y) = (found.x + found.width / 2, found.y + found.height / 2);
            take_hold(from_x, from_y)?;
            glide(
                point(from_x, from_y),
                point(to_x, to_y),
                Duration::from_millis(600),
                true,
            )?;
            // Over the target long enough for the window there to say whether
            // it will take the file.
            sleep(Duration::from_millis(300));
            let_go(to_x, to_y)?;
            sleep(Duration::from_millis(500));
            close_finder_window(&folder)?;
            front(run.pid)?;
            println!("dragfile: {} to {}", path.display(), at.trim());
            Ok(())
        }
        "written" => {
            // `written:PATH|TEXT` checks a file in code, mid-test, right when
            // the step before it claims to have written it. Failing here names
            // the step that lied, instead of a pile of expectations at the end.
            let (path, want) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read written: {value}"))?;
            let path = path_of(path);
            let want = want.trim().replace("\\n", "\n");
            // The save runs off the plugin's drawing thread, so the file is
            // looked for until it is there and holds what it should.
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline
                && !std::fs::read_to_string(&path).is_ok_and(|got| got.contains(&want))
            {
                sleep(Duration::from_millis(25));
            }
            let got = std::fs::read_to_string(&path)
                .map_err(|e| format!("{} was not written: {e}", path.display()))?;
            if !got.contains(&want) {
                return Err(format!(
                    "{} holds {got:?}, expected it to contain {want:?}",
                    path.display()
                ));
            }
            Ok(())
        }
        "shot" => {
            let path = path_of(value);
            shot(run.rect, &path)?;
            println!("shot:   {}", path.display());
            Ok(())
        }
        "copies" => {
            let parts: Vec<&str> = value.split('|').collect();
            if parts.len() != 3 {
                return Err(format!("cannot read copies: {value}"));
            }
            let from = path_of(parts[0]);
            let prefix = path_of(parts[1]);
            let count: u32 = parts[2]
                .trim()
                .parse()
                .map_err(|_| format!("not a count: {}", parts[2]))?;
            if !from.exists() {
                return Err(format!("nothing to copy at {}", from.display()));
            }
            let extension = from
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            for index in 1..=count {
                let to = PathBuf::from(format!("{}-{index:02}{extension}", prefix.display()));
                std::fs::copy(&from, &to)
                    .map_err(|e| format!("copying {} to {}: {e}", from.display(), to.display()))?;
            }
            Ok(())
        }
        "row" => {
            let (dir, name) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read row: {value}"))?;
            let dir = path_of(dir);
            let mut names: Vec<String> = std::fs::read_dir(&dir)
                .map_err(|e| format!("reading {}: {e}", dir.display()))?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|e| e == "preset"))
                .filter_map(|path| {
                    path.file_stem().map(|s| s.to_string_lossy().into_owned())
                })
                .collect();
            names.sort();
            if !names.iter().any(|known| known == name.trim()) {
                return Err(format!(
                    "no preset called {:?} in {}; there is: {}",
                    name.trim(),
                    dir.display(),
                    names.join(", ")
                ));
            }

            // Look, then click: photograph the dialog, find the row in the
            // picture, and click where it is. A row further down than the
            // dialog shows is scrolled to, a wheel's worth at a time, looking
            // again after each turn.
            let probe = run
                .state
                .parent()
                .map(|work| work.join("row-probe.png"))
                .ok_or("nowhere to put the row picture")?;
            let mut turns = 0;
            loop {
                if let Some((x, y)) = find_on_screen(&run.finder, run.rect, name.trim(), &probe)? {
                    click(x, y)?;
                    sleep(Duration::from_millis(500));
                    return Ok(());
                }
                if turns >= 30 {
                    return Err(format!(
                        "{:?} never came on screen in the list; the last look is {}",
                        name.trim(),
                        probe.display()
                    ));
                }
                scroll_down(run.rect)?;
                turns += 1;
            }
        }
        "kill" => {
            // Force quit, for a test that ends with a native modal dialog up:
            // nothing can ask the window to close underneath one. The plugin
            // gets no shutdown, so such a test asserts on files, not state.
            run.child
                .kill()
                .map_err(|e| format!("could not force quit the host: {e}"))?;
            let _ = run.child.wait();
            run.killed = true;
            Ok(())
        }
        "restart" => run.restart(value),
        "geometry" => {
            // The plugin's editor is a subview of the host's, and nothing
            // outside the process can measure a subview. The host writes down
            // what it and the plugin measure; this takes a copy of it.
            let to = path_of(value);
            let text = std::fs::read_to_string(&run.reported).map_err(|e| {
                format!("the host wrote nothing about its geometry: {e}")
            })?;
            std::fs::write(&to, &text)
                .map_err(|e| format!("writing {}: {e}", to.display()))?;
            println!("geometry: {}", text.replace('\n', " ").trim_end());
            Ok(())
        }
        "cursor" => {
            // `cursor:SHAPE` checks what the window asked the cursor to be
            // where the pointer is; `cursor:WHERE|SHAPE` parks the pointer at
            // WHERE first. The window only changes it when it next redraws,
            // so it is looked at until it is what was asked for.
            let (place, want) = locate::split_cursor(value);
            let where_at = match &place {
                Some(named) => {
                    let (x, y) = run.resolve(named, "cursor")?;
                    mouse(CGEventType::MouseMoved, point(x, y))?;
                    named.to_string()
                }
                None => "the pointer".to_string(),
            };
            let deadline = Instant::now() + Duration::from_secs(2);
            let shown = loop {
                let shown = cursor_now();
                if shown == want || Instant::now() >= deadline {
                    break shown;
                }
                sleep(Duration::from_millis(25));
            };
            if shown != want {
                return Err(format!("cursor at {where_at} is {shown}, expected {want}"));
            }
            println!("cursor: {where_at} is {want}");
            Ok(())
        }
        "film" => {
            if run.film.is_some() {
                return Err("already filming".to_string());
            }
            let (to, size) = match value.split_once('|') {
                Some((to, size)) => (to, Some(size)),
                None => (value, None),
            };
            let to = path_of(to);
            let video = PathBuf::from(format!("{}.mp4", to.display()));
            let stop = PathBuf::from(format!("{}.stop", to.display()));
            let done = video.with_extension("done");
            let started = video.with_extension("started");
            for stale in [&video, &stop, &done, &started] {
                let _ = std::fs::remove_file(stale);
            }

            // Always the window under test, whatever the steps are
            // addressing: anything it opens is on top of it, so one region
            // catches the lot. The host's own report is where it is now,
            // which is not where it was when the run started if a step has
            // moved or resized it since.
            let area = reported_window(&run.reported).unwrap_or(run.host);
            run.host = area;
            let (width, height) = match size {
                Some(size) => pair(size, "film size")?,
                None => (area.width, area.height),
            };
            let region = Rect {
                x: area.x,
                y: area.y,
                width,
                height,
            };

            photograph(
                &[
                    "film",
                    &region.x.to_string(),
                    &region.y.to_string(),
                    &region.width.to_string(),
                    &region.height.to_string(),
                    &video.display().to_string(),
                    &stop.display().to_string(),
                ],
                &started,
                &video,
            )?;
            run.film = Some(Film { to, video, stop, done });
            Ok(())
        }
        "endfilm" => {
            let film = run.film.take().ok_or("not filming")?;
            // The stop file tells the recorder to finish the movie, and the
            // done file says it is on disk.
            std::fs::write(&film.stop, "")
                .map_err(|e| format!("writing {}: {e}", film.stop.display()))?;
            let deadline = Instant::now() + Duration::from_secs(30);
            while !film.done.exists() {
                if Instant::now() > deadline {
                    let said = std::fs::read_to_string(film.video.with_extension("error"))
                        .unwrap_or_else(|_| "the recording was never saved".to_string());
                    return Err(said);
                }
                sleep(Duration::from_millis(200));
            }
            let video = film.video.clone();

            let total = film_frames(&video, false)?;
            let distinct = film_frames(&video, true)?;
            if total == 0 {
                return Err(format!("{} holds no frames", video.display()));
            }
            let report = PathBuf::from(format!("{}.txt", film.to.display()));
            std::fs::write(
                &report,
                format!("frames={total}\ndistinct_frames={distinct}\n"),
            )
            .map_err(|e| format!("writing {}: {e}", report.display()))?;
            println!(
                "film:   {distinct} distinct of {total} frames; {}",
                video.display()
            );
            Ok(())
        }
        "wiggle" => {
            let ms: u64 = value
                .trim()
                .parse()
                .map_err(|_| format!("not a number of milliseconds: {value}"))?;
            let path = [
                (0.5, 0.5), (0.2, 0.2), (0.8, 0.9), (0.5, 0.1),
                (1.6, 0.5), (0.5, 2.5), (-0.6, 0.5), (0.5, -1.5),
            ];
            let until = Instant::now() + Duration::from_millis(ms);
            let mut step = 0;
            while Instant::now() < until {
                let (fx, fy) = path[step % path.len()];
                mouse(
                    CGEventType::MouseMoved,
                    point(
                        run.rect.x + (run.rect.width as f64 * fx) as i32,
                        run.rect.y + (run.rect.height as f64 * fy) as i32,
                    ),
                )?;
                sleep(Duration::from_millis(100));
                step += 1;
            }
            Ok(())
        }
        other => Err(format!("unknown step: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_goes_beside_the_one_under_test_where_there_is_room() {
        let screen = Rect { x: 0, y: 0, width: 1920, height: 1080 };

        let host = Rect { x: 100, y: 100, width: 800, height: 600 };
        assert_eq!(room_beside(host, screen, 640, 480), Ok((900, 100)));

        let at_the_right = Rect { x: 1100, y: 100, width: 800, height: 600 };
        assert_eq!(room_beside(at_the_right, screen, 640, 480), Ok((460, 100)));

        let wide = Rect { x: 0, y: 0, width: 1920, height: 500 };
        assert_eq!(room_beside(wide, screen, 640, 480), Ok((0, 500)));

        let whole = Rect { x: 0, y: 0, width: 1920, height: 1080 };
        assert!(room_beside(whole, screen, 640, 480).is_err());
    }

    #[test]
    fn a_place_is_read_as_position_then_size() {
        assert_eq!(
            placed("543, 252, 900, 646"),
            Ok(Rect { x: 543, y: 252, width: 900, height: 646 })
        );
        assert!(placed("543, 252").is_err());
    }
}
