//! The scenarios themselves — ordinary markdown writing, performed the way a
//! person performs it: one keystroke at a time, through the plugin's editor
//! window.

use markdown_notes_core::{Key, Mods};

use crate::scenario::{scenario, Scenario, Step::*};

pub fn all() -> Vec<Scenario> {
    vec![
        scenario(
            "headings and paragraphs",
            vec![
                Type("# Shopping list\nEverything I need this week."),
                ExpectSource("# Shopping list\n\nEverything I need this week."),
                ExpectRendered("Shopping list\n\nEverything I need this week."),
            ],
        ),
        scenario(
            "every heading level",
            vec![
                Type("# One\n## Two\n### Three\n#### Four\n##### Five\n###### Six"),
                ExpectRendered("One\n\nTwo\n\nThree\n\nFour\n\nFive\n\nSix"),
            ],
        ),
        scenario(
            "enter after a paragraph starts a new block, not a soft break",
            vec![
                Type("Cool beans.\nWe're all checked off."),
                ExpectSource("Cool beans.\n\nWe're all checked off."),
                ExpectRendered("Cool beans.\n\nWe're all checked off."),
            ],
        ),
        // The first item is typed with a star, which is normalised as it is
        // typed, so this covers that too.
        scenario(
            "bullet lists continue themselves",
            vec![
                Type("* milk\neggs\nbread"),
                ExpectSource("- milk\n- eggs\n- bread"),
                ExpectRendered("milk\neggs\nbread"),
            ],
        ),
        scenario(
            "enter on an empty bullet ends the list",
            vec![
                Type("- one\n\nback to prose"),
                ExpectSource("- one\nback to prose"),
            ],
        ),
        scenario(
            "numbered lists renumber as they grow",
            vec![
                Type("1. first\nsecond\nthird"),
                ExpectSource("1. first\n2. second\n3. third"),
                ExpectRendered("first\nsecond\nthird"),
            ],
        ),
        scenario(
            "tab indents a list item",
            vec![
                Type("- top\n"),
                Press(Key::Tab, Mods::NONE),
                Type("nested"),
                ExpectSource("- top\n  - nested"),
            ],
        ),
        scenario(
            "typing a checkbox produces a task item",
            vec![
                Type("-[] Buy milk"),
                ExpectSource("- [ ] Buy milk"),
                ExpectRendered("Buy milk"),
            ],
        ),
        scenario(
            "a pre-ticked checkbox is recognised",
            vec![
                Type("-[x] Ship it"),
                ExpectSource("- [x] Ship it"),
                ExpectRendered("Ship it"),
            ],
        ),
        scenario(
            "bold and italic typed by hand",
            vec![
                Type("**bold** and *italic* and ~~struck~~"),
                ExpectSource("**bold** and *italic* and ~~struck~~"),
                ExpectRendered("bold and italic and struck"),
            ],
        ),
        scenario(
            "ctrl+B wraps the selection in bold",
            vec![
                Type("make this bold"),
                Press(Key::Char('a'), Mods::CTRL),
                Press(Key::Char('b'), Mods::CTRL),
                ExpectSource("**make this bold**"),
                ExpectRendered("make this bold"),
            ],
        ),
        scenario(
            "shift and the arrows select, and typing replaces the selection",
            vec![
                Type("hello"),
                Press(Key::Home, Mods::NONE),
                Press(Key::Right, Mods::SHIFT),
                Press(Key::Right, Mods::SHIFT),
                Type("X"),
                ExpectSource("Xllo"),
            ],
        ),
        // Ctrl and an arrow land on the start of the next word, which is where
        // Windows puts them.
        scenario(
            "ctrl and the arrows move a word at a time",
            vec![
                Type("one two"),
                Press(Key::Home, Mods::NONE),
                Press(Key::Right, Mods::CTRL),
                Type("!"),
                ExpectSource("one !two"),
            ],
        ),
        scenario(
            "ctrl and shift together select a word at a time",
            vec![
                Type("one two"),
                Press(Key::Home, Mods::NONE),
                Press(Key::Right, Mods::CTRL_SHIFT),
                Type("X"),
                ExpectSource("Xtwo"),
            ],
        ),
        scenario(
            "underscores inside a word stay literal",
            vec![
                Type("call snake_case_name here"),
                ExpectRendered("call snake_case_name here"),
            ],
        ),
        scenario(
            "ctrl+K turns a selection into a link, ready for the URL",
            vec![
                Type("Anthropic"),
                Press(Key::Char('a'), Mods::CTRL),
                Press(Key::Char('k'), Mods::CTRL),
                ExpectSource("[Anthropic]()"),
                Type("https://www.anthropic.com"),
                ExpectSource("[Anthropic](https://www.anthropic.com)"),
                ExpectRendered("Anthropic"),
            ],
        ),
        scenario(
            "inline code hides its backticks",
            vec![
                Type("run `cargo test` now"),
                ExpectSource("run `cargo test` now"),
                ExpectRendered("run cargo test now"),
            ],
        ),
        scenario(
            "a fence closes itself and holds raw markdown",
            vec![
                Type("```rust\nlet x = *y;"),
                ExpectSource("```rust\nlet x = *y;\n```"),
            ],
        ),
        scenario(
            "blockquotes continue on the next line",
            vec![
                Type("> quoted\nstill quoted"),
                ExpectSource("> quoted\n> still quoted"),
                ExpectRendered("quoted\nstill quoted"),
            ],
        ),
        scenario(
            "undo and redo a burst of typing",
            vec![
                Type("hello"),
                ExpectSource("hello"),
                Press(Key::Char('z'), Mods::CTRL),
                ExpectSource(""),
                Press(Key::Char('z'), Mods::CTRL_SHIFT),
                ExpectSource("hello"),
            ],
        ),
        scenario(
            "the view mode toggles and is remembered across a session",
            vec![
                Type("# Notes"),
                ExpectMode("wysiwyg"),
                Press(Key::Char('/'), Mods::CTRL),
                ExpectMode("raw"),
                ReopenProject,
                ExpectMode("raw"),
                ExpectSource("# Notes"),
            ],
        ),
        scenario(
            "the theme starts on auto and cycles with Ctrl+T",
            vec![
                ExpectTheme("auto"),
                Press(Key::Char('t'), Mods::CTRL),
                ExpectTheme("light"),
                Press(Key::Char('t'), Mods::CTRL),
                ExpectTheme("dark"),
                Press(Key::Char('t'), Mods::CTRL),
                ExpectTheme("auto"),
            ],
        ),
        // Auto is the theme worth reopening on: the other two are whatever was
        // chosen, while auto has a resolved value that must not be written down
        // in its place.
        scenario(
            "auto is remembered as auto, not as whatever it resolved to",
            vec![
                Type("# Notes"),
                ExpectTheme("auto"),
                ReopenProject,
                ExpectTheme("auto"),
                ExpectSource("# Notes"),
            ],
        ),
        scenario(
            "the window is resizable, and its size is the host's to keep",
            vec![
                ExpectSize(900, 620),
                Resize(1024, 768),
                ExpectSize(1024, 768),
                // Reopening restores the document, not the window: how big the
                // editor is belongs to whoever put it on screen.
                ReopenProject,
                ExpectSize(900, 620),
            ],
        ),
        scenario(
            "a too-small window is clamped to a usable size",
            vec![ExpectClampedSize {
                proposed: (100, 50),
                accepted: (320, 200),
            }],
        ),
        // What the document holds is a string, so a reopen has nothing to say
        // about which blocks are in it. What it can go wrong on is the encoding.
        scenario(
            "a document survives closing and reopening the project",
            vec![
                Type("# 見出し\n日本語の**太字**とemoji ✓"),
                ExpectSource("# 見出し\n\n日本語の**太字**とemoji ✓"),
                ExpectRendered("見出し\n\n日本語の太字とemoji ✓"),
                ReopenProject,
                ExpectSource("# 見出し\n\n日本語の**太字**とemoji ✓"),
            ],
        ),
        scenario(
            "the plugin is an effect, and says so everywhere a host looks",
            vec![ExpectEffectNotInstrument],
        ),
        scenario(
            "audio passes through the plugin untouched",
            vec![ExpectAudioPassThrough],
        ),
        scenario(
            "with no input bus the output is silenced, not left as garbage",
            vec![ExpectSilenceWithNoInput],
        ),
        scenario(
            "a mono track is accepted and reported honestly",
            vec![ExpectMonoIsHonoured, ExpectAudioPassThrough],
        ),
        scenario(
            "audio still passes through while the document is being edited",
            vec![
                Type("# Notes taken while the track plays"),
                ExpectAudioPassThrough,
                ExpectSource("# Notes taken while the track plays"),
            ],
        ),
        // A DAW can hand back anything: a truncated chunk, a project written by
        // a newer build, a file someone edited by hand. None of it may crash
        // the plugin, and none of it may lose the words.
        scenario(
            "state that is not JSON at all is kept as note text",
            vec![
                LoadRawState(b"just some words, not json"),
                ExpectSource("just some words, not json"),
            ],
        ),
        scenario(
            "empty state opens an empty document",
            vec![LoadRawState(b""), ExpectSource(""), Type("!"), ExpectSource("!")],
        ),
        scenario(
            "a document of multi-byte characters loads and takes typing",
            vec![
                // "é" is two bytes (\u{e9} = 0xC3 0xA9); a caret landing inside
                // it would split a character.
                LoadRawState(b"{\"notes\":\"\xc3\xa9x\"}"),
                ExpectSource("éx"),
                Type("!"),
                ExpectSource("éx!"),
            ],
        ),
        scenario(
            "a window size in a project is ignored, however absurd",
            vec![
                LoadRawState(br#"{"notes":"x","width":2147483647,"height":-5}"#),
                ExpectSource("x"),
                ExpectSize(900, 620),
            ],
        ),
        scenario(
            "arrow keys and backspace edit in the middle of a line",
            vec![
                Type("helo"),
                Press(Key::Left, Mods::NONE),
                Type("l"),
                ExpectSource("hello"),
                // Where the caret went is shown by what typing does next.
                Press(Key::Home, Mods::NONE),
                Type("["),
                ExpectSource("[hello"),
                Press(Key::End, Mods::NONE),
                Press(Key::Backspace, Mods::NONE),
                ExpectSource("[hell"),
            ],
        ),
    ]
}
