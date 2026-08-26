//! The editor window, drawn with egui inside the window the host gives us.
//!
//! # How markdown is drawn
//!
//! The document model hands back one [`Block`] per line, each carrying spans
//! tagged with a style and a visibility flag. Drawing is therefore a
//! translation job rather than a parsing one: build an egui `LayoutJob` from
//! the visible spans, give each one a `TextFormat` matching its style, and let
//! egui lay it out. Hidden markers simply are not appended — which is exactly
//! what makes the view WYSIWYG, and why toggling to raw mode (where every
//! marker is visible) needs no separate renderer.
//!
//! # A note on bold
//!
//! egui ships no bold font family, so bold is drawn the way egui draws its own
//! emphasis: a stronger foreground colour. Italic, strikethrough and underline
//! are real text formatting.

use std::sync::Arc;
use std::time::{Duration, Instant};

use baseview::dpi::{LogicalSize, Size};
use baseview::{WindowHandle, WindowScalePolicy};
use egui::text::LayoutJob;
use egui::{Color32, FontFamily, FontId, Sense, Stroke, TextFormat};
use egui_baseview::{EguiWindow, EguiWindowSettings, ExtraOutputCommands};
use markdown_notes_core::{
    Block, BlockKind, Command, Key, Mods, Span, SpanRole, Style, Theme, ViewMode,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::Shared;

/// A parent window supplied by the host, wrapped so baseview can adopt it.
pub struct ParentWindow(RawWindowHandle);

impl ParentWindow {
    /// Build from the pointer the host passes to `IPlugView::attached`.
    ///
    /// # Safety
    /// `ptr` must be a valid window handle of this platform's native type for
    /// as long as the child window lives.
    pub unsafe fn from_ptr(ptr: *mut std::ffi::c_void) -> Option<ParentWindow> {
        #[cfg(target_os = "windows")]
        {
            use raw_window_handle::Win32WindowHandle;
            let hwnd = std::num::NonZeroIsize::new(ptr as isize)?;
            Some(ParentWindow(RawWindowHandle::Win32(
                Win32WindowHandle::new(hwnd),
            )))
        }
        #[cfg(target_os = "macos")]
        {
            use raw_window_handle::AppKitWindowHandle;
            let view = std::ptr::NonNull::new(ptr)?;
            Some(ParentWindow(RawWindowHandle::AppKit(
                AppKitWindowHandle::new(view),
            )))
        }
        #[cfg(target_os = "linux")]
        {
            use raw_window_handle::XcbWindowHandle;
            let id = std::num::NonZeroU32::new(ptr as usize as u32)?;
            Some(ParentWindow(RawWindowHandle::Xcb(XcbWindowHandle::new(id))))
        }
    }
}

impl HasWindowHandle for ParentWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // Safety: the handle belongs to the host's window, which outlives ours.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(self.0) })
    }
}

/// How often to re-read the OS theme while set to Auto.
///
/// Reading it is a registry/desktop-portal call, far too expensive to do every
/// frame, but the user may flip their system theme while the plugin is open.
const SYSTEM_THEME_POLL: Duration = Duration::from_secs(2);

/// Breathing room around the toolbar row.
const TOOLBAR_MARGIN: egui::Margin = egui::Margin::symmetric(10, 6);

/// Height of the toolbar row, so its contents have somewhere to centre in.
const TOOLBAR_HEIGHT: f32 = 26.0;

/// Breathing room around the document. Text pressed against the window edge is
/// unpleasant to read and worse to click at, since the first character has no
/// margin to aim at.
const DOCUMENT_MARGIN: egui::Margin = egui::Margin::symmetric(18, 12);

/// How much larger the note's name is than the toolbar's buttons.
const TITLE_SIZE_BUMP: f32 = 5.0;

/// How far a code block reaches above and below the rows it holds.
const CODE_PADDING: f32 = 5.0;

/// Breathing room around the section strip.
const SECTION_MARGIN: egui::Margin = egui::Margin::symmetric(10, 4);

/// How wide a section's button is: whatever this string measures in the button
/// font. It is the same string [`markdown_notes_core::MAX_TITLE_CHARS`] counts,
/// so a title that fits the character limit fits the button.
const SECTION_WIDTH_SAMPLE: &str = "this many words";

/// State handed to the egui update closure.
struct Gui {
    editor: Shared,
    error: Option<String>,
    /// What a dialog running on its own thread had to say. Read each frame.
    report: crate::files::Report,
    /// Last known system setting, refreshed on a timer.
    system_dark: bool,
    checked_at: Instant,
    /// What is currently applied, so visuals are only rebuilt on a change.
    applied_dark: Option<bool>,
    applied_colours: Option<markdown_notes_core::Colours>,
    /// Window size as of last frame, used to tell "the window was resized"
    /// apart from "someone changed the stored size".
    last_seen_size: Option<(i32, i32)>,
    /// Scripts already loaded on demand.
    scripts: std::collections::HashSet<crate::fonts::Script>,
    /// The note's name while the field has focus. The editor is only
    /// updated when it changes, and the field is only refreshed from the
    /// editor while it is not being typed into.
    title: String,
    /// Section being dragged, and where each section was drawn last frame,
    /// which is what a drop position is worked out against.
    dragging: Option<usize>,
    section_rects: Vec<egui::Rect>,
    /// Where a drag through the document started, as a source offset. The
    /// selection runs from here to wherever the pointer is now.
    selecting_from: Option<usize>,
    settings_open: bool,
    /// Whether the document is what keystrokes go to. Set by clicking into it,
    /// cleared when a field takes the keyboard.
    document_focused: bool,
}

impl Gui {
    fn new(editor: Shared) -> Gui {
        Gui {
            editor,
            error: None,
            report: crate::files::Report::default(),
            system_dark: system_is_dark(),
            checked_at: Instant::now(),
            applied_dark: None,
            applied_colours: None,
            last_seen_size: None,
            scripts: std::collections::HashSet::new(),
            title: String::new(),
            dragging: None,
            section_rects: Vec::new(),
            selecting_from: None,
            settings_open: false,
            document_focused: true,
        }
    }

    /// Re-read the OS theme if the poll interval has elapsed.
    fn refresh_system_theme(&mut self) {
        if self.checked_at.elapsed() >= SYSTEM_THEME_POLL {
            self.system_dark = system_is_dark();
            self.checked_at = Instant::now();
        }
    }
}

/// What the operating system currently reports.
///
/// `Unspecified` — and any failure to ask — is treated as dark, matching the
/// editor's default appearance rather than flipping to a jarring white.
fn system_is_dark() -> bool {
    !matches!(dark_light::detect(), Ok(dark_light::Mode::Light))
}

fn settings(width: i32, height: i32) -> EguiWindowSettings {
    EguiWindowSettings::new()
        .with_tile("Markdown Notes")
        .with_size(Size::Logical(LogicalSize {
            width: width as f64,
            height: height as f64,
        }))
        .with_scale_policy(WindowScalePolicy::SystemScaleFactor)
}

/// Open the editor window as a child of the host's window.
pub fn open(parent: &ParentWindow, editor: Shared, width: i32, height: i32) -> WindowHandle {
    EguiWindow::open_parented(
        parent,
        settings(width, height),
        Gui::new(editor),
        // Fonts before the first frame: `set_fonts` binds them for the pass
        // after the one it is called in, and the first pass already draws text
        // in every family the editor uses.
        |ctx: &egui::Context, _cmds: &mut ExtraOutputCommands, _state: &mut Gui| {
            crate::fonts::install_base(ctx);
        },
        |_out: &egui::FullOutput, _vp: &egui::ViewportOutput, _state: &mut Gui| {},
        |ui: &mut egui::Ui, cmds: &mut ExtraOutputCommands, state: &mut Gui| draw(ui, state, cmds),
    )
}

