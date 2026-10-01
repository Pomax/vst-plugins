//! Keys as the desktop is told about them: by keysym, which names what a key
//! produces and not where it sits, so the layout in use does not change what
//! arrives.

pub const CONTROL: u32 = 0xffe3;
pub const SHIFT: u32 = 0xffe1;
pub const ALT: u32 = 0xffe9;

const RETURN: u32 = 0xff0d;
const ESCAPE: u32 = 0xff1b;
const BACKSPACE: u32 = 0xff08;
const END: u32 = 0xff57;
const DOWN: u32 = 0xff54;
const TAB: u32 = 0xff09;

/// One thing a `type:` step asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Stroke {
    /// A character, typed as itself.
    Character(char),
    /// A named key, by its keysym, pressed this many times.
    Named(u32, u32),
}

/// The keysym of a character.
///
/// Latin-1 has keysyms of its own, which are its code points. Everything else
/// is its code point with the Unicode bit set.
pub fn keysym_of(character: char) -> u32 {
    let code = character as u32;
    match code {
        0x20..=0x7e | 0xa0..=0xff => code,
        _ => 0x0100_0000 | code,
    }
}

/// The keysym of a key a `type:` step names in braces.
pub fn named(name: &str) -> Option<u32> {
    Some(match name.to_ascii_uppercase().as_str() {
        "ENTER" => RETURN,
        "ESC" => ESCAPE,
        "BS" | "BACKSPACE" => BACKSPACE,
        "END" => END,
        "DOWN" => DOWN,
        "TAB" => TAB,
        _ => return None,
    })
}

/// The keysym of a modifier a `shortcut:` step names.
pub fn modifier(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "ctrl" => CONTROL,
        "shift" => SHIFT,
        "alt" => ALT,
        _ => return None,
    })
}

/// A `type:` step as the keys it asks for, in order.
///
/// `{ENTER}`, `{ESC}`, `{END}`, `{DOWN}`, `{TAB}` are those keys. `{BS 40}` is
/// backspace forty times. `{(}` and `{)}` are the brackets themselves. Anything
/// else is a character. This is the notation the tests are written in, which
/// is Windows `SendKeys`.
pub fn parse_keys(step: &str) -> Result<Vec<Stroke>, String> {
    let mut strokes = Vec::new();
    let mut rest = step;
    while let Some(open) = rest.find('{') {
        strokes.extend(rest[..open].chars().map(Stroke::Character));
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("no closing brace: {step}"))?;
        let name = &after[..close];
        rest = &after[close + 1..];

        if name == "(" || name == ")" {
            strokes.extend(name.chars().map(Stroke::Character));
            continue;
        }

        let (name, times) = match name.split_once(' ') {
            Some((name, count)) => (
                name,
                count
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| format!("not a repeat count: {name} {count}"))?,
            ),
            None => (name, 1),
        };
        let keysym = named(name).ok_or_else(|| format!("unknown key: {name}"))?;
        strokes.push(Stroke::Named(keysym, times));
    }
    strokes.extend(rest.chars().map(Stroke::Character));
    Ok(strokes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(typed: &str) -> Vec<Stroke> {
        typed.chars().map(Stroke::Character).collect()
    }

    #[test]
    fn a_character_is_sent_as_its_own_keysym() {
        assert_eq!(keysym_of('a'), 0x61);
        assert_eq!(keysym_of('X'), 0x58);
        assert_eq!(keysym_of(' '), 0x20);
        assert_eq!(keysym_of('`'), 0x60);
        assert_eq!(keysym_of('é'), 0xe9);
        assert_eq!(keysym_of('€'), 0x0100_20ac);
    }

    #[test]
    fn named_keys_and_repeats_are_read_out_of_braces() {
        let mut expected = vec![Stroke::Named(END, 1), Stroke::Named(BACKSPACE, 40)];
        expected.extend(text("a name"));
        expected.push(Stroke::Named(RETURN, 1));
        expected.push(Stroke::Named(DOWN, 1));
        expected.push(Stroke::Named(TAB, 1));
        expected.push(Stroke::Named(ESCAPE, 1));

        assert_eq!(
            parse_keys("{END}{BS 40}a name{ENTER}{down}{TAB}{ESC}"),
            Ok(expected)
        );
    }

    #[test]
    fn bracket_escapes_are_literal_brackets() {
        assert_eq!(parse_keys("def cake{(}self{)}:"), Ok(text("def cake(self):")));
    }

    #[test]
    fn a_name_that_is_not_a_key_is_refused() {
        assert!(parse_keys("before{NOPE}after").is_err());
        assert!(parse_keys("{ENTER").is_err());
        assert!(parse_keys("{BS many}").is_err());
    }
}
