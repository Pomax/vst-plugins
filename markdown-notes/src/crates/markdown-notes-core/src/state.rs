//! Plugin state serialisation.
//!
//! This is what the DAW writes into its project file. It carries the notes, the
//! view mode, the editor window size and the path of the file on disk (if the
//! notes came from one), so reopening a project restores the session exactly.

use serde::{Deserialize, Serialize};

use crate::colours::ColourScheme;
use crate::edit::{Editor, Theme, ViewMode, DEFAULT_TITLE};

/// Bumped whenever the layout changes incompatibly.
pub const STATE_VERSION: u32 = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default = "default_version")]
    pub version: u32,
    /// The document, whole. This is the state.
    ///
    /// What is stored with it is what the user set and cannot be worked out
    /// again: the note's name, the colours, the view mode, the theme.
    /// Nothing else. The sections, which one was in front, where the caret sat,
    /// how big the window was and which file the document was last written to
    /// are all how it was being looked at, not what it is. State that carries
    /// any of them is read for its document and the rest ignored.
    #[serde(default)]
    pub notes: String,
    /// The note's name, which the user sets in the toolbar. Absent in state
    /// written before it existed, where it defaults.
    #[serde(default = "default_title")]
    pub title: String,
    /// Both colour schemes. Absent in state written before they existed.
    #[serde(default)]
    pub colours: ColourScheme,
    #[serde(default = "default_mode")]
    pub mode: String,
    /// "light", "dark" or "auto". Absent in state written before themes
    /// existed, which is why it defaults rather than failing the whole parse.
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_version() -> u32 {
    STATE_VERSION
}
fn default_mode() -> String {
    "wysiwyg".to_string()
}
fn default_theme() -> String {
    Theme::default().as_str().to_string()
}
fn default_title() -> String {
    DEFAULT_TITLE.to_string()
}
impl Default for PluginState {
    fn default() -> Self {
        PluginState {
            version: STATE_VERSION,
            notes: String::new(),
            title: default_title(),
            colours: ColourScheme::default(),
            mode: default_mode(),
            theme: default_theme(),
        }
    }
}

impl PluginState {
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| b"{}".to_vec())
    }

    /// Parse state written by [`PluginState::to_bytes`].
    ///
    /// Anything that is not valid JSON is treated as raw note text, so a state
    /// blob from a hand-written host (or a future format we fail to parse)
    /// still restores the user's words rather than losing them.
    pub fn from_bytes(bytes: &[u8]) -> PluginState {
        if bytes.is_empty() {
            return PluginState::default();
        }
        match serde_json::from_slice::<PluginState>(bytes) {
            Ok(state) => state,
            Err(_) => PluginState {
                notes: String::from_utf8_lossy(bytes).into_owned(),
                ..PluginState::default()
            },
        }
    }
}

impl Editor {
    /// Capture everything the DAW should persist.
    pub fn save_state(&self) -> PluginState {
        PluginState {
            version: STATE_VERSION,
            notes: self.document_text(),
            title: self.title.clone(),
            colours: self.colours,
            mode: self.mode.as_str().to_string(),
            theme: self.theme.as_str().to_string(),
        }
    }

    /// Restore from persisted state.
    pub fn load_state(&mut self, state: &PluginState) {
        // The document is the state. Sections are how it is shown, not part of
        // it, so they are worked out again from the text the same way opening a
        // file works them out, and the first of them is the one that opens:
        // which section was in front is no more part of the document than the
        // sections themselves.
        self.set_document_text(&state.notes);
        self.mode = ViewMode::from_str(&state.mode);
        self.theme = Theme::from_str(&state.theme);
        // An empty name is a name: somebody cleared it, and it stays cleared.
        // Only state with no name at all falls back, which `default_title`
        // does while parsing.
        self.title = state.title.clone();
        self.colours = state.colours;
        // The document that arrives is not the file that happened to be open
        // when it was written. It belongs to whoever loaded it now.
        self.file = None;
        self.dirty = false;
        self.clear_history();
    }

    pub fn state_bytes(&self) -> Vec<u8> {
        self.save_state().to_bytes()
    }