/// Open the editor as a standalone window and block until it closes.
///
/// This runs the *same* drawing and input code the plugin uses, so it is a
/// faithful way to look at the editor without loading it into a DAW.
pub fn open_blocking(editor: Shared, width: i32, height: i32) {
    EguiWindow::open_blocking(
        settings(width, height),
        Gui::new(editor),
        // Fonts before the first frame: `set_fonts` binds them for the pass
        // after the one it is called in, and the first pass already draws text
        // in every family the editor uses.
        |ctx: &egui::Context, _cmds: &mut ExtraOutputCommands, _state: &mut Gui| {
            crate::fonts::install_base(ctx);
        },
        |_out: &egui::FullOutput, _vp: &egui::ViewportOutput, _state: &mut Gui| {},
        |ui: &mut egui::Ui, cmds: &mut ExtraOutputCommands, state: &mut Gui| draw(ui, state, cmds),
    );
}

fn draw(ui: &mut egui::Ui, gui: &mut Gui, commands: &mut ExtraOutputCommands) {
    let background = draw_ui(ui, gui);
    // Also hand the background to the renderer, which clears to it before any
    // of our painting happens.
    commands.clear_color(egui::Rgba::from(background));
}

/// Draw one frame and return the background colour the theme calls for.
///
/// Split out from [`draw`] so it can be run against any `Ui` — including a
/// headless one — without an `ExtraOutputCommands` to hand.
fn draw_ui(ui: &mut egui::Ui, gui: &mut Gui) -> Color32 {
    // Keep drawing. The document is not egui's: the host replaces it through
    // `IComponent::setState` when a project or a preset is loaded, and nothing
    // about that reaches egui, so an editor that only redraws on input sits
    // showing whatever was on screen when the state changed under it.
    ui.ctx().request_repaint();


    // Load a font for any script in the document the current fonts cannot draw.
    // A newly added font takes effect next pass, so ask for one.
    let text = gui.editor.lock().map(|e| e.text().to_string()).ok();
    if let Some(text) = text {
        if crate::fonts::ensure_coverage(ui.ctx(), &text, &mut gui.scripts) {
            ui.ctx().request_repaint();
        }
    }

    // Theme first: everything below reads colours from the active visuals.
    let background = apply_theme(ui, gui);

    // Paint the background ourselves rather than relying solely on the
    // renderer's clear colour. `run_ui` gives a bare root `Ui` with no panel
    // behind it, so without this the window shows whatever was cleared last.
    // The root `Ui` spans the whole viewport, so its rect is the window.
    ui.painter().rect_filled(ui.max_rect(), 0.0, background);

    sync_window_size(ui, gui);

    // A field taking the keyboard takes it from the document.
    if ui.memory(|m| m.focused()).is_some() {
        gui.document_focused = false;
    }

    // Keyboard next, so the document is current before it is laid out — but
    // only when the document is what was clicked into. The note's name and
    // the colour pickers are fields of their own, and their keys are not
    // document text.
    // Pick up anything a dialog thread finished with since the last frame.
    if let Ok(mut slot) = gui.report.lock() {
        if let Some(message) = slot.take() {
            gui.error = Some(message);
        }
    }

    if gui.document_focused {
        let (events, modifiers) = ui.input(|i| (i.events.clone(), i.modifiers));
        let command = apply_input(
            ui.ctx(),
            &gui.editor,
            &events,
            modifiers.ctrl || modifiers.command,
        );
        if let Some(command) = command {
            crate::files::perform_off_thread(&gui.editor, &gui.report, command);
        }
    }

    // The bars and their rules stack with nothing between them. Scoped, so the
    // spacing everything else uses is untouched.
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;

        egui::Frame::default()
            .fill(toolbar_fill(gui, ui.visuals()))
            .inner_margin(TOOLBAR_MARGIN)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                toolbar(ui, gui);
            });
        rule(ui);

        egui::Frame::default()
            .fill(toolbar_fill(gui, ui.visuals()))
            .inner_margin(SECTION_MARGIN)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                sections(ui, gui);
            });
        rule(ui);

        if let Some(error) = &gui.error {
            let colour = ui.visuals().error_fg_color;
            egui::Frame::default()
                .inner_margin(TOOLBAR_MARGIN)
                .show(ui, |ui| ui.colored_label(colour, error));
            rule(ui);
        }
    });

    // The scroll area itself spans the full width so its bar sits against the
    // window edge; the padding goes inside, around the text.
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::default()
                .inner_margin(DOCUMENT_MARGIN)
                .show(ui, |ui| document(ui, gui));
        });

    settings_dialog(ui, gui);

    background
}

/// Persistent GUI state for headless rendering.
///
/// The state has to live across frames exactly as it does in a real window:
/// rebuilding it each frame would re-apply the theme every time and request a
/// repaint forever.
#[doc(hidden)]
pub struct TestGui(Gui);

impl TestGui {
    pub fn new(editor: Shared, system_dark: bool) -> TestGui {
        let mut gui = Gui::new(editor);
        gui.system_dark = system_dark;
        TestGui(gui)
    }

    /// Install the fonts, as opening a window does before its first frame.
    ///
    /// Headless rendering has no window to do it, and the families have to be
    /// bound before anything is laid out in them.
    pub fn install_fonts(ctx: &egui::Context) {
        crate::fonts::install_base(ctx);
    }
}

/// Draw one frame into an arbitrary `Ui`.
///
/// Exists so the snapshot tool can render the real GUI headlessly and check
/// the themes pixel by pixel, rather than anyone having to eyeball a window.
#[doc(hidden)]
pub fn draw_frame_for_test(ui: &mut egui::Ui, state: &mut TestGui) {
    draw_ui(ui, &mut state.0);
}

/// Record the window's size, so it is saved with the project.
///
/// The window is the truth. The host owns the frame, and the editor is drawn
/// into whatever it is given: a plugin that asks for a size of its own is a
/// plugin arguing with its host.
fn sync_window_size(ui: &mut egui::Ui, gui: &mut Gui) {
    let size = ui.ctx().viewport_rect().size();
    let window = (size.x.round() as i32, size.y.round() as i32);
    if window.0 <= 0 || window.1 <= 0 {
        return;
    }
    if gui.last_seen_size == Some(window) {
        return;
    }
    gui.last_seen_size = Some(window);
    if let Ok(mut e) = gui.editor.lock() {
        e.set_size(window.0, window.1);
    }
}

/// Resolve the chosen theme against the system and apply it.
///
/// Two things here are easy to get wrong and were:
///
/// 1. **Nothing paints the window background.** `run_ui` hands us a bare root
///    `Ui` on the background layer — there is no panel behind it — so the
///    background is entirely the renderer's clear colour, which defaults to
///    black. Switching to the light theme without this gives dark text on a
///    black background, which looks like the theme did nothing at all.
/// 2. **`set_visuals` lands a frame late.** The root `Ui` was built from the
///    style as it was when this frame started, so the current frame must have
///    its style updated directly or the change is invisible until something
///    else triggers a repaint.
fn apply_theme(ui: &mut egui::Ui, gui: &mut Gui) -> Color32 {
    let (theme, colours) = match gui.editor.lock() {
        Ok(e) => (e.theme, e.colours),
        Err(_) => (Theme::Auto, markdown_notes_core::ColourScheme::default()),
    };

    // Only the Auto setting needs to know what the system is doing.
    if theme == Theme::Auto {
        gui.refresh_system_theme();
    }

    let dark = theme.is_dark(gui.system_dark);
    let scheme = *colours.for_mode(dark);
    if gui.applied_dark != Some(dark) || gui.applied_colours != Some(scheme) {
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        paint_visuals(&mut visuals, &scheme, dark);
        ui.ctx().set_visuals(visuals.clone());
        ui.style_mut().visuals = visuals;
        gui.applied_dark = Some(dark);
        gui.applied_colours = Some(scheme);
        ui.ctx().request_repaint();
    }

    ui.visuals().panel_fill
}

