//! Where the keyboard is when the plugin starts, one test per thing that was
//! asked for, each walking the steps and then reading what the window and the
//! editor say: who has the keyboard, what is selected, where the caret is and
//! which section is in front. Typing comes after that, to show the selection
//! does what it is for.
//!
//! The editor is put in the state the plugin puts it in: made new, the way a
//! plugin that has just been added is, or restored from the bytes a host hands
//! back, the way a project or a preset is.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use markdown_notes_core::{Editor, PluginState, Theme, DEFAULT_TITLE};
use markdown_notes_plugin::gui::{Keyboard, KeyboardReport};

const NAMED: &str = "funky cake";
const UNNAMED_SECTION: &str = "# Section Title";
const NAMED_SECTION: &str = "# Drums\n\nkick on one";
const TWO_SECTIONS: &str = "# One\n\nfirst\n\n# Two\n\nsecond";

struct Window {
    harness: Harness<'static>,
    editor: Arc<Mutex<Editor>>,
    keyboard: KeyboardReport,
}

/// The bytes a host keeps for a note with this name and this document.
fn saved(title: &str, notes: &str) -> Vec<u8> {
    PluginState {
        title: title.to_string(),
        notes: notes.to_string(),
        theme: Theme::Light.as_str().to_string(),
        ..PluginState::default()
    }
    .to_bytes()
}

/// A plugin that has just been added to a track: nothing restored.
fn added() -> Editor {
    let mut editor = Editor::new();
    editor.theme = Theme::Light;
    editor
}

/// A plugin a host has restored from `state`.
fn restored(state: &[u8]) -> Editor {
    let mut editor = Editor::new();
    editor.load_state_bytes(state);
    editor
}

/// Open the editor's window on `editor`, with or without the keyboard focus a
/// window inside a host may not have yet.
fn open_window(editor: Editor, focused: bool) -> Window {
    let editor = Arc::new(Mutex::new(editor));
    let mut state = markdown_notes_plugin::gui::TestGui::opening(Arc::clone(&editor), false);
    let keyboard = state.keyboard();

    let harness = Harness::builder()
        .with_size(egui::vec2(900.0, 400.0))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));
    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);

    let mut window = Window { harness, editor, keyboard };
    window.focus(focused);
    window
}

fn open(editor: Editor) -> Window {
    open_window(editor, true)
}

impl Window {
    fn focus(&mut self, focused: bool) {
        self.harness.input_mut().focused = focused;
        self.harness.input_mut().events.push(egui::Event::WindowFocused(focused));
        self.harness.run_steps(4);
    }

    fn type_text(&mut self, text: &str) {
        self.harness.input_mut().events.push(egui::Event::Text(text.to_string()));
        self.harness.run_steps(3);
    }

    fn press(&mut self, key: egui::Key) {
        self.harness.input_mut().events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        self.harness.run_steps(3);
    }

    fn press_enter(&mut self) {
        self.press(egui::Key::Enter);
    }

    fn keyboard(&self) -> Keyboard {
        self.keyboard.lock().unwrap().clone()
    }

    fn title(&self) -> String {
        self.editor.lock().unwrap().title.clone()
    }

    fn document(&self) -> String {
        self.editor.lock().unwrap().document_text()
    }

    /// What is selected in the document, the caret, the text of the section
    /// in front, and which section that is.
    fn in_the_document(&self) -> (String, usize, String, usize) {
        let e = self.editor.lock().unwrap();
        (e.selected_text(), e.caret(), e.text(), e.active_section())
    }
}

/// "when the plugin starts, we should check whether the title is the default
/// placeholder. If so, we should pre-select the entire title"
#[test]
fn a_plugin_that_starts_with_the_placeholder_title_has_the_whole_title_selected() {
    let window = open(added());

    assert_eq!(window.title(), DEFAULT_TITLE);
    assert_eq!(
        window.keyboard(),
        Keyboard {
            name_has_it: true,
            name_selected: DEFAULT_TITLE.to_string(),
            document_has_it: false,
        }
    );
}

/// There is one cursor. While the title is selected the text area shows no
/// caret and no selection, on any frame, and the same holds while the title is
/// being typed into.
#[test]
fn the_text_area_has_no_caret_or_selection_while_the_title_has_the_keyboard() {
    let mut window = open(added());

    for frame in 0..6 {
        window.harness.run_steps(1);
        let e = window.editor.lock().unwrap();
        assert!(!e.has_caret(), "frame {frame}: the text area has a caret");
        assert_eq!(e.selected_text(), "", "frame {frame}: the text area has a selection");
    }

    window.type_text("funky");
    {
        let e = window.editor.lock().unwrap();
        assert!(!e.has_caret(), "typing the title gave the text area a caret");
        assert_eq!(e.selected_text(), "", "typing the title selected text in the text area");
    }

    window.press_enter();
    let (selected, ..) = window.in_the_document();
    assert_eq!(selected, "Section Title");
    assert!(window.editor.lock().unwrap().has_caret());
}

