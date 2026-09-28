//! Text override lookup boundary for generated browser/font compatibility data.

use super::WrapMode;

pub(crate) fn lookup_flowchart_markdown_italic_word_delta_em(
    wrap_mode: WrapMode,
    word: &str,
) -> Option<f64> {
    crate::generated::flowchart_text_overrides_11_12_2::
        lookup_flowchart_markdown_italic_word_delta_em(wrap_mode, word)
}

pub(crate) fn lookup_flowchart_markdown_bold_word_delta_em(
    wrap_mode: WrapMode,
    word: &str,
) -> Option<f64> {
    crate::generated::flowchart_text_overrides_11_12_2::lookup_flowchart_markdown_bold_word_delta_em(
        wrap_mode, word,
    )
}

pub(crate) fn lookup_flowchart_markdown_bold_word_extra_delta_em(
    wrap_mode: WrapMode,
    word: &str,
) -> f64 {
    crate::generated::flowchart_text_overrides_11_12_2::
        lookup_flowchart_markdown_bold_word_extra_delta_em(wrap_mode, word)
}

pub(crate) fn lookup_flowchart_markdown_bold_char_extra_delta_em(
    wrap_mode: WrapMode,
    word: &str,
    ch: char,
) -> f64 {
    crate::generated::flowchart_text_overrides_11_12_2::
        lookup_flowchart_markdown_bold_char_extra_delta_em(wrap_mode, word, ch)
}

pub(crate) fn lookup_flowchart_html_width_px(
    font_key: &str,
    font_size_px: f64,
    text: &str,
) -> Option<f64> {
    crate::generated::flowchart_text_overrides_11_12_2::lookup_flowchart_html_width_px(
        font_key,
        font_size_px,
        text,
    )
}

pub(crate) fn lookup_flowchart_svg_bbox_x_px(
    font_key: &str,
    font_size_px: f64,
    text: &str,
) -> Option<(f64, f64)> {
    crate::generated::flowchart_text_overrides_11_12_2::lookup_flowchart_svg_bbox_x_px(
        font_key,
        font_size_px,
        text,
    )
}

/// The sequence diagram's table of measured overrides went with the diagram.
pub(crate) fn lookup_sequence_svg_override_em(_font_key: &str, _text: &str) -> Option<(f64, f64)> {
    None
}