fn rgba(c: markdown_notes_core::Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Write the scheme into the egui visuals that egui's own widgets read.
///
/// What the editor paints itself is taken from the scheme directly; buttons,
/// fields, checkboxes, the scrollbar and the selection are drawn by egui, and
/// this is the only way to reach them.
fn paint_visuals(visuals: &mut egui::Visuals, c: &markdown_notes_core::Colours, dark: bool) {
    visuals.dark_mode = dark;
    // Neither Windows nor macOS rounds a plain application window.
    visuals.window_corner_radius = egui::CornerRadius::ZERO;
    visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    visuals.panel_fill = rgba(c.window_background);
    visuals.window_fill = rgba(c.window_background);
    visuals.extreme_bg_color = rgba(c.field_background);
    visuals.hyperlink_color = rgba(c.link);
    visuals.error_fg_color = rgba(c.error_text);
    visuals.selection.bg_fill = rgba(c.selection);
    visuals.selection.stroke.color = rgba(c.section_selected_label);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_stroke.color = rgba(c.separator);
    widgets.noninteractive.fg_stroke.color = rgba(c.body_text);
    widgets.noninteractive.bg_fill = rgba(c.window_background);

    widgets.inactive.weak_bg_fill = rgba(c.button_face);
    widgets.inactive.bg_fill = rgba(c.checkbox_fill);
    widgets.inactive.bg_stroke = Stroke::new(1.0, rgba(c.button_outline));
    widgets.inactive.fg_stroke.color = rgba(c.button_label);

    widgets.hovered.weak_bg_fill = rgba(c.button_hover_face);
    widgets.hovered.bg_fill = rgba(c.button_hover_face);
    widgets.hovered.bg_stroke = Stroke::new(1.0, rgba(c.button_outline));
    widgets.hovered.fg_stroke.color = rgba(c.button_label);

    widgets.active.weak_bg_fill = rgba(c.button_pressed_face);
    widgets.active.bg_fill = rgba(c.button_pressed_face);
    widgets.active.bg_stroke = Stroke::new(1.0, rgba(c.button_outline));
    widgets.active.fg_stroke.color = rgba(c.button_label);

    // The section in front is drawn as a "selected" button.
    widgets.open.weak_bg_fill = rgba(c.section_selected_fill);
    widgets.open.fg_stroke.color = rgba(c.section_selected_label);
}

/// Background of the toolbar: a step away from the page, in whichever
/// direction the theme leaves room.
/// A one-pixel line across the window, with nothing above or below it.
///
/// `Ui::separator` allocates `item_spacing.y` on each side of its line, which
/// shows as a band of window background between the bar and the rule.
fn rule(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
}

fn toolbar_fill(gui: &Gui, visuals: &egui::Visuals) -> Color32 {
    match gui.editor.lock() {
        Ok(e) => rgba(e.colours.for_mode(visuals.dark_mode).bar_fill),
        Err(_) => visuals.panel_fill,
    }
}

fn toolbar(ui: &mut egui::Ui, gui: &mut Gui) {
    // Only a note backed by a file can be out of step with one; notes that live
    // solely in plugin state are always saved with the project.
    let (mode, theme, unsaved) = match gui.editor.lock() {
        Ok(e) => (e.mode, e.theme, e.file.is_some() && e.is_dirty()),
        Err(_) => (ViewMode::Wysiwyg, Theme::Auto, false),
    };

    // A button should look like a button whether or not the pointer is over it,
    // so the outline is on in every state rather than appearing on hover. The
    // face is kept at the page colour so it stands out from the darker bar.
    let dark = ui.visuals().dark_mode;
    let outline = Stroke::new(
        1.0,
        if dark {
            Color32::from_gray(105)
        } else {
            Color32::from_gray(150)
        },
    );
    let face = ui.visuals().panel_fill;
    let widgets = &mut ui.visuals_mut().widgets;
    for state in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active] {
        state.bg_stroke = outline;
    }
    widgets.inactive.weak_bg_fill = face;

    // Everything is laid out from the right, so the title gets whatever width
    // is left over and sits in the middle of it.
    ui.horizontal(|ui| {
        ui.set_min_height(TOOLBAR_HEIGHT);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            toolbar_buttons(ui, gui, mode, theme, unsaved);
            title_field(ui, gui);
        });
    });
}

/// The toolbar's controls, added right to left: the gear is furthest right.
fn toolbar_buttons(
    ui: &mut egui::Ui,
    gui: &mut Gui,
    mode: ViewMode,
    theme: Theme,
    unsaved: bool,
) {
    if cog_button(ui, gui).on_hover_text("Settings").clicked() {
        gui.settings_open = !gui.settings_open;
    }

    {
        // Theme. Auto shows what it currently resolves to, so the button never
        // leaves you guessing which way "auto" went.
        let theme_label = match theme {
            Theme::Light => "Theme: light".to_string(),
            Theme::Dark => "Theme: dark".to_string(),
            Theme::Auto => format!(
                "Theme: auto ({})",
                if gui.system_dark { "dark" } else { "light" }
            ),
        };
        let theme_width = fixed_width(
            ui,
            &[
                "Theme: light",
                "Theme: dark",
                "Theme: auto (light)",
                "Theme: auto (dark)",
            ],
        );
        if ui
            .add(egui::Button::new(theme_label).min_size(theme_width))
            .on_hover_text("Ctrl+T — cycles auto → light → dark")
            .clicked()
        {
            if let Ok(mut e) = gui.editor.lock() {
                e.cycle_theme();
            }
        }

        let label = match mode {
            ViewMode::Wysiwyg => "Markdown source",
            ViewMode::Raw => "Formatted",
        };
        let mode_width = fixed_width(ui, &["Markdown source", "Formatted"]);
        if ui
            .add(egui::Button::new(label).min_size(mode_width))
            .on_hover_text("Ctrl+/")
            .clicked()
        {
            if let Ok(mut e) = gui.editor.lock() {
                e.toggle_mode();
            }
        }
    }

    ui.separator();

    if ui.button("Save As…").clicked() {
        crate::files::perform_off_thread(&gui.editor, &gui.report, Command::SaveAs);
    }
    let save = if unsaved {
        egui::RichText::new("Save *").strong()
    } else {
        egui::RichText::new("Save")
    };
    let save_width = fixed_width(ui, &["Save", "Save *"]);
    if ui.add(egui::Button::new(save).min_size(save_width)).clicked() {
        crate::files::perform_off_thread(&gui.editor, &gui.report, Command::Save);
    }
    if ui.button("Open…").clicked() {
        crate::files::perform_off_thread(&gui.editor, &gui.report, Command::Open);
    }

    ui.separator();
}

