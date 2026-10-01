//! Clicks and keystrokes, handed to the desktop through the portal session.
//!
//! Nothing here is sent to a window. The desktop moves its own pointer and
//! gives each key to whichever window has the keyboard, which is what makes
//! this the same thing a hand on the mouse and keyboard does.

use std::thread::sleep;
use std::time::Duration;

use super::keys::{self, Stroke};
use super::portal::Portal;

/// How long one keystroke takes: distinct keystrokes rather than a machine
/// flooding the queue, fast enough that a run does not crawl.
const KEYSTROKE: Duration = Duration::from_millis(20);

/// One step of a glide. A window being dragged reads where the pointer is
/// over and over, and this is how often it is somewhere new.
const GLIDE_STEP: Duration = Duration::from_millis(16);

pub struct Input {
    portal: &'static Portal,
    /// Where the pointer was last put, on the desktop.
    at: (i32, i32),
    /// Whether the mouse button is down.
    holding: bool,
    /// The modifier keys that are down.
    held: Vec<u32>,
}

impl Input {
    /// `at` is where the pointer is now, which is where the first glide
    /// starts from.
    pub fn new(portal: &'static Portal, at: (i32, i32)) -> Input {
        Input { portal, at, holding: false, held: Vec::new() }
    }

    /// Put the pointer somewhere in one jump.
    pub fn jump(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.portal.pointer_to(x as f64, y as f64)?;
        self.at = (x, y);
        Ok(())
    }

    /// Move the pointer there the way a hand does, across rather than by
    /// jumping.
    pub fn glide(&mut self, to: (i32, i32), over: Duration) -> Result<(), String> {
        let steps = (over.as_millis() / GLIDE_STEP.as_millis()).max(1) as i32;
        let from = self.at;
        for step in 1..=steps {
            let x = from.0 as f64 + (to.0 - from.0) as f64 * step as f64 / steps as f64;
            let y = from.1 as f64 + (to.1 - from.1) as f64 * step as f64 / steps as f64;
            self.portal.pointer_to(x, y)?;
            sleep(GLIDE_STEP);
        }
        self.at = to;
        Ok(())
    }

    fn press(&mut self) -> Result<(), String> {
        self.portal.button(true)?;
        self.holding = true;
        Ok(())
    }

    fn release(&mut self) -> Result<(), String> {
        self.holding = false;
        self.portal.button(false)
    }

    /// A click the window can see: the pointer moves, settles, presses, and
    /// only then releases. Sent back to back, the press and release land in
    /// one frame and are missed.
    pub fn click(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.jump(x, y)?;
        sleep(Duration::from_millis(120));
        self.press()?;
        sleep(Duration::from_millis(120));
        self.release()?;
        sleep(Duration::from_millis(120));
        Ok(())
    }

    /// Glide somewhere, press there, and keep holding.
    pub fn take_hold(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.glide((x, y), Duration::from_millis(200))?;
        sleep(Duration::from_millis(150));
        self.press()?;
        sleep(Duration::from_millis(150));
        Ok(())
    }

    /// Move the pointer somewhere in the time a hand would take, button or no
    /// button.
    pub fn move_to(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.glide((x, y), Duration::from_millis(400))?;
        sleep(Duration::from_millis(150));
        Ok(())
    }

    /// Move there and let the button go.
    pub fn let_go(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.move_to(x, y)?;
        self.release()?;
        sleep(Duration::from_millis(200));
        Ok(())
    }

    /// Turn the wheel, down when `steps` is positive.
    pub fn wheel(&mut self, steps: i32) -> Result<(), String> {
        self.portal.wheel(steps)?;
        sleep(Duration::from_millis(400));
        Ok(())
    }

    /// A key going down and coming back up, taking the time one keystroke
    /// takes.
    pub fn tap(&mut self, keysym: u32) -> Result<(), String> {
        self.portal.key(keysym, true)?;
        sleep(Duration::from_millis(8));
        self.portal.key(keysym, false)?;
        sleep(KEYSTROKE.saturating_sub(Duration::from_millis(8)));
        Ok(())
    }

    /// Hold a modifier, tap a key, let go, as a keyboard does it.
    pub fn shortcut(&mut self, modifier: u32, keysym: u32) -> Result<(), String> {
        self.portal.key(modifier, true)?;
        self.held.push(modifier);
        sleep(Duration::from_millis(60));
        self.tap(keysym)?;
        self.portal.key(modifier, false)?;
        self.held.retain(|held| *held != modifier);
        sleep(Duration::from_millis(60));
        Ok(())
    }

    /// Type text, one character per keystroke.
    pub fn write(&mut self, text: &str) -> Result<(), String> {
        for character in text.chars() {
            self.tap(keys::keysym_of(character))?;
        }
        Ok(())
    }

    /// Type what a `type:` step says.
    pub fn send_keys(&mut self, step: &str) -> Result<(), String> {
        for stroke in keys::parse_keys(step)? {
            match stroke {
                Stroke::Character(character) => self.tap(keys::keysym_of(character))?,
                Stroke::Named(keysym, times) => {
                    for _ in 0..times {
                        self.tap(keysym)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Let go of whatever is still pressed.
    ///
    /// A button or a modifier left down stays down on the desktop after the
    /// run, for whoever uses the machine next.
    pub fn release_all(&mut self) {
        if self.holding {
            let _ = self.release();
        }
        for modifier in std::mem::take(&mut self.held) {
            let _ = self.portal.key(modifier, false);
        }
    }
}
