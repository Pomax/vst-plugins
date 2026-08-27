//! The editor: caret, selection, key handling and as-you-type markdown conversion.
//!
//! The markdown source is always the single source of truth. "Conversion" comes
//! in two flavours, matching how Typora behaves:
//!
//! * **Rewrites.** The source itself changes as you type: `* ` becomes `- `,
//!   Enter continues a list and renumbers it, an empty item exits the list,
//!   a fence auto-closes, selections get wrapped in emphasis markers.
//! * **Rendering.** The source is left alone and the *display* changes:
//!   `## Title` draws as a large heading with the `## ` hidden, until the caret
//!   moves onto that line and the marker is revealed for editing.
//!
//! Everything here is headless and deterministic, so the same code path serves
//! the GUI and the VST3 host's synthetic key injection.

use std::ops::Range;
use std::path::PathBuf;

use crate::block::{self, Block, BlockKind, RenderDoc};
use crate::sections;
use crate::text;

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

/// Which view the editor is presenting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    /// Rendered markdown with markers hidden except on the caret's line.
    Wysiwyg,
    /// The raw markdown source.
    Raw,
}

impl ViewMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewMode::Wysiwyg => "wysiwyg",
            ViewMode::Raw => "raw",
        }
    }
    pub fn from_str(s: &str) -> ViewMode {
        match s {
            "raw" => ViewMode::Raw,
            _ => ViewMode::Wysiwyg,
        }
    }
    pub fn toggled(self) -> ViewMode {
        match self {
            ViewMode::Wysiwyg => ViewMode::Raw,
            ViewMode::Raw => ViewMode::Wysiwyg,
        }
    }
}

/// Which colour scheme the editor draws in.
///
/// `Auto` defers to the operating system. Resolving it needs a platform call,
/// which this crate deliberately does not make. The GUI layer passes what the
/// system reports into [`Theme::is_dark`], keeping the decision itself testable
/// without an OS in the loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    Light,
    Dark,
    /// Follow the system setting.
    #[default]
    Auto,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::Auto => "auto",
        }
    }

    /// Parse a stored theme. Anything unrecognised falls back to `Auto`, so a
    /// state blob from a newer version degrades to following the system rather
    /// than to an arbitrary choice.
    pub fn from_str(s: &str) -> Theme {
        match s {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::Auto,
        }
    }

    /// The order the Ctrl+T shortcut and the toolbar button walk through.
    pub fn cycled(self) -> Theme {
        match self {
            Theme::Auto => Theme::Light,
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Auto,
        }
    }

    /// Whether to draw dark, given what the system currently reports.
    ///
    /// `system_dark` is only consulted for [`Theme::Auto`].
    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Theme::Light => false,
            Theme::Dark => true,
            Theme::Auto => system_dark,
        }
    }
}

/// An action the editor cannot perform itself because it needs the host
/// (native file dialogs, disk access).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Open,
    Save,
    SaveAs,
}

/// Outcome of a key press.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KeyResult {
    /// The editor recognised the key.
    pub handled: bool,
    /// The document text changed.
    pub changed: bool,
    /// A host-level action was requested.
    pub command: Option<Command>,
}

impl KeyResult {
    fn moved() -> Self {
        KeyResult { handled: true, changed: false, command: None }
    }
    fn edited() -> Self {
        KeyResult { handled: true, changed: true, command: None }
    }
    fn command(c: Command) -> Self {
        KeyResult { handled: true, changed: false, command: Some(c) }
    }
}

/// What a note is called until the user renames it.
pub const DEFAULT_TITLE: &str = "...project title goes here...";
pub const MIN_WIDTH: i32 = 320;
pub const MIN_HEIGHT: i32 = 200;
/// Upper bound on the editor window, so a corrupt state cannot ask the host
/// for an absurd one.
pub const MAX_WIDTH: i32 = 8192;
pub const MAX_HEIGHT: i32 = 8192;
pub const DEFAULT_WIDTH: i32 = 900;
pub const DEFAULT_HEIGHT: i32 = 620;

/// The editor state. Owns the document and everything persisted as plugin state.
///
/// The document itself, its selection and its undo history are
/// [`kode_core::Editor`]'s: buffer, cursor motion by character, word and line,
/// selection and undo all come from there rather than being written here, and
/// the markdown rewrites from [`kode_markdown`], which works on the same type.
/// Each section is one of them, and the one in front is `sections[active]`.
/// The whole document is in the vec, with no live copy beside it.
///
/// Not `kode_markdown::MarkdownEditor`: that is the same editor with a
/// tree-sitter parse kept beside it, and nothing here reads the tree. The
/// markdown commands and input rules all work off the buffer, and what the
/// renderer needs — where each marker starts and ends, which lines a fence
/// covers — is [`block::parse_document`]. Carrying the parse costs a grammar
/// in the binary and a reparse of the document on every keystroke.
pub struct Editor {
    sections: Vec<kode_core::Editor>,
    /// Whether each section has a caret. One that was clicked away has none,
    /// and nothing can be typed into it until one is placed again.
    placed: Vec<bool>,
    active: usize,
    pub mode: ViewMode,
    pub theme: Theme,
    pub colours: crate::colours::ColourScheme,
    pub width: i32,
    pub height: i32,
    pub file: Option<PathBuf>,
    /// The note's own name, edited in the toolbar. It is not part of the
    /// markdown and is not written to disk with it.
    pub title: String,
    pub dirty: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new()
    }
}