    pub fn load_state_bytes(&mut self, bytes: &[u8]) {
        let state = PluginState::from_bytes(bytes);
        self.load_state(&state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::ViewMode;

    /// Every section comes back, with its own text, not just the live one.
    ///
    /// This is what a preset holds: the same bytes `IComponent::getState`
    /// hands a DAW. A section arriving empty, or holding another section's
    /// text, is the document coming back wrong.
    ///
    /// The sections come back because the document does, not because they were
    /// kept: nothing about them is written down, which is why the one in front
    /// is the first and not whichever was in front when it was saved.
    #[test]
    fn round_trips_every_section() {
        let mut e = Editor::new();
        e.set_document_text("# First\n\nthe first part\n\n# Second\n\nthe second part");
        e.set_active_section(1);
        e.title = "Project notes".to_string();

        let bytes = e.state_bytes();
        let mut restored = Editor::new();
        restored.load_state_bytes(&bytes);

        assert_eq!(restored.section_count(), 2);
        assert_eq!(restored.section_text(0), "# First\n\nthe first part");
        assert_eq!(restored.section_text(1), "# Second\n\nthe second part");
        // The first section opens, whichever one was in front when it was
        // saved.
        assert_eq!(restored.active_section(), 0);
        assert_eq!(restored.title, "Project notes");
    }

    /// The document and the settings come back. Nothing about the window, the
    /// caret or a file does, and a document that arrives belongs to nobody.
    #[test]
    fn the_document_and_the_settings_round_trip() {
        let mut e = Editor::with_text("# Notes\n\n- one\n- two");
        e.mode = ViewMode::Raw;
        e.set_size(1024, 768);
        e.file = Some("C:/notes/todo.md".into());
        e.set_caret(3);

        let bytes = e.state_bytes();
        let mut restored = Editor::new();
        restored.set_size(640, 480);
        restored.load_state_bytes(&bytes);

        assert_eq!(restored.text(), "# Notes\n\n- one\n- two");
        assert_eq!(restored.mode, ViewMode::Raw);
        assert_eq!(restored.width, 640, "the window is the host's, not the state's");
        assert_eq!(restored.height, 480);
        assert!(restored.file.is_none(), "a loaded document is tied to no file");
        assert!(!restored.dirty);

        // By key, not by substring: `caret` is also the name of a colour.
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let object = json.as_object().unwrap();
        for absent in ["tabs", "active", "caret", "file", "width", "height"] {
            assert!(
                !object.contains_key(absent),
                "{absent} should not be in the state: {:?}",
                object.keys().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn the_saved_notes_hold_the_whole_document() {
        let mut e = Editor::with_text("# One");
        e.new_section();
        e.set_text("# Two");
        let state = e.save_state();
        assert_eq!(state.notes, "# One\n\n# Two");
    }

    #[test]
    /// State carrying a list of its own alongside the document opens from the
    /// document, and the list is ignored.
    fn a_stored_list_of_parts_is_ignored_in_favour_of_the_document() {
        let s = PluginState::from_bytes(
            br##"{"notes":"# One\n\n# Two","tabs":[{"notes":"nonsense","caret":0}],"active":1}"##,
        );

        let mut e = Editor::new();
        e.load_state(&s);
        assert_eq!(e.section_count(), 2);
        assert_eq!(e.section_text(0), "# One");
        assert_eq!(e.section_text(1), "# Two");
        assert_eq!(e.active_section(), 0);
    }

    /// A colour whose key was written under an older name is still read, and
    /// the state around it still parses as state.
    ///
    /// Failing to parse does not lose the document, it renders it: unparsed
    /// bytes are taken for note text, so the whole blob would open as the
    /// note. A saved preset is exactly that blob, so this is what a preset
    /// written by an earlier build turns into when a key stops being found.
    #[test]
    fn a_colour_under_an_older_key_is_still_read() {
        let mut e = Editor::new();
        let mut colours = ColourScheme::default();
        colours.dark.section_selected_fill = crate::colours::Rgba::rgb(1, 2, 3);
        colours.dark.section_selected_label = crate::colours::Rgba::rgb(4, 5, 6);
        e.colours = colours;
        let current = String::from_utf8(e.state_bytes()).unwrap();

        let older = current
            .replace("\"section_selected_fill\"", "\"tab_selected_fill\"")
            .replace("\"section_selected_label\"", "\"tab_selected_label\"");
        assert!(older.contains("tab_selected_fill"), "the state to read back was not built");

        let state = PluginState::from_bytes(older.as_bytes());
        assert_eq!(state.version, STATE_VERSION);
        assert!(
            !state.notes.contains('{'),
            "the state was taken for note text: {}",
            state.notes
        );
        assert_eq!(state.colours.dark.section_selected_fill, crate::colours::Rgba::rgb(1, 2, 3));
        assert_eq!(state.colours.dark.section_selected_label, crate::colours::Rgba::rgb(4, 5, 6));
    }

    #[test]
    fn the_title_round_trips_and_defaults() {
        let mut e = Editor::new();
        assert_eq!(e.title, "...project title goes here...");
        e.title = "funky cake".to_string();

        let mut restored = Editor::new();
        restored.load_state_bytes(&e.state_bytes());
        assert_eq!(restored.title, "funky cake");

        // Written before the title existed.
        let s = PluginState::from_bytes(br#"{"notes":"x"}"#);
        let mut old = Editor::new();
        old.load_state(&s);
        assert_eq!(old.title, "...project title goes here...");
    }

    /// A title that is there and empty was emptied on purpose. Only a title
    /// that is not there at all falls back, which is state written before
    /// there were titles.
    #[test]
    fn a_title_that_was_cleared_comes_back_cleared() {
        for stored in ["", "   "] {
            let json = format!(r#"{{"notes":"x","title":{}}}"#, serde_json::to_string(stored).unwrap());
            let s = PluginState::from_bytes(json.as_bytes());
            let mut e = Editor::new();
            e.load_state(&s);
            assert_eq!(e.title, stored, "a title of {stored:?} was not kept");
        }
    }

    #[test]
    fn theme_round_trips() {
        for theme in [Theme::Light, Theme::Dark, Theme::Auto] {
            let mut e = Editor::with_text("x");
            e.theme = theme;
            let mut restored = Editor::new();
            restored.load_state_bytes(&e.state_bytes());
            assert_eq!(restored.theme, theme, "theme {theme:?} did not survive");
        }
    }

    #[test]
    fn theme_defaults_to_auto() {
        assert_eq!(Editor::new().theme, Theme::Auto);
        assert_eq!(PluginState::default().theme, "auto");
    }

    #[test]
    fn state_written_before_themes_existed_still_loads() {
        // No "theme" key at all — the field must default rather than the whole
        // parse failing and the notes being treated as raw text.
        let s = PluginState::from_bytes(br#"{"notes":"older project","mode":"raw"}"#);
        assert_eq!(s.notes, "older project");
        assert_eq!(s.theme, "auto");

        let mut e = Editor::new();
        e.load_state(&s);
        assert_eq!(e.theme, Theme::Auto);
        assert_eq!(e.mode, ViewMode::Raw);
    }

    #[test]
    fn an_unknown_theme_falls_back_to_auto() {
        let s = PluginState::from_bytes(br#"{"notes":"x","theme":"solarized"}"#);
        let mut e = Editor::new();
        e.load_state(&s);
        assert_eq!(e.theme, Theme::Auto);
    }

    #[test]
    fn empty_state_is_a_fresh_document() {
        let s = PluginState::from_bytes(&[]);
        assert_eq!(s.notes, "");
        assert_eq!(s.mode, "wysiwyg");
    }

    #[test]
    fn non_json_state_is_kept_as_note_text() {
        let s = PluginState::from_bytes(b"just some notes");
        assert_eq!(s.notes, "just some notes");
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s = PluginState::from_bytes(br#"{"notes":"hi"}"#);
        assert_eq!(s.notes, "hi");
        assert_eq!(s.mode, "wysiwyg");
    }

    /// A window size in old state is ignored, not applied.
    #[test]
    fn a_stored_window_size_does_not_resize_anything() {
        let s = PluginState::from_bytes(br#"{"notes":"x","width":10,"height":10}"#);
        let mut e = Editor::new();
        e.set_size(1024, 768);
        e.load_state(&s);
        assert_eq!(e.width, 1024);
        assert_eq!(e.height, 768);
    }

    #[test]
    fn unicode_survives_the_round_trip() {
        let e = Editor::with_text("# 見出し\n\n- 日本語 **太字** ✓\n");
        let mut restored = Editor::new();
        restored.load_state_bytes(&e.state_bytes());
        assert_eq!(restored.text(), e.text());
    }
}
