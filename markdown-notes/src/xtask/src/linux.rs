//! The window driver for Linux.
//!
//! Windows has `tools/capture-window.ps1` and macOS has `macos.rs`. This does
//! the same job on a GNOME desktop: launch the host, find its window, deliver
//! real clicks and keystrokes to it, photograph it, and close it so the
//! program writes its state on the way out.
//!
//! Input and pictures go through the desktop portal, see `portal.rs`. The
//! host and the plugin are X11 windows, so where they are and which one is in
//! front is asked of the X server, see `xwindows.rs`.

mod cursor;
mod input;
mod keys;
mod look;
mod portal;
mod screen;
mod xwindows;

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::{Duration, Instant};

use x11rb::protocol::xproto::Window;

use cursor::Picture;
use input::Input;
use portal::Portal;
use screen::Film;
use xwindows::Desktop;

/// A place on the desktop, in the units windows are placed in, from the top
/// left.
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

/// The title of the window under test.
const TITLE: &str = "Mini VST Host";

/// How tall the host's own strip is, above the plugin: the host's
/// `chrome::HEIGHT`.
const STRIP: i32 = 26;

/// How long a hand takes to go from the mouse to the next thing. Input sent
/// faster than a person can make it reaches the plugin out of order.
const HAND: Duration = Duration::from_millis(150);

/// How long a look that found nothing is left before looking again.
const POLL: Duration = Duration::from_millis(25);

/// The colour a test picture is filled with, the same one the other drivers
/// fill theirs with.
const PICTURE_COLOUR: [u8; 3] = [200, 40, 160];

/// The portal session, opened once for the whole run of tests.
///
/// It is opened before any host is started, so that the desktop's question,
/// when it has one, is answered before there is a window for it to take the
/// keyboard from.
fn portal(root: &Path) -> Result<&'static Portal, String> {
    static PORTAL: OnceLock<Result<Portal, String>> = OnceLock::new();
    PORTAL
        .get_or_init(|| Portal::open(&root.join(".cache")))
        .as_ref()
        .map_err(Clone::clone)
}

/// A path written in a test file, as this platform spells it.
fn path_of(value: &str) -> PathBuf {
    PathBuf::from(value.replace('\\', "/"))
}

fn pair(value: &str, what: &str) -> Result<(i32, i32), String> {
    let unreadable = || format!("cannot read {what}: {value}");
    let (x, y) = value.split_once(',').ok_or_else(unreadable)?;
    let x = x.trim().parse().map_err(|_| unreadable())?;
    let y = y.trim().parse().map_err(|_| unreadable())?;
    Ok((x, y))
}

/// Look until something is there, and hand it back, or hand back nothing once
/// `patience` has gone by without it.
fn wait_until<T>(patience: Duration, mut look: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + patience;
    loop {
        if let Some(found) = look() {
            return Some(found);
        }
        if Instant::now() >= deadline {
            return None;
        }
        sleep(POLL);
    }
}

/// What the host says its drawable area measures, from its geometry file.
fn reported_host_size(reported: &Path) -> Option<(i32, i32)> {
    let text = std::fs::read_to_string(reported).ok()?;
    let line = text.lines().find(|line| line.starts_with("host="))?;
    let (width, height) = line.split_once('=')?.1.split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

/// Start the host on a plugin and wait for its window to be up, in front,
/// and drawn.
fn launch(
    desktop: &Desktop,
    portal: &Portal,
    program: &Path,
    plugin: &Path,
    state: &Path,
    reported: &Path,
    extra: &[String],
    work: &Path,
) -> Result<(Child, Window), String> {
    let mut child = Command::new(program)
        .arg(plugin)
        .arg("--state")
        .arg(state)
        .arg("--geometry")
        .arg(reported)
        .args(extra)
        .spawn()
        .map_err(|e| format!("running {}: {e}", program.display()))?;

    let ready = (|| {
        let pid = child.id();
        let host = wait_until(Duration::from_secs(30), || desktop.window_of(pid, TITLE))
            .ok_or("the host's window never appeared")?;
        desktop.activate(host)?;
        wait_loaded(desktop, portal, host, work)?;
        let rect = desktop.frame_rect(host)?;
        println!("window: {},{} {}x{}", rect.x, rect.y, rect.width, rect.height);
        Ok(host)
    })();
    match ready {
        Ok(host) => Ok((child, host)),
        Err(e) => {
            // A host that did not come up is not left behind.
            let _ = child.kill();
            let _ = child.wait();
            Err(e)
        }
    }
}

/// Wait for the plugin inside the host's window to be up, the way a person
/// does before touching it: until there is something to read below the
/// host's own strip. The host draws its strip long before the plugin has
/// loaded and drawn its first frame.
fn wait_loaded(desktop: &Desktop, portal: &Portal, host: Window, work: &Path) -> Result<(), String> {
    let probe = work.join("loaded.png");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let frame = desktop.frame_rect(host)?;
        let below = desktop.frame_extents(host)[2] + STRIP;
        let read = look::read_in(portal, frame, &probe)?;
        if read.iter().any(|(found, _)| found.y >= below) {
            let _ = std::fs::remove_file(&probe);
            sleep(HAND);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the plugin never drew anything to read in the host's window; see {}",
                probe.display()
            ));
        }
    }
}