impl Editor {
    pub fn new() -> Editor {
        Editor {
            sections: vec![kode_core::Editor::empty()],
            placed: vec![true],
            active: 0,
            mode: ViewMode::Wysiwyg,
            theme: Theme::Auto,
            colours: crate::colours::ColourScheme::default(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            file: None,
            title: DEFAULT_TITLE.to_string(),
            dirty: false,
        }
    }

    pub fn with_text(text: impl Into<String>) -> Editor {
        let mut e = Editor::new();
        let text = text.into();
        e.sections[0] = kode_core::Editor::new(&text);
        e.sections[0].move_to_end();
        e
    }


    /// The section in front.
    fn live(&self) -> &kode_core::Editor {
        &self.sections[self.active]
    }

    fn live_mut(&mut self) -> &mut kode_core::Editor {
        &mut self.sections[self.active]
    }

    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    pub fn active_section(&self) -> usize {
        self.active
    }

    /// The markdown in section `index`.
    pub fn section_text(&self, index: usize) -> String {
        self.sections.get(index).map(|t| t.text()).unwrap_or_default()
    }

    /// The section's heading, or `untitled`.
    pub fn section_title(&self, index: usize) -> String {
        sections::title_of(&self.section_text(index))
    }

    /// The title as it fits on the section's button, cut short with `...` when
    /// too long.
    pub fn section_display_title(&self, index: usize) -> String {
        sections::display_title(&self.section_title(index))
    }

    /// Whether the section shows a shortened title and so needs a tooltip.
    pub fn section_title_is_truncated(&self, index: usize) -> bool {
        sections::is_truncated(&self.section_title(index))
    }

    pub fn set_active_section(&mut self, index: usize) {
        if index >= self.sections.len() || index == self.active {
            return;
        }
        self.active = index;
    }

    /// Add an empty section after the last one and switch to it.
    pub fn new_section(&mut self) -> usize {
        self.sections.push(kode_core::Editor::empty());
        self.placed.push(true);
        self.active = self.sections.len() - 1;
        self.dirty = true;
        self.active
    }

    /// Close section `index`. The last remaining section is emptied rather than
    /// removed, because there is always a buffer to type into.
    pub fn close_section(&mut self, index: usize) {
        if index >= self.sections.len() {
            return;
        }
        self.dirty = true;
        if self.sections.len() == 1 {
            self.sections[0] = kode_core::Editor::empty();
            self.placed[0] = true;
            return;
        }
        self.sections.remove(index);
        self.placed.remove(index);
        self.active = if self.active > index {
            self.active - 1
        } else {
            self.active.min(self.sections.len() - 1)
        };
    }

    /// Move a section to another position, as a drag does.
    pub fn move_section(&mut self, from: usize, to: usize) {
        let last = self.sections.len().saturating_sub(1);
        if from > last || from == to {
            return;
        }
        let to = to.min(last);
        let buffer = self.sections.remove(from);
        self.sections.insert(to, buffer);
        let placed = self.placed.remove(from);
        self.placed.insert(to, placed);

        // The active section keeps its contents, not its position.
        let active = if self.active == from {
            to
        } else {
            let mut a = self.active;
            if a > from {
                a -= 1;
            }
            if a >= to {
                a += 1;
            }
            a
        };
        self.active = active.min(last);
        self.dirty = true;
    }

    /// Every section joined back into the one document they are a view of.
    ///
    /// A single section is the document, byte for byte: nothing is normalised
    /// away, so opening a file and saving it back does not rewrite it.
    pub fn document_text(&self) -> String {
        if self.sections.len() == 1 {
            return self.sections[0].text();
        }
        let parts: Vec<String> = (0..self.sections.len()).map(|i| self.section_text(i)).collect();
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        sections::join_document(&borrowed)
    }

    /// Replace every section by splitting `text` on its top-level headings.
    pub fn set_document_text(&mut self, text: &str) {
        self.sections = sections::split_document(text)
            .into_iter()
            .map(|part| {
                let mut section = kode_core::Editor::new(&part);
                section.move_to_end();
                section
            })
            .collect();
        if self.sections.is_empty() {
            self.sections.push(kode_core::Editor::empty());
        }
        self.placed = vec![true; self.sections.len()];
        self.active = 0;
    }


    pub fn text(&self) -> String {
        self.live().text()
    }

    /// A byte offset into [`Editor::text`].
    ///
    /// The engine works in `(line, column)` positions with the column counted
    /// in characters; everything above this works in byte offsets, and the
    /// buffer converts between them.
    fn byte_of(&self, pos: kode_core::Position) -> usize {
        let buffer = self.live().buffer();
        buffer.char_to_byte(buffer.pos_to_char(buffer.clamp_pos(pos)))
    }

    fn pos_of(&self, byte: usize) -> kode_core::Position {
        let buffer = self.live().buffer();
        let byte = byte.min(buffer.len_bytes());
        buffer.char_to_pos(buffer.byte_to_char(byte))
    }

    pub fn caret(&self) -> usize {
        self.byte_of(self.live().cursor())
    }

    pub fn anchor(&self) -> usize {
        self.byte_of(self.live().selection().anchor)
    }

    /// The selected range, empty when there is no selection.
    pub fn selection(&self) -> Range<usize> {
        let (caret, anchor) = (self.caret(), self.anchor());
        caret.min(anchor)..caret.max(anchor)
    }

    pub fn selected_text(&self) -> String {
        self.live().selected_text()
    }

    pub fn has_selection(&self) -> bool {
        !self.live().selection().is_cursor()
    }

    pub fn caret_line(&self) -> usize {
        self.live().cursor().line
    }

    pub fn caret_column(&self) -> usize {
        self.live().cursor().col
    }

    /// Replace the whole document, e.g. when loading a file or plugin state.
    pub fn set_text(&mut self, new: impl Into<String>) {
        let new = new.into();
        let caret = self.caret().min(new.len());
        let caret = text::clamp_boundary(&new, caret);
        self.sections[self.active] = kode_core::Editor::new(&new);
        self.set_caret(caret);
    }

    /// Whether the section in front has somewhere to type.
    ///
    /// A click that lands where there is no line to put it on takes the caret
    /// away, and until one is placed again there is nowhere for text to go.
    pub fn has_caret(&self) -> bool {
        self.placed.get(self.active).copied().unwrap_or(false)
    }

    /// Take the caret away, and the selection with it.
    pub fn clear_caret(&mut self) {
        let at = self.live().cursor();
        self.live_mut().set_cursor(at);
        if let Some(placed) = self.placed.get_mut(self.active) {
            *placed = false;
        }
    }

    fn place_caret(&mut self) {
        if let Some(placed) = self.placed.get_mut(self.active) {
            *placed = true;
        }
    }

    pub fn set_caret(&mut self, pos: usize) {
        let pos = self.pos_of(pos);
        self.live_mut().set_cursor(pos);
        self.place_caret();
    }

    pub fn select(&mut self, range: Range<usize>) {
        let (start, end) = (self.pos_of(range.start), self.pos_of(range.end));
        self.live_mut().set_selection(start, end);
        self.place_caret();
    }

    pub fn select_all(&mut self) {
        self.live_mut().select_all();
        self.place_caret();
    }

    /// Set the window size, clamped to something a window can actually be.
    ///
    /// The upper bound matters as much as the lower one: this value can arrive
    /// from a project file, and a corrupt or hand-edited state carrying
    /// `i32::MAX` would otherwise be handed to the host as a real window size.
    pub fn set_size(&mut self, width: i32, height: i32) {
        self.width = width.clamp(MIN_WIDTH, MAX_WIDTH);
        self.height = height.clamp(MIN_HEIGHT, MAX_HEIGHT);
    }

    /// Step to the next theme: Auto → Light → Dark → Auto.
    pub fn cycle_theme(&mut self) {
        self.theme = self.theme.cycled();
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.toggled();
    }

    /// Lay the document out for rendering. In raw mode markers are always
    /// visible, since the user is looking at the source.
    pub fn render(&self) -> RenderDoc {
        let text = self.text();
        match self.mode {
            ViewMode::Wysiwyg => block::parse_document(&text, Some(self.caret())),
            ViewMode::Raw => {
                let mut doc = block::parse_document(&text, Some(self.caret()));
                for b in &mut doc.blocks {
                    b.marker_visible = true;
                    for s in &mut b.spans {
                        s.visible = true;
                    }
                }
                doc
            }
        }
    }

    /// The document as the reader sees it, with markers stripped. Handy for
    /// asserting on WYSIWYG output in tests.
    pub fn rendered_text(&self) -> String {
        let text = self.text();
        let doc = block::parse_document(&text, None);
        doc.blocks
            .iter()
            .map(|b| b.visible_text(&text))
            .collect::<Vec<_>>()
            .join("\n")
    }


    /// Drop undo history, after loading a document, so the user cannot
    /// undo their way back into the previous file's contents.
    ///
    /// Loading replaces the section wholesale, and a fresh one has no history.
    pub fn clear_history(&mut self) {
        let text = self.text();
        let at = self.live().cursor();
        self.sections[self.active] = kode_core::Editor::new(&text);
        // A fresh editor starts at the top; the caret was not what was being
        // cleared.
        self.live_mut().set_cursor(at);
    }

    pub fn undo(&mut self) -> bool {
        let before = self.live().version();
        self.live_mut().undo();
        self.live().version() != before
    }

    pub fn redo(&mut self) -> bool {
        let before = self.live().version();
        self.live_mut().redo();
        self.live().version() != before
    }


    fn replace_range(&mut self, range: Range<usize>, with: &str) {
        let (start, end) = (self.pos_of(range.start), self.pos_of(range.end));
        self.live_mut().set_selection(start, end);
        self.live_mut().insert(with);
        self.dirty = true;
    }

    /// Delete the selection, if there is one. Reports whether there was.
    pub fn delete_selection(&mut self) -> bool {
        if !self.has_selection() {
            return false;
        }
        self.live_mut().delete_selection();
        self.dirty = true;
        true
    }

    /// Insert text at the caret, replacing any selection. With no caret there
    /// is nowhere to put it, so a paste lands nowhere either.
    pub fn insert_str(&mut self, s: &str) {
        if !self.has_caret() {
            return;
        }
        self.live_mut().insert(s);
        self.dirty = true;
    }


    /// Route a key press.
    ///
    /// What an editor does with a key is the engine's business; this maps our
    /// key enum onto its calls and reports whether the document changed.
    pub fn handle_key(&mut self, key: Key, mods: Mods) -> KeyResult {
        if mods.ctrl {
            if let Some(r) = self.handle_ctrl(key, mods) {
                return r;
            }
        }
        // No caret is nowhere to put a character, and nothing to move.
        if !self.has_caret() {
            return KeyResult { handled: false, changed: false, command: None };
        }
        let before = self.live().version();
        match key {
            Key::Char(c) => {
                let mut buffer = [0u8; 4];
                self.live_mut().insert(c.encode_utf8(&mut buffer));
                match c {
                    ' ' => self.normalise_bullet(),
                    ']' => self.expand_checkbox(),
                    _ => {}
                }
            }
            Key::Enter => {
                // The engine's rule first: Enter inside a list continues it,
                // and on an empty item ends it.
                if self.close_fence() {
                    // An opened fence gets its closing line, and the caret is
                    // left on the empty line between the two.
                } else if !kode_markdown::InputRules::handle_enter(self.live_mut()) {
                    self.end_block();
                }
            }
            Key::Backspace => {
                if kode_markdown::InputRules::handle_backspace_at_prefix(self.live_mut()) {
                    // The rule took it: a marker was removed, not a character.
                } else if mods.ctrl {
                    self.live_mut().delete_word_back();
                } else {
                    self.live_mut().backspace();
                }
            }
            Key::Delete => {
                if mods.ctrl {
                    self.live_mut().delete_word_forward();
                } else {
                    self.live_mut().delete_forward();
                }
            }
            Key::Tab => {
                let handled = if mods.shift {
                    kode_markdown::InputRules::handle_shift_tab(self.live_mut())
                } else {
                    kode_markdown::InputRules::handle_tab(self.live_mut())
                };
                if handled {
                    // The rule took it: a list item moved a level.
                } else if mods.shift {
                    self.live_mut().outdent();
                } else {
                    self.live_mut().indent();
                }
            }
            Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End
            | Key::PageUp | Key::PageDown => self.motion(key, mods),
            Key::Escape => {
                let at = self.live().cursor();
                self.live_mut().set_cursor(at);
            }
        }
        if self.live().version() != before {
            self.dirty = true;
            KeyResult::edited()
        } else {
            KeyResult::moved()
        }
    }

    /// Move the caret, extending the selection when shift is held.
    fn motion(&mut self, key: Key, mods: Mods) {
        // A page is a screenful of lines at the editor's current height.
        let page = ((self.height / 20).max(1)) as usize;
        let section = &mut self.sections[self.active];
        match (key, mods.shift, mods.ctrl) {
            (Key::Left, false, false) => section.move_left(),
            (Key::Left, true, false) => section.extend_selection_left(),
            (Key::Left, false, true) => section.move_word_left(),
            (Key::Left, true, true) => section.extend_selection_word_left(),
            (Key::Right, false, false) => section.move_right(),
            (Key::Right, true, false) => section.extend_selection_right(),
            (Key::Right, false, true) => section.move_word_right(),
            (Key::Right, true, true) => section.extend_selection_word_right(),
            (Key::Up, false, _) => section.move_up(),
            (Key::Up, true, _) => section.extend_selection_up(),
            (Key::Down, false, _) => section.move_down(),
            (Key::Down, true, _) => section.extend_selection_down(),
            (Key::Home, false, false) => section.move_to_line_start(),
            (Key::Home, true, false) => section.extend_selection_to_line_start(),
            (Key::Home, false, true) => section.move_to_start(),
            (Key::Home, true, true) => section.extend_selection_to_start(),
            (Key::End, false, false) => section.move_to_line_end(),
            (Key::End, true, false) => section.extend_selection_to_line_end(),
            (Key::End, false, true) => section.move_to_end(),
            (Key::End, true, true) => section.extend_selection_to_end(),
            (Key::PageUp, _, _) => section.page_up(page),
            (Key::PageDown, _, _) => section.page_down(page),
            _ => {}
        }
    }

    fn handle_ctrl(&mut self, key: Key, mods: Mods) -> Option<KeyResult> {
        let c = match key {
            Key::Char(c) => c.to_ascii_lowercase(),
            _ => return None,
        };
        // Opening, saving, the view mode and the theme are the window's, and
        // work whether or not there is a caret. The rest are the document's.
        let window = matches!(c, 'o' | 's' | '/' | 't');
        if !window && !self.has_caret() {
            return Some(KeyResult { handled: false, changed: false, command: None });
        }
        let before = self.live().version();
        let r = match c {
            'b' => {
                self.markdown_command(kode_markdown::MarkdownCommands::toggle_bold);
                KeyResult::edited()
            }
            'i' => {
                self.markdown_command(kode_markdown::MarkdownCommands::toggle_italic);
                KeyResult::edited()
            }
            'd' => {
                self.markdown_command(kode_markdown::MarkdownCommands::toggle_strikethrough);
                KeyResult::edited()
            }
            'e' | '`' => {
                self.markdown_command(kode_markdown::MarkdownCommands::toggle_inline_code);
                KeyResult::edited()
            }
            'k' => {
                kode_markdown::MarkdownCommands::insert_link(self.live_mut(), "");
                // Between the brackets, which is where the URL goes and where
                // the caret is left waiting for it.
                self.live_mut().move_left();
                KeyResult::edited()
            }
            'a' => {
                self.select_all();
                KeyResult::moved()
            }
            // The clipboard is the platform's. These are claimed so the letter
            // is not typed as well, and the copy, cut or paste itself arrives
            // separately as a clipboard event.
            'c' | 'x' | 'v' => KeyResult::moved(),
            'z' => {
                let changed = if mods.shift { self.redo() } else { self.undo() };
                KeyResult { handled: true, changed, command: None }
            }
            'y' => {
                let changed = self.redo();
                KeyResult { handled: true, changed, command: None }
            }
            '/' => {
                self.toggle_mode();
                KeyResult::moved()
            }
            't' => {
                self.cycle_theme();
                KeyResult::moved()
            }
            'o' => KeyResult::command(Command::Open),
            's' => {
                if mods.shift {
                    KeyResult::command(Command::SaveAs)
                } else {
                    KeyResult::command(Command::Save)
                }
            }
            _ => return None,
        };
        if self.live().version() != before {
            self.dirty = true;
        }
        Some(r)
    }

    /// The line the caret is on, classified.
    ///
    /// This is [`block`]'s parse and not the section's tree-sitter tree, because
    /// the rules that use it fire while a fence is being typed and tree-sitter
    /// cannot classify a fence that nothing closes yet: it parses as an error
    /// node holding a delimiter. Fences open and close across the document, so
    /// whether a line is code is not something the line says by itself, and
    /// [`block::parse_document`] carries that state down the document.
    fn caret_block(&self) -> Option<Block> {
        let text = self.text();
        let line = self.caret_line();
        block::parse_document(&text, None).blocks.get(line).cloned()
    }

    /// Whether the caret is inside a fenced code block, its fences included.
    fn in_code(&self) -> bool {
        self.caret_block().is_some_and(|b| {
            matches!(b.kind, BlockKind::Code | BlockKind::Fence { .. })
        })
    }

    /// `* ` and `+ ` become `- ` as they are typed.
    ///
    /// Markdown accepts all three and means the same thing by them; the
    /// document keeps one of them so a file written here reads consistently.
    /// The engine has no opinion on which bullet you use, so this is ours.
    fn normalise_bullet(&mut self) {
        let text = self.text();
        let caret = self.caret();
        let Some((start, _)) = text::line_range(&text, self.caret_line()) else {
            return;
        };
        let line = &text[start..caret];
        let indent = line.len() - line.trim_start().len();
        // Only the bullet itself, and only the moment its space is typed.
        if line[indent..] != *"* " && line[indent..] != *"+ " {
            return;
        }
        self.replace_range(start + indent..start + indent + 1, "-");
        // One character for one character, so typing carries on where it was
        // rather than in the middle of the bullet just rewritten.
        self.set_caret(caret);
    }

    /// `-[]` and `-[x]` become `- [ ] ` and `- [x] ` as the bracket closes.
    ///
    /// Typing a task item in full is `- [ ] `: a bullet, a space, a box with
    /// a space in it, a space. The shorthand is what people actually type, and
    /// spelling it out is this editor's job, not the engine's.
    fn expand_checkbox(&mut self) {
        let text = self.text();
        let caret = self.caret();
        let Some((start, _)) = text::line_range(&text, self.caret_line()) else {
            return;
        };
        let line = &text[start..caret];
        let indent = line.len() - line.trim_start().len();
        let body = &line[indent..];
        let ticked = match body {
            "-[]" | "*[]" | "+[]" => " ",
            "-[x]" | "*[x]" | "+[x]" | "-[X]" | "*[X]" | "+[X]" => "x",
            _ => return,
        };
        // No trailing space: the one the user is about to type completes it.
        let from = start + indent;
        self.replace_range(from..caret, &format!("- [{ticked}]"));
    }

    /// Enter on a line that opens a code fence writes the closing fence too,
    /// and leaves the caret on the empty line between them.
    ///
    /// Reports whether it did, so Enter can fall through to the other rules.
    /// A fence with nothing to close it swallows the rest of the document.
    fn close_fence(&mut self) -> bool {
        let text = self.text();
        let line = self.caret_line();
        let Some((_, end)) = text::line_range(&text, line) else {
            return false;
        };
        // Only a fence that opens one, and only from the end of it.
        let opens = self
            .caret_block()
            .is_some_and(|b| matches!(b.kind, BlockKind::Fence { open: true, .. }));
        if !opens || self.caret() != end {
            return false;
        }
        let already_closed = text[end..]
            .lines()
            .any(|l| l.trim_start().starts_with("```"));
        if already_closed {
            return false;
        }
        self.live_mut().insert("\n\n```");
        // Back onto the blank line, which is where the code goes.
        self.live_mut().move_up();
        self.live_mut().move_to_line_end();
        true
    }

    /// Enter ends a block: a heading or a paragraph is followed by a blank
    /// line, because a single newline in markdown is a soft break and would
    /// leave the next line inside the same block.
    ///
    /// Only outside lists and code, where a single newline is what is meant,
    /// and where the engine's own rule has already had its say.
    fn end_block(&mut self) {
        let text = self.text();
        let line = self.caret_line();

        // Inside a fence a newline is a newline: the code is what it says it
        // is, and a blank line put between two statements is the editor
        // rewriting the user's program.
        if self.in_code() {
            self.live_mut().insert_newline();
            return;
        }

        let ends_a_block = text::line_range(&text, line)
            .map(|(start, end)| {
                let content = &text[start..end];
                let (quote_len, _) = block::quote_prefix(content);
                let body = &content[quote_len..];
                !body.trim().is_empty() && block::list_marker(body).is_none()
            })
            .unwrap_or(false);
        if ends_a_block && self.caret() >= text::line_range(&text, line).map_or(0, |(_, e)| e) {
            self.live_mut().insert("\n\n");
        } else {
            self.live_mut().insert_newline();
        }
    }

    /// Run one of the markdown commands over the section in front.
    fn markdown_command(&mut self, run: fn(&mut kode_core::Editor)) {
        run(self.live_mut());
    }

    /// Toggle the task checkbox on `line`, adding one if the item lacks it.
    ///
    /// Not the engine's: a checkbox is markdown this editor offers a click on.
    pub fn toggle_checkbox(&mut self, line: usize) -> bool {
        let source = self.text();
        let Some((ls, le)) = text::line_range(&source, line) else {
            return false;
        };
        let src = source[ls..le].to_string();
        let (quote_len, _) = block::quote_prefix(&src);
        let Some((marker_len, kind)) = block::list_marker(&src[quote_len..]) else {
            return false;
        };
        let marker_end = ls + quote_len + marker_len;
        let caret = self.caret();
        match kind.checked() {
            Some(true) => self.replace_range(marker_end - 4..marker_end, "[ ] "),
            Some(false) => self.replace_range(marker_end - 4..marker_end, "[x] "),
            None => {
                self.replace_range(marker_end..marker_end, "[ ] ");
                // Typing continues where it was, not where the tick landed.
                if caret >= marker_end {
                    self.set_caret(caret + 4);
                }
            }
        }
        self.dirty = true;
        true
    }
}

#[cfg(test)]
mod typing_tests {
    use super::*;