/// A cog, painted and clickable. No frame, no label.
///
/// It is drawn rather than typed: U+2699 is absent from the interface font on
/// both platforms, and where a fallback supplies it at all it arrives at
/// whatever size and baseline that font uses.
fn cog_button(ui: &mut egui::Ui, gui: &Gui) -> egui::Response {
    let side = TOOLBAR_HEIGHT - 4.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());

    let centre = rect.center();
    let radius = side * 0.30;
    // Hovering changes the line colour and nothing else. No fill, no frame.
    let colours = match gui.editor.lock() {
        Ok(e) => *e.colours.for_mode(ui.visuals().dark_mode),
        Err(_) => *markdown_notes_core::ColourScheme::default().for_mode(ui.visuals().dark_mode),
    };
    let colour = if response.hovered() {
        rgba(colours.highlight)
    } else {
        rgba(colours.button_label)
    };
    let stroke = Stroke::new(1.3, colour);
    let ring = radius * 0.66;

    let painter = ui.painter();
    painter.circle_stroke(centre, ring, stroke);
    painter.circle_stroke(centre, radius * 0.24, stroke);
    for tooth in 0..8 {
        let angle = std::f32::consts::TAU * tooth as f32 / 8.0;
        let dir = egui::vec2(angle.cos(), angle.sin());
        painter.line_segment([centre + dir * ring, centre + dir * radius], stroke);
    }

    response
}

/// Size a button so it fits the widest label it can ever show.
///
/// A button that resizes when its own label changes drags every widget after
/// it sideways, and the pointer ends up over something else.
fn fixed_width(ui: &egui::Ui, labels: &[&str]) -> egui::Vec2 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let widest = labels
        .iter()
        .map(|label| {
            ui.painter()
                .layout_no_wrap((*label).to_string(), font.clone(), Color32::PLACEHOLDER)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);
    egui::Vec2::new(widest + 2.0 * ui.spacing().button_padding.x, 0.0)
}

/// Identifies the name field, so the document can tell when it has the keys.
fn title_id() -> egui::Id {
    egui::Id::new("markdown-notes-title")
}

/// Identifies the document. Keys reach the document only while this has focus,
/// and it only has focus once the document has been clicked into.
fn document_id() -> egui::Id {
    egui::Id::new("markdown-notes-document")
}

/// Put the whole of a text field's contents in its selection.
fn select_all(ctx: &egui::Context, id: egui::Id, chars: usize) {
    let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) else {
        return;
    };
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(chars),
    )));
    state.store(ctx, id);
}

/// The note's own name, editable in place.
fn title_field(ui: &mut egui::Ui, gui: &mut Gui) {
    let (stored, bold) = match gui.editor.lock() {
        Ok(e) => (
            e.title.clone(),
            rgba(e.colours.for_mode(ui.visuals().dark_mode).bold_text),
        ),
        Err(_) => return,
    };

    let id = title_id();
    // Leave the buffer alone while it is being typed into, or every keystroke
    // would be overwritten by what is still stored.
    if !ui.memory(|m| m.has_focus(id)) {
        gui.title = stored.clone();
    }

    // It reads as a heading, not a form control: the whole width the buttons
    // left over, the toolbar's own fill behind it, no border, centred, and in
    // the bold text colour. Clicking still edits it.
    let font = egui::TextStyle::Button.resolve(ui.style());
    let bar = toolbar_fill(gui, ui.visuals());
    let response = ui.add(
        egui::TextEdit::singleline(&mut gui.title)
            .id(id)
            .desired_width(ui.available_width())
            .horizontal_align(egui::Align::Center)
            .frame(egui::Frame::NONE.fill(bar))
            .background_color(bar)
            .font(egui::FontId {
                size: font.size + TITLE_SIZE_BUMP,
                family: crate::fonts::bold_family(ui),
            })
            .text_color(bold),
    );

    // A name nobody has set yet is there to be replaced, so it arrives
    // selected and the first keystroke takes all of it. A name somebody chose
    // is clicked into to change part of it, and keeps the caret where it was
    // put.
    if response.gained_focus() && gui.title == markdown_notes_core::DEFAULT_TITLE {
        select_all(ui.ctx(), id, gui.title.chars().count());
    }

    if response.changed() {
        if let Ok(mut e) = gui.editor.lock() {
            // Not `dirty`: that tracks the document against the file on disk,
            // and the name is not written to it.
            e.title = gui.title.clone();
        }
    }

    // Enter means the name is finished, so the keyboard goes back to the
    // section that is open and typing carries straight on.
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        gui.document_focused = true;
    }
}

/// The strip of sections: one button per section of the document.
///
/// The sections are the same document seen in parts, so there is no unsaved
/// one: closing a section removes it from what gets saved.

/// Which section a drag would land on, given where the pointer is.
fn landing(rects: &[egui::Rect], x: f32) -> usize {
    rects
        .iter()
        .position(|r| x < r.right())
        .unwrap_or(rects.len().saturating_sub(1))
}

/// How solid a section is while it is being dragged, both the one on the
/// pointer and the one it came from.
///
/// Faint enough to read the strip through either of them: the one on the
/// pointer passes over the others, and the one left behind is a placeholder for
/// a section that is not there any more.
const GHOST: f32 = 0.4;

/// How far the drop mark stands off a section when there is no gap to sit in,
/// which is before the first and after the last.
const BESIDE: f32 = 4.0;

/// Halfway between two edges.
fn midway(left: f32, right: f32) -> f32 {
    (left + right) / 2.0
}

/// Show that a section is being dragged, and where it would go.
///
/// Three things say what is going on, the way every other strip of this kind
/// does it: the section being dragged travels with the pointer, the one it came
/// from fades, and a bar marks the place it would drop into.
fn show_the_drag(
    ui: &egui::Ui,
    rects: &[egui::Rect],
    from: usize,
    target: usize,
    pointer: egui::Pos2,
    title: &str,
    highlight: Color32,
) {
    let Some(source) = rects.get(from) else {
        return;
    };
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("drag")));

    // The mark for where it would land: in the gap between two sections, never
    // against one. Before the first and after the last there is no gap to sit
    // in, so it keeps the same distance from the section it is beside.
    if let Some(over) = rects.get(target) {
        let gap = if target > from {
            let after = rects.get(target + 1).map(|next| next.left());
            midway(over.right(), after.unwrap_or(over.right() + BESIDE * 2.0))
        } else {
            let before = if target == 0 {
                None
            } else {
                rects.get(target - 1).map(|previous| previous.right())
            };
            midway(before.unwrap_or(over.left() - BESIDE * 2.0), over.left())
        };
        // The height of the strip the sections are in, which is their row and
        // the space above and below it, not the window.
        let middle = source.center().y;
        painter.line_segment(
            [
                egui::pos2(gap, middle - TOOLBAR_HEIGHT / 2.0),
                egui::pos2(gap, middle + TOOLBAR_HEIGHT / 2.0),
            ],
            Stroke::new(2.0, highlight),
        );
    }

    // The section itself, under the pointer, held at the height of the strip so
    // it slides along it rather than wandering off.
    let carried = egui::Rect::from_center_size(
        egui::pos2(pointer.x, source.center().y),
        source.size(),
    );
    ghost_section(&painter, ui, carried, title);
}

/// Draw a section as the strip draws it, faded to [`GHOST`].
///
/// The same fill, border and label, so what is on the pointer is recognisably
/// the thing that was picked up, and what is left behind is recognisably the
/// place it was picked up from.
fn ghost_section(painter: &egui::Painter, ui: &egui::Ui, rect: egui::Rect, title: &str) {
    let visuals = ui.visuals().widgets.inactive;
    painter.rect_filled(rect, 3.0, visuals.weak_bg_fill.gamma_multiply(GHOST));
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, visuals.bg_stroke.color.gamma_multiply(GHOST)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        title,
        egui::TextStyle::Button.resolve(ui.style()),
        visuals.text_color().gamma_multiply(GHOST),
    );
}

