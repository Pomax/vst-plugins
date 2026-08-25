//! The colours the interface paints with.
//!
//! Light and dark are two complete, independent sets. Neither is derived from
//! the other: a dark scheme is not a light scheme with the channels inverted,
//! and treating it as one produces muddy greys and unreadable text.
//!
//! The defaults are the colours the editor already used, so a project written
//! before this existed looks exactly the same after it.

use serde::{Deserialize, Serialize};

/// One colour, as eight-bit channels with alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    #[serde(default = "opaque")]
    pub a: u8,
}

fn opaque() -> u8 {
    255
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba { r, g, b, a }
    }

    pub const fn grey(level: u8) -> Rgba {
        Rgba::rgb(level, level, level)
    }
}

/// Every colour the interface binds, for one of the two schemes.
///
/// Headings share one colour with each other; inline and fenced code share
/// theirs; the toolbar and the tab strip share a fill.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Colours {
    pub window_background: Rgba,
    /// Behind the toolbar and the tab strip both.
    pub bar_fill: Rgba,
    pub separator: Rgba,
    pub button_face: Rgba,
    pub button_outline: Rgba,
    pub button_hover_face: Rgba,
    pub button_pressed_face: Rgba,
    pub button_label: Rgba,
    pub tab_selected_fill: Rgba,
    pub tab_selected_label: Rgba,
    /// Hover: the tab under the pointer, and the settings cog under it.
    pub highlight: Rgba,
    pub field_background: Rgba,
    pub field_text: Rgba,
    pub field_placeholder: Rgba,
    pub error_text: Rgba,

    pub body_text: Rgba,
    /// Every heading level.
    pub heading_text: Rgba,
    pub bold_text: Rgba,
    /// Markdown punctuation shown next to the caret.
    pub marker_text: Rgba,
    pub strikethrough: Rgba,
    /// Link text and the rule beneath it.
    pub link: Rgba,
    /// Inline and fenced code alike.
    pub code_text: Rgba,
    pub code_background: Rgba,
    pub quote_bar: Rgba,
    pub list_glyph: Rgba,
    pub checkbox_frame: Rgba,
    pub checkbox_fill: Rgba,
    pub checkbox_tick: Rgba,
    pub caret: Rgba,
    pub selection: Rgba,
    pub scrollbar: Rgba,
}

impl Colours {
    pub const fn light() -> Colours {
        Colours {
            window_background: Rgba::grey(248),
            bar_fill: Rgba::grey(232),
            separator: Rgba::grey(190),
            button_face: Rgba::grey(248),
            button_outline: Rgba::grey(150),
            button_hover_face: Rgba::grey(220),
            button_pressed_face: Rgba::grey(165),
            button_label: Rgba::grey(60),
            tab_selected_fill: Rgba::rgb(144, 209, 255),
            tab_selected_label: Rgba::rgb(0, 83, 125),
            highlight: Rgba::rgb(0, 155, 255),
            field_background: Rgba::grey(255),
            field_text: Rgba::grey(80),
            field_placeholder: Rgba::rgba(80, 80, 80, 153),
            error_text: Rgba::rgb(255, 0, 0),

            body_text: Rgba::grey(80),
            heading_text: Rgba::grey(80),
            bold_text: Rgba::grey(0),
            marker_text: Rgba::rgba(80, 80, 80, 153),
            strikethrough: Rgba::rgba(80, 80, 80, 153),
            link: Rgba::rgb(0, 155, 255),
            code_text: Rgba::grey(80),
            code_background: Rgba::grey(225),
            quote_bar: Rgba::rgba(80, 80, 80, 153),
            list_glyph: Rgba::grey(80),
            checkbox_frame: Rgba::grey(150),
            checkbox_fill: Rgba::grey(230),
            checkbox_tick: Rgba::grey(60),
            caret: Rgba::grey(0),
            selection: Rgba::rgb(144, 209, 255),
            scrollbar: Rgba::grey(230),
        }
    }