    fn typed(s: &str) -> Editor {
        let mut e = Editor::new();
        for c in s.chars() {
            if c == '\n' {
                e.handle_key(Key::Enter, Mods::NONE);
            } else {
                e.handle_key(Key::Char(c), Mods::NONE);
            }
        }
        e
    }

    #[test]
    fn a_checkbox_is_typed_as_it_is_written() {
        assert_eq!(typed("- [ ] Buy milk").text(), "- [ ] Buy milk");
    }

    #[test]
    fn the_checkbox_shorthand_is_spelled_out() {
        assert_eq!(typed("-[] Buy milk").text(), "- [ ] Buy milk");
        assert_eq!(typed("-[x] Ship it").text(), "- [x] Ship it");
    }

    #[test]
    fn a_loaded_document_keeps_its_caret_when_history_is_cleared() {
        let mut e = Editor::new();
        e.set_document_text("éx");
        e.clear_history();
        e.handle_key(Key::Char('!'), Mods::NONE);
        assert_eq!(e.text(), "éx!");
    }

    #[test]
    fn a_star_bullet_becomes_a_dash() {
        assert_eq!(typed("* milk").text(), "- milk");
    }

    #[test]
    fn a_loaded_document_takes_typing_at_its_end() {
        let mut e = Editor::new();
        e.set_document_text("éx");
        e.handle_key(Key::Char('!'), Mods::NONE);
        assert_eq!(e.text(), "éx!");
    }