fn sections(ui: &mut egui::Ui, gui: &mut Gui) {
    let (count, active, titles) = match gui.editor.lock() {
        Ok(e) => {
            let titles: Vec<(String, String, bool)> = (0..e.section_count())
                .map(|i| (e.section_display_title(i), e.section_title(i), e.section_title_is_truncated(i)))
                .collect();
            (e.section_count(), e.active_section(), titles)
        }
        Err(_) => return,
    };

    // Every section is the same width, that of the longest title allowed, so the
    // strip does not shuffle sideways while a heading is being typed.
    let section_size = fixed_width(ui, &[SECTION_WIDTH_SAMPLE]);
    let section_width = section_size.x;

    let mut select: Option<usize> = None;
    let mut close: Option<usize> = None;
    let mut added = false;
    let mut drop_at: Option<(usize, usize)> = None;
    let mut rects: Vec<egui::Rect> = Vec::with_capacity(count);

    let highlight = match gui.editor.lock() {
        Ok(e) => rgba(e.colours.for_mode(ui.visuals().dark_mode).highlight),
        Err(_) => ui.visuals().widgets.hovered.weak_bg_fill,
    };

    ui.horizontal(|ui| {
        ui.set_min_height(TOOLBAR_HEIGHT);
        // Only the strip: elsewhere a hovered button keeps its own colour.
        ui.visuals_mut().widgets.hovered.weak_bg_fill = highlight;

        for (index, (shown, full, truncated)) in titles.iter().enumerate() {
            let selected = index == active;
            // The section being dragged is drawn faint, in its own place: it is on
            // the pointer now, and what is left is where it came from.
            if gui.dragging == Some(index) {
                ui.set_opacity(GHOST);
            } else {
                ui.set_opacity(1.0);
            }
            let response = ui
                .add(
                    egui::Button::new(shown.as_str())
                        .selected(selected)
                        .min_size(egui::Vec2::new(section_width, 0.0))
                        // A title of wide glyphs can still overrun the limit
                        // in pixels; clipping keeps them all the same size.
                        .truncate()
                        .sense(egui::Sense::click_and_drag()),
                )
                .on_hover_cursor(egui::CursorIcon::Grab);
            let response = if *truncated {
                response.on_hover_text(full)
            } else {
                response
            };
            rects.push(response.rect);

            if response.clicked() {
                select = Some(index);
            }
            if response.drag_started() {
                gui.dragging = Some(index);
                select = Some(index);
            }
            // Middle click closes, the way it does in a browser.
            if response.middle_clicked() {
                close = Some(index);
            }
            response.context_menu(|ui| {
                if ui.button("Close section").clicked() {
                    close = Some(index);
                    ui.close();
                }
            });
        }

        ui.set_opacity(1.0);
        if ui.button("+").on_hover_text("New section").clicked() {
            if let Ok(mut e) = gui.editor.lock() {
                e.new_section();
            }
            added = true;
        }
    });

    // A drag ends over whichever section the pointer is on; that is where it
    // lands.
    if let Some(from) = gui.dragging {
        let released = ui.input(|i| i.pointer.any_released());
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if let Some(pos) = pointer {
            let target = landing(&rects, pos.x);
            if !released {
                let title = titles.get(from).map(|(shown, ..)| shown.as_str());
                show_the_drag(ui, &rects, from, target, pos, title.unwrap_or(""), highlight);
            }
        }
        if released {
            if let Some(pos) = pointer {
                let target = landing(&rects, pos.x);
                if target != from {
                    drop_at = Some((from, target));
                }
            }
            gui.dragging = None;
        }
    }
    gui.section_rects = rects;

    // The strip is part of the document, so touching it takes the keyboard
    // back from whatever field had it. Without this, adding a section after
    // renaming the note leaves the new section unable to receive anything.
    let touched = drop_at.is_some() || select.is_some() || close.is_some() || added;
    if touched {
        gui.document_focused = true;
    }

    if let Ok(mut e) = gui.editor.lock() {
        if let Some((from, to)) = drop_at {
            e.move_section(from, to);
        } else if let Some(index) = select {
            e.set_active_section(index);
        }
        if let Some(index) = close {
            e.close_section(index);
        }
    }
}

/// Every bound colour, named for the dialog.
///
/// Editing goes through the accessor rather than a copy, so a change lands in
/// the scheme the editor is holding and takes effect on the next frame.
type Field = (&'static str, fn(&mut markdown_notes_core::Colours) -> &mut markdown_notes_core::Rgba);

const CHROME_FIELDS: &[Field] = &[
    ("Window background", |c| &mut c.window_background),
    ("Toolbar and sections", |c| &mut c.bar_fill),
    ("Separators", |c| &mut c.separator),
    ("Button face", |c| &mut c.button_face),
    ("Button outline", |c| &mut c.button_outline),
    ("Button hover", |c| &mut c.button_hover_face),
    ("Button pressed", |c| &mut c.button_pressed_face),
    ("Button label", |c| &mut c.button_label),
    ("Selected section", |c| &mut c.section_selected_fill),
    ("Selected section label", |c| &mut c.section_selected_label),
    ("Highlight (hover)", |c| &mut c.highlight),
    ("Field background", |c| &mut c.field_background),
    ("Field text", |c| &mut c.field_text),
    ("Field placeholder", |c| &mut c.field_placeholder),
    ("Error text", |c| &mut c.error_text),
];

const DOCUMENT_FIELDS: &[Field] = &[
    ("Body text", |c| &mut c.body_text),
    ("Headings", |c| &mut c.heading_text),
    ("Bold text", |c| &mut c.bold_text),
    ("Markdown markers", |c| &mut c.marker_text),
    ("Strikethrough", |c| &mut c.strikethrough),
    ("Links", |c| &mut c.link),
    ("Code text", |c| &mut c.code_text),
    ("Code background", |c| &mut c.code_background),
    ("Blockquote bar", |c| &mut c.quote_bar),
    ("List glyphs", |c| &mut c.list_glyph),
    ("Checkbox outline", |c| &mut c.checkbox_frame),
    ("Checkbox fill", |c| &mut c.checkbox_fill),
    ("Checkbox tick", |c| &mut c.checkbox_tick),
    ("Caret", |c| &mut c.caret),
    ("Selection", |c| &mut c.selection),
    ("Scrollbar", |c| &mut c.scrollbar),
];

/// A section title: a filled band across the dialog, in capitals.
///
/// A heading has to be recognisable as one without reading it. Size alone is
/// not enough at these sizes.
fn section_header(ui: &mut egui::Ui, text: &str, band: Color32, label: Color32) {
    ui.add_space(12.0);
    egui::Frame::default()
        .fill(band)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(text.to_uppercase())
                    .strong()
                    .size(13.0)
                    .color(label),
            );
        });
    ui.add_space(8.0);
}

/// Colour pickers for whichever scheme is currently on screen.
fn colour_settings(ui: &mut egui::Ui, gui: &mut Gui) {
    let dark = ui.visuals().dark_mode;
    ui.label(if dark {
        "Colours — dark scheme"
    } else {
        "Colours — light scheme"
    });
    ui.label(
        egui::RichText::new("The other scheme is edited by switching to it.")
            .small()
            .weak(),
    );

    let Ok(mut editor) = gui.editor.lock() else {
        return;
    };
    let band = rgba(editor.colours.for_mode(dark).bar_fill);
    let label = rgba(editor.colours.for_mode(dark).bold_text);
    let mut changed = false;

    egui::ScrollArea::vertical()
        .max_height(320.0)
        // Without this the area is only as wide as its widest row, and the bar
        // lands in the middle of the dialog rather than against its edge.
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (heading, fields) in [("Interface", CHROME_FIELDS), ("Document", DOCUMENT_FIELDS)] {
                section_header(ui, heading, band, label);
                for (name, field) in fields {
                    ui.horizontal(|ui| {
                        let colour = field(editor.colours.for_mode_mut(dark));
                        let mut value =
                            Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, colour.a);
                        if ui.color_edit_button_srgba(&mut value).changed() {
                            let [r, g, b, a] = value.to_srgba_unmultiplied();
                            *colour = markdown_notes_core::Rgba::rgba(r, g, b, a);
                            changed = true;
                        }
                        ui.label(*name);
                    });
                }
            }
        });

    ui.add_space(6.0);
    if ui.button("Reset this scheme").clicked() {
        editor.colours.reset(dark);
        changed = true;
    }
    if changed {
        editor.dirty = true;
    }
}

