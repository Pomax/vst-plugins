//! Dragging a section has to look like dragging a section.
//!
//! The one being dragged travels with the pointer and the one it came from
//! stays in place, both faint enough to read the strip through. Faint is the
//! point, so these measure it rather than looking for a colour: a pixel of
//! either must sit between the strip's background and the colour a solid
//! section is drawn in.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use markdown_notes_core::{Editor, Rgba, Theme};

/// Three sections: two to drag between, and a third so there is a gap that is
/// neither end of the strip.
const SAMPLE: &str =
    "# First\n\nthe first part\n\n# Second\n\nthe second part\n\n# Third\n\nthe third part";

/// Wide enough for the whole toolbar, and for a stretch of empty strip to the
/// right of the buttons.
const SIZE: (f32, f32) = (900.0, 400.0);

/// The titles the sections take from their headings, which is what the strip
/// shows and so how they are found.
const FIRST: &str = "First";
const SECOND: &str = "Second";
const THIRD: &str = "Third";

fn harness() -> Harness<'static> {
    with_editor().0
}

/// The same window, with a handle on the document it is showing, for the tests
/// that care where the sections ended up rather than what was drawn.
fn with_editor() -> (Harness<'static>, Arc<Mutex<Editor>>) {
    let editor = Arc::new(Mutex::new(Editor::new()));
    let kept = Arc::clone(&editor);
    if let Ok(mut e) = editor.lock() {
        // A whole document, which is one section per heading.
        e.set_document_text(SAMPLE);
        e.theme = Theme::Light;
        // The mark for where a section would land is drawn in the highlight colour.
        // Set to one nothing else in the window uses, it can be found exactly.
        e.colours.light.highlight = Rgba::rgba(255, 0, 255, 255);
        e.set_caret(0);
    }
    let mut state = markdown_notes_plugin::gui::TestGui::new(editor, false);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(SIZE.0, SIZE.1))
        .wgpu()
        .build_ui(move |ui| markdown_notes_plugin::gui::draw_frame_for_test(ui, &mut state));

    markdown_notes_plugin::gui::TestGui::install_fonts(&harness.ctx);
    harness.run_steps(3);
    (harness, kept)
}

fn section(harness: &Harness<'static>, title: &str) -> egui::Rect {
    harness.get_by_label(title).rect()
}

/// Where the section being carried is dragged to: past the last button, so it lies
/// over bare strip and can be measured against it.
fn empty_strip(harness: &Harness<'static>) -> egui::Pos2 {
    let plus = section(harness, "+");
    let width = section(harness, FIRST).width();
    egui::pos2(plus.right() + width / 2.0 + 10.0, plus.center().y)
}

/// Press on the first section and move the pointer out past the last button,
/// without letting go.
fn drag_without_letting_go(harness: &mut Harness<'static>) -> egui::Pos2 {
    let to = empty_strip(harness);
    drag_from_to(harness, FIRST, to);
    to
}

/// Press on the section with this title and move the pointer to `to`, holding the
/// button down.
fn drag_from_to(harness: &mut Harness<'static>, title: &str, to: egui::Pos2) {
    let from = section(harness, title).center();

    harness.input_mut().events.push(egui::Event::PointerMoved(from));
    harness.run_steps(2);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(2);
    move_to(harness, to);
}

/// Move the pointer, with the button still down.
fn move_to(harness: &mut Harness<'static>, to: egui::Pos2) {
    harness.input_mut().events.push(egui::Event::PointerMoved(to));
    harness.run_steps(3);
}

/// Let go where the pointer is.
fn let_go(harness: &mut Harness<'static>, at: egui::Pos2) {
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(3);
}

fn pixel(image: &image::RgbaImage, at: egui::Pos2) -> [u8; 4] {
    image.get_pixel(at.x as u32, at.y as u32).0
}

/// A point inside a section that the label is not written on, so what is sampled
/// there is the section's own colour.
fn fill_point(centre: egui::Pos2, width: f32) -> egui::Pos2 {
    egui::pos2(centre.x + width * 0.38, centre.y)
}

/// The colour a section of this kind is drawn in when nothing is being dragged.
fn solid_section(harness: &Harness<'static>, image: &image::RgbaImage, title: &str) -> [u8; 4] {
    let rect = section(harness, title);
    pixel(image, fill_point(rect.center(), rect.width()))
}

/// How far apart two colours are, summed over the channels.
fn apart(a: [u8; 4], b: [u8; 4]) -> i32 {
    (0..3).map(|c| (a[c] as i32 - b[c] as i32).abs()).sum()
}