    #[test]
    fn ctrl_k_leaves_the_caret_between_the_brackets() {
        let mut e = Editor::with_text("Anthropic");
        e.select(0..9);
        e.handle_key(Key::Char('k'), Mods::CTRL);
        for c in "https://example.com".chars() {
            e.handle_key(Key::Char(c), Mods::NONE);
        }
        assert_eq!(e.text(), "[Anthropic](https://example.com)");
    }

    /// A fence counts as code the moment it is typed, before anything closes
    /// it, which is when the rules that ask need to know.
    #[test]
    fn an_unclosed_fence_is_a_code_block() {
        let mut e = Editor::new();
        for c in "```rust".chars() {
            e.handle_key(Key::Char(c), Mods::NONE);
        }
        assert!(e.in_code(), "the caret is on `{}`", e.text());
    }

    #[test]
    fn a_fence_closes_itself() {
        assert_eq!(typed("```rust\n").text(), "```rust\n\n```");
    }

    /// Inside a fence a newline is one newline. Enter ends a block everywhere
    /// else, and a blank line dropped between two statements would be the
    /// editor rewriting the code.
    #[test]
    fn lines_of_code_do_not_get_spaced_out() {
        let mut e = typed("```rust\n");
        for line in ["let x = 1;", "let y = 2;"] {
            for c in line.chars() {
                e.handle_key(Key::Char(c), Mods::NONE);
            }
            e.handle_key(Key::Enter, Mods::NONE);
        }
        assert_eq!(e.text(), "```rust\nlet x = 1;\nlet y = 2;\n\n```");
    }