/// The settings dialog behind the toolbar's gear.
fn settings_dialog(ui: &mut egui::Ui, gui: &mut Gui) {
    if !gui.settings_open {
        return;
    }
    let mut open = gui.settings_open;
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 40.0))
        .show(ui.ctx(), |ui| {
            let current = gui.editor.lock().map(|e| e.theme).unwrap_or(Theme::Auto);
            ui.label("Theme");
            let mut chosen = current;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut chosen, Theme::Auto, "Auto");
                ui.selectable_value(&mut chosen, Theme::Light, "Light");
                ui.selectable_value(&mut chosen, Theme::Dark, "Dark");
            });
            if chosen != current {
                if let Ok(mut e) = gui.editor.lock() {
                    e.theme = chosen;
                }
            }

            ui.separator();
            colour_settings(ui, gui);
        });
    gui.settings_open = open;
}

fn document(ui: &mut egui::Ui, gui: &mut Gui) {
    let Ok(mut editor) = gui.editor.lock() else {
        return;
    };

    let doc = editor.render();
    let src = editor.text().to_string();
    let caret = editor.caret();
    let selection = editor.selection();
    let raw = editor.mode == ViewMode::Raw;
    let palette = Palette::from(editor.colours.for_mode(ui.visuals().dark_mode));

    ui.spacing_mut().item_spacing.y = 0.0;

    let mut clicked: Option<Pointer> = None;
    let mut toggled: Option<usize> = None;

    let em = egui::TextStyle::Body.resolve(ui.style()).size;
    let mut previous: Option<&Block> = None;

    // The fence lines are the block's edges, not lines of it. They take no row
    // of their own unless the caret is on one, where the backticks and the
    // language have to be there to edit.
    let rows: Vec<&Block> = doc
        .blocks
        .iter()
        .filter(|block| {
            let editing_fence = caret >= block.range.start && caret <= block.range.end;
            raw || !matches!(block.kind, BlockKind::Fence { .. }) || editing_fence
        })
        .collect();

    for (index, block) in rows.iter().enumerate() {
        // A code block is padded at the top and bottom, and the rows that get
        // that padding are the ones drawn at its edges, which is not the same
        // as the lines at its edges: the fences may not be among them.
        let is_code = |b: &Block| matches!(b.kind, BlockKind::Code | BlockKind::Fence { .. });
        let pad = Padding {
            top: index == 0 || !is_code(rows[index - 1]),
            bottom: index + 1 == rows.len() || !is_code(rows[index + 1]),
        };

        if let Some(previous) = previous {
            ui.add_space(block_gap(previous, block, em));
        }
        previous = Some(block);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;

            // Blockquote bars and list indentation.
            // Painted rather than drawn as a "▌" glyph: that character is not
            // in egui's default font and rendered as a tofu box.
            for _ in 0..block.quote_depth {
                let height = ui.text_style_height(&egui::TextStyle::Body);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, height), Sense::hover());
                let colour = palette.quote_bar;
                ui.painter().rect_filled(rect, 1.0, colour);
                ui.add_space(6.0);
            }
            ui.add_space(block.kind.indent() as f32 * 14.0);

            // The list glyph replaces the hidden marker. In raw mode the marker
            // text itself is shown instead, so no glyph is drawn.
            if !raw {
                match &block.kind {
                    BlockKind::Bullet { checked: Some(done), .. }
                    | BlockKind::Numbered { checked: Some(done), .. } => {
                        let mut checked = *done;
                        if ui.checkbox(&mut checked, "").changed() {
                            toggled = Some(block.line);
                        }
                        ui.add_space(4.0);
                    }
                    BlockKind::Bullet { .. } => {
                        ui.colored_label(palette.list_glyph, "•");
                        ui.add_space(6.0);
                    }
                    BlockKind::Numbered { number, .. } => {
                        ui.colored_label(palette.list_glyph, format!("{number}."));
                        ui.add_space(6.0);
                    }
                    _ => {}
                }
            }

            if let Some(hit) = line_body(ui, block, &src, caret, &selection, raw, &palette, pad) {
                clicked = Some(hit);
            }
        });
    }

    let touched = clicked.is_some();
    if let Some(line) = toggled {
        editor.toggle_checkbox(line);
    } else if let Some(hit) = clicked {
        if hit.pressed {
            // Where a drag will select from, and where the caret goes if it
            // turns out to be a plain click.
            gui.selecting_from = Some(hit.at);
            editor.set_caret(hit.at);
        } else if hit.dragged {
            if let Some(from) = gui.selecting_from {
                let (a, b) = (from.min(hit.at), from.max(hit.at));
                editor.select(a..b);
            }
        }
    }

    // Clicking anywhere in the document — on a line or in the empty space
    // below it — hands the keys back from whatever field had them.
    let rest = ui.available_rect_before_wrap();
    let response = ui
        .interact(rest, document_id(), Sense::click())
        .on_hover_cursor(egui::CursorIcon::Text);
    if touched || toggled.is_some() || response.clicked() {
        gui.document_focused = true;
    }
}

/// Vertical space to leave between two consecutive lines.
///
/// One line is not one block: a wrapped paragraph, the lines of a fenced code
/// block and the items of a list are each several lines belonging together, and
/// only get separated where one element ends and the next begins.
fn block_gap(previous: &Block, current: &Block, em: f32) -> f32 {
    use BlockKind::*;

    // Rows of one code block. A gap here would be a stripe of window showing
    // through the middle of it.
    let fenced = |b: &Block| matches!(b.kind, Code | Fence { .. });
    if fenced(previous) && fenced(current) {
        return 0.0;
    }

    // A blank line beside a code block is a line the user asked for on top of
    // the space a block gets anyway. Without this the second press of Enter
    // buys the few pixels by which a blank row is taller than the gap it
    // replaces, and the text does not move.
    if fenced(previous) && matches!(current.kind, Blank) {
        return em;
    }
    if matches!(previous.kind, Blank) && fenced(current) {
        return em;
    }

    // Elsewhere a blank line is already a line's worth of space, and the gap
    // would be a second one.
    if matches!(previous.kind, Blank) || matches!(current.kind, Blank) {
        return 0.0;
    }

    // Continuation lines of one blockquote.
    if previous.quote_depth > 0 && previous.quote_depth == current.quote_depth {
        return 0.0;
    }

    match (&previous.kind, &current.kind) {
        // Lines of one paragraph, and lines inside one fence.
        (Paragraph, Paragraph) => 0.0,
        (Code, Code) | (Fence { .. }, Code) | (Code, Fence { .. }) => 0.0,

        // Items of one list sit closer together than separate elements.
        (Bullet { .. } | Numbered { .. }, Bullet { .. } | Numbered { .. }) => em * 0.3,

        _ => em,
    }
}