/// Frame by frame: from the frame the title has the keyboard it is selected,
/// and it stays selected for as long as nothing is typed or clicked.
#[test]
fn the_placeholder_title_stays_selected_on_every_frame_after_it_gets_the_keyboard() {
    let mut window = open(added());

    let mut frames: Vec<Keyboard> = Vec::new();
    for _ in 0..6 {
        window.harness.run_steps(1);
        frames.push(window.keyboard());
    }

    for (frame, keyboard) in frames.iter().enumerate() {
        assert!(
            keyboard.name_has_it && keyboard.name_selected == DEFAULT_TITLE,
            "frame {frame} of {frames:#?}"
        );
    }
}

/// "so that when the user types anything, that immediately replaces the
/// titles"
#[test]
fn typing_into_a_plugin_that_has_just_started_replaces_the_placeholder_title() {
    let mut window = open(added());

    window.type_text(NAMED);

    assert_eq!(window.title(), NAMED);
    assert_eq!(window.document(), UNNAMED_SECTION, "what was typed reached the document");
}

/// The same, in a window that has no keyboard focus when it opens and is given
/// it afterwards, which is how a plugin's window arrives in a host.
#[test]
fn the_placeholder_title_is_selected_once_the_window_gets_the_keyboard() {
    let mut window = open_window(added(), false);
    window.focus(true);

    assert_eq!(
        window.keyboard(),
        Keyboard {
            name_has_it: true,
            name_selected: DEFAULT_TITLE.to_string(),
            document_has_it: false,
        }
    );
    window.type_text(NAMED);
    assert_eq!(window.title(), NAMED);
}

/// "Then when they press enter, the cursor should move to the text area."
#[test]
fn enter_in_the_title_moves_the_cursor_to_the_text_area() {
    let mut window = open(added());
    window.type_text(NAMED);

    window.press_enter();

    let keyboard = window.keyboard();
    assert!(!keyboard.name_has_it, "the title still has the keyboard: {keyboard:?}");
    assert!(keyboard.document_has_it, "the text area does not have the keyboard: {keyboard:?}");
    assert!(window.editor.lock().unwrap().has_caret(), "the text area has no caret");
    assert_eq!(window.title(), NAMED, "Enter changed the title");
    assert_eq!(window.document(), UNNAMED_SECTION, "Enter was typed into the document");
}

/// Tab in the title does the same as Enter: the keyboard goes to the text
/// area, the placeholder section title is selected, and what is typed next
/// replaces it. No tab character reaches the title or the document.
#[test]
fn tab_in_the_title_does_the_same_as_enter() {
    let mut window = open(added());
    window.type_text(NAMED);

    window.press(egui::Key::Tab);

    let keyboard = window.keyboard();
    assert!(!keyboard.name_has_it, "the title still has the keyboard: {keyboard:?}");
    assert!(keyboard.document_has_it, "the text area does not have the keyboard: {keyboard:?}");
    assert_eq!(window.title(), NAMED, "Tab changed the title");
    let (selected, _, text, _) = window.in_the_document();
    assert_eq!(selected, "Section Title");
    assert_eq!(text, UNNAMED_SECTION, "Tab was typed into the document");

    window.type_text("Drums");
    assert_eq!(window.document(), "# Drums");
    assert_eq!(window.title(), NAMED);
}

/// "If that text area has a top level placeholder section title, it should
/// select the entire title (but not the `# ` prefix) so that the user can
/// immediately change that, too."
#[test]
fn enter_in_the_title_selects_the_placeholder_section_title_without_its_prefix() {
    let mut window = open(added());
    window.type_text(NAMED);
    window.press_enter();

    let (selected, _, text, _) = window.in_the_document();
    assert_eq!(selected, "Section Title");
    assert_eq!(text, UNNAMED_SECTION, "selecting it changed it");

    window.type_text("Drums");
    assert_eq!(window.document(), "# Drums");
    assert_eq!(window.title(), NAMED);
}

