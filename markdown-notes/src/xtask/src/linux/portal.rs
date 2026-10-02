//! The desktop portal session a run's input and pictures go through.
//!
//! One session carries both. Keys and pointer moves are handed to the desktop
//! through the RemoteDesktop interface, and the desktop delivers them with its
//! own keyboard focus and pointer, the way `SendInput` and `CGEvent` posting
//! do on the other platforms. The screen is read from the ScreenCast stream
//! of the same session.
//!
//! The desktop asks the person at the machine before it allows either. It
//! hands back a token with its answer, and a session started with that token
//! is allowed without asking again.

use std::future::Future;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ashpd::desktop::remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::{PersistMode, Session};
use ashpd::zbus;

/// How long the desktop is given to answer, which is how long the person at
/// the machine has to answer its dialog.
const PATIENCE: Duration = Duration::from_secs(200);

/// The left mouse button, as the kernel's input events number it.
const BTN_LEFT: i32 = 0x110;

pub struct Portal {
    remote: RemoteDesktop<'static>,
    screencast: Screencast<'static>,
    session: Session<'static, RemoteDesktop<'static>>,
    bus: zbus::Connection,
    /// The PipeWire node the monitor's pictures arrive on.
    node: u32,
    /// Where the monitor is in the desktop, and how big, in the units the
    /// desktop lays windows out in.
    position: (i32, i32),
    size: (i32, i32),
}

impl Portal {
    /// Open the session, asking the desktop for the keyboard, the pointer and
    /// the screen.
    ///
    /// `cache` is where the token from the last answer is kept.
    pub fn open(cache: &Path) -> Result<Portal, String> {
        let kept = cache.join("portal-token");
        let token = std::fs::read_to_string(&kept)
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());

