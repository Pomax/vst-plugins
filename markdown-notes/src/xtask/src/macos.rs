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

fn source() -> Result<CGEventSource, String> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "could not open an event source; grant Accessibility permission".to_string())
}

fn post(event: CGEvent) {
    event.post(CGEventTapLocation::HID);
}

fn mouse(kind: CGEventType, at: CGPoint) -> Result<(), String> {
    let event = CGEvent::new_mouse_event(source()?, kind, at, CGMouseButton::Left)
        .map_err(|_| "could not make a mouse event".to_string())?;
    // A press with no click state is not a click. Windows work out what was
    // clicked, and how many times, from this field, and a zero means the event
    // is delivered and then ignored: the pointer moves, and nothing else
    // happens.
    let pressing = matches!(
        kind,
        CGEventType::LeftMouseDown | CGEventType::LeftMouseUp | CGEventType::LeftMouseDragged
    );
    if pressing {
        event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, 1);
        event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, 0);
    }
    post(event);
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

fn drag(from: (i32, i32), to: (i32, i32)) -> Result<(), String> {
    take_hold(from.0, from.1)?;
    let_go(to.0, to.1)
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
fn dialog_path(area: Rect, work: &Path, path: &str) -> Result<(), String> {
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
        if find_on_screen(area, "Cancel", &opened)?.is_some() {
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
        if find_on_screen(area, &tail, &typed)?.is_none() {
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
    if find_on_screen(area, &tail, &typed)?.is_none() {
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
    if find_on_screen(area, &name, &named)?.is_none() {
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
const DOWN: u16 = 125;

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
/// `{ENTER}`, `{ESC}`, `{END}`, `{DOWN}` are those keys. `{BS 40}` is
/// backspace forty times. `{(}` and `{)}` are the brackets themselves, which
/// would otherwise be read as the start of a name. This is the notation the
/// tests are already written in, and it is Windows `SendKeys`.
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
        let code = match name.to_ascii_uppercase().as_str() {
            "ENTER" => RETURN,
            "ESC" => ESCAPE,
            "BS" | "BACKSPACE" => BACKSPACE,
            "END" => END,
            "DOWN" => DOWN,
            other => return Err(format!("unknown key: {other}")),
        };
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
    let answer = osascript(&format!(
        "tell application \"System Events\" to tell (first process whose unix id is {pid}) \
         to get {{position, size}} of {which}"
    ))?;
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

/// A path written in a test file, as this platform spells it.
///
/// The tests were written on Windows and separate directories with a
/// backslash. Left alone, `%CACHE%\shot.png` names one file with a backslash
/// in it rather than a file in that directory.
fn path_of(value: &str) -> PathBuf {
    PathBuf::from(value.replace('\\', "/"))
}

/// Where a piece of text is on screen inside `area`, found by looking.
///
/// The area is photographed and the system's text recognition finds the text
/// in the picture, so the click goes where the row really is rather than
/// where a layout constant says it ought to be. Returns the centre of the
/// text, in screen points, or nothing when it is not on screen.
fn find_on_screen(area: Rect, text: &str, probe: &Path) -> Result<Option<(i32, i32)>, String> {
    Ok(find_box_on_screen(area, text, probe)?.map(|found| {
        (found.x + found.width / 2, found.y + found.height / 2)
    }))
}

/// The box the text occupies on screen inside `area`, found by looking.
fn find_box_on_screen(area: Rect, text: &str, probe: &Path) -> Result<Option<Rect>, String> {
    shot(area, probe)?;
    let out = Command::new(finder()?)
        .arg(probe)
        .arg(text)
        .output()
        .map_err(|e| format!("running the text finder: {e}"))?;
    if out.status.code() == Some(1) {
        return Ok(None);
    }
    if !out.status.success() {
        return Err(format!(
            "the text finder failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let numbers: Vec<f64> = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter_map(|part| part.parse().ok())
        .collect();
    if numbers.len() != 6 || numbers[4] <= 0.0 {
        return Err(format!(
            "the text finder said something unreadable: {}",
            String::from_utf8_lossy(&out.stdout).trim()
        ));
    }
    // The picture is of `area`, so its pixel count over the area's width is
    // the display's scale.
    let scale = numbers[4] / area.width as f64;
    Ok(Some(Rect {
        x: area.x + (numbers[0] / scale) as i32,
        y: area.y + (numbers[1] / scale) as i32,
        width: (numbers[2] / scale) as i32,
        height: (numbers[3] / scale) as i32,
    }))
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
        let (child, pid, rect) = launch(program, plugin, state, reported, &[], settle)?;
        Ok(Run {
            child,
            pid,
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

    /// A point inside the window the steps are addressing, on screen.
    fn at(&self, x: i32, y: i32) -> (i32, i32) {
        (self.rect.x + x, self.rect.y + y)
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
            dialog_path(run.rect, &work, &value.replace('\\', "/"))
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
            let (x, y) = pair(value, "click")?;
            let (x, y) = run.at(x, y);
            click(x, y)?;
            sleep(Duration::from_millis(500));
            Ok(())
        }
        "hold" => {
            let (x, y) = pair(value, "hold")?;
            let (x, y) = run.at(x, y);
            run.holding = true;
            take_hold(x, y)
        }
        "moveto" => {
            let (x, y) = pair(value, "moveto")?;
            let (x, y) = run.at(x, y);
            move_to(x, y, run.holding)
        }
        "letgo" => {
            let (x, y) = pair(value, "letgo")?;
            let (x, y) = run.at(x, y);
            run.holding = false;
            let_go(x, y)?;
            sleep(Duration::from_millis(400));
            Ok(())
        }
        "drag" => {
            let numbers: Vec<i32> = value
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            if numbers.len() != 4 {
                return Err(format!("cannot read drag: {value}"));
            }
            let from = run.at(numbers[0], numbers[1]);
            let to = run.at(numbers[2], numbers[3]);
            drag(from, to)?;
            sleep(Duration::from_millis(400));
            Ok(())
        }
        "shortcut" => {
            let (modifier, key) = value
                .trim()
                .to_ascii_lowercase()
                .split_once('+')
                .map(|(m, k)| (m.to_string(), k.to_string()))
                .ok_or_else(|| format!("cannot read shortcut: {value}"))?;
            let letter = key
                .chars()
                .next()
                .ok_or_else(|| format!("cannot read shortcut: {value}"))?;
            let code = code_for(letter).ok_or_else(|| format!("unknown key: {letter}"))?;

            // Cut, copy and paste belong to the platform, not to the plugin:
            // it claims the letter so it is not typed and waits for the
            // clipboard event the platform sends. Here the key that produces
            // that event is Cmd, so Ctrl over one of those letters would
            // reach the document as nothing at all. The plugin's own
            // shortcuts, select-all among them, stay on Ctrl.
            let clipboard = matches!(letter, 'c' | 'x' | 'v');
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
            // `press:LABEL|X,Y` looks first and clicks second. X,Y is where
            // the code puts the control, in window coordinates; the region
            // around that spot is photographed, the label is found in the
            // picture to confirm or correct the position, the picture is
            // deleted, and the click goes where the label really is. A label
            // that is not in its region is a failure, not a blind click.
            let (label, at) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read press: {value}"))?;
            let label = label.trim();
            if label.is_empty() {
                return Err("press: needs a label".to_string());
            }
            let (x, y) = pair(at, "press")?;
            let window = window_rect(run.pid, &run.title)?;
            let region = Rect {
                x: window.x + (x - 110).max(0),
                y: window.y + (y - 30).max(0),
                width: 220.min(window.width),
                height: 60.min(window.height),
            };
            let probe = run
                .state
                .parent()
                .map(|work| work.join("press.png"))
                .ok_or("nowhere to put the look")?;
            let found = find_on_screen(region, label, &probe);
            let _ = std::fs::remove_file(&probe);
            let Some((found_x, found_y)) = found? else {
                return Err(format!(
                    "no control labelled {label:?} is near {x},{y}"
                ));
            };
            click(found_x, found_y)?;
            sleep(Duration::from_millis(500));
            println!(
                "press:  {label} at {},{}",
                found_x - window.x,
                found_y - window.y
            );
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
            // a dialog being up.
            let deadline = Instant::now() + Duration::from_secs(3);
            let front = loop {
                let front = window_rect(run.pid, "").ok();
                let other = front.filter(|rect| *rect != run.host);
                if other.is_some() || kind == "nodialog" || Instant::now() > deadline {
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
                    let found = find_on_screen(rect, want, &probe)?;
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
            // `showing:TEXT|Y` reads the window below Y and fails unless TEXT
            // is there; `hidden:` fails if it is. Below Y so the toolbars and
            // the tab labels are out of it: what is being asked about is what
            // the document displays, and a tab's own label is not that.
            let (text, below) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read {kind}: {value}"))?;
            let text = text.trim();
            let below: i32 = below
                .trim()
                .parse()
                .map_err(|_| format!("not a row: {value}"))?;
            let window = window_rect(run.pid, &run.title)?;
            let area = Rect {
                x: window.x,
                y: window.y + below,
                width: window.width,
                height: (window.height - below).max(1),
            };
            let probe = run
                .state
                .parent()
                .map(|work| work.join("showing.png"))
                .ok_or("nowhere to put the look")?;
            // Looked at until it is as the step says: what the step before did
            // may not have been drawn yet, and a tooltip takes its time coming
            // and going. No step pauses for a length of time instead.
            let wanted = kind == "showing";
            let deadline = Instant::now() + Duration::from_secs(5);
            let found = loop {
                let found = find_on_screen(area, text, &probe);
                let _ = std::fs::remove_file(&probe);
                let found = found?.is_some();
                if found == wanted || Instant::now() >= deadline {
                    break found;
                }
            };
            match (kind, found) {
                ("showing", false) => {
                    Err(format!("the document does not show {text:?}"))
                }
                ("hidden", true) => Err(format!("the document still shows {text:?}")),
                _ => {
                    println!("{kind}: {text}");
                    Ok(())
                }
            }
        }
        "dragtext" => {
            // `dragtext:TEXT|Y|X,Y2` selects by dragging from where TEXT
            // starts to X,Y2 in window coordinates. Where TEXT is comes from
            // looking: the window below Y is photographed, the first TEXT in
            // it is found, the picture is deleted, and the press lands on
            // TEXT's first character. What width a font gives the characters
            // before it stops mattering, and the same words in the toolbar
            // above Y are not mistaken for it.
            let mut parts = value.splitn(3, '|');
            let (text, below, to) = match (parts.next(), parts.next(), parts.next()) {
                (Some(text), Some(below), Some(to)) => (text, below, to),
                _ => return Err(format!("cannot read dragtext: {value}")),
            };
            let below: i32 = below
                .trim()
                .parse()
                .map_err(|_| format!("not a row: {value}"))?;
            let (to_x, to_y) = pair(to, "dragtext")?;
            let window = window_rect(run.pid, &run.title)?;
            let band = Rect {
                x: window.x,
                y: window.y + below,
                width: window.width,
                height: (window.height - below).max(1),
            };
            let probe = run
                .state
                .parent()
                .map(|work| work.join("press.png"))
                .ok_or("nowhere to put the look")?;
            let found = find_box_on_screen(band, text.trim(), &probe);
            let _ = std::fs::remove_file(&probe);
            let Some(from) = found? else {
                return Err(format!("{text:?} is not on screen to select from"));
            };
            // On the first glyph, not beside it. A press lands on the nearest
            // character boundary, and the boundary before the first letter is
            // the one its ink starts at.
            let start = from.x;
            take_hold(start, from.y + from.height / 2)?;
            let_go(window.x + to_x, window.y + to_y)?;
            sleep(Duration::from_millis(500));
            println!(
                "dragtext: from {},{}",
                start - window.x,
                from.y + from.height / 2 - window.y
            );
            Ok(())
        }
        "dragedge" => {
            let numbers: Vec<i32> = value
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            if numbers.len() != 3 {
                return Err(format!("cannot read dragedge: {value}"));
            }
            let corner = (run.host.right() - 3, run.host.bottom() - 3);
            take_hold(corner.0, corner.1)?;
            glide(
                point(corner.0, corner.1),
                point(corner.0 + numbers[0], corner.1 + numbers[1]),
                Duration::from_millis(numbers[2] as u64),
                true,
            )?;
            let_go(corner.0 + numbers[0], corner.1 + numbers[1])?;
            sleep(Duration::from_millis(500));
            run.rect = window_rect(run.pid, "")?;
            run.host = run.rect;
            println!(
                "dragedge: {},{} over {}ms",
                numbers[0], numbers[1], numbers[2]
            );
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
                if let Some((x, y)) = find_on_screen(run.rect, name.trim(), &probe)? {
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
            let (where_at, want) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read cursor: {value}"))?;
            let (x, y) = pair(where_at, "cursor")?;
            let (x, y) = run.at(x, y);
            mouse(CGEventType::MouseMoved, point(x, y))?;
            sleep(Duration::from_millis(700));
            let shown = cursor_now();
            let want = want.trim();
            if shown != want {
                return Err(format!("cursor at {where_at} is {shown}, expected {want}"));
            }
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
