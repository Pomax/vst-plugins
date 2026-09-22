# Markdown Notes

A VST3 markdown note-taking effect. It loads on any track in any DAW, keeps your notes in
the project file, and converts markdown as you type it — the way Typora does.

Written in Rust. (The plan asked whether Rust could do this before considering
anything else: it can. The [`vst3`](https://crates.io/crates/vst3) crate ships
complete VST3 bindings with no C++ SDK dependency, and supports both
*implementing* COM interfaces — the plugin — and *calling* them — the test host.)

## Layout

| Crate | What it is |
|---|---|
| `src/crates/markdown-notes-core` | The editor: a document, WYSIWYG layout, as-you-type conversion, plugin state, file I/O. No UI, no plugin dependencies. The buffer, caret, selection and undo are [`kode-markdown`](https://crates.io/crates/kode-markdown)'s; see [EDITOR_CHOICE.md](EDITOR_CHOICE.md). |
| `src/crates/markdown-notes-plugin` | The VST3 plugin, plus the egui GUI. |
| `src/crates/markdown-notes-testrunner` | Runs scripted editing scenarios against the real plugin binary, through the mini host in `../tools/mini-host`. |
| `src/xtask` | Build tasks: assembles the `.vst3` bundle, runs the test suite. |
| `src/tools` | Screenshot helpers for the real editor window. |

The VST3 host this project tests against is a separate project:
`../tools/mini-host`, documented in [docs/mini-host](../mini-host/DEVELOPERS.md).

## Building

```bash
cargo dist
```

That leaves the installable plugin in `binaries/`, beside the tools:

```text
binaries/
  Markdown Notes.vst3
```

On Windows and Linux that is the plugin binary; on macOS it is the `.vst3`
bundle directory, which is the only form a plugin can take there. Either way it
is what gets copied into the VST3 folder:

- Windows: `C:\Program Files\Common Files\VST3\`
- macOS: `~/Library/Audio/Plug-Ins/VST3/`

This project's own result in `binaries/` is replaced by every build, so it only
ever holds the current one.

**A successful build deletes `target/`.** Everything worth keeping has been
copied to `binaries/`; what remains only helps when something went wrong, so it
survives a *failed* build and not a successful one. The trade is that the next
build is a cold one.

`cargo dist` is an alias for `cargo run -p xtask -- bundle --release`; plain
`cargo build` cannot do any of this, because cargo has no post-build hook that
can see the finished cdylib. `cargo dist-debug` builds the debug profile.

Cross-compiling for macOS uses the same task, though it needs an Apple linker
and SDK, which a Windows machine does not have — CI does this on a macOS
runner:

```bash
cargo run -p xtask -- bundle --release --target aarch64-apple-darwin
```

## Testing

```bash
cargo run -p xtask -- test
```

That builds the plugin, then runs the unit tests and the scenario suite in that
order, and, since it too is a build, clears this project's result from
`binaries/` first and deletes
`target/` when everything passes. `--keep-target` leaves the build output in
place for a faster next run. The order is the point: the scenario runner loads the plugin binary at
runtime rather than linking it, so cargo has no idea the two are related and
will happily run the suite against a stale `.dll`. (The runner refuses to run
if it spots one, because that actually happened here — a deliberately broken
`process` sailed through a green suite.)

The two halves can still be run alone:

```bash
cargo test --workspace
cargo build -p markdown-notes-plugin && cargo run -p markdown-notes-testrunner
```

The scenario runner loads the built plugin binary and drives it through the
same interfaces a DAW uses — `GetPluginFactory`, `createInstance`,
`createView`, `IPlugView::onKeyDown`, `IComponent::get/setState`. There is no
test-only backdoor: a scenario types `* milk`, Enter, `eggs` one keystroke at a
time and then reads the document back out of the state blob the plugin would
write into a project file.

To use the built plugin without a DAW — the real thing, loaded through the
factory and attached to a window:

```bash
test.bat
```

```bash
./test.sh
```

Both open `../binaries/Markdown Notes.vst3` in the mini host, which lives in `../tools/mini-host` and must be built there first.

The test task runs the pixel tests along with everything else. They sit behind
the `snapshots` feature because the headless renderer pulls in the whole
wgpu/naga stack, which the plugin itself has no use for.

One test on its own, with nothing built beside it and `binaries/` left alone:

```bash
test.bat --only mermaid_blocks
test.bat --only mermaid_blocks::the_source_view_shows_the_source
test.bat --only block::tests
```

The name is a pixel test file under the plugin's `tests/`, optionally followed
by `::` and a test inside it, or else a filter on the workspace's unit tests.
`./test.sh` takes the same arguments.

Those tests render the drawing code through a real rasteriser with no window
involved. [`tests/theme_rendering.rs`](../../markdown-notes/src/crates/markdown-notes-plugin/tests/theme_rendering.rs)
asserts on actual pixels: that light really is light, that the text contrasts
with it, and that the two themes do not look alike. They exist because the
light theme once passed every non-visual check while the window stayed black:
nothing painted the background, so egui's dark-on-light text was drawn onto a
black clear colour. Only pixels catch that.

The headless renderer proves the *drawing code* is right. To prove the *real
window* is, which is the path through baseview and OpenGL where the background
is the renderer's clear colour rather than anything egui draws, run the UI
tests: they open the plugin in the mini host and photograph it.

```bash
powershell -ExecutionPolicy Bypass -File tools/capture-window.ps1 -Exe ../binaries/mini-host.exe -Title "Mini VST Host" -Out window.png
```

`-ExecutionPolicy Bypass` is needed wherever unsigned scripts are blocked. It
takes `-ExeArgs` for what to launch the host with, and `-Title` to say which
window to photograph.

On macOS the UI tests photograph the window themselves, through the `Window
Shot` app in `tools/window-shot`.

The UI tests are step files in `markdown-notes/src/tools/uitests/`, run in the
order `order.txt` gives: `./test.bat --ui NAME` runs one test or one suite.
There are four, and each is one long run that covers a whole area rather than a
launch of the host per behaviour: `typing-goes-into-the-document` (the
baseline), `editing-the-document`, `sections-and-files` and `opening-a-note`.
A new behaviour goes into the run whose note it can reuse, not into a new file
that types the same title, heading and body again.

No step pauses for a length of time, and there is no `wait:` step. A step that
needs the window to have caught up says what it is waiting to see, and the
driver looks until it is there: `showing:`, `hidden:`, `press:`, `dragtext:`,
`written:`, `cursor:`, `window:`, `nowindow:` and `dialog:` all poll, and a
window that has just opened is waited on until it has drawn something that can
drawn. The only timing in the driver is a hand's: keys go one at a time, a
click holds the button for a moment, the pointer travels rather than jumps, and
a click, a drag or a dialog opening is followed by the moment a hand takes to
get to the next thing. Input sent faster than a person can make it reaches the
plugin out of order, because the pointer and the keyboard come in by different
roads.

The plugin writes its state when the host closes, and `expect:` lines are
checked against it. A `restart:` step closes the host and starts it again, so
a test with restarts has several runs and several states: an `expect:` line is
checked against the state of the run it is written in, which for every run but
the last is the one `restart:` put aside as `NAME.state.1`, `NAME.state.2` and
so on in `.cache/uitests/`.

The host is also a standalone tool that loads **any** VST3 plugin, not just
this one:

```bash
cd ../tools/mini-host && cargo run --bin vst3-host -- ../../binaries/Markdown Notes.vst3
```

See [the mini host guide](../mini-host/DEVELOPERS.md) for how to use it and how a
VST3 plugin is loaded.

## CI

Two workflows, because checking the code and shipping it are different jobs.

`.github/workflows/markdown-notes-ci.yml` runs on every pull request touching this project or the mini host: the full suite including
the pixel tests, on both `windows-latest` and `macos-14`, plus a
[zizmor](https://docs.zizmor.sh) audit of the workflows themselves.

`.github/workflows/markdown-notes-build.yml` runs on pushes to `main` that change the code.
Markdown, `docs/`, `.github/`, `LICENSE` and `.gitignore` are ignored, so
editing the workflows does not cut a release. It builds and zips both
platforms, then publishes them as a GitHub release. Before publishing, each job
loads the bundle it just built and asks the factory for its classes, so a build
that produces something no host can open fails there rather than in a DAW. Releases are numbered by how
many already exist — the first is `1`, the one after release `8` is `9`. There
is no version number and no semver; this is a product, not a library.

## Keys

| | |
|---|---|
| `Ctrl+B` / `Ctrl+I` / `Ctrl+D` | bold / italic / strikethrough (wraps the selection) |
| `Ctrl+E` | inline code |
| `Ctrl+K` | turn the selection into a link, caret left in the `()` |
| `Ctrl+/` | toggle WYSIWYG ↔ raw markdown |
| `Ctrl+T` | cycle the theme: auto → light → dark |
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo |
| `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S` | Open / Save / Save As |
| `Tab` / `Shift+Tab` | indent / outdent a list item |

Typing converts as you go: `* ` becomes a `- ` bullet, `-[] ` becomes a
`- [ ] ` task box, Enter continues lists and blockquotes, Enter on an empty
list item ends the list, numbered lists renumber themselves, and an opening
code fence closes itself.

## Opening

A note whose name is still `...project title goes here...` opens with the
keyboard in the name and the whole of it selected, so the first thing typed
replaces it. Enter or Tab there sends the keyboard to the document. A note
that has a name opens in the document straight away.

Arriving in the document always means the first section, whichever was in
front. If that section still opens with `# Section Title`, the words of the
heading are selected, without the `# `. If it does not, the caret goes after
the end of the section. `Editor::arrive_in_document` is that rule, and
`Gui::arrive` applies it on the first frame of a window.

A name too long for its field is shown as the start of it and `...`, cut by
measured width (`fit::fitted`), and hovering over it shows the whole name in a
tooltip. That is only what is drawn: the name itself is untouched, and the
field holds all of it again while it is being edited.

The cut follows the window: it is worked out every frame from the room the
toolbar leaves. A resize while the name has the keyboard takes the keyboard
from it (`sync_window_size`), so a name that was just typed is cut like any
other once the window is too narrow for it.

The name is centred in its field for as long as it fits. One being typed past
the end of the field is laid out from the left instead, because egui's
single-line field only scrolls to keep the caret in view when it is.

There is one cursor in the window. While the name, or any other field, has the
keyboard, the document has no caret and no selection: `draw_ui` clears them
with `Editor::clear_caret` for as long as a field is focused.

## Pictures

A PNG or JPEG dropped on the window, or pasted from the clipboard with Ctrl+V,
goes into the note at the caret as a reference on a line of its own, with a
blank line before and after: `![cat][1]` for a file called `cat.png`,
`![image][1]` for a paste. The data goes into the document's images tail as
`[1]: data:image/png;base64,...`, one line per picture, after the last
section. The tail is what the `images` tab in the section strip shows: it is
always last, it is read and not typed into, and it saves as the bottom of the
document. `markdown_notes_core::images` is the tail, the reference syntax and
the numbering; `Editor::insert_image` and `Editor::tidy_images` put a picture
in and keep the tail in step with the text. A reference deleted from the text
loses its definition on the next frame, and the definitions after it move up
so the numbers run without gaps.

The renumbering is written through each section's history as one transaction,
so undo takes it back before it takes the deletion back. `Editor::undo` puts
the definitions back too: each tidy remembers the text it started from and the
definitions it had, and an undo that lands on that text, or on text referring
to a picture the tidy took away, restores them. Nothing is tidied while the
document stands where an undo left it, so what came back stays until
something else is typed.

In the document the reference is drawn as the picture, at its size, scaled
down to the width when the window is narrower. The caret on its line brings
the text back to edit, the same as a mermaid block. `pictures.rs` decodes and
keeps the textures.

Where a picture comes from:

- A drop. baseview delivers drops to egui-baseview, which discards them, so
  the plugin registers its own drop target on the window baseview opened:
  `drop.rs`, an `IDropTarget` on Windows, an overlay `NSView` registered for
  file URLs on macOS. Both queue the files for the next frame. The Windows
  target is proven by hand: a file dragged from Explorer lands. The macOS one
  has not been run.
- A paste. egui-baseview turns Ctrl+V into a text event only when the
  clipboard holds text, so on that key with no text the plugin reads the
  clipboard's picture with `arboard` and encodes it as PNG.

The real-window test `pictures-in-a-note` pastes; nothing in the driver can
drag a file. The headless `pictures_in_notes` tests drop through egui's own
dropped-files input, which reaches the same queue.

## Sections

The sections are one document seen in parts, not several documents. Saving
joins every section in order; opening splits the file again at each top-level
`#`, one section per heading. Text before the first heading becomes the first
section, and a file with no headings at all opens as a single section,
unchanged byte for byte.

A section is titled by the heading it starts with, or `untitled`. Titles are
capped at the width of the string `this many words`; anything longer is cut
with `...` and shown in full on hover. Every section is that same width, so
typing a heading never shifts the strip sideways.

Click a section to switch, drag it to reorder, `+` to add one, middle-click or
right-click to close. Closing the last one empties it instead of removing it.

## Mermaid

A closed code fence whose language is `mermaid` is drawn as the diagram it
describes while the caret is anywhere else. Moving the caret into the block, by
key or by clicking the picture, shows the code again, fences included. The
source view never draws a picture. The text is never changed by any of this.

Code that cannot be drawn, an empty block included, is drawn as a picture
reading `error in mermaid code`. A fence nothing closes stays code.

[`merman`](https://crates.io/crates/merman) turns the code into SVG and
[`resvg`](https://crates.io/crates/resvg) turns the SVG into pixels, in
[`diagram.rs`](../../markdown-notes/src/crates/markdown-notes-plugin/src/diagram.rs).
The site config is the theme and look Mermaid 12 gives a state diagram by
default, which is what mermaid.live shows: `theme: redux-color`
(`redux-dark-color` in the dark scheme) and `look: neo`. Flowcharts default to
`flowchart.curve: linear`. A block's own frontmatter overrides all of it.
Mermaid 12's other two defaults, `layout: elk` and `state.minNodeWidth`, are
not something merman 0.7.0 reads: it follows Mermaid 11 and lays out with its
dagre port.

merman sizes a state from its name alone: it has no `minNodeWidth`, and
`state.padding` changes nothing. So
[`node_widths.rs`](../../markdown-notes/src/crates/markdown-notes-plugin/src/node_widths.rs)
redraws the outline of any state narrower than 72 at that width, and the
editor asks merman for enough `state.nodeSpacing` to widen them into. Rows are
100 apart (`rankSpacing`, for states and flowcharts), which is the room the
edges are routed in.

merman 0.7.0 draws the edges of a state diagram as splines and reads no setting
for it, so
[`elbows.rs`](../../markdown-notes/src/crates/markdown-notes-plugin/src/elbows.rs)
redraws them, and a flowchart's the same way, from the layout points merman
leaves in each edge's `data-points`: out of the bottom of one node, through the
points in vertical and horizontal runs, into the top of the next. A node's side
is divided evenly between the edges on it, in the order of where they go, so a
branch is seen at the node it branches from. A label the layout left between
two nodes is slid under the nearer one where no other label is in the way, so
its edge bends once and not twice. Horizontal runs that would lie along each
other get heights of their own: out of a node the longest highest, into a node
the shortest highest, which keeps edges of one node from crossing each other.
Where an edge does cross another it goes over it in a bump.

`mermaid-rs-renderer` was tried against the same diagram with spacing of up to
160 (its own options, frontmatter, `stateDiagram-v2` and as a flowchart). It
ignores spacing for state diagrams, and its flowchart routing loops edges
around nodes and leaves labels off their lines. The pictures are made by
`tests/mermaid_renderers.rs`. The label is put back on its point, its box is made solid so the line
does not show through the words, and the picture is widened if the label now
reaches past its edge. The `neo` look's
arrowheads are sized in stroke widths, which its two pixel lines double, so a
redrawn edge is pointed at the `-margin` arrowhead merman also defines, which
is sized in pixels. An edge that runs up the page keeps merman's curve.

[`tests/mermaid_renderers.rs`](../../markdown-notes/src/crates/markdown-notes-plugin/tests/mermaid_renderers.rs)
draws one large state diagram with merman, with the editor's own path, and with
`mermaid-rs-renderer` (a dev-dependency kept for that comparison), and leaves
the pictures in `markdown-notes/.cache/uitests/`.
A picture is drawn once per code, theme and screen scale, and dropped when no
frame asks for it. Finding the blocks is `RenderDoc::diagrams` in
`markdown-notes-core`.

The release profile aborts on panic, so a panic inside either crate ends the
host process. `catch_unwind` around the renderer only turns a panic into the
error picture in builds that unwind, which is the test profile.

## Colours

Light and dark are two complete sets, in
[`colours.rs`](../../markdown-notes/src/crates/markdown-notes-core/src/colours.rs), stored in plugin
state and edited under the toolbar's gear. Neither is derived from the other.

The defaults reproduce what egui's own light and dark visuals produced before
the schemes existed, so an old project looks the same after loading.

Some of what the editor shows is drawn by egui rather than by this code —
buttons, fields, checkboxes, the scrollbar, the selection — so the scheme is
written into `egui::Visuals` in `paint_visuals` before a frame is laid out, and
read directly from the scheme everywhere the editor paints for itself.

`Rgba` holds unmultiplied channels, matching what a colour picker produces.
`egui::Color32` is premultiplied, and the two are converted at the boundary.

## Design notes

**Markdown source is the source of truth.** The parser keeps marker spans
rather than stripping them, tagging each visible or hidden. Hiding them yields
WYSIWYG; revealing the ones on the caret's line gives Typora's behaviour where
punctuation appears on the line you are editing. Raw mode just makes every
marker visible. One code path serves the GUI and the tests, and the text you
save is exactly the text you typed.

**The plugin is a single-component effect** — one COM object implementing both
`IComponent` and `IEditController`. The document is edited in the GUI but
persisted by the processor, and splitting them into two objects would mean
marshalling the whole note text through parameters or `IMessage` on every
keystroke.

**State** is JSON: notes, view mode, theme, window size, caret, and file path.
Anything that fails to parse as JSON is kept as raw note text rather than
discarded, so a malformed blob loses formatting but never the user's words.
Unknown fields are ignored and missing ones default, so a project saved before
a setting existed still opens.

**Theme** is `light`, `dark` or `auto`. `auto` is stored *as* `auto` rather
than as whatever it resolved to, so a project moved between a light machine and
a dark one follows each. Resolving it is the GUI's job: `markdown-notes-core` has no
OS dependency and instead exposes `Theme::is_dark(system_dark)`, which keeps the
decision table unit-testable without an operating system in the loop. The
system setting is polled every two seconds rather than every frame, since
reading it hits the registry on Windows and a desktop portal on Linux.

**Fonts come from the operating system; none are bundled.** egui's default set
is four typefaces and about 1.4 MB, which is a quarter of the plugin, and every
platform it runs on already has fonts. The editor starts with the system UI and
monospace faces — enough for Latin, Greek, Cyrillic, Hebrew and Arabic — and
when text arrives in a script those cannot draw, the font for it is fetched
from the system at that moment and added as a fallback. Nothing is captured at
build time except a list of family names; the lookup happens on the machine
running the plugin.

## Known limitations

- **Bold is drawn as a stronger colour, not a bold typeface.** egui ships no
  bold font family; this is the same approach egui uses for its own emphasis.
  Embedding a bold font would fix it properly.
- **Input ownership.** When the host has attached a window, the GUI receives
  key events natively and `onKeyDown` returns `kResultFalse` — handling both
  would type every character twice. Without a window, `onKeyDown` is the only
  input path, which is how the test host drives the plugin.
