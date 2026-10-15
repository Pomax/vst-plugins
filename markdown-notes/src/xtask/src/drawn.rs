//! What is drawn near a piece of text in a picture of a window.
//!
//! The text finder says where words are. The things beside them that carry
//! no word, a button with a glyph on it, a task's box, a colour swatch, a
//! dialog's close cross, and the empty space a line has beside and below it,
//! are found from the picture itself: along the words' row, or down their
//! columns, looking for where something is drawn and where nothing is.
//!
//! Something is drawn in a column where the column's pixels vary down the
//! band of rows the words are on: letters, a glyph, the outline of a box,
//! the edge of a swatch all vary. A fill, a frame line, a separator and a
//! caret run straight down the band in one colour and vary nowhere, so they
//! are not things, whatever colour they or the ground around them are.

use image::RgbaImage;

/// A box in a picture, in its pixels, from the top left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Place {
    fn right(&self) -> i32 {
        self.x + self.width
    }

    fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub fn middle(&self) -> (i32, i32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// How far two pixels' colours have to be apart to be two colours: the three
/// channels' differences added up.
const APART: i32 = 60;

/// How many rows above and below the words' own rows the band of rows looked
/// at takes in, so the top and bottom of a box or a swatch drawn round the
/// words' height are in it, and the frame of the tab or button the words
/// are on, further out, is not.
const BAND: i32 = 3;

/// How many empty columns or rows in a row end the words: the gaps inside a
/// word are narrower, and the finder's box can be off by a little.
const CLEAR: i32 = 4;

/// How many empty columns may sit inside one thing drawn: the gaps between
/// three dots, or between the strokes of a glyph.
const WITHIN: i32 = 3;

/// How wide a thing has to be to be the thing: narrower is a line, or the
/// smoothing of one.
const THING: i32 = 5;

fn apart(a: [u8; 4], b: [u8; 4]) -> bool {
    let distance: i32 = (0..3)
        .map(|channel| (a[channel] as i32 - b[channel] as i32).abs())
        .sum();
    distance > APART
}

/// Whether column `x` is drawn in: its pixels vary between rows `y0` and `y1`.
fn column_varies(picture: &RgbaImage, x: i32, y0: i32, y1: i32) -> bool {
    if x < 0 || x >= picture.width() as i32 {
        return false;
    }
    let mut rows = (y0.max(0)..y1.min(picture.height() as i32)).map(|y| picture.get_pixel(x as u32, y as u32).0);
    let Some(first) = rows.next() else {
        return false;
    };
    rows.any(|pixel| apart(pixel, first))
}

/// Whether row `y` is drawn in: its pixels vary between columns `x0` and `x1`.
fn row_varies(picture: &RgbaImage, y: i32, x0: i32, x1: i32) -> bool {
    if y < 0 || y >= picture.height() as i32 {
        return false;
    }
    let mut columns = (x0.max(0)..x1.min(picture.width() as i32)).map(|x| picture.get_pixel(x as u32, y as u32).0);
    let Some(first) = columns.next() else {
        return false;
    };
    columns.any(|pixel| apart(pixel, first))
}

/// The rows the words really take up: from the middle of their box up and
/// down, every row drawn in across the box's columns. The finder's box can
/// be taller than the letters, and a band taken from it would reach the
/// frame of the tab or button the words are on.
fn rows_of(picture: &RgbaImage, words: Place) -> (i32, i32) {
    let (x0, x1) = (words.x, words.right());
    let middle = words.middle().1;
    let mut top = middle;
    while top > 0 && row_varies(picture, top - 1, x0, x1) {
        top -= 1;
    }
    let mut bottom = middle;
    while bottom + 1 < picture.height() as i32 && row_varies(picture, bottom + 1, x0, x1) {
        bottom += 1;
    }
    (top, bottom + 1)
}

/// The columns from the words' edge outward, past the words themselves: the
/// first `CLEAR` empty columns in a row end them. The column the words end
/// in, and then every column beyond the gap with whether it is drawn in.
fn columns_beyond(picture: &RgbaImage, words: Place, side: Side) -> (i32, Vec<(i32, bool)>) {
    let (top, bottom) = rows_of(picture, words);
    let (y0, y1) = (top - BAND, bottom + BAND);
    let (mut first, step) = match side {
        Side::Right => (words.right() - 1, 1),
        Side::Left => (words.x, -1),
    };
    // A box wider than the letters starts outside them: step in to the last
    // column drawn in, which is where they end.
    let inside = (words.x..words.right()).contains(&first);
    while inside && (words.x..words.right()).contains(&(first - step)) && !column_varies(picture, first, y0, y1) {
        first -= step;
    }
    let width = picture.width() as i32;
    let mut x = first;
    let mut clear = 0;
    let mut end = first;
    let mut beyond = Vec::new();
    while x >= 0 && x < width {
        let drawn = column_varies(picture, x, y0, y1);
        if clear < CLEAR {
            if drawn {
                clear = 0;
                end = x;
            } else {
                clear += 1;
            }
        } else {
            beyond.push((x, drawn));
        }
        x += step;
    }
    (end, beyond)
}

/// How many empty columns in a row end a label: more than the space between
/// two words of one label, fewer than the gap between one control and the
/// next.
const LABEL_GAP: i32 = 10;

/// The words widened to the rest of the label they are part of: the columns
/// drawn on the line they were read on, toward `side`, until a gap wider
/// than a word's space. A button's label is read as several words, and the
/// thing beside the button is past all of them. The line's own edge bounds
/// it; with no line, the words are the label.
pub fn label_of(picture: &RgbaImage, words: Place, line: Option<Place>, side: Side) -> Place {
    let Some(line) = line else {
        return words;
    };
    let (top, bottom) = rows_of(picture, words);
    let (y0, y1) = (top - BAND, bottom + BAND);
    let (mut x, step) = match side {
        Side::Right => (words.right(), 1),
        Side::Left => (words.x - 1, -1),
    };
    let mut end = None;
    let mut clear = 0;
    while x >= line.x && x < line.right() {
        if column_varies(picture, x, y0, y1) {
            end = Some(x);
            clear = 0;
        } else {
            clear += 1;
            if clear >= LABEL_GAP {
                break;
            }
        }
        x += step;
    }
    match (end, side) {
        (Some(end), Side::Right) => Place { width: end + 1 - words.x, ..words },
        (Some(end), Side::Left) => Place { x: end, width: words.right() - end, ..words },
        (None, _) => words,
    }
}

/// How far a pixel's strongest channel is above its weakest for the pixel to
/// be the drop mark's colour: the accent, a medium blue, where the frames
/// and the strip are greys and a tab's fill and the ghost of one are pale.
const MARK_COLOURED: i32 = 70;

/// Whether column `x` is a line of the mark's colour down rows `y0` to `y1`:
/// every pixel of it that strongly coloured.
fn column_marked(picture: &RgbaImage, x: i32, y0: i32, y1: i32) -> bool {
    if x < 0 || x >= picture.width() as i32 {
        return false;
    }
    let rows = y0.max(0)..y1.min(picture.height() as i32);
    if rows.is_empty() {
        return false;
    }
    rows.map(|y| picture.get_pixel(x as u32, y as u32).0).all(|pixel| {
        let channels = [pixel[0] as i32, pixel[1] as i32, pixel[2] as i32];
        let (most, least) = (channels.iter().max(), channels.iter().min());
        most.zip(least).is_some_and(|(most, least)| most - least > MARK_COLOURED)
    })
}

/// Where the drop mark is in a picture, within columns `x0` to `x1`, on the
/// rows of the words the span is measured from. Nothing when there is none.
pub fn mark_in(picture: &RgbaImage, words: Place, x0: i32, x1: i32) -> Option<(i32, i32)> {
    let (top, bottom) = rows_of(picture, words);
    (x0.max(0)..x1.min(picture.width() as i32))
        .find(|x| column_marked(picture, *x, top, bottom))
        .map(|x| (x, words.middle().1))
}

/// The middle column of the first thing drawn beside the words on their row,
/// past the words themselves and past any line, such as the frame of the
/// button the words are on. Nothing when nothing is drawn there.
pub fn beside(picture: &RgbaImage, words: Place, side: Side) -> Option<i32> {
    let mut run: Option<(i32, i32, i32)> = None;
    for (x, drawn) in columns_beyond(picture, words, side).1 {
        match (&mut run, drawn) {
            (None, true) => run = Some((x, x, 0)),
            (None, false) => {}
            (Some((_, end, gap)), true) => {
                *end = x;
                *gap = 0;
            }
            (Some((start, end, gap)), false) => {
                *gap += 1;
                if *gap > WITHIN {
                    if (*end - *start).abs() + 1 >= THING {
                        return Some((*start + *end) / 2);
                    }
                    run = None;
                }
            }
        }
    }
    match run {
        Some((start, end, _)) if (end - start).abs() + 1 >= THING => Some((start + end) / 2),
        _ => None,
    }
}

/// The middle of the empty space right of a line on its row: between the
/// line's end and the next thing drawn there, or the picture's edge.
pub fn after(picture: &RgbaImage, line: Place) -> i32 {
    let (end, beyond) = columns_beyond(picture, line, Side::Right);
    let next = beyond
        .into_iter()
        .find(|(_, drawn)| *drawn)
        .map(|(x, _)| x)
        .unwrap_or(picture.width() as i32);
    (end + 1 + next) / 2
}

/// The middle of the empty space under a line, in its columns: between the
/// line's bottom and the next thing drawn under it, or the picture's bottom.
/// The line's bottom is found from its middle row down, past every row drawn
/// in, since the finder's box can be off by a little.
pub fn below(picture: &RgbaImage, line: Place) -> i32 {
    let height = picture.height() as i32;
    let (x0, x1) = (line.x, line.right());
    let mut clear = 0;
    let mut end = line.middle().1;
    let mut next = height;
    for y in line.middle().1..height {
        let drawn = row_varies(picture, y, x0, x1);
        if clear < CLEAR {
            if drawn {
                clear = 0;
                end = y;
            } else {
                clear += 1;
            }
            continue;
        }
        if drawn {
            next = y;
            break;
        }
    }
    (end + 1 + next) / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAPER: [u8; 4] = [245, 245, 245, 255];
    const INK: [u8; 4] = [30, 30, 30, 255];

    fn paper(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_pixel(width, height, image::Rgba(PAPER))
    }

    fn paint(picture: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, colour: [u8; 4]) {
        for y in y0..y1 {
            for x in x0..x1 {
                picture.put_pixel(x as u32, y as u32, image::Rgba(colour));
            }
        }
    }

    /// Words at 20..60 across and 20..32 down: two letters' worth of ink
    /// with a gap of two columns between them, as letters have.
    fn words() -> Place {
        Place { x: 20, y: 20, width: 40, height: 12 }
    }

    fn paint_words(picture: &mut RgbaImage) {
        paint_words_at(picture, 20);
    }

    fn paint_words_at(picture: &mut RgbaImage, y: i32) {
        paint(picture, 20, y, 39, y + 12, INK);
        paint(picture, 41, y, 60, y + 12, INK);
    }

    #[test]
    fn the_thing_beside_the_words_is_past_their_frame() {
        let mut picture = paper(200, 60);
        paint_words(&mut picture);
        // The frame of the button the words are on: a line one pixel wide,
        // taller than the band.
        paint(&mut picture, 66, 4, 67, 56, INK);
        // A glyph fourteen pixels wide, after a gap.
        paint(&mut picture, 74, 22, 88, 30, INK);

        assert_eq!(beside(&picture, words(), Side::Right), Some(80));
        assert_eq!(beside(&picture, words(), Side::Left), None);
    }

    #[test]
    fn the_words_box_may_be_off_by_a_little() {
        let mut picture = paper(200, 60);
        paint_words(&mut picture);
        paint(&mut picture, 74, 22, 88, 30, INK);

        let short = Place { x: 20, y: 22, width: 30, height: 10 };
        assert_eq!(beside(&picture, short, Side::Right), Some(80));
        let long = Place { x: 18, y: 18, width: 46, height: 16 };
        assert_eq!(beside(&picture, long, Side::Right), Some(80));
    }

    #[test]
    fn three_dots_are_one_thing() {
        let mut picture = paper(200, 60);
        paint_words(&mut picture);
        for dot in [70, 74, 78] {
            paint(&mut picture, dot, 25, dot + 2, 27, INK);
        }
        assert_eq!(beside(&picture, words(), Side::Right), Some(74));
    }

    #[test]
    fn a_box_left_of_the_words_is_found_by_its_outline() {
        let mut picture = paper(200, 60);
        paint_words(&mut picture);
        // An empty square, outline only, from 4 to 16, a little taller than
        // the words.
        paint(&mut picture, 4, 19, 16, 20, INK);
        paint(&mut picture, 4, 32, 16, 33, INK);
        paint(&mut picture, 4, 19, 5, 33, INK);
        paint(&mut picture, 15, 19, 16, 33, INK);
        assert_eq!(beside(&picture, words(), Side::Left), Some(9));
    }

    #[test]
    fn a_fill_beyond_the_words_is_not_a_thing() {
        // The words on a tab, which is filled, with the strip beyond the
        // tab's edge another colour, and a glyph past that.
        let mut picture = paper(200, 60);
        paint(&mut picture, 0, 10, 100, 42, [160, 200, 240, 255]);
        paint_words(&mut picture);
        paint(&mut picture, 120, 22, 130, 30, INK);
        assert_eq!(beside(&picture, words(), Side::Right), Some(124));
    }

    #[test]
    fn the_tabs_own_frame_is_outside_the_band_however_loose_the_box() {
        // The words on a tab drawn with a frame five rows above and below
        // the letters, and a glyph past the tab. The finder's box is looser
        // than the letters by four rows each way.
        let mut picture = paper(200, 60);
        paint(&mut picture, 0, 14, 100, 15, INK);
        paint(&mut picture, 0, 37, 100, 38, INK);
        paint(&mut picture, 99, 14, 100, 38, INK);
        paint_words(&mut picture);
        paint(&mut picture, 120, 22, 130, 30, INK);

        let loose = Place { x: 18, y: 16, width: 44, height: 20 };
        assert_eq!(beside(&picture, loose, Side::Right), Some(124));
        assert_eq!(after(&picture, loose), (60 + 120) / 2);
    }

    #[test]
    fn the_space_after_a_line_reaches_the_next_thing_or_the_edge() {
        let mut picture = paper(200, 60);
        paint_words(&mut picture);
        assert_eq!(after(&picture, words()), (60 + 200) / 2);

        // A caret, or a separator: a line the height of the band and more,
        // which is not a thing.
        paint(&mut picture, 64, 4, 65, 56, INK);
        assert_eq!(after(&picture, words()), (60 + 200) / 2);

        // The next word.
        paint(&mut picture, 120, 20, 150, 32, INK);
        assert_eq!(after(&picture, words()), (60 + 120) / 2);
    }

    #[test]
    fn the_space_below_a_line_reaches_the_next_line_or_the_bottom() {
        let mut picture = paper(200, 100);
        paint_words(&mut picture);
        assert_eq!(below(&picture, words()), (32 + 100) / 2);

        paint_words_at(&mut picture, 50);
        assert_eq!(below(&picture, words()), (32 + 50) / 2);

        // The box off by a few rows either way finds the same space.
        let high = Place { x: 20, y: 16, width: 40, height: 12 };
        assert_eq!(below(&picture, high), (32 + 50) / 2);
        let low = Place { x: 20, y: 24, width: 40, height: 12 };
        assert_eq!(below(&picture, low), (32 + 50) / 2);
    }

    #[test]
    fn a_swatch_is_found_by_its_edges() {
        let mut picture = paper(200, 60);
        paint_words(&mut picture);
        paint(&mut picture, 2, 18, 14, 34, [200, 40, 160, 255]);
        assert_eq!(beside(&picture, words(), Side::Left), Some(7));
    }

    #[test]
    fn a_label_takes_in_the_words_after_it_a_space_apart() {
        let mut picture = paper(300, 60);
        paint_words(&mut picture);
        paint(&mut picture, 65, 20, 90, 32, INK);
        paint(&mut picture, 95, 20, 130, 32, INK);
        paint(&mut picture, 136, 4, 137, 56, INK);
        paint(&mut picture, 150, 22, 164, 30, INK);
        let line = Some(Place { x: 20, y: 20, width: 110, height: 12 });

        let label = label_of(&picture, words(), line, Side::Right);
        assert_eq!((label.x, label.right()), (20, 130));
        assert_eq!(beside(&picture, label, Side::Right), Some(156));
        assert_eq!(beside(&picture, words(), Side::Right), Some(77));
    }

    #[test]
    fn a_label_stops_at_the_lines_edge_and_at_a_wide_gap() {
        let mut picture = paper(300, 60);
        paint_words(&mut picture);
        paint(&mut picture, 85, 20, 120, 32, INK);
        let line = Some(Place { x: 20, y: 20, width: 100, height: 12 });
        let label = label_of(&picture, words(), line, Side::Right);
        assert_eq!((label.x, label.right()), (20, 60));

        let own = Some(words());
        assert_eq!(label_of(&picture, words(), own, Side::Right), words());
        assert_eq!(label_of(&picture, words(), None, Side::Left), words());
    }

    #[test]
    fn a_label_widens_to_the_left_as_well() {
        let mut picture = paper(300, 60);
        paint(&mut picture, 66, 20, 82, 32, INK);
        paint(&mut picture, 84, 20, 100, 32, INK);
        paint_words(&mut picture);
        let right_word = Place { x: 66, y: 20, width: 34, height: 12 };
        let line = Some(Place { x: 20, y: 20, width: 80, height: 12 });
        let label = label_of(&picture, right_word, line, Side::Left);
        assert_eq!((label.x, label.right()), (20, 100));
    }

    #[test]
    fn the_drop_mark_is_a_coloured_column_on_the_words_rows() {
        let mut picture = paper(200, 60);
        paint_words(&mut picture);
        paint(&mut picture, 70, 10, 71, 44, INK);
        assert_eq!(mark_in(&picture, words(), 60, 200), None);
        paint(&mut picture, 80, 10, 82, 44, [40, 110, 220, 255]);
        assert_eq!(mark_in(&picture, words(), 60, 200), Some((80, 26)));
        assert_eq!(mark_in(&picture, words(), 0, 20), None);
        paint(&mut picture, 10, 24, 12, 28, [40, 110, 220, 255]);
        assert_eq!(mark_in(&picture, words(), 0, 20), None);
    }
}
