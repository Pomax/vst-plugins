//! Saving and restoring a plugin's state as a file on disk.
//!
//! The state itself comes from the plugin through `IComponent::getState` and
//! goes back through `IComponent::setState` — the same calls a DAW makes when
//! it writes and reads its project file. Nothing here interprets those bytes;
//! only the plugin knows what they mean.
//!
//! Presets live next to the executable, one directory per plugin, named after
//! the plugin's own file: `presets/vst-cake-loader/` for
//! `vst-cake-loader.dll`.

use std::io;
use std::path::{Path, PathBuf};

/// What every preset file starts with, so one written by something else — or
/// by a later version of this — is refused rather than fed to a plugin.
const MAGIC: &[u8; 8] = b"MHPRESET";
const VERSION: u8 = 1;

/// The extension presets are written with.
pub const EXTENSION: &str = "preset";

/// The directory holding every plugin's presets.
///
/// Next to the executable rather than the working directory: the host is run
/// from wherever, and presets should not scatter.
pub fn root() -> PathBuf {
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("presets")));
    beside_exe.unwrap_or_else(|| PathBuf::from("presets"))
}

/// Where this plugin's presets go: a directory named after its own file.
///
/// A bundle is named for the bundle, not the binary buried inside it, so
/// `Notepad.vst3/Contents/x86_64-win/Notepad.vst3` and a bare `Notepad.vst3`
/// share one directory.
pub fn directory_for(plugin: &Path) -> PathBuf {
    root().join(plugin_name(plugin))
}

/// The name a plugin's presets are filed under.
pub fn plugin_name(plugin: &Path) -> String {
    // Walk up out of a bundle to the `.vst3` that names it.
    let mut named = plugin;
    while let Some(parent) = named.parent() {
        let is_bundle_part = parent
            .file_name()
            .is_some_and(|n| n == "Contents" || n.to_string_lossy().contains('-'));
        let parent_is_bundle = parent
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("vst3"));
        if parent_is_bundle {
            named = parent;
            break;
        }
        if !is_bundle_part {
            break;
        }
        named = parent;
    }

    named
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown-plugin".to_string())
}

/// Write `state` for the plugin identified by `cid`.
pub fn save(path: &Path, cid: [u8; 16], state: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::with_capacity(MAGIC.len() + 1 + cid.len() + state.len());
    bytes.extend_from_slice(MAGIC);
    bytes.push(VERSION);
    bytes.extend_from_slice(&cid);
    bytes.extend_from_slice(state);
    std::fs::write(path, bytes)
}

/// Read a preset back: the plugin it was written for, and its state.
pub fn load(path: &Path) -> io::Result<([u8; 16], Vec<u8>)> {
    let bytes = std::fs::read(path)?;
    let header = MAGIC.len() + 1 + 16;
    if bytes.len() < header || &bytes[..MAGIC.len()] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not a preset", path.display()),
        ));
    }
    let version = bytes[MAGIC.len()];
    if version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} was written by a later version", path.display()),
        ));
    }
    let mut cid = [0u8; 16];
    cid.copy_from_slice(&bytes[MAGIC.len() + 1..header]);
    Ok((cid, bytes[header..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mini-host-presets-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_plugin_is_filed_under_its_own_file_name() {
        assert_eq!(
            plugin_name(Path::new("C:/plugins/vst-cake-loader.dll")),
            "vst-cake-loader"
        );
        assert_eq!(plugin_name(Path::new("/plugins/Notepad.vst3")), "Notepad");
    }

    #[test]
    fn a_bundle_and_the_binary_inside_it_share_a_directory() {
        let bundle = Path::new("/plugins/Notepad.vst3");
        let inside = Path::new("/plugins/Notepad.vst3/Contents/x86_64-win/Notepad.vst3");
        assert_eq!(plugin_name(bundle), plugin_name(inside));

        let mac = Path::new("/plugins/Notepad.vst3/Contents/MacOS/Notepad");
        assert_eq!(plugin_name(mac), "Notepad");
    }

    #[test]
    fn a_nameless_path_still_gets_a_directory() {
        assert_eq!(plugin_name(Path::new("/")), "unknown-plugin");
    }

    #[test]
    fn presets_sit_next_to_the_executable() {
        let dir = directory_for(Path::new("Notepad.vst3"));
        assert!(dir.ends_with(Path::new("presets/Notepad")), "{dir:?}");
    }

    #[test]
    fn a_preset_round_trips() {
        let dir = temp("round-trip");
        let path = dir.join("first.preset");
        let cid = [7u8; 16];
        save(&path, cid, b"whatever the plugin said").unwrap();

        let (read_cid, state) = load(&path).unwrap();
        assert_eq!(read_cid, cid);
        assert_eq!(state, b"whatever the plugin said");
    }

    #[test]
    fn saving_creates_the_directory() {
        let dir = temp("mkdir");
        let path = dir.join("nested/deeper/first.preset");
        save(&path, [0; 16], b"x").unwrap();
        assert!(path.exists());
    }

    #[test]
    fn an_empty_state_round_trips() {
        let dir = temp("empty");
        let path = dir.join("empty.preset");
        save(&path, [1; 16], b"").unwrap();
        let (_, state) = load(&path).unwrap();
        assert!(state.is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_preset_is_refused() {
        let dir = temp("bogus");
        let path = dir.join("bogus.preset");
        std::fs::write(&path, b"just some bytes that are long enough to be a header").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn a_truncated_preset_is_refused_rather_than_read_past() {
        let dir = temp("short");
        let path = dir.join("short.preset");
        std::fs::write(&path, MAGIC).unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn a_preset_from_a_later_version_is_refused() {
        let dir = temp("future");
        let path = dir.join("future.preset");
        let mut bytes = MAGIC.to_vec();
        bytes.push(VERSION + 1);
        bytes.extend_from_slice(&[0u8; 16]);
        std::fs::write(&path, bytes).unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn a_missing_preset_is_an_error_not_a_panic() {
        assert!(load(&temp("missing").join("nope.preset")).is_err());
    }
}