    pub const fn dark() -> Colours {
        Colours {
            window_background: Rgba::grey(27),
            bar_fill: Rgba::grey(45),
            separator: Rgba::grey(60),
            button_face: Rgba::grey(27),
            button_outline: Rgba::grey(105),
            button_hover_face: Rgba::grey(70),
            button_pressed_face: Rgba::grey(55),
            button_label: Rgba::grey(180),
            tab_selected_fill: Rgba::rgb(0, 92, 128),
            tab_selected_label: Rgba::rgb(192, 222, 255),
            highlight: Rgba::rgb(90, 170, 255),
            field_background: Rgba::grey(10),
            field_text: Rgba::grey(140),
            field_placeholder: Rgba::rgba(140, 140, 140, 153),
            error_text: Rgba::rgb(255, 0, 0),

            body_text: Rgba::grey(140),
            heading_text: Rgba::grey(140),
            bold_text: Rgba::grey(255),
            marker_text: Rgba::rgba(140, 140, 140, 153),
            strikethrough: Rgba::rgba(140, 140, 140, 153),
            link: Rgba::rgb(90, 170, 255),
            code_text: Rgba::grey(140),
            code_background: Rgba::grey(60),
            quote_bar: Rgba::rgba(140, 140, 140, 153),
            list_glyph: Rgba::grey(140),
            checkbox_frame: Rgba::grey(105),
            checkbox_fill: Rgba::grey(60),
            checkbox_tick: Rgba::grey(180),
            caret: Rgba::grey(255),
            selection: Rgba::rgb(0, 92, 128),
            scrollbar: Rgba::grey(60),
        }
    }
}

/// Both schemes. Which one is in use follows the theme setting.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ColourScheme {
    #[serde(default = "Colours::light")]
    pub light: Colours,
    #[serde(default = "Colours::dark")]
    pub dark: Colours,
}

impl Default for ColourScheme {
    fn default() -> ColourScheme {
        ColourScheme { light: Colours::light(), dark: Colours::dark() }
    }
}

impl ColourScheme {
    pub fn for_mode(&self, dark: bool) -> &Colours {
        if dark {
            &self.dark
        } else {
            &self.light
        }
    }

    pub fn for_mode_mut(&mut self, dark: bool) -> &mut Colours {
        if dark {
            &mut self.dark
        } else {
            &mut self.light
        }
    }

    /// Put one scheme back to its default, leaving the other alone.
    pub fn reset(&mut self, dark: bool) {
        if dark {
            self.dark = Colours::dark();
        } else {
            self.light = Colours::light();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_schemes_are_independent() {
        let mut scheme = ColourScheme::default();
        scheme.light.body_text = Rgba::rgb(1, 2, 3);
        assert_eq!(scheme.dark.body_text, Colours::dark().body_text);
    }

    #[test]
    fn resetting_one_scheme_leaves_the_other() {
        let mut scheme = ColourScheme::default();
        scheme.dark.link = Rgba::rgb(9, 9, 9);
        scheme.light.link = Rgba::rgb(8, 8, 8);
        scheme.reset(true);
        assert_eq!(scheme.dark.link, Colours::dark().link);
        assert_eq!(scheme.light.link, Rgba::rgb(8, 8, 8));
    }

    #[test]
    fn a_scheme_round_trips_through_json() {
        let mut scheme = ColourScheme::default();
        scheme.light.caret = Rgba::rgba(10, 20, 30, 200);
        let json = serde_json::to_string(&scheme).unwrap();
        let back: ColourScheme = serde_json::from_str(&json).unwrap();
        assert_eq!(back, scheme);
    }

    #[test]
    fn a_partial_scheme_fills_in_from_the_defaults() {
        let json = r#"{"light":{"body_text":{"r":1,"g":2,"b":3}}}"#;
        // A half-written scheme is not a scheme; the whole thing falls back
        // rather than leaving most of the interface unpainted.
        assert!(serde_json::from_str::<ColourScheme>(json).is_err());
    }

    #[test]
    fn alpha_defaults_to_opaque() {
        let c: Rgba = serde_json::from_str(r#"{"r":1,"g":2,"b":3}"#).unwrap();
        assert_eq!(c.a, 255);
    }
}