struct Run {
    desktop: Desktop,
    portal: &'static Portal,
    input: Input,
    child: Child,
    pid: u32,
    /// How many times `restart:` has started the host again.
    restarts: usize,
    /// What the run was started with, so `restart:` can do it again.
    program: PathBuf,
    plugin: PathBuf,
    state: PathBuf,
    /// Where the host writes what it and the plugin inside it measure.
    reported: PathBuf,
    /// The test's working directory, where the looks are kept.
    work: PathBuf,
    /// The window under test, which is what is photographed at the end
    /// whichever window the steps were last addressing.
    host: Window,
    /// The window the steps are addressing now.
    window: Window,
    /// The recording `film:` started, and the path it was asked to go to.
    film: Option<(Film, PathBuf)>,
    /// What put a picture on the clipboard. The picture is there for as long
    /// as this is.
    clipboard: Option<arboard::Clipboard>,
    /// The cursor theme's pictures of the cursors a `cursor:` step can name.
    stock: Vec<(&'static str, Picture)>,
}

impl Run {
    fn start(
        portal: &'static Portal,
        program: &Path,
        plugin: &Path,
        state: &Path,
        reported: &Path,
        work: &Path,
    ) -> Result<Run, String> {
        let desktop = Desktop::open()?;
        let (child, host) = launch(&desktop, portal, program, plugin, state, reported, &[], work)?;
        let theme = desktop
            .resource("Xcursor.theme")
            .unwrap_or_else(|| "default".to_string());
        let at = desktop.pointer().unwrap_or((0, 0));
        Ok(Run {
            input: Input::new(portal, at),
            stock: cursor::stock(&theme),
            desktop,
            portal,
            pid: child.id(),
            child,
            restarts: 0,
            program: program.to_path_buf(),
            plugin: plugin.to_path_buf(),
            state: state.to_path_buf(),
            reported: reported.to_path_buf(),
            work: work.to_path_buf(),
            host,
            window: host,
            film: None,
            clipboard: None,
        })
    }

    /// Where the window the steps are addressing is, in the terms its
    /// coordinates are written in: the window under test with its frame,
    /// title bar included, and a dialog by its own area, so a step in one
    /// does not have to know how tall a title bar is.
    fn area(&self) -> Result<Rect, String> {
        if self.window == self.host {
            self.desktop.frame_rect(self.host)
        } else {
            self.desktop.client_rect(self.window)
        }
    }

    /// A point inside the window the steps are addressing, on the desktop.
    fn at(&self, x: i32, y: i32) -> Result<(i32, i32), String> {
        let area = self.area()?;
        Ok((area.x + x, area.y + y))
    }

    /// Refuse to click unless the window being addressed is the one in
    /// front. A click goes to whatever is under the pointer, and under the
    /// pointer is another program's window when this one is not on top.
    fn at_the_front(&self) -> Result<(), String> {
        if self.desktop.wait_active(self.window, Duration::from_secs(2)) {
            Ok(())
        } else {
            Err("the window is not at the front, so nothing was clicked: \
                 the click could have landed in another program"
                .to_string())
        }
    }

