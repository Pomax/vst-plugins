# Choosing what edits the document

The editor has to do two things at once, and until now it did one at a time.

- **A3, A4.** Markdown converts as you write. Typing `# ` makes a heading and
  the `# ` stops being shown; the markers come back on the line the caret is
  on, and a toolbar toggle shows the raw source.
- **Text is text.** Select-all, drag-select, double-click a word, copy, cut,
  paste, undo, a caret that blinks.

The editor written for this project does the first and not the second: caret,
selection, undo and key handling are all hand-written in `markdown-notes-core`, and
only what was written exists. There is no clipboard in it at all.

## What the choice is constrained by

A VST3 plugin does not own a window. The host hands it an `HWND` on Windows or
an `NSView` on macOS and the editor has to draw inside that. That is what
`baseview` is for, and it rules out any framework that insists on creating its
own window.

## Options

| Option | Hides markers | Editing already written | Draws in the host's window |
|---|---|---|---|
| `wry` webview + CodeMirror 6 or Milkdown | Yes | All of it | Yes, `build_as_child` takes an `HWND`/`NSView` |
| `kode-core` + `kode-markdown`, drawn by us | Yes, by our renderer | Buffer, selection, word and line motion, select word/line, select-all, undo, indent, markdown input rules, formatting commands | Yes: headless, no UI at all |
| `taino-edit-core`, drawn by us | Yes, by our renderer | ProseMirror-style document model, transforms, history, commands, keymap, input rules | Yes: proven headless, its adapters are DOM-only |
| `iced_baseview` + iced `text_editor` | No | All of it | Yes |
| `slint-baseview` | No | Some | Yes |
| GPUI + `gpui-component` editor | Decorations that follow edits, the closest native match | Yes | No: GPUI creates its own window |
| egui `TextEdit` with a layouter | No | All of it | Yes |

Four of these lose on the same point. `TextEdit`, iced's `text_editor` and
Slint's edit widgets all edit the string they display: a highlighter can colour
a range but cannot make one disappear, so the `#` stays on screen. That is
styled source, not as-you-write conversion. GPUI can conceal but cannot be put
inside somebody else's window.

## The choice

**`kode-core` + `kode-markdown`, with the existing renderer.**

The renderer already conceals correctly, which is the part of this project that
works. What
was missing was everything a text field does, and that is exactly what these
two crates are: `kode-core` is a ropey buffer with selection, cursor motion and
undo and no UI, and `kode-markdown` adds markdown input rules, formatting
commands and a tree whose byte ranges tell a renderer what to hide.

The webview would also work and would cost less code. It was not chosen because
it puts WebView2 and a JavaScript bundle inside an audio plugin, and because
everything else here is Rust that runs in the host's process.

`taino-edit-core` models rich text in general; markdown would have to be mapped
onto it. `kode-markdown` is already markdown.

## What this changes

- `markdown-notes-core::Editor` keeps its public shape and hands the work to
  `kode_markdown::MarkdownEditor`. One per section, so the document lives in one
  place instead of a live copy beside a vec of stale ones.
- Caret, selection, undo history, word and line motion, and the Enter/Tab
  behaviour inside lists stop being ours.
- The renderer, the section strip, the colours, the state and the file handling
  stay as they are.