/// A colour is see-through over `background` if it is neither the background
/// nor the `solid` thing drawn on it, but lies between them.
fn assert_see_through(what: &str, drawn: [u8; 4], solid: [u8; 4], background: [u8; 4]) {
    assert!(
        apart(drawn, background) > 6,
        "{what} is not drawn at all: it matches the strip behind it, {drawn:?}"
    );
    assert!(
        apart(drawn, solid) > 6,
        "{what} is drawn solid: {drawn:?} against a solid section's {solid:?}"
    );
    for c in 0..3 {
        let (low, high) = (
            solid[c].min(background[c]) as i32,
            solid[c].max(background[c]) as i32,
        );
        assert!(
            (drawn[c] as i32) >= low - 2 && (drawn[c] as i32) <= high + 2,
            "{what} is {drawn:?}, which is not between the strip's {background:?} \
             and a solid section's {solid:?}"
        );
    }
}

#[test]
fn a_section_being_dragged_is_drawn_under_the_pointer() {
    let mut harness = harness();
    let before = harness.render().expect("rendering failed");
    let width = section(&harness, FIRST).width();
    let background = pixel(&before, empty_strip(&harness));
    // The section on the pointer is drawn the way an ordinary one is, which is
    // the one that is not the active one.
    let solid = solid_section(&harness, &before, SECOND);

    let pointer = drag_without_letting_go(&mut harness);
    let during = harness.render().expect("rendering failed");

    assert_see_through(
        "the section on the pointer",
        pixel(&during, fill_point(pointer, width)),
        solid,
        background,
    );
}

#[test]
fn the_section_it_came_from_stays_in_place_and_fades() {
    let mut harness = harness();
    let before = harness.render().expect("rendering failed");
    let rect = section(&harness, FIRST);
    let where_it_was = fill_point(rect.center(), rect.width());
    let background = pixel(&before, empty_strip(&harness));
    let solid = pixel(&before, where_it_was);

    drag_without_letting_go(&mut harness);
    let during = harness.render().expect("rendering failed");

    assert_see_through(
        "the section left behind",
        pixel(&during, where_it_was),
        solid,
        background,
    );
}

/// The middle of the mark, across.
fn mark_at(image: &image::RgbaImage) -> f32 {
    let columns = mark(image).0;
    let left = *columns.iter().min().expect("no mark is drawn") as f32;
    let right = *columns.iter().max().expect("no mark is drawn") as f32;
    (left + right) / 2.0
}

/// Where the mark is drawn: pixels of the highlight colour, as columns and
/// rows.
fn mark(image: &image::RgbaImage) -> (Vec<u32>, Vec<u32>) {
    let found: Vec<(u32, u32)> = image
        .enumerate_pixels()
        .filter(|(_, _, p)| p.0[3] == 255 && p.0[0] > 200 && p.0[1] < 80 && p.0[2] > 200)
        .map(|(x, y, _)| (x, y))
        .collect();
    (
        found.iter().map(|(x, _)| *x).collect(),
        found.iter().map(|(_, y)| *y).collect(),
    )
}

/// The mark stands clear of the section it is beside, and runs the whole height of
/// the strip rather than the height of a section.
#[test]
fn the_mark_for_where_it_lands_touches_nothing() {
    let mut harness = harness();
    let before = harness.render().expect("rendering failed");
    assert!(
        mark(&before).0.is_empty(),
        "the mark is drawn when nothing is being dragged"
    );

    // Dragged out past the last section, so it would land at the end.
    let last = section(&harness, SECOND);
    drag_without_letting_go(&mut harness);
    let during = harness.render().expect("rendering failed");

    let (columns, rows) = mark(&during);
    assert!(!columns.is_empty(), "no mark is drawn during a drag");

    let left = *columns.iter().min().expect("no columns") as f32;
    let right = *columns.iter().max().expect("no columns") as f32;
    assert!(
        left > last.right() + 1.0,
        "the mark starts at {left}, and the section it follows ends at {}",
        last.right()
    );
    assert!(
        right - left <= 3.0,
        "the mark is {} wide, so it is not a line",
        right - left
    );

    // Taller than a section, and no taller than the strip the sections sit in: this is
    // a mark in the section bar, not a line down the window.
    let top = *rows.iter().min().expect("no rows") as f32;
    let bottom = *rows.iter().max().expect("no rows") as f32;
    assert!(
        top < last.top() && bottom > last.bottom(),
        "the mark covers rows {top}..{bottom}, and the section beside it covers \
         {}..{}: it should be taller than the section, not shorter",
        last.top(),
        last.bottom()
    );
    let strip = 26.0;
    assert!(
        bottom - top <= strip + 1.0,
        "the mark is {} tall, and the strip it is in is {strip}",
        bottom - top
    );
}

