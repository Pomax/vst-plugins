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
use crate::tabs;
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

/// What a notepad is called until the user renames it.
pub const DEFAULT_TITLE: &str = "...project title here...";
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
/// [`kode_markdown::MarkdownEditor`]'s: buffer, cursor motion by character,
/// word and line, selection, undo and the markdown input rules all come from
/// there rather than being written here. Each tab is one of them, and the one
/// in front is `tabs[active]`. The whole document is in the vec, with no live
/// copy beside it.
pub struct Editor {
    tabs: Vec<kode_markdown::MarkdownEditor>,
    active: usize,
    pub mode: ViewMode,
    pub theme: Theme,
    pub colours: crate::colours::ColourScheme,
    pub width: i32,
    pub height: i32,
    pub file: Option<PathBuf>,
    /// The notepad's own name, edited in the toolbar. It is not part of the
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
            tabs: vec![kode_markdown::MarkdownEditor::empty()],
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
        e.tabs[0] = kode_markdown::MarkdownEditor::new(&text);
        e.tabs[0].move_to_end();
        e
    }

    // ---- tabs ------------------------------------------------------------

    /// The tab in front.
    fn live(&self) -> &kode_markdown::MarkdownEditor {
        &self.tabs[self.active]
    }

    fn live_mut(&mut self) -> &mut kode_markdown::MarkdownEditor {
        &mut self.tabs[self.active]
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_tab(&self) -> usize {
        self.active
    }

    /// The markdown in tab `index`.
    pub fn tab_text(&self, index: usize) -> String {
        self.tabs.get(index).map(|t| t.text()).unwrap_or_default()
    }

    /// The tab's heading, or `untitled`.
    pub fn tab_title(&self, index: usize) -> String {
        tabs::title_of(&self.tab_text(index))
    }

    /// The title as it fits on the tab, cut short with `...` when too long.
    pub fn tab_display_title(&self, index: usize) -> String {
        tabs::display_title(&self.tab_title(index))
    }

    /// Whether the tab shows a shortened title and so needs a tooltip.
    pub fn tab_title_is_truncated(&self, index: usize) -> bool {
        tabs::is_truncated(&self.tab_title(index))
    }

    pub fn set_active_tab(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active {
            return;
        }
        self.active = index;
    }

    /// Add an empty tab after the last one and switch to it.
    pub fn new_tab(&mut self) -> usize {
        self.tabs.push(kode_markdown::MarkdownEditor::empty());
        self.active = self.tabs.len() - 1;
        self.dirty = true;
        self.active
    }

    /// Close tab `index`. The last remaining tab is emptied rather than
    /// removed, because there is always a buffer to type into.
    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.dirty = true;
        if self.tabs.len() == 1 {
            self.tabs[0] = kode_markdown::MarkdownEditor::empty();
            return;
        }
        self.tabs.remove(index);
        self.active = if self.active > index {
            self.active - 1
        } else {
            self.active.min(self.tabs.len() - 1)
        };
    }

    /// Move a tab to another position, as a drag does.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        let last = self.tabs.len().saturating_sub(1);
        if from > last || from == to {
            return;
        }
        let to = to.min(last);
        let buffer = self.tabs.remove(from);
        self.tabs.insert(to, buffer);

        // The active tab keeps its contents, not its position.
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

    /// Every tab joined back into the one document they are a view of.
    ///
    /// A single tab is the document, byte for byte: nothing is normalised
    /// away, so opening a file and saving it back does not rewrite it.
    pub fn document_text(&self) -> String {
        if self.tabs.len() == 1 {
            return self.tabs[0].text();
        }
        let parts: Vec<String> = (0..self.tabs.len()).map(|i| self.tab_text(i)).collect();
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        tabs::join_document(&borrowed)
    }

    /// Replace every tab by splitting `text` on its top-level headings.
    pub fn set_document_text(&mut self, text: &str) {
        self.tabs = tabs::split_document(text)
            .into_iter()
            .map(|part| {
                let mut tab = kode_markdown::MarkdownEditor::new(&part);
                tab.move_to_end();
                tab
            })
            .collect();
        if self.tabs.is_empty() {
            self.tabs.push(kode_markdown::MarkdownEditor::empty());
        }
        self.active = 0;
    }

    // ---- accessors -------------------------------------------------------

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
        let tab = kode_markdown::MarkdownEditor::new(&new);
        self.tabs[self.active] = tab;
        self.set_caret(caret);
    }

    pub fn set_caret(&mut self, pos: usize) {
        let pos = self.pos_of(pos);
        self.live_mut().set_cursor(pos);
    }

    pub fn select(&mut self, range: Range<usize>) {
        let (start, end) = (self.pos_of(range.start), self.pos_of(range.end));
        self.live_mut().set_selection(start, end);
    }

    pub fn select_all(&mut self) {
        self.live_mut().select_all();
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

    // ---- undo ------------------------------------------------------------

    /// Drop undo history, after loading a document, so the user cannot
    /// undo their way back into the previous file's contents.
    ///
    /// Loading replaces the tab wholesale, and a fresh one has no history.
    pub fn clear_history(&mut self) {
        let text = self.text();
        let at = self.live().cursor();
        self.tabs[self.active] = kode_markdown::MarkdownEditor::new(&text);
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

    // ---- primitive edits -------------------------------------------------

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

    /// Insert text at the caret, replacing any selection.
    pub fn insert_str(&mut self, s: &str) {
        self.live_mut().insert(s);
        self.dirty = true;
    }

    // ---- key handling ----------------------------------------------------

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
                } else {
                    let inner = self.tabs[self.active].editor_mut();
                    if kode_markdown::InputRules::handle_enter(inner) {
                        self.live_mut().sync_tree();
                    } else {
                        self.end_block();
                    }
                }
            }
            Key::Backspace => {
                let inner = self.tabs[self.active].editor_mut();
                if kode_markdown::InputRules::handle_backspace_at_prefix(inner) {
                    self.live_mut().sync_tree();
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
                let inner = self.tabs[self.active].editor_mut();
                let handled = if mods.shift {
                    kode_markdown::InputRules::handle_shift_tab(inner)
                } else {
                    kode_markdown::InputRules::handle_tab(inner)
                };
                if handled {
                    self.live_mut().sync_tree();
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
        let tab = &mut self.tabs[self.active];
        match (key, mods.shift, mods.ctrl) {
            (Key::Left, false, false) => tab.move_left(),
            (Key::Left, true, false) => tab.extend_selection_left(),
            (Key::Left, false, true) => tab.move_word_left(),
            (Key::Left, true, true) => tab.extend_selection_word_left(),
            (Key::Right, false, false) => tab.move_right(),
            (Key::Right, true, false) => tab.extend_selection_right(),
            (Key::Right, false, true) => tab.move_word_right(),
            (Key::Right, true, true) => tab.extend_selection_word_right(),
            (Key::Up, false, _) => tab.move_up(),
            (Key::Up, true, _) => tab.extend_selection_up(),
            (Key::Down, false, _) => tab.move_down(),
            (Key::Down, true, _) => tab.extend_selection_down(),
            (Key::Home, false, false) => tab.move_to_line_start(),
            (Key::Home, true, false) => tab.extend_selection_to_line_start(),
            (Key::Home, false, true) => tab.move_to_start(),
            (Key::Home, true, true) => tab.extend_selection_to_start(),
            (Key::End, false, false) => tab.move_to_line_end(),
            (Key::End, true, false) => tab.extend_selection_to_line_end(),
            (Key::End, false, true) => tab.move_to_end(),
            (Key::End, true, true) => tab.extend_selection_to_end(),
            (Key::PageUp, _, _) => tab.page_up(page),
            (Key::PageDown, _, _) => tab.page_down(page),
            _ => {}
        }
    }

    fn handle_ctrl(&mut self, key: Key, mods: Mods) -> Option<KeyResult> {
        let c = match key {
            Key::Char(c) => c.to_ascii_lowercase(),
            _ => return None,
        };
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
                let inner = self.tabs[self.active].editor_mut();
                kode_markdown::MarkdownCommands::insert_link(inner, "");
                self.live_mut().sync_tree();
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
    /// This is [`block`]'s parse and not the tab's tree-sitter tree, because
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

    /// Run one of the markdown commands over the tab in front.
    fn markdown_command(&mut self, run: fn(&mut kode_core::Editor)) {
        run(self.tabs[self.active].editor_mut());
        self.live_mut().sync_tree();
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
    /// it. The tab's tree-sitter tree cannot say so — an unclosed fence parses
    /// as an error node — which is why these rules use our own parse.
    #[test]
    fn an_unclosed_fence_is_a_code_block() {
        let mut e = Editor::new();
        for c in "```rust".chars() {
            e.handle_key(Key::Char(c), Mods::NONE);
        }
        assert!(e.in_code(), "the caret is on `{}`", e.text());
        assert_eq!(
            e.live().tree().sexp().as_deref(),
            Some("(document (ERROR (fenced_code_block_delimiter)))"),
            "tree-sitter now parses an unclosed fence: these rules can use it"
        );
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
mod tab_tests {
    use super::*;

    #[test]
    fn a_new_editor_has_one_empty_tab() {
        let e = Editor::new();
        assert_eq!(e.tab_count(), 1);
        assert_eq!(e.active_tab(), 0);
        assert_eq!(e.tab_title(0), "untitled");
    }

    #[test]
    fn adding_a_tab_switches_to_it_and_leaves_the_other_alone() {
        let mut e = Editor::with_text("# First\n\nbody");
        let index = e.new_tab();
        assert_eq!(index, 1);
        assert_eq!(e.active_tab(), 1);
        assert_eq!(e.text(), "");
        assert_eq!(e.tab_text(0), "# First\n\nbody");
    }

    #[test]
    fn each_tab_keeps_its_own_caret_and_text() {
        let mut e = Editor::with_text("# One");
        e.set_caret(2);
        e.new_tab();
        for c in "# Two".chars() {
            e.handle_key(Key::Char(c), Mods::NONE);
        }
        assert_eq!(e.text(), "# Two");

        e.set_active_tab(0);
        assert_eq!(e.text(), "# One");
        assert_eq!(e.caret(), 2);

        e.set_active_tab(1);
        assert_eq!(e.text(), "# Two");
    }

    #[test]
    fn undo_history_does_not_leak_between_tabs() {
        let mut e = Editor::with_text("first");
        e.set_caret(5);
        e.handle_key(Key::Char('!'), Mods::NONE);
        e.new_tab();
        // The fresh tab has nothing to undo, even though tab 0 does.
        assert!(!e.undo());
        e.set_active_tab(0);
        assert!(e.undo());
        assert_eq!(e.text(), "first");
    }

    #[test]
    fn a_tab_is_titled_by_its_leading_heading() {
        let mut e = Editor::with_text("# Drum bus\n\nbody");
        assert_eq!(e.tab_title(0), "Drum bus");
        e.new_tab();
        assert_eq!(e.tab_title(1), "untitled");
    }

    #[test]
    fn a_long_title_is_shortened_and_flagged_for_a_tooltip() {
        let e = Editor::with_text("# A heading far too long for the bar");
        assert_eq!(e.tab_title(0), "A heading far too long for the bar");
        assert!(e.tab_title_is_truncated(0));
        assert_eq!(e.tab_display_title(0).chars().count(), tabs::MAX_TITLE_CHARS);
        assert!(e.tab_display_title(0).ends_with("..."));
    }

    #[test]
    fn closing_a_tab_keeps_the_neighbour_active() {
        let mut e = Editor::with_text("# One");
        e.new_tab();
        e.set_text("# Two");
        e.new_tab();
        e.set_text("# Three");
        assert_eq!(e.tab_count(), 3);

        e.set_active_tab(1);
        e.close_tab(1);
        assert_eq!(e.tab_count(), 2);
        assert_eq!(e.tab_text(0), "# One");
        assert_eq!(e.tab_text(1), "# Three");
        assert_eq!(e.active_tab(), 1);
    }

    #[test]
    fn closing_the_last_tab_empties_it_instead_of_removing_it() {
        let mut e = Editor::with_text("# Only");
        e.close_tab(0);
        assert_eq!(e.tab_count(), 1);
        assert_eq!(e.text(), "");
    }

    #[test]
    fn dragging_a_tab_reorders_the_document() {
        let mut e = Editor::with_text("# One\n\nfirst");
        e.new_tab();
        e.set_text("# Two\n\nsecond");
        e.new_tab();
        e.set_text("# Three\n\nthird");

        e.move_tab(2, 0);
        assert_eq!(e.tab_title(0), "Three");
        assert_eq!(e.tab_title(1), "One");
        assert_eq!(e.tab_title(2), "Two");
        // The dragged tab is the one still being edited.
        assert_eq!(e.active_tab(), 0);
        assert_eq!(e.text(), "# Three\n\nthird");
    }

    #[test]
    fn a_drag_that_does_not_move_the_active_tab_keeps_it_active() {
        let mut e = Editor::with_text("# One");
        e.new_tab();
        e.set_text("# Two");
        e.new_tab();
        e.set_text("# Three");

        e.set_active_tab(0);
        e.move_tab(1, 2);
        assert_eq!(e.active_tab(), 0);
        assert_eq!(e.text(), "# One");
        assert_eq!(e.tab_title(1), "Three");
        assert_eq!(e.tab_title(2), "Two");
    }

    #[test]
    fn the_document_is_every_tab_in_order() {
        let mut e = Editor::with_text("# One\n\nfirst");
        e.new_tab();
        e.set_text("# Two\n\nsecond");
        assert_eq!(e.document_text(), "# One\n\nfirst\n\n# Two\n\nsecond");
    }

    #[test]
    fn loading_a_document_makes_one_tab_per_heading() {
        let mut e = Editor::new();
        e.set_document_text("# One\n\nfirst\n\n# Two\n\nsecond\n\n# Three\n");
        assert_eq!(e.tab_count(), 3);
        assert_eq!(e.tab_title(0), "One");
        assert_eq!(e.tab_title(1), "Two");
        assert_eq!(e.tab_title(2), "Three");
        assert_eq!(e.active_tab(), 0);
    }

    #[test]
    fn tabs_survive_a_save_and_load_of_the_document() {
        let mut e = Editor::with_text("# One\n\nfirst");
        e.new_tab();
        e.set_text("# Two\n\nsecond");

        let document = e.document_text();
        let mut reopened = Editor::new();
        reopened.set_document_text(&document);

        assert_eq!(reopened.tab_count(), 2);
        assert_eq!(reopened.tab_text(0), "# One\n\nfirst");
        assert_eq!(reopened.tab_text(1), "# Two\n\nsecond");
    }
}