/// "If there is already a project title set, the cursor should immediately be
/// placed on the textarea, with the same "if there is no real section header
/// yet" behaviour."
#[test]
fn a_plugin_that_starts_with_a_title_set_starts_in_the_text_area_on_the_placeholder_section_title() {
    let mut window = open(restored(&saved(NAMED, UNNAMED_SECTION)));

    let keyboard = window.keyboard();
    assert!(!keyboard.name_has_it, "the title has the keyboard: {keyboard:?}");
    assert!(keyboard.document_has_it, "the text area does not have the keyboard: {keyboard:?}");
    let (selected, _, text, _) = window.in_the_document();
    assert_eq!(selected, "Section Title");
    assert_eq!(text, UNNAMED_SECTION);

    window.type_text("Drums");
    assert_eq!(window.document(), "# Drums");
    assert_eq!(window.title(), NAMED, "what was typed reached the title");
}

/// "If there *IS* a real section heading already, the cursor should be placed
/// at the end of the textarea's document."
#[test]
fn a_plugin_that_starts_with_a_title_and_a_real_section_heading_has_the_cursor_at_the_end() {
    let mut window = open(restored(&saved(NAMED, NAMED_SECTION)));

    let keyboard = window.keyboard();
    assert!(keyboard.document_has_it && !keyboard.name_has_it, "{keyboard:?}");
    let (selected, caret, text, _) = window.in_the_document();
    assert_eq!(selected, "", "something is selected");
    assert_eq!(text, NAMED_SECTION);
    assert_eq!(caret, NAMED_SECTION.len(), "the cursor is not at the end");

    window.type_text("!");
    assert_eq!(window.document(), format!("{NAMED_SECTION}!"));
}

/// The same rule after Enter in the title: a placeholder title over a document
/// that already has a real section heading.
#[test]
fn enter_in_the_title_puts_the_cursor_at_the_end_when_the_section_has_a_real_heading() {
    let mut window = open(restored(&saved(DEFAULT_TITLE, NAMED_SECTION)));
    assert!(window.keyboard().name_has_it, "the placeholder title did not get the keyboard");

    window.type_text(NAMED);
    window.press_enter();

    let (selected, caret, text, _) = window.in_the_document();
    assert_eq!(selected, "");
    assert_eq!(text, NAMED_SECTION);
    assert_eq!(caret, NAMED_SECTION.len());
}

/// "If there are multiple tabs, on open the first tab should be selected for
/// this, not the last."
#[test]
fn a_plugin_that_starts_with_several_tabs_starts_on_the_first() {
    let mut window = open(restored(&saved(NAMED, TWO_SECTIONS)));

    let (selected, caret, text, section) = window.in_the_document();
    assert_eq!(section, 0, "the tab in front is not the first");
    assert_eq!(text.trim_end(), "# One\n\nfirst");
    assert_eq!(selected, "");
    assert_eq!(caret, text.len(), "the cursor is not at the end of the first tab");

    window.type_text("!");
    let e = window.editor.lock().unwrap();
    assert!(e.section_text(0).contains("first!"), "{:?}", e.section_text(0));
    assert!(!e.section_text(1).contains('!'), "it was typed into the last tab");
}

/// Opening also happens to a window that is already up: a preset or a project
/// replaces the document under it. The last tab is in front when it does.
#[test]
fn a_document_opened_under_a_window_on_its_last_tab_opens_on_the_first() {
    let mut window = open(restored(&saved(NAMED, TWO_SECTIONS)));
    window.editor.lock().unwrap().set_active_section(1);
    window.harness.run_steps(2);

    window.editor.lock().unwrap().load_state_bytes(&saved(NAMED, TWO_SECTIONS));
    window.harness.run_steps(3);

    let (_, caret, text, section) = window.in_the_document();
    assert_eq!(section, 0, "the tab in front is not the first");
    assert_eq!(caret, text.len());
}

/// A document with a title, opened under a window whose title still had the
/// keyboard: the keyboard goes to the text area, as it does at the start.
#[test]
fn a_titled_document_opened_under_a_window_takes_the_keyboard_from_the_title() {
    let mut window = open(added());
    assert!(window.keyboard().name_has_it);

    window.editor.lock().unwrap().load_state_bytes(&saved(NAMED, NAMED_SECTION));
    window.harness.run_steps(3);

    let keyboard = window.keyboard();
    assert!(keyboard.document_has_it && !keyboard.name_has_it, "{keyboard:?}");
    window.type_text("!");
    assert_eq!(window.title(), NAMED, "what was typed reached the title");
    assert_eq!(window.document(), format!("{NAMED_SECTION}!"));
}
