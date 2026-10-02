//! The plugin's editor still fills the host's window after the window's size
//! has gone back and forth quickly, and the host's report says so.
//!
//! This opens the host's window, so it only runs when it is asked for:
//!
//! ```text
//! cargo test --test editor_follows_a_window_resized_quickly -- --ignored
//! ```
//!
//! It needs the plugin in `binaries/`, which `markdown-notes/build.sh` puts
//! there. The window is resized by asking the X server, not with the pointer.

#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

/// How long the host is given to open its window and report on it.
const LIMIT: Duration = Duration::from_secs(30);
/// How long the windows are given to settle after the last change of size.
const SETTLE: Duration = Duration::from_secs(3);
/// The height of the host's strip, which the editor sits under.
const STRIP: u16 = 26;
/// How many times the window is taken back and forth, each time ending at a
/// size of its own.
const ROUNDS: u16 = 12;
/// How many changes of size one round makes.
const CHANGES: u16 = 40;

/// The host, stopped however the test ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Look until `look` has an answer, or give up after `patience`.
fn wait_for<T>(patience: Duration, mut look: impl FnMut() -> Option<T>) -> Option<T> {
    let started = Instant::now();
    loop {
        if let Some(found) = look() {
            return Some(found);
        }
        if started.elapsed() >= patience {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The window on the desktop that belongs to the process `pid`.
fn window_of(conn: &RustConnection, root: Window, pid: u32) -> Option<Window> {
    let atom = |name: &str| Some(conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom);
    let (clients, owner) = (atom("_NET_CLIENT_LIST")?, atom("_NET_WM_PID")?);
    let listed = conn
        .get_property(false, root, clients, AtomEnum::WINDOW, 0, 4096)
        .ok()?
        .reply()
        .ok()?;
    let windows: Vec<Window> = listed.value32()?.collect();
    windows.into_iter().find(|window| {
        conn.get_property(false, *window, owner, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|asked| asked.reply().ok())
            .and_then(|reply| reply.value32().and_then(|mut values| values.next()))
            == Some(pid)
    })
}

/// A window's width and height.
fn size_of(conn: &RustConnection, window: Window) -> Option<(u16, u16)> {
    let geometry = conn.get_geometry(window).ok()?.reply().ok()?;
    Some((geometry.width, geometry.height))
}

/// The window the host puts the editor in, and the editor: the window inside
/// the host's that starts under the strip, and the biggest window inside
/// that.
fn socket_and_editor(conn: &RustConnection, host: Window) -> Option<(Window, Window)> {
    let inside = conn.query_tree(host).ok()?.reply().ok()?.children;
    let socket = inside.into_iter().find(|child| {
        conn.get_geometry(*child)
            .ok()
            .and_then(|asked| asked.reply().ok())
            .is_some_and(|geometry| geometry.y == STRIP as i16)
    })?;
    let editor = conn
        .query_tree(socket)
        .ok()?
        .reply()
        .ok()?
        .children
        .into_iter()
        .max_by_key(|child| size_of(conn, *child).map(|(w, h)| w as u32 * h as u32))?;
    Some((socket, editor))
}

#[test]
#[ignore = "opens the host's window"]
fn the_editor_fills_the_window_after_its_size_went_back_and_forth() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plugin = manifest.join("../../../../../binaries/Markdown Notes.vst3");
    assert!(
        plugin.exists(),
        "{} is not there: markdown-notes/build.sh builds it",
        plugin.display()
    );

    // A report of this run's own, so that one left by an earlier run cannot
    // answer for this one.
    let scratch = manifest.join("../../../.cache/tests");
    std::fs::create_dir_all(&scratch).expect("could not make .cache/tests");
    let report = scratch.join(format!("editor-follows-{}.geometry", std::process::id()));

    let host = Command::new(env!("CARGO_BIN_EXE_mini-host"))
        .arg(&plugin)
        .arg("--geometry")
        .arg(&report)
        .spawn()
        .expect("the host did not start");
    let pid = host.id();
    let _host = Running(host);

    let reported = || std::fs::read_to_string(&report).unwrap_or_default();
    wait_for(LIMIT, || reported().contains("inset=0,26,0,0").then_some(()))
        .unwrap_or_else(|| panic!("the editor never filled the window. The report:\n{}", reported()));

    let (conn, screen) = x11rb::connect(None).expect("no connection to the X server");
    let root = conn.setup().roots[screen].root;
    let window = wait_for(LIMIT, || window_of(&conn, root, pid)).expect("the host has no window");
    let (socket, editor) =
        socket_and_editor(&conn, window).expect("the host's window has no editor in it");

    for round in 0..ROUNDS {
        // Two sizes a few pixels apart, the way a hand that wavers on the
        // corner gives them, with a different beat each round. The last
        // change of the round is to the round's own size.
        let (width, height) = (880 - round * 6, 600 - round * 4);
        for change in 0..CHANGES {
            let back = if change % 2 == 0 { 2 } else { 0 };
            let size = ConfigureWindowAux::new()
                .width((width + back) as u32)
                .height((height + back) as u32);
            conn.configure_window(window, &size).expect("the window could not be resized");
            conn.flush().expect("the X server went away");
            std::thread::sleep(Duration::from_millis(((change + round) % 5) as u64));
        }
        let size = ConfigureWindowAux::new().width(width as u32).height(height as u32);
        conn.configure_window(window, &size).expect("the window could not be resized");
        conn.flush().expect("the X server went away");

        // What the X server says, which is where the windows are.
        let below_the_strip = (width, height - STRIP);
        let sizes = || (size_of(&conn, window), size_of(&conn, socket), size_of(&conn, editor));
        let wanted = (Some((width, height)), Some(below_the_strip), Some(below_the_strip));
        wait_for(SETTLE, || (sizes() == wanted).then_some(())).unwrap_or_else(|| {
            let (host, socket, editor) = sizes();
            panic!(
                "round {round}: the host's window is {host:?}, the socket {socket:?} and the \
                 editor {editor:?}; the editor should be {below_the_strip:?}"
            )
        });

        // What the host says, which is what a test of the host reads.
        let said = format!(
            "host={width}x{height}\neditor=0,26,{}x{}\ninset=0,26,0,0\n",
            below_the_strip.0, below_the_strip.1
        );
        wait_for(SETTLE, || reported().contains(&said).then_some(())).unwrap_or_else(|| {
            panic!(
                "round {round}: the editor fills the window and the host's report does not say \
                 so. The report:\n{}",
                reported()
            )
        });
    }
}