/// Between two sections the mark goes in the middle of the gap, the same distance
/// from each.
#[test]
fn the_mark_sits_midway_between_two_sections() {
    let mut harness = harness();
    let second = section(&harness, SECOND);
    let third = section(&harness, THIRD);

    // The first section, dragged onto the second: it would land between the second
    // and the third.
    drag_from_to(&mut harness, FIRST, second.center());
    let during = harness.render().expect("rendering failed");

    let middle = (second.right() + third.left()) / 2.0;
    let at = mark_at(&during);
    assert!(
        (at - middle).abs() <= 1.5,
        "the mark is at {at}, and the middle of the gap between the sections \
         ending at {} and starting at {} is {middle}",
        second.right(),
        third.left()
    );
}

/// Before the first section there is no gap, so the mark keeps a little distance
/// from it rather than sitting on its edge.
#[test]
fn the_mark_stands_off_the_first_section() {
    let mut harness = harness();
    let first = section(&harness, FIRST);

    // The last section, dragged onto the first: it would land in front of it.
    drag_from_to(&mut harness, THIRD, first.center());
    let during = harness.render().expect("rendering failed");

    let at = mark_at(&during);
    assert!(
        at < first.left() - 1.0 && at > first.left() - 8.0,
        "the mark is at {at}, and the first section starts at {}: it should be \
         just clear of it",
        first.left()
    );
}

/// The section on the pointer goes where the pointer goes.
#[test]
fn the_carried_section_follows_the_pointer() {
    let mut harness = harness();
    let before = harness.render().expect("rendering failed");
    let bare = empty_strip(&harness);
    let width = section(&harness, FIRST).width();

    drag_from_to(&mut harness, FIRST, bare);
    let here = harness.render().expect("rendering failed");
    let first_stop = pixel(&here, fill_point(bare, width));

    let further = egui::pos2(bare.x + width, bare.y);
    move_to(&mut harness, further);
    let there = harness.render().expect("rendering failed");

    assert!(
        apart(first_stop, pixel(&before, fill_point(bare, width))) > 6,
        "the section was not drawn at the first place the pointer stopped"
    );
    assert!(
        apart(pixel(&there, fill_point(bare, width)), first_stop) > 6,
        "the section is still drawn where the pointer used to be"
    );
    assert!(
        apart(
            pixel(&there, fill_point(further, width)),
            pixel(&before, fill_point(further, width))
        ) > 6,
        "the section did not follow the pointer to where it moved"
    );
}

/// The pointer says what it is doing, the way it does over anything draggable.
#[test]
fn the_pointer_says_it_is_carrying_something() {
    let mut harness = harness();
    drag_without_letting_go(&mut harness);
    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
}

/// And the section lands where the mark said it would.
#[test]
fn a_dropped_section_lands_where_the_mark_was() {
    let (mut harness, editor) = with_editor();
    let second = section(&harness, SECOND);

    drag_from_to(&mut harness, FIRST, second.center());
    let during = harness.render().expect("rendering failed");
    let third = section(&harness, THIRD);
    let at = mark_at(&during);
    assert!(
        at < third.left(),
        "the mark is at {at}, past the section it would land in front of"
    );

    let_go(&mut harness, second.center());

    let document = editor.lock().expect("the editor is locked").document_text();
    let order: Vec<&str> = document
        .lines()
        .filter(|line| line.starts_with("# "))
        .collect();
    assert_eq!(
        order,
        ["# Second", "# First", "# Third"],
        "the section did not land between the two sections the mark was between"
    );
}

#[test]
fn the_strip_goes_back_to_normal_when_the_drag_ends() {
    let mut harness = harness();
    let before = harness.render().expect("rendering failed");
    let solid = solid_section(&harness, &before, FIRST);

    let pointer = drag_without_letting_go(&mut harness);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(3);
    // Away from the strip, so nothing is left hovered.
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(450.0, 300.0)));
    harness.run_steps(3);
    let after = harness.render().expect("rendering failed");

    // The section moved to the end of the strip, so it is looked up again rather
    // than sampled where it used to be.
    let landed = solid_section(&harness, &after, FIRST);
    assert!(
        apart(landed, solid) <= 6,
        "the section is still faded after the drag ended: {landed:?} against {solid:?}"
    );
    assert!(
        apart(pixel(&after, pointer), pixel(&before, pointer)) <= 6,
        "something is still drawn where the drag ended"
    );
}
