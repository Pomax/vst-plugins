//! The host's windows, asked about through the X server.
//!
//! The host and the plugin are X11 windows, on a Wayland desktop as well,
//! where they are XWayland's. Which windows a process has, where they are,
//! which one is at the front and what the cursor looks like are all things
//! the X server answers. Nothing is typed or clicked through it.

use std::thread::sleep;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window,
};
use x11rb::rust_connection::RustConnection;

use super::cursor::Picture;
use super::Rect;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_CLIENT_LIST,
        _NET_WM_PID,
        _NET_WM_NAME,
        _NET_ACTIVE_WINDOW,
        _NET_FRAME_EXTENTS,
        UTF8_STRING,
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
    }
}

pub struct Desktop {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
}

impl Desktop {
    pub fn open() -> Result<Desktop, String> {
        let said = |e: &dyn std::fmt::Display| format!("no connection to the X server: {e}");
        let (conn, screen) = x11rb::connect(None).map_err(|e| said(&e))?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)
            .map_err(|e| said(&e))?
            .reply()
            .map_err(|e| said(&e))?;
        // The cursor's picture is only handed over once the extension has
        // been asked which version it speaks.
        conn.xfixes_query_version(5, 0)
            .map_err(|e| said(&e))?
            .reply()
            .map_err(|e| said(&e))?;
        Ok(Desktop { conn, root, atoms })
    }

    fn numbers(&self, window: Window, property: u32) -> Vec<u32> {
        self.conn
            .get_property(false, window, property, AtomEnum::ANY, 0, u32::MAX)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(|values| values.collect()))
            .unwrap_or_default()
    }

    fn title(&self, window: Window) -> String {
        let read = |property: u32| -> Option<String> {
            let reply = self
                .conn
                .get_property(false, window, property, AtomEnum::ANY, 0, u32::MAX)
                .ok()?
                .reply()
                .ok()?;
            (!reply.value.is_empty()).then(|| String::from_utf8_lossy(&reply.value).into_owned())
        };
        read(self.atoms._NET_WM_NAME)
            .or_else(|| read(AtomEnum::WM_NAME.into()))
            .unwrap_or_default()
    }

    /// The window of this process with this title, if the window manager is
    /// managing one.
    pub fn window_of(&self, pid: u32, title: &str) -> Option<Window> {
        self.numbers(self.root, self.atoms._NET_CLIENT_LIST)
            .into_iter()
            .find(|window| {
                self.numbers(*window, self.atoms._NET_WM_PID).first() == Some(&pid)
                    && self.title(*window) == title
            })
    }

    /// Where a window's own area is on the desktop, without its frame.
    pub fn client_rect(&self, window: Window) -> Result<Rect, String> {
        let gone = |e: &dyn std::fmt::Display| format!("the window is not there to measure: {e}");
        let size = self
            .conn
            .get_geometry(window)
            .map_err(|e| gone(&e))?
            .reply()
            .map_err(|e| gone(&e))?;
        let place = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .map_err(|e| gone(&e))?
            .reply()
            .map_err(|e| gone(&e))?;
        Ok(Rect {
            x: place.dst_x as i32,
            y: place.dst_y as i32,
            width: size.width as i32,
            height: size.height as i32,
        })
    }

    /// How much the window manager's frame adds on each side of a window:
    /// left, right, top, bottom.
    pub fn frame_extents(&self, window: Window) -> [i32; 4] {
        let extents = self.numbers(window, self.atoms._NET_FRAME_EXTENTS);
        match extents.as_slice() {
            [left, right, top, bottom] => [*left as i32, *right as i32, *top as i32, *bottom as i32],
            _ => [0; 4],
        }
    }

    /// Where a window is on the desktop with its frame, title bar included.
    pub fn frame_rect(&self, window: Window) -> Result<Rect, String> {
        Ok(grown(self.client_rect(window)?, self.frame_extents(window)))
    }

    /// The window that has the keyboard, as the window manager tells it.
    pub fn active(&self) -> Option<Window> {
        self.numbers(self.root, self.atoms._NET_ACTIVE_WINDOW)
            .first()
            .copied()
            .filter(|window| *window != 0)
    }

    /// Wait for a window to be the one at the front.
    pub fn wait_active(&self, window: Window, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        loop {
            if self.active() == Some(window) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            sleep(Duration::from_millis(25));
        }
    }

    /// Ask the window manager to bring a window to the front, and wait for it
    /// to have done so.
    pub fn activate(&self, window: Window) -> Result<(), String> {
        // The request a pager makes: from a program asking on the user's
        // behalf, which is what this is.
        const FROM_A_PAGER: u32 = 2;
        let event = ClientMessageEvent::new(
            32,
            window,
            self.atoms._NET_ACTIVE_WINDOW,
            [FROM_A_PAGER, x11rb::CURRENT_TIME, 0, 0, 0],
        );
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| format!("asking for the window to come to the front: {e}"))?;
        self.conn
            .flush()
            .map_err(|e| format!("asking for the window to come to the front: {e}"))?;
        if self.wait_active(window, Duration::from_secs(3)) {
            Ok(())
        } else {
            Err("the window would not come to the front".to_string())
        }
    }

    /// Ask a window to close, the way its close button does.
    pub fn close(&self, window: Window) -> Result<(), String> {
        let event = ClientMessageEvent::new(
            32,
            window,
            self.atoms.WM_PROTOCOLS,
            [self.atoms.WM_DELETE_WINDOW, x11rb::CURRENT_TIME, 0, 0, 0],
        );
        self.conn
            .send_event(false, window, EventMask::NO_EVENT, event)
            .map_err(|e| format!("asking the window to close: {e}"))?;
        self.conn
            .flush()
            .map_err(|e| format!("asking the window to close: {e}"))
    }

    /// Where the pointer is on the desktop, as far as the X server knows.
    ///
    /// It knows while the pointer is over an X window. Over anything else
    /// this is where it last was.
    pub fn pointer(&self) -> Option<(i32, i32)> {
        let reply = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        Some((reply.root_x as i32, reply.root_y as i32))
    }

    /// The picture of the cursor that is on screen.
    pub fn cursor_image(&self) -> Option<Picture> {
        let reply = self.conn.xfixes_get_cursor_image().ok()?.reply().ok()?;
        Some(Picture {
            width: reply.width as u32,
            height: reply.height as u32,
            pixels: reply.cursor_image,
        })
    }

    /// One of the desktop's X resources, such as `Xcursor.theme`.
    pub fn resource(&self, name: &str) -> Option<String> {
        let database = x11rb::resource_manager::new_from_default(&self.conn).ok()?;
        database.get_string(name, "").map(str::to_string)
    }
}

/// A window's area grown by the frame around it: left, right, top, bottom.
pub fn grown(client: Rect, extents: [i32; 4]) -> Rect {
    let [left, right, top, bottom] = extents;
    Rect {
        x: client.x - left,
        y: client.y - top,
        width: client.width + left + right,
        height: client.height + top + bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_the_client_area_grown_by_its_extents() {
        let client = Rect { x: 543, y: 252, width: 900, height: 646 };

        assert_eq!(
            grown(client, [0, 0, 37, 0]),
            Rect { x: 543, y: 215, width: 900, height: 683 }
        );
        assert_eq!(
            grown(client, [1, 2, 30, 4]),
            Rect { x: 542, y: 222, width: 903, height: 680 }
        );
        assert_eq!(grown(client, [0; 4]), client);
    }
}
