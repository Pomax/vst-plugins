//! Keys, as a host thinks of them.
//!
//! These are the host's own types rather than any plugin's, so that driving a
//! plugin from here does not mean depending on that plugin's editor model.
//! [`crate::encode_key`] turns them into the virtual key codes VST3 expects.

/// A logical key press.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Escape,
}

/// Modifier state accompanying a key press.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { ctrl: false, shift: false, alt: false };
    pub const CTRL: Mods = Mods { ctrl: true, shift: false, alt: false };
    pub const SHIFT: Mods = Mods { ctrl: false, shift: true, alt: false };
    pub const CTRL_SHIFT: Mods = Mods { ctrl: true, shift: true, alt: false };
}
