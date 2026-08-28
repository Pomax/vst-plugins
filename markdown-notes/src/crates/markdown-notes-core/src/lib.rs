//! # markdown-notes-core
//!
//! The markdown editor behind the VST3 note-taking plugin: document model,
//! as-you-type conversion, WYSIWYG layout, plugin-state serialisation and file
//! I/O. It has no UI and no plugin dependencies, which is what lets the same
//! logic be driven by the GUI, by unit tests, and by synthetic key events
//! injected from a VST3 host.
//!
//! ```
//! use markdown_notes_core::{Editor, Key, Mods};
//!
//! // Blank: a document nobody has opened starts on a heading to be named,
//! // and this types its own.
//! let mut editor = Editor::with_text("");
//! for c in "# Shopping".chars() {
//!     editor.handle_key(Key::Char(c), Mods::NONE);
//! }
//! editor.handle_key(Key::Enter, Mods::NONE);
//! for c in "* milk".chars() {
//!     editor.handle_key(Key::Char(c), Mods::NONE);
//! }
//!
//! // Enter ended the heading's block, so a blank line separates the two, and
//! // the `*` bullet was normalised to `-` as it was typed.
//! assert_eq!(editor.text(), "# Shopping\n\n- milk");
//! // And the reader sees it without the markdown punctuation.
//! assert_eq!(editor.rendered_text(), "Shopping\n\nmilk");
//! ```

pub mod block;
pub mod colours;
pub mod edit;
pub mod file;
pub mod inline;
pub mod state;
pub mod sections;
pub mod text;

pub use block::{parse_document, Block, BlockKind, RenderDoc};
pub use colours::{ColourScheme, Colours, Rgba};
pub use edit::{
    Command, Editor, Key, KeyResult, Mods, Theme, ViewMode, DEFAULT_HEIGHT, DEFAULT_TITLE,
    DEFAULT_WIDTH, MAX_HEIGHT, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH,
};
pub use inline::{Span, SpanRole, Style};
pub use state::{PluginState, STATE_VERSION};
pub use sections::{split_document, MAX_TITLE_CHARS, UNTITLED};