    /// The fence that closes a block does not open another one.
    #[test]
    fn a_closing_fence_is_not_an_opening_one() {
        let mut e = typed("```rust\n");
        e.handle_key(Key::End, Mods::CTRL);
        e.handle_key(Key::Enter, Mods::NONE);
        assert_eq!(e.text(), "```rust\n\n```\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names_round_trip() {
        for theme in [Theme::Light, Theme::Dark, Theme::Auto] {
            assert_eq!(Theme::from_str(theme.as_str()), theme);
        }
    }

    #[test]
    fn theme_cycles_auto_light_dark() {
        let mut t = Theme::Auto;
        t = t.cycled();
        assert_eq!(t, Theme::Light);
        t = t.cycled();
        assert_eq!(t, Theme::Dark);
        t = t.cycled();
        assert_eq!(t, Theme::Auto, "cycling must return to where it started");
    }

    #[test]
    fn auto_follows_the_system_and_the_others_do_not() {
        // (theme, system is dark) -> draws dark
        let cases = [
            (Theme::Light, true, false),
            (Theme::Light, false, false),
            (Theme::Dark, true, true),
            (Theme::Dark, false, true),
            (Theme::Auto, true, true),
            (Theme::Auto, false, false),
        ];
        for (theme, system_dark, want) in cases {
            assert_eq!(
                theme.is_dark(system_dark),
                want,
                "{theme:?} with system_dark={system_dark}"
            );
        }
    }

    #[test]
    fn ctrl_t_cycles_the_theme_without_touching_the_document() {
        let mut e = Editor::with_text("# Notes");
        assert_eq!(e.theme, Theme::Auto);

        let result = e.handle_key(Key::Char('t'), Mods::CTRL);
        assert!(result.handled);
        assert!(!result.changed, "changing theme is not a document edit");
        assert_eq!(e.theme, Theme::Light);
        assert_eq!(e.text(), "# Notes");

        e.handle_key(Key::Char('t'), Mods::CTRL);
        assert_eq!(e.theme, Theme::Dark);
        e.handle_key(Key::Char('t'), Mods::CTRL);
        assert_eq!(e.theme, Theme::Auto);
    }

    #[test]
    fn a_plain_t_is_typed_rather_than_cycling_the_theme() {
        let mut e = Editor::new();
        e.handle_key(Key::Char('t'), Mods::NONE);
        assert_eq!(e.text(), "t");
        assert_eq!(e.theme, Theme::Auto);
    }

    #[test]
    fn the_theme_is_not_undoable() {
        // Undo restores document snapshots; a view preference is not part of
        // the document and must survive an undo.
        let mut e = Editor::new();
        for c in "hello".chars() {
            e.handle_key(Key::Char(c), Mods::NONE);
        }
        e.handle_key(Key::Char('t'), Mods::CTRL);
        assert_eq!(e.theme, Theme::Light);

        assert!(e.undo());
        assert_eq!(e.text(), "");
        assert_eq!(e.theme, Theme::Light, "undo must not revert the theme");
    }
}

#[cfg(test)]
mod section_tests {
    use super::*;

