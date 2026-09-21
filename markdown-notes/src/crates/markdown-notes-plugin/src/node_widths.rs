//! A minimum width for the states of a state diagram.
//!
//! Mermaid 12 draws no state narrower than 120, which is what gives a state
//! named `1` a box to sit in and its edges a side to leave from. merman 0.7.0
//! has no such setting, and sizes a state from its name alone whatever
//! `state.padding` says. So the outline of a state narrower than
//! [`MIN_WIDTH`] is drawn again at that width, around the same centre, before
//! the edges are routed to it.
//!
//! The layout is already done by then. The room comes from `state.nodeSpacing`,
//! which the editor asks merman for with this widening in mind.

use crate::elbows::extent;

/// Narrowest a state is drawn.
pub const MIN_WIDTH: f64 = 120.0;

/// How round the corners of a redrawn state are.
const CORNER: f64 = 5.0;

const STATE: &str = "<g class=\"node statediagram-state\"";
const OUTLINE: &str = "<path d=\"";

/// Redraw every state narrower than [`MIN_WIDTH`] at that width.
pub fn widen_the_states(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;

    while let Some(at) = rest.find(STATE) {
        out.push_str(&rest[..at + STATE.len()]);
        rest = &rest[at + STATE.len()..];

        // The outline is the first group inside the state, and is drawn
        // twice over: once filled, once stroked.
        let mut outline_group = rest.find("</g>").unwrap_or(0);
        while let Some(p) = rest[..outline_group].find(OUTLINE) {
            let from = p + OUTLINE.len();
            let Some(len) = rest[from..].find('"') else {
                break;
            };
            out.push_str(&rest[..from]);
            match extent(&rest[from..from + len]) {
                Some((half_wide, half_tall)) if half_wide * 2.0 < MIN_WIDTH => {
                    out.push_str(&outline(MIN_WIDTH / 2.0, half_tall));
                }
                _ => out.push_str(&rest[from..from + len]),
            }
            rest = &rest[from + len..];
            outline_group -= from + len;
        }
    }
    out.push_str(rest);
    out
}

/// A rectangle with rounded corners around the origin, in the commands
/// [`extent`] can measure.
fn outline(w: f64, h: f64) -> String {
    let r = CORNER.min(w).min(h);
    format!(
        "M{a} {t} L{b} {t} Q{w} {t}, {w} {c} L{w} {e} Q{w} {h}, {b} {h} \
         L{a} {h} Q{l} {h}, {l} {e} L{l} {c} Q{l} {t}, {a} {t} Z",
        a = -w + r,
        b = w - r,
        c = -h + r,
        e = h - r,
        l = -w,
        t = -h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elbows::attribute;

    fn state(d: &str) -> String {
        format!(
            "<svg><g class=\"node statediagram-state\" id=\"s\" transform=\"translate(9, 9)\">\
             <g class=\"outer-path\"><path d=\"{d}\" style=\"x\"/></g><text>1</text></g></svg>"
        )
    }

    fn outline_of(svg: &str) -> &str {
        let at = svg.find(OUTLINE).expect("the state has no outline");
        attribute(&svg[at..], "d").expect("the outline has no path data")
    }

    #[test]
    fn a_narrow_state_is_drawn_at_the_least_width_and_its_own_height() {
        let svg = widen_the_states(&state("M-12 -18 L12 -18 L12 18 L-12 18 Z"));
        let (half_wide, half_tall) = extent(outline_of(&svg)).expect("the outline is unreadable");
        assert_eq!(half_wide * 2.0, MIN_WIDTH);
        assert_eq!(half_tall, 18.0);
        assert!(svg.contains("style=\"x\"/></g><text>1</text>"), "the rest was lost: {svg}");
    }

    #[test]
    fn the_fill_and_the_stroke_of_a_state_are_both_redrawn() {
        let twice = state("M-12 -18 L12 -18 L12 18 L-12 18 Z").replace(
            "style=\"x\"/>",
            "style=\"x\"/><path d=\"M-12 -18 L12 -18 L12 18 L-12 18 Z\" fill=\"none\"/>",
        );
        let svg = widen_the_states(&twice);
        assert!(!svg.contains("M-12 -18"), "an outline was left narrow: {svg}");
        assert_eq!(svg.matches(OUTLINE).count(), 2);
    }

    #[test]
    fn a_path_outside_the_outline_is_left_alone() {
        let with_icon = state("M-12 -18 L12 -18 L12 18 L-12 18 Z")
            .replace("<text>1</text>", "<path d=\"M-3 -3 L3 3\"/><text>1</text>");
        let svg = widen_the_states(&with_icon);
        assert!(svg.contains("<path d=\"M-3 -3 L3 3\"/>"), "{svg}");
    }

    #[test]
    fn a_state_already_wide_enough_is_left_alone() {
        let wide = state("M-60 -18 L60 -18 L60 18 L-60 18 Z");
        assert_eq!(widen_the_states(&wide), wide);
    }

    #[test]
    fn a_node_that_is_not_a_state_is_left_alone() {
        let other = state("M-12 -18 L12 -18 L12 18 L-12 18 Z")
            .replace("node statediagram-state", "node default");
        assert_eq!(widen_the_states(&other), other);
    }
}