    /// Refuse to type unless what is typed has somewhere of this test's to
    /// go: the window being addressed, in front, or a file dialog the host
    /// has open.
    fn keys_are_ours(&self) -> Result<(), String> {
        if self.desktop.active() == Some(self.window) || self.portal.request_open(self.pid)? {
            return Ok(());
        }
        // A file dialog that has just closed may not have handed the window
        // the front back yet.
        self.desktop.activate(self.window).map_err(|e| {
            format!("{e}, so nothing was typed: it could have gone to another program")
        })
    }

    /// Fail unless the pointer is where it was just sent.
    fn arrived(&self, x: i32, y: i32) -> Result<(), String> {
        let there = wait_until(Duration::from_millis(500), || {
            let (px, py) = self.desktop.pointer()?;
            ((px - x).abs() <= 1 && (py - y).abs() <= 1).then_some(())
        });
        match there {
            Some(()) => Ok(()),
            None => Err(format!(
                "the pointer was sent to {x},{y} and is at {:?}",
                self.desktop.pointer()
            )),
        }
    }

    /// Close the host and start it again.
    ///
    /// The state the old run wrote is put aside first, as `STATE.1` for the
    /// first run and `STATE.2` for the second, where the runner checks it
    /// against the expectations written before the step. It is moved rather
    /// than left, so whatever is asserted afterwards can only have come from
    /// the new run.
    ///
    /// `extra` is `ARG|ARG`, arguments the host is started with after its
    /// own. They are paths as a test file writes them.
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
        let (child, host) = launch(
            &self.desktop,
            self.portal,
            &self.program,
            &self.plugin,
            &self.state,
            &self.reported,
            &extra,
            &self.work,
        )?;
        self.pid = child.id();
        self.child = child;
        self.host = host;
        self.window = host;
        Ok(())
    }

    /// Close the host and hold it to a clean exit.
    ///
    /// A crash on the way out is a failure like any other: the checks all
    /// passing and the program then dying is not a pass. A host that will
    /// not close is killed, so none is ever left running.
    fn finish(&mut self) -> Result<(), String> {
        if let Ok(Some(status)) = self.child.try_wait() {
            if status.success() {
                return Ok(());
            }
            return Err(format!("the host quit on its own: {status}"));
        }
        let _ = self.desktop.close(self.host);
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
        let _ = self.child.wait();
        Err("the window would not close".to_string())
    }
}

/// Run one test's steps against a freshly launched host.
pub fn drive(
    root: &Path,
    host: &Path,
    plugin: &Path,
    steps: &Path,
    state: &Path,
    shot_to: &Path,
) -> Result<(), String> {
    let text = std::fs::read_to_string(steps)
        .map_err(|e| format!("reading {}: {e}", steps.display()))?;
    let reported = steps.with_extension("geometry");
    let work = state
        .parent()
        .map(Path::to_path_buf)
        .ok_or("the test has no working directory")?;

    let portal = portal(root)?;
    let mut run = Run::start(portal, host, plugin, state, &reported, &work)?;
    let result = play(&mut run, &text, shot_to);

    // A test that failed halfway can leave the recorder rolling and a button
    // or a modifier down.
    if let Some((film, _)) = run.film.take() {
        let _ = film.stop();
    }
    run.input.release_all();
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
    // addressing.
    let area = run.desktop.frame_rect(run.host)?;
    screen::shot(run.portal, area, shot_to)
}