    #[test]
    fn a_new_editor_has_one_empty_section() {
        let e = Editor::new();
        assert_eq!(e.section_count(), 1);
        assert_eq!(e.active_section(), 0);
        assert_eq!(e.section_title(0), "untitled");
    }

    #[test]
    fn adding_a_section_switches_to_it_and_leaves_the_other_alone() {
        let mut e = Editor::with_text("# First\n\nbody");
        let index = e.new_section();
        assert_eq!(index, 1);
        assert_eq!(e.active_section(), 1);
        assert_eq!(e.text(), "");
        assert_eq!(e.section_text(0), "# First\n\nbody");
    }

    #[test]
    fn each_section_keeps_its_own_caret_and_text() {
        let mut e = Editor::with_text("# One");
        e.set_caret(2);
        e.new_section();
        for c in "# Two".chars() {
            e.handle_key(Key::Char(c), Mods::NONE);
        }
        assert_eq!(e.text(), "# Two");

        e.set_active_section(0);
        assert_eq!(e.text(), "# One");
        assert_eq!(e.caret(), 2);

        e.set_active_section(1);
        assert_eq!(e.text(), "# Two");
    }

    #[test]
    fn undo_history_does_not_leak_between_sections() {
        let mut e = Editor::with_text("first");
        e.set_caret(5);
        e.handle_key(Key::Char('!'), Mods::NONE);
        e.new_section();
        // The fresh section has nothing to undo, even though section 0 does.
        assert!(!e.undo());
        e.set_active_section(0);
        assert!(e.undo());
        assert_eq!(e.text(), "first");
    }