        println!(
            "portal: asking the desktop for the keyboard, the pointer and the screen. \
             If it puts up a dialog, allow it: the run carries on by itself"
        );
        let asked = Instant::now();
        // On a thread of its own, so that an answer that never comes can be
        // given up on.
        let (answer, answered) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = answer.send(pollster::block_on(connect(token)));
        });
        let (portal, token) = answered.recv_timeout(PATIENCE).map_err(|_| {
            format!(
                "the desktop did not answer within {} seconds, so no UI test can run",
                PATIENCE.as_secs()
            )
        })??;
        println!(
            "portal: answered after {:.1} seconds",
            asked.elapsed().as_secs_f64()
        );

        if let Some(token) = token {
            let _ = std::fs::create_dir_all(cache);
            std::fs::write(&kept, token).map_err(|e| format!("writing {}: {e}", kept.display()))?;
        }
        Ok(portal)
    }

    pub fn node(&self) -> u32 {
        self.node
    }

    /// The monitor's place and size: x, y, width, height.
    pub fn monitor(&self) -> (i32, i32, i32, i32) {
        (self.position.0, self.position.1, self.size.0, self.size.1)
    }

    fn wait<T>(&self, what: &str, call: impl Future<Output = ashpd::Result<T>>) -> Result<T, String> {
        pollster::block_on(call).map_err(|e| format!("{what}: {e}"))
    }

    /// Put the pointer at a place on the desktop.
    pub fn pointer_to(&self, x: f64, y: f64) -> Result<(), String> {
        self.wait(
            "moving the pointer",
            self.remote.notify_pointer_motion_absolute(
                &self.session,
                self.node,
                x - self.position.0 as f64,
                y - self.position.1 as f64,
            ),
        )
    }

    /// Press or release the left mouse button.
    pub fn button(&self, pressed: bool) -> Result<(), String> {
        self.wait(
            "pressing the mouse button",
            self.remote
                .notify_pointer_button(&self.session, BTN_LEFT, state(pressed)),
        )
    }

    /// Turn the wheel by so many steps, down when positive.
    pub fn wheel(&self, steps: i32) -> Result<(), String> {
        self.wait(
            "turning the wheel",
            self.remote
                .notify_pointer_axis_discrete(&self.session, Axis::Vertical, steps),
        )
    }

    /// Press or release the key that produces `keysym`.
    pub fn key(&self, keysym: u32, pressed: bool) -> Result<(), String> {
        self.wait(
            "pressing a key",
            self.remote
                .notify_keyboard_keysym(&self.session, keysym as i32, state(pressed)),
        )
    }

    /// A connection to PipeWire that the session's stream can be read on.
    pub fn pipewire(&self) -> Result<OwnedFd, String> {
        self.wait(
            "opening the screen stream",
            self.screencast.open_pipe_wire_remote(&self.session),
        )
    }

    /// Whether a process has a request open with the portal.
    ///
    /// A file dialog is one: the request is made when the dialog is asked for
    /// and stays for as long as the dialog is up. The portal keeps every open
    /// request in its object tree under the name of the connection that made
    /// it, and the bus says which process a connection belongs to.
    pub fn request_open(&self, pid: u32) -> Result<bool, String> {
        pollster::block_on(async {
            let reply = self
                .bus
                .call_method(
                    Some("org.freedesktop.portal.Desktop"),
                    "/org/freedesktop/portal/desktop/request",
                    Some("org.freedesktop.DBus.Introspectable"),
                    "Introspect",
                    &(),
                )
                .await
                .map_err(|e| format!("asking the portal for its requests: {e}"))?;
            let tree: String = reply
                .body()
                .deserialize()
                .map_err(|e| format!("reading the portal's requests: {e}"))?;

            let bus = zbus::fdo::DBusProxy::new(&self.bus)
                .await
                .map_err(|e| format!("asking the bus about its connections: {e}"))?;
            for sender in senders(&tree) {
                let Ok(name) = zbus::names::BusName::try_from(sender.as_str()) else {
                    continue;
                };
                // A connection that has gone since the tree was read has no
                // process, and is not the one being asked about.
                if bus.get_connection_unix_process_id(name).await.ok() == Some(pid) {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }
}

fn state(pressed: bool) -> KeyState {
    if pressed {
        KeyState::Pressed
    } else {
        KeyState::Released
    }
}

/// The connections that have a request open, read out of the portal's tree.
///
/// The tree names each as the portal spells a connection in a path: its
/// unique name without the leading colon and with every dot turned into an
/// underscore. This turns them back.
fn senders(tree: &str) -> Vec<String> {
    const OPENING: &str = "<node name=\"";
    tree.match_indices(OPENING)
        .filter_map(|(at, _)| {
            let name = &tree[at + OPENING.len()..];
            let name = &name[..name.find('"')?];
            // The tree's own root is named by its path.
            (!name.is_empty() && !name.starts_with('/')).then(|| format!(":{}", name.replace('_', ".")))
        })
        .collect()
}

async fn connect(token: Option<String>) -> Result<(Portal, Option<String>), String> {
    let no_portal = |e: ashpd::Error| {
        format!("the desktop portal is not there to ask, so no UI test can run: {e}")
    };
    let refused = |e: ashpd::Error| {
        format!(
            "the desktop did not allow the keyboard, the pointer and the screen, \
             so no UI test can run: {e}"
        )
    };

    let remote = RemoteDesktop::new().await.map_err(no_portal)?;
    let screencast = Screencast::new().await.map_err(no_portal)?;
    let bus = zbus::Connection::session()
        .await
        .map_err(|e| format!("no session bus: {e}"))?;
    let session = remote.create_session().await.map_err(no_portal)?;

    // The answer is remembered through the devices alone. A session that
    // carries both may not ask for the screen to be remembered as well.
    remote
        .select_devices(
            &session,
            DeviceType::Keyboard | DeviceType::Pointer,
            token.as_deref(),
            PersistMode::ExplicitlyRevoked,
        )
        .await
        .and_then(|request| request.response())
        .map_err(refused)?;
    screencast
        .select_sources(
            &session,
            CursorMode::Hidden,
            SourceType::Monitor.into(),
            false,
            None,
            PersistMode::DoNot,
        )
        .await
        .and_then(|request| request.response())
        .map_err(refused)?;

    let granted = remote
        .start(&session, None)
        .await
        .and_then(|request| request.response())
        .map_err(refused)?;

    let devices = granted.devices();
    if !devices.contains(DeviceType::Keyboard) || !devices.contains(DeviceType::Pointer) {
        return Err(
            "the desktop allowed less than the keyboard and the pointer, so no UI test can run"
                .to_string(),
        );
    }
    let stream = granted
        .streams()
        .and_then(|streams| streams.first())
        .ok_or("the desktop allowed no monitor to be read, so no UI test can run")?;
    let size = stream
        .size()
        .ok_or("the desktop did not say how big its monitor is")?;

    let token = granted.restore_token().map(str::to_string);
    let portal = Portal {
        node: stream.pipe_wire_node_id(),
        position: stream.position().unwrap_or((0, 0)),
        size,
        remote,
        screencast,
        session,
        bus,
    };
    Ok((portal, token))
}

#[cfg(test)]
mod tests {
    use super::senders;

    #[test]
    fn the_senders_with_open_requests_are_read_out_of_the_portals_tree() {
        let tree = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <node name="1_204"/>
  <node name="1_87"/>
</node>
"#;
        assert_eq!(senders(tree), vec![":1.204", ":1.87"]);

        // With no request open there is nothing under the root, and the root
        // is not a sender, whether or not it carries its own path as a name.
        assert!(senders("<node>\n</node>\n").is_empty());
        let named_root = "<node name=\"/org/freedesktop/portal/desktop/request\">\n</node>";
        assert!(senders(named_root).is_empty());
    }
}