/// Give a file dialog a path.
///
/// The dialog is the desktop's own and not one of the host's windows, so it
/// is known to be up by the request the host has open with the portal, and
/// nothing is typed until it is: the keys would land in the editor instead.
/// Ctrl+L is how its path field is asked for. The test's own Enter confirms
/// the dialog afterwards, the same keystroke it is on the other platforms.
fn dialog_path(run: &mut Run, path: &str) -> Result<(), String> {
    // Numbered pictures of every stage, because none of this is visible in
    // the test's own output when it goes wrong. They are of the whole
    // monitor: nothing says where the dialog is.
    let stage = {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DIALOGS: AtomicUsize = AtomicUsize::new(0);
        DIALOGS.fetch_add(1, Ordering::Relaxed)
    };
    let picture = |step: &str| run.work.join(format!("dialog-{stage}-{step}.png"));

    let opened = wait_until(Duration::from_secs(6), || {
        run.portal.request_open(run.pid).ok().filter(|open| *open)
    });
    if opened.is_none() {
        return Err("no file dialog is open, so there is nowhere to type a path".to_string());
    }
    sleep(Duration::from_millis(500));
    let _ = screen::frame(run.portal, &picture("opened"));

    run.input.shortcut(keys::CONTROL, keys::keysym_of('l'))?;
    sleep(Duration::from_millis(400));
    let _ = screen::frame(run.portal, &picture("path-field"));

    run.input.write(path)?;
    sleep(Duration::from_millis(400));
    let _ = screen::frame(run.portal, &picture("path-typed"));
    Ok(())
}