/// Draw one line's text, its caret, and report a click position.
/// What the pointer did over a line of the document, in source offsets.
struct Pointer {
    at: usize,
    pressed: bool,
    dragged: bool,
}

/// Which edges of a code block this row is at, and so where its padding goes.
#[derive(Clone, Copy)]
struct Padding {
    top: bool,
    bottom: bool,
}

fn line_body(
    ui: &mut egui::Ui,
    block: &Block,
    src: &str,
    caret: usize,
    selection: &std::ops::Range<usize>,
    raw: bool,
    palette: &Palette,
    pad: Padding,
) -> Option<Pointer> {
    let base = base_format(block, palette);
    let bold = crate::fonts::bold_family(ui);
    // A fence and the lines inside it are one block, not text that happens to
    // be coloured. Every row of it is painted, so consecutive rows join up.
    let fenced = matches!(block.kind, BlockKind::Code | BlockKind::Fence { .. });


    let mut job = LayoutJob::default();
    // Byte offset in the source for each byte offset in the job, so a click can
    // be mapped back onto the document.
    let mut map: Vec<usize> = Vec::new();

    // The raw marker is drawn when the caret is on this line (Typora-style) or
    // when the whole document is in raw mode.
    if block.marker_visible && !block.marker.is_empty() {
        let text = &src[block.marker.clone()];
        push(
            &mut job,
            &mut map,
            text,
            block.marker.start,
            marker_format(&base, &palette),
        );
    }

    for span in &block.spans {
        if !span.visible {
            continue;
        }
        push(
            &mut job,
            &mut map,
            span.text(src),
            span.range.start,
            span_format(span, &base, &palette, &bold, fenced),
        );
    }

    // An empty line still needs height and a click target.
    if job.text.is_empty() {
        push(&mut job, &mut map, " ", block.range.start, base.clone());
    }
    map.push(block.range.end);

    let galley = ui.painter().layout_job(job);

    // The whole width of the line, not the width of what is written on it.
    // Clicking past the end of a line is how everyone puts the caret at the
    // end of it, and a line only as wide as its text has nothing there to
    // click.
    // The code sits inside its block rather than flush against the edge, so
    // the row carries the padding: wider by the inset on both sides, and taller
    // at whichever end of the block it is at. Padding drawn outside the row
    // would land on the line above or below instead.
    let (inset, above, below) = if fenced {
        (
            CODE_PADDING * 2.0,
            if pad.top { CODE_PADDING } else { 0.0 },
            if pad.bottom { CODE_PADDING } else { 0.0 },
        )
    } else {
        (0.0, 0.0, 0.0)
    };
    let size = egui::vec2(
        ui.available_width().max(galley.size().x + inset),
        galley.size().y + above + below,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    // An I-beam over text, because that is what an I-beam means.
    let response = response.on_hover_cursor(egui::CursorIcon::Text);

    // Where a source offset sits along this line. The galley is asked, because
    // it is the thing that placed the glyphs: a line mixes fonts, and anything
    // that re-measures the text in one font puts the caret in the wrong place
    // the moment a bold or code span precedes it.
    let x_of = |offset: usize| -> f32 { offset_x(&galley, &map, offset) };

    // The block itself, behind everything else on the row, and the text inside
    // it clear of its edges. Rows of one block are drawn touching, so they read
    // as one shape.
    let origin = if fenced {
        // Rows land on fractional pixels, which leaves a hairline of window
        // showing between them. Each row is drawn a pixel into its neighbour
        // to cover it, but only at an edge inside the block: at the block's
        // own edges there is a line of prose to run into.
        let mut fill = rect;
        if !pad.top {
            fill.min.y -= 1.0;
        }
        if !pad.bottom {
            fill.max.y += 1.0;
        }
        ui.painter()
            .rect_filled(fill, 0.0, palette.code_background);
        rect.min + egui::vec2(CODE_PADDING, above)
    } else {
        rect.min
    };

    // The selection goes behind the text, so the words stay readable.
    let (from, to) = (
        selection.start.max(block.range.start),
        selection.end.min(block.range.end),
    );
    if from < to {
        let band = egui::Rect::from_min_max(
            origin + egui::vec2(x_of(from), 0.0),
            origin + egui::vec2(x_of(to), galley.size().y),
        );
        ui.painter().rect_filled(band, 0.0, palette.selection);
    }

    ui.painter()
        .galley(origin, Arc::clone(&galley), palette.body);

    if caret >= block.range.start && caret <= block.range.end {
        let top = origin + egui::vec2(x_of(caret), 0.0);
        ui.painter().line_segment(
            [top, top + egui::vec2(0.0, galley.size().y)],
            Stroke::new(1.5, palette.caret),
        );
    }

    let _ = raw;

    // A press puts the caret down; dragging from there selects.
    let pressed = response.drag_started() || response.clicked();
    let dragged = response.dragged();
    if !pressed && !dragged {
        return None;
    }
    let pos = response.interact_pointer_pos()?;
    let cursor = galley.cursor_from_pos(pos - origin);
    let index = cursor.index.0.min(map.len().saturating_sub(1));
    let at = map.get(index).copied()?;
    Some(Pointer { at, pressed, dragged })
}

/// How far along the line the source offset `offset` sits.
///
/// `map` gives the source offset of every character in the galley, so the
/// offset first becomes a character index and the galley then says where that
/// character was put. Only the galley knows: a line mixes fonts, and measuring
/// the preceding text in any single one of them places the caret inside the
/// word before it as soon as a bold or code span comes first.
fn offset_x(galley: &egui::Galley, map: &[usize], offset: usize) -> f32 {
    let char_index = map
        .iter()
        .position(|&at| at >= offset)
        .unwrap_or(map.len().saturating_sub(1));
    galley
        .pos_from_cursor(egui::text::CCursor::new(char_index))
        .min
        .x
}

/// Append `text` to the job, recording the source offset of every byte.
fn push(job: &mut LayoutJob, map: &mut Vec<usize>, text: &str, source_start: usize, fmt: TextFormat) {
    for (i, _) in text.char_indices() {
        map.push(source_start + i);
    }
    job.append(text, 0.0, fmt);
}

fn base_format(block: &Block, palette: &Palette) -> TextFormat {
    let size = match block.kind {
        BlockKind::Heading(1) => 28.0,
        BlockKind::Heading(2) => 23.0,
        BlockKind::Heading(3) => 20.0,
        BlockKind::Heading(4) => 18.0,
        BlockKind::Heading(5) => 16.5,
        BlockKind::Heading(6) => 15.5,
        _ => 15.0,
    };
    let family = match block.kind {
        BlockKind::Code | BlockKind::Fence { .. } => FontFamily::Monospace,
        _ => FontFamily::Proportional,
    };
    TextFormat {
        font_id: FontId::new(size, family),
        color: match block.kind {
            BlockKind::Heading(_) => palette.heading,
            BlockKind::Code | BlockKind::Fence { .. } => palette.code,
            _ => palette.body,
        },
        ..Default::default()
    }
}

/// Colours taken from the active theme.
///
/// Everything the document renderer draws comes from here rather than from
/// literals, so light mode is not a washed-out copy of the dark palette.
#[derive(Clone, Copy)]
struct Palette {
    body: Color32,
    heading: Color32,
    strong: Color32,
    dim: Color32,
    strike: Color32,
    link: Color32,
    code: Color32,
    code_background: Color32,
    quote_bar: Color32,
    list_glyph: Color32,
    caret: Color32,
    selection: Color32,
}

impl Palette {
    fn from(c: &markdown_notes_core::Colours) -> Palette {
        Palette {
            body: rgba(c.body_text),
            heading: rgba(c.heading_text),
            strong: rgba(c.bold_text),
            dim: rgba(c.marker_text),
            strike: rgba(c.strikethrough),
            link: rgba(c.link),
            code: rgba(c.code_text),
            code_background: rgba(c.code_background),
            quote_bar: rgba(c.quote_bar),
            list_glyph: rgba(c.list_glyph),
            caret: rgba(c.caret),
            selection: rgba(c.selection),
        }
    }
}

/// Markdown punctuation, shown dimmed when revealed.
fn marker_format(base: &TextFormat, palette: &Palette) -> TextFormat {
    TextFormat {
        color: palette.dim,
        ..base.clone()
    }
}

fn span_format(
    span: &Span,
    base: &TextFormat,
    palette: &Palette,
    bold: &FontFamily,
    fenced: bool,
) -> TextFormat {
    let mut fmt = base.clone();

    // Inline code is a run of coloured text. Code in a fence is not: the whole
    // block is painted behind the line, so putting a background on the glyphs
    // as well would draw a second, narrower box on top of it.
    if span.style.contains(Style::CODE) {
        fmt.font_id = FontId::new(fmt.font_id.size - 1.0, FontFamily::Monospace);
        if !fenced {
            fmt.background = palette.code_background;
        }
    }
    if span.style.contains(Style::ITALIC) {
        fmt.italics = true;
    }
    if span.style.contains(Style::STRIKE) {
        fmt.strikethrough = Stroke::new(1.0, palette.strike);
    }
    // egui has no bold family, so bold is a stronger colour — the same trick
    // egui's own `RichText::strong` uses.
    // The bold face the machine has, at the size this text is already using.
    if span.style.contains(Style::BOLD) {
        fmt.font_id = FontId::new(fmt.font_id.size, bold.clone());
        fmt.color = palette.strong;
    }
    if span.is_marker() {
        fmt.color = palette.dim;
    }
    match &span.role {
        SpanRole::Link { .. } | SpanRole::Image { .. } => {
            fmt.color = palette.link;
            fmt.underline = Stroke::new(1.0, palette.link);
        }
        _ => {}
    }
    fmt
}

/// Feed egui's key and text events into the editor.
///
/// Returns a host command if one was requested (Ctrl+O / Ctrl+S / Ctrl+Shift+S),
/// so the caller can run it after releasing the document lock.
fn apply_input(
    ctx: &egui::Context,
    editor: &Shared,
    events: &[egui::Event],
    command_held: bool,
) -> Option<Command> {
    let mut command = None;
    let Ok(mut ed) = editor.lock() else {
        return None;
    };

    // Whether a command modifier was down, taken from the key events
    // themselves rather than from the frame. A shortcut pressed and released
    // inside one frame leaves the frame's modifiers clear by the time this
    // runs, and the letter would be typed as well as acted on.
    let mut held = command_held;

    for event in events {
        match event {
            // Printable input, and pastes.
            //
            // A paste arrives here too: the backend reads the clipboard and
            // hands the contents over as text. It is not skipped when a
            // command modifier is down, because that is exactly when a paste
            // happens — the backend has already dropped the letter of any
            // other shortcut, so text under Ctrl is a paste and nothing else.
            egui::Event::Text(text) => {
                if held {
                    ed.insert_str(text);
                    continue;
                }
                for c in text.chars() {
                    if !c.is_control() {
                        ed.handle_key(Key::Char(c), Mods::NONE);
                    }
                }
            }
            // The clipboard. egui turns the platform's shortcuts into these,
            // so this is where Ctrl+C, Ctrl+X and Ctrl+V arrive.
            egui::Event::Copy => {
                if ed.has_selection() {
                    ctx.copy_text(ed.selected_text());
                }
            }
            egui::Event::Cut => {
                if ed.has_selection() {
                    ctx.copy_text(ed.selected_text());
                    ed.delete_selection();
                }
            }
            egui::Event::Paste(text) => {
                ed.insert_str(text);
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let mods = Mods {
                    ctrl: modifiers.ctrl || modifiers.command,
                    shift: modifiers.shift,
                    alt: modifiers.alt,
                };
                held = mods.ctrl;
                if let Some(k) = translate_key(*key, mods.ctrl) {
                    let result = ed.handle_key(k, mods);
                    if let Some(c) = result.command {
                        command = Some(c);
                    }
                }
            }
            _ => {}
        }
    }
    command
}

fn translate_key(key: egui::Key, ctrl: bool) -> Option<Key> {
    use egui::Key as E;
    let k = match key {
        E::Enter => Key::Enter,
        E::Backspace => Key::Backspace,
        E::Delete => Key::Delete,
        E::Tab => Key::Tab,
        E::ArrowLeft => Key::Left,
        E::ArrowRight => Key::Right,
        E::ArrowUp => Key::Up,
        E::ArrowDown => Key::Down,
        E::Home => Key::Home,
        E::End => Key::End,
        E::PageUp => Key::PageUp,
        E::PageDown => Key::PageDown,
        E::Escape => Key::Escape,
        // Letters and `/` only matter while Ctrl is held; without it they
        // arrive as text events and are inserted there.
        other if ctrl => {
            let name = other.name();
            if other == E::Slash {
                Key::Char('/')
            } else if name.len() == 1 {
                let c = name.chars().next()?;
                if c.is_ascii_alphabetic() {
                    Key::Char(c.to_ascii_lowercase())
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The caret goes after a bold word, not inside it.
    ///
    /// A line of mixed formats used to be re-measured in the body font to place
    /// the caret, so every bold or code span before it made the caret drift
    /// back by the difference between the two faces: type `**well** yo` and the
    /// caret sits between the `y` and the `o`.
    #[test]
    fn the_caret_lands_after_bold_text_not_inside_it() {
        let ctx = egui::Context::default();
        crate::fonts::install_base(&ctx);
        // Fonts arrive on the pass after the one that installs them.
        let _ = ctx.run_ui(Default::default(), |_| {});

        let body = FontId::new(15.0, FontFamily::Proportional);
        let bold = FontId::new(15.0, FontFamily::Name(crate::fonts::BOLD.into()));

        let mut job = LayoutJob::default();
        let mut map: Vec<usize> = Vec::new();
        // The source is `testing as **well** yo`; the markers are not drawn,
        // so the character after the bold word is at source offset 18.
        push(&mut job, &mut map, "testing as ", 0, TextFormat::simple(body.clone(), Color32::WHITE));
        push(&mut job, &mut map, "well", 13, TextFormat::simple(bold, Color32::WHITE));
        push(&mut job, &mut map, " yo", 19, TextFormat::simple(body.clone(), Color32::WHITE));
        map.push(22);

        let mut galley = None;
        let mut flat = 0.0;
        let _ = ctx.run_ui(Default::default(), |ui| {
            galley = Some(ui.painter().layout_job(job.clone()));
            // Where measuring the plain text in the body font would put it.
            flat = ui
                .painter()
                .layout_no_wrap("testing as well y".to_string(), body.clone(), Color32::WHITE)
                .size()
                .x;
        });
        let galley = galley.expect("nothing was laid out");
        let x = offset_x(&galley, &map, 21);

        // Where the glyphs actually are: the caret belongs at the left edge of
        // the last character.
        let last = galley.rows[0]
            .glyphs
            .last()
            .expect("the line laid out no glyphs");
        assert!(
            (x - last.pos.x).abs() < 0.5,
            "caret drawn at {x} for a character whose glyph starts at {}",
            last.pos.x
        );

        // And that is not where one font puts it, or the test would pass
        // against the bug.
        assert!(
            x - flat > 1.0,
            "the bold face should be wider: galley {x}, one font {flat}"
        );
    }
}