    #[test]
    fn a_section_is_titled_by_its_leading_heading() {
        let mut e = Editor::with_text("# Drum bus\n\nbody");
        assert_eq!(e.section_title(0), "Drum bus");
        e.new_section();
        assert_eq!(e.section_title(1), "untitled");
    }

    #[test]
    fn a_long_title_is_shortened_and_flagged_for_a_tooltip() {
        let e = Editor::with_text("# A heading far too long for the bar");
        assert_eq!(e.section_title(0), "A heading far too long for the bar");
        assert!(e.section_title_is_truncated(0));
        assert_eq!(e.section_display_title(0).chars().count(), sections::MAX_TITLE_CHARS);
        assert!(e.section_display_title(0).ends_with("..."));
    }

    #[test]
    fn closing_a_section_keeps_the_neighbour_active() {
        let mut e = Editor::with_text("# One");
        e.new_section();
        e.set_text("# Two");
        e.new_section();
        e.set_text("# Three");
        assert_eq!(e.section_count(), 3);

        e.set_active_section(1);
        e.close_section(1);
        assert_eq!(e.section_count(), 2);
        assert_eq!(e.section_text(0), "# One");
        assert_eq!(e.section_text(1), "# Three");
        assert_eq!(e.active_section(), 1);
    }

    #[test]
    fn closing_the_last_section_empties_it_instead_of_removing_it() {
        let mut e = Editor::with_text("# Only");
        e.close_section(0);
        assert_eq!(e.section_count(), 1);
        assert_eq!(e.text(), "");
    }