fn step(run: &mut Run, kind: &str, value: &str) -> Result<(), String> {
    match kind {
        // A typed absolute path is dialog input: no test types a path as
        // document text. The tests spell paths with backslashes, which here
        // are ordinary characters, so they are swapped first.
        "type" if value.starts_with('/') => dialog_path(run, &value.replace('\\', "/")),
        "type" => {
            run.keys_are_ours()?;
            run.input.send_keys(value)
        }
        "click" => {
            let (x, y) = pair(value, "click")?;
            let (x, y) = run.at(x, y)?;
            run.at_the_front()?;
            run.input.click(x, y)?;
            run.arrived(x, y)?;
            sleep(HAND);
            Ok(())
        }
        "hold" => {
            let (x, y) = pair(value, "hold")?;
            let (x, y) = run.at(x, y)?;
            run.at_the_front()?;
            run.input.take_hold(x, y)?;
            run.arrived(x, y)
        }
        "moveto" => {
            let (x, y) = pair(value, "moveto")?;
            let (x, y) = run.at(x, y)?;
            run.input.move_to(x, y)?;
            run.arrived(x, y)
        }
        "letgo" => {
            let (x, y) = pair(value, "letgo")?;
            let (x, y) = run.at(x, y)?;
            run.input.let_go(x, y)?;
            run.arrived(x, y)?;
            sleep(HAND);
            Ok(())
        }
        "shortcut" => {
            // `shortcut:ctrl+a` is a modifier held down over one key.
            let lowered = value.trim().to_ascii_lowercase();
            let (modifier, key) = lowered
                .split_once('+')
                .ok_or_else(|| format!("cannot read shortcut: {value}"))?;
            let held = keys::modifier(modifier).ok_or_else(|| format!("unknown modifier: {modifier}"))?;
            let mut letters = key.chars();
            let (Some(letter), None) = (letters.next(), letters.next()) else {
                return Err(format!("unknown key: {key}"));
            };
            run.keys_are_ours()?;
            run.input.shortcut(held, keys::keysym_of(letter))?;
            sleep(Duration::from_millis(300));
            Ok(())
        }
        "window" => {
            // Clicks are relative to a window, and a dialog is a window of
            // its own. This says which one the steps after it mean. With no
            // title it goes back to the window under test.
            let wanted = if value.trim().is_empty() { TITLE } else { value.trim() };
            let target = wait_until(Duration::from_secs(4), || {
                run.desktop.window_of(run.pid, wanted)
            })
            .ok_or_else(|| format!("no window titled {wanted:?} to click in"))?;
            run.desktop.activate(target)?;
            run.window = target;
            // A dialog that has only just opened has drawn nothing yet, and
            // has no field for the keys that follow to go to.
            if target != run.host {
                let probe = run.work.join("window.png");
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    let area = run.area()?;
                    if !look::read_in(run.portal, area, &probe)?.is_empty() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "the {wanted} window opened and never drew anything to read; see {}",
                            probe.display()
                        ));
                    }
                }
                let _ = std::fs::remove_file(&probe);
                sleep(HAND);
            }
            let rect = run.area()?;
            println!(
                "window: {wanted} at {},{} {}x{}",
                rect.x, rect.y, rect.width, rect.height
            );
            Ok(())
        }
        "nowindow" => {
            // `nowindow:TITLE` fails while a window with that title is still
            // there: what a dialog that closed cleanly leaves.
            let wanted = value.trim();
            let gone = wait_until(Duration::from_secs(3), || {
                run.desktop.window_of(run.pid, wanted).is_none().then_some(())
            });
            if gone.is_none() {
                return Err(format!("the window {wanted:?} is still open"));
            }
            println!("nowindow: {wanted}");
            Ok(())
        }
        "press" => {
            // `press:LABEL|X,Y` looks first and clicks second. X,Y is where
            // the code puts the control, in window coordinates; the window
            // is photographed, the label nearest that spot is found in the
            // picture, and the click goes where the label really is. A label
            // the window does not show is a failure, not a blind click.
            let (label, at) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read press: {value}"))?;
            let label = label.trim();
            if label.is_empty() {
                return Err("press: needs a label".to_string());
            }
            let near = pair(at, "press")?;
            let area = run.area()?;
            // A pointer left on the control by the step before draws it
            // highlighted, so it is put on the title bar first.
            if run.window == run.host {
                run.input.jump(area.x + area.width / 2, area.y + 12)?;
            }
            let probe = run.work.join("press.png");
            // Looked for until it is there: the control may be one the step
            // before has only just caused to be drawn.
            let deadline = Instant::now() + Duration::from_secs(5);
            let found = loop {
                let found = look::find_in(run.portal, area, label, &probe)?;
                if let Some(found) = look::nearest(&found, near) {
                    break Some(found);
                }
                if Instant::now() >= deadline {
                    break None;
                }
            };
            let Some(found) = found else {
                // The picture stays when the label is not in it, and what
                // was read in it is said.
                let saw: Vec<String> = look::read_in(run.portal, area, &probe)?
                    .into_iter()
                    .map(|(_, text)| text)
                    .collect();
                return Err(format!(
                    "no control labelled {label:?} is in the window; it reads: {}; it is in {}",
                    saw.join(" | "),
                    probe.display()
                ));
            };
            let _ = std::fs::remove_file(&probe);
            let (x, y) = (found.x + found.width / 2, found.y + found.height / 2);
            run.at_the_front()?;
            run.input.move_to(area.x + x, area.y + y)?;
            run.input.click(area.x + x, area.y + y)?;
            run.arrived(area.x + x, area.y + y)?;
            sleep(HAND);
            println!("press:  {label} at {x},{y}");
            Ok(())
        }
        "dialog" | "nodialog" => {
            // `dialog:WORD` fails unless a file dialog is open with WORD on
            // it; `nodialog:WORD` fails if one is open at all.
            //
            // The dialog is the desktop's and not a window of the host's, so
            // whether one is up is asked of the portal: the host has a
            // request open with it for exactly as long as its dialog is up.
            let want = value.trim();
            if kind == "nodialog" {
                let closed = wait_until(Duration::from_secs(3), || {
                    run.portal.request_open(run.pid).ok().filter(|open| !*open)
                });
                if closed.is_none() {
                    return Err(format!("a {want} dialog is still open"));
                }
                println!("nodialog: {want}");
                return Ok(());
            }
            let opened = wait_until(Duration::from_secs(10), || {
                run.portal.request_open(run.pid).ok().filter(|open| *open)
            });
            if opened.is_none() {
                return Err(format!("no {want} dialog is open, so the button did nothing"));
            }
            // Which dialog it is, is read off the screen: the word has to be
            // on the row its Cancel button is on.
            let probe = run.work.join("dialog.png");
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                screen::frame(run.portal, &probe)?;
                let (_, cancels) = look::find_all(&probe, "Cancel")?;
                let (_, words) = look::find_all(&probe, want)?;
                let on_one_row = cancels.iter().any(|cancel| {
                    words.iter().any(|word| {
                        let apart = (cancel.y + cancel.height / 2) - (word.y + word.height / 2);
                        apart.abs() <= cancel.height.max(word.height)
                    })
                });
                if on_one_row {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(format!(
                        "a dialog is open, but nothing beside its Cancel says {want:?}; see {}",
                        probe.display()
                    ));
                }
            }
            let _ = std::fs::remove_file(&probe);
            sleep(HAND);
            println!("dialog: {want}");
            Ok(())
        }
        "showing" | "hidden" => {
            // `showing:TEXT|Y` reads the window below Y and fails unless
            // TEXT is there; `hidden:` fails if it is. Below Y so the
            // toolbars and the tab labels are out of it: what is being asked
            // about is what the document displays.
            let (text, below) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read {kind}: {value}"))?;
            let text = text.trim();
            let below: i32 = below
                .trim()
                .parse()
                .map_err(|_| format!("not a row: {value}"))?;
            let area = run.area()?;
            let probe = run.work.join("showing.png");
            // The whole window is photographed and the answer kept to what
            // is below Y afterwards: a picture cut to a band is read worse.
            // Looked at until it is as the step says: what the step before
            // did may not have been drawn yet.
            let wanted = kind == "showing";
            let deadline = Instant::now() + Duration::from_secs(5);
            let found = loop {
                let found = look::find_in(run.portal, area, text, &probe)?
                    .iter()
                    .any(|found| found.y >= below);
                if found == wanted || Instant::now() >= deadline {
                    break found;
                }
            };
            if found != wanted {
                // Only a step that is about to fail reads the window again,
                // to say what it holds instead.
                let saw: Vec<String> = look::read_in(run.portal, area, &probe)?
                    .into_iter()
                    .filter(|(found, _)| found.y >= below)
                    .map(|(_, text)| text)
                    .collect();
                let _ = std::fs::remove_file(&probe);
                let complaint = if wanted { "does not show" } else { "still shows" };
                return Err(format!(
                    "the document {complaint} {text:?}; below {below} the window reads: {}",
                    saw.join(" | ")
                ));
            }
            let _ = std::fs::remove_file(&probe);
            println!("{kind}: {text}");
            Ok(())
        }
        "dragtext" => {
            // `dragtext:TEXT|Y|X,Y2` selects by dragging from where TEXT
            // starts to X,Y2 in window coordinates. Where TEXT is comes from
            // looking: the window is photographed, the first TEXT below Y is
            // found, and the press lands on TEXT's first character.
            let mut parts = value.splitn(3, '|');
            let (text, below, to) = match (parts.next(), parts.next(), parts.next()) {
                (Some(text), Some(below), Some(to)) => (text.trim(), below, to),
                _ => return Err(format!("cannot read dragtext: {value}")),
            };
            let below: i32 = below
                .trim()
                .parse()
                .map_err(|_| format!("not a row: {value}"))?;
            let (to_x, to_y) = pair(to, "dragtext")?;
            let area = run.area()?;
            let probe = run.work.join("press.png");
            let deadline = Instant::now() + Duration::from_secs(5);
            let found = loop {
                let found = look::find_in(run.portal, area, text, &probe)?
                    .into_iter()
                    .find(|found| found.y >= below);
                if found.is_some() || Instant::now() >= deadline {
                    break found;
                }
            };
            let Some(from) = found else {
                let saw: Vec<String> = look::read_in(run.portal, area, &probe)?
                    .into_iter()
                    .filter(|(found, _)| found.y >= below)
                    .map(|(_, text)| text)
                    .collect();
                return Err(format!(
                    "{text:?} is not on screen to select from; below {below} the window reads: {}; see {}",
                    saw.join(" | "),
                    probe.display()
                ));
            };
            let _ = std::fs::remove_file(&probe);
            // On the first glyph, not beside it: a press lands on the
            // nearest character boundary, and the boundary before the first
            // letter is the one its ink starts at.
            let (x, y) = (area.x + from.x, area.y + from.y + from.height / 2);
            run.at_the_front()?;
            run.input.take_hold(x, y)?;
            run.arrived(x, y)?;
            run.input.let_go(area.x + to_x, area.y + to_y)?;
            sleep(HAND);
            println!("dragtext: {text}");
            Ok(())
        }
        "cursor" => {
            // `cursor:X,Y|ibeam` parks the pointer and checks what the
            // window asked the cursor to be there.
            let (where_at, want) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read cursor: {value}"))?;
            let want = want.trim();
            let (x, y) = pair(where_at, "cursor")?;
            let (x, y) = run.at(x, y)?;
            run.input.jump(x, y)?;
            run.arrived(x, y)?;
            // The window only changes it when it next redraws, so it is
            // looked at until it is what was asked for.
            let shown = || {
                run.desktop
                    .cursor_image()
                    .map(|picture| cursor::name_of(&picture, &run.stock))
                    .unwrap_or("unreadable")
            };
            let right = wait_until(Duration::from_secs(2), || (shown() == want).then_some(()));
            if right.is_none() {
                return Err(format!("cursor at {where_at} is {}, expected {want}", shown()));
            }
            Ok(())
        }
        "row" => {
            // `row:DIR|NAME` clicks the row for a named preset in the list.
            let (dir, name) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read row: {value}"))?;
            let name = name.trim();
            let dir = path_of(dir);
            let mut names: Vec<String> = std::fs::read_dir(&dir)
                .map_err(|e| format!("reading {}: {e}", dir.display()))?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|e| e == "preset"))
                .filter_map(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .collect();
            names.sort();
            if !names.iter().any(|known| known == name) {
                return Err(format!(
                    "no preset called {name:?} in {}; there is: {}",
                    dir.display(),
                    names.join(", ")
                ));
            }

            // Look, then click: photograph the dialog, find the row in the
            // picture, and click where it is. A row further down than the
            // dialog shows is scrolled to, a wheel's worth at a time,
            // looking again after each turn.
            let probe = run.work.join("row-probe.png");
            let mut turns = 0;
            loop {
                let area = run.area()?;
                if let Some((x, y)) = look::find_on_screen(run.portal, area, name, &probe)? {
                    let _ = std::fs::remove_file(&probe);
                    run.at_the_front()?;
                    run.input.click(x, y)?;
                    run.arrived(x, y)?;
                    sleep(HAND);
                    return Ok(());
                }
                if turns >= 30 {
                    return Err(format!(
                        "{name:?} never came on screen in the list; the last look is {}",
                        probe.display()
                    ));
                }
                run.at_the_front()?;
                run.input.jump(area.x + area.width / 2, area.y + area.height / 2)?;
                sleep(Duration::from_millis(120));
                run.input.wheel(3)?;
                turns += 1;
            }
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
            let [width, height, over] = numbers[..] else {
                return Err(format!("cannot read dragto: {value}"));
            };
            let (now_width, now_height) = reported_host_size(&run.reported)
                .ok_or("the host has not reported its size")?;
            let (dx, dy) = (width - now_width, height - now_height);
            let frame = run.desktop.frame_rect(run.host)?;
            let corner = (frame.right() - 3, frame.bottom() - 3);
            if !run.desktop.wait_active(run.host, Duration::from_secs(2)) {
                return Err("the window under test is not at the front, so its corner was \
                            not taken hold of"
                    .to_string());
            }
            run.input.take_hold(corner.0, corner.1)?;
            run.input
                .glide((corner.0 + dx, corner.1 + dy), Duration::from_millis(over.max(0) as u64))?;
            run.input.let_go(corner.0 + dx, corner.1 + dy)?;
            // The window is still working through the last of the pointer's
            // moves when the button comes up, so it is looked at until it is
            // the size it was dragged to. One that never gets there is left
            // for the expectations to refuse.
            wait_until(Duration::from_secs(2), || {
                (reported_host_size(&run.reported) == Some((width, height))).then_some(())
            });
            println!("dragto: {width}x{height} over {over}ms");
            Ok(())
        }
        "geometry" => {
            // `geometry:PATH` keeps what the window under test and the
            // plugin's window inside it measure, as the host reports it, so
            // a test can assert on it. Taken once the plugin's window
            // reaches the host's right and bottom edges, which after a drag
            // it is expected to, or as it stands when it never does.
            let filled = |text: &str| {
                text.lines()
                    .any(|line| line.starts_with("inset=") && line.ends_with(",0,0"))
            };
            wait_until(Duration::from_secs(2), || {
                std::fs::read_to_string(&run.reported)
                    .ok()
                    .filter(|text| filled(text))
            });
            let text = std::fs::read_to_string(&run.reported)
                .map_err(|e| format!("the host wrote nothing about its geometry: {e}"))?;
            let to = path_of(value);
            std::fs::write(&to, &text).map_err(|e| format!("writing {}: {e}", to.display()))?;
            println!("geometry: {}", text.replace('\n', " ").trim_end());
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
            let path = path_of(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("creating {}: {e}", parent.display()))?;
            }
            std::fs::write(&path, crate::png::solid(width as u32, height as u32, PICTURE_COLOUR))
                .map_err(|e| format!("writing {}: {e}", path.display()))
        }
        "clipboard" => {
            // `clipboard:PATH` puts that picture on the clipboard, as a
            // screenshot tool leaves one, for a paste to find.
            let path = path_of(value);
            let picture = image::open(&path)
                .map_err(|e| format!("{} is not a picture: {e}", path.display()))?
                .into_rgba8();
            let held = arboard::ImageData {
                width: picture.width() as usize,
                height: picture.height() as usize,
                bytes: picture.into_raw().into(),
            };
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|e| format!("the clipboard is not there to put a picture on: {e}"))?;
            clipboard
                .set_image(held)
                .map_err(|e| format!("the clipboard would not take the picture: {e}"))?;
            run.clipboard = Some(clipboard);
            Ok(())
        }
        "written" => {
            // `written:PATH|TEXT` checks a file in code, mid-test, right
            // when the step before it claims to have written it.
            let (path, want) = value
                .split_once('|')
                .ok_or_else(|| format!("cannot read written: {value}"))?;
            let path = path_of(path);
            let want = want.trim().replace("\\n", "\n");
            // The save runs off the plugin's drawing thread, so the file is
            // looked for until it is there and holds what it should.
            wait_until(Duration::from_secs(5), || {
                std::fs::read_to_string(&path)
                    .is_ok_and(|got| got.contains(&want))
                    .then_some(())
            });
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
            // A picture of whichever window the steps are addressing.
            let path = path_of(value);
            let area = run.area()?;
            screen::shot(run.portal, area, &path)?;
            println!("shot:   {}", path.display());
            Ok(())
        }
        "copies" => {
            // `copies:SRC|PREFIX|N` makes N copies of a file, named
            // PREFIX-01 upwards.
            let parts: Vec<&str> = value.split('|').collect();
            let [from, prefix, count] = parts[..] else {
                return Err(format!("cannot read copies: {value}"));
            };
            let from = path_of(from);
            let prefix = path_of(prefix);
            let count: u32 = count
                .trim()
                .parse()
                .map_err(|_| format!("not a count: {count}"))?;
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
        "restart" => run.restart(value),
        "film" => {
            // `film:PATH|W,H` starts filming, and keeps filming while the
            // steps that follow run, until `endfilm:`. With a size it films
            // a region of that size and not the window's own: a window that
            // is about to grow needs it, since the region is fixed for the
            // whole recording.
            if run.film.is_some() {
                return Err("already filming".to_string());
            }
            let (to, size) = match value.split_once('|') {
                Some((to, size)) => (to, Some(size)),
                None => (value, None),
            };
            let to = path_of(to);
            let video = PathBuf::from(format!("{}.mp4", to.display()));
            let _ = std::fs::remove_file(&video);
            // Always the window under test, whatever the steps are
            // addressing: anything it opens is on top of it, so one region
            // catches the lot.
            let area = run.desktop.frame_rect(run.host)?;
            let (width, height) = match size {
                Some(size) => pair(size, "film size")?,
                None => (area.width, area.height),
            };
            let region = Rect { x: area.x, y: area.y, width, height };
            let film = screen::film(run.portal, region, &video)?;
            run.film = Some((film, to));
            Ok(())
        }
        "endfilm" => {
            let (film, to) = run.film.take().ok_or("not filming")?;
            let video = film.stop()?;
            let total = screen::frames(&video, false)?;
            let distinct = screen::frames(&video, true)?;
            if total == 0 {
                return Err(format!("{} holds no frames", video.display()));
            }
            let report = PathBuf::from(format!("{}.txt", to.display()));
            std::fs::write(&report, format!("frames={total}\ndistinct_frames={distinct}\n"))
                .map_err(|e| format!("writing {}: {e}", report.display()))?;
            println!(
                "film:   {distinct} distinct of {total} frames; {}",
                video.display()
            );
            Ok(())
        }
        other => Err(format!("unknown step: {other}")),
    }
}