    #[test]
    fn dragging_a_section_reorders_the_document() {
        let mut e = Editor::with_text("# One\n\nfirst");
        e.new_section();
        e.set_text("# Two\n\nsecond");
        e.new_section();
        e.set_text("# Three\n\nthird");

        e.move_section(2, 0);
        assert_eq!(e.section_title(0), "Three");
        assert_eq!(e.section_title(1), "One");
        assert_eq!(e.section_title(2), "Two");
        // The dragged section is the one still being edited.
        assert_eq!(e.active_section(), 0);
        assert_eq!(e.text(), "# Three\n\nthird");
    }

    #[test]
    fn a_drag_that_does_not_move_the_active_section_keeps_it_active() {
        let mut e = Editor::with_text("# One");
        e.new_section();
        e.set_text("# Two");
        e.new_section();
        e.set_text("# Three");

        e.set_active_section(0);
        e.move_section(1, 2);
        assert_eq!(e.active_section(), 0);
        assert_eq!(e.text(), "# One");
        assert_eq!(e.section_title(1), "Three");
        assert_eq!(e.section_title(2), "Two");
    }

    #[test]
    fn the_document_is_every_section_in_order() {
        let mut e = Editor::with_text("# One\n\nfirst");
        e.new_section();
        e.set_text("# Two\n\nsecond");
        assert_eq!(e.document_text(), "# One\n\nfirst\n\n# Two\n\nsecond");
    }

    #[test]
    fn loading_a_document_makes_one_section_per_heading() {
        let mut e = Editor::new();
        e.set_document_text("# One\n\nfirst\n\n# Two\n\nsecond\n\n# Three\n");
        assert_eq!(e.section_count(), 3);
        assert_eq!(e.section_text(0), "# One\n\nfirst");
        assert_eq!(e.section_text(1), "# Two\n\nsecond");
        assert_eq!(e.section_title(0), "One");
        assert_eq!(e.section_title(1), "Two");
        assert_eq!(e.section_title(2), "Three");
        assert_eq!(e.active_section(), 0);
    }
}

#[cfg(test)]
mod caret_tests {
    use super::*;

    /// Somewhere to type is what a document opens with, in every section, so
    /// whichever one is picked up carries on where it was left.
    #[test]
    fn every_section_opens_with_its_caret_at_the_end() {
        let mut e = Editor::new();
        e.set_document_text("# One\n\nfirst\n\n# Two\n\nsecond");
        for section in 0..e.section_count() {
            e.set_active_section(section);
            assert!(e.has_caret(), "section {section} opened without a caret");
            assert_eq!(
                e.caret(),
                e.text().len(),
                "section {section} opened with its caret somewhere other than the end"
            );
        }
    }

    #[test]
    fn a_new_editor_has_a_caret() {
        assert!(Editor::new().has_caret());
    }

    /// Clicking where there is no line to put it on takes the caret away.
    #[test]
    fn the_caret_can_be_taken_away() {
        let mut e = Editor::with_text("a line");
        e.clear_caret();
        assert!(!e.has_caret());
    }

    /// With no caret there is nowhere for a character to go, so nothing
    /// happens: not the character, and not a caret appearing to hold it.
    #[test]
    fn a_key_does_nothing_while_there_is_no_caret() {
        let mut e = Editor::with_text("a line");
        e.clear_caret();
        for key in [Key::Char('x'), Key::Enter, Key::Backspace, Key::Right] {
            let result = e.handle_key(key, Mods::NONE);
            assert!(!result.changed, "{key:?} edited the document with no caret");
        }
        assert_eq!(e.text(), "a line");
        assert!(!e.has_caret(), "a key put the caret back");
        assert!(!e.is_dirty());
    }

    /// Clicking on a line puts it back, which is what `set_caret` is.
    #[test]
    fn placing_the_caret_brings_it_back() {
        let mut e = Editor::with_text("a line");
        e.clear_caret();
        e.set_caret(2);
        assert!(e.has_caret());
        assert_eq!(e.caret(), 2);
        e.handle_key(Key::Char('!'), Mods::NONE);
        assert_eq!(e.text(), "a !line");
    }

    /// The caret belongs to the section it was taken away in.
    #[test]
    fn taking_the_caret_away_leaves_the_other_sections_alone() {
        let mut e = Editor::new();
        e.set_document_text("# One\n\nfirst\n\n# Two\n\nsecond");
        e.clear_caret();
        assert!(!e.has_caret());
        e.set_active_section(1);
        assert!(e.has_caret(), "the section switched to came up without a caret");
        e.set_active_section(0);
        assert!(!e.has_caret(), "the caret came back on its own");
    }

    /// A selection is a caret with something held: taking the caret away takes
    /// the selection with it.
    #[test]
    fn taking_the_caret_away_drops_the_selection() {
        let mut e = Editor::with_text("a line");
        e.select(0..3);
        e.clear_caret();
        assert!(!e.has_selection());
        assert!(e.selected_text().is_empty());
    }
}
