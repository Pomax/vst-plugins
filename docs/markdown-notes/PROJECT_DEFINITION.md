# A VST note-taking app

High level goal: write a VST3 plugin that models a markdown editor, with as-you-write content conversion.

## Acceptance criteria

Original:

- **A1** — needs to be a universal VST3 plugin that'll load in anything, with both a windows .dll and mac .vst3 build target.
- **A2** — needs to store the user's notes as plugin state setting
- **A3** — needs to allow users to type markdown, with automatic conversion similar to tools like Typora
- **A4** — needs a way to toggle between wysiwyg and raw markdown
- **A5** — the plugin window must be resizable, which should also be stored as plugin state setting
- **A6** — an option to load .md files from disk
- **A7** — save the current file to disk as a standard "Save" operation
- **A8** — ...and as "Save as"

Added during development:

- **A9** — a theme selector: light, dark, or auto (following whatever the system uses). Must be part of plugin state, and must have tests.
- **A10** — the editor needs padding. Text running up to the window border is a bad experience.
- **A11** — audio must pass through the plugin untouched. It is a note-taking effect and has no business altering audio, but it sits in an insert slot: a plugin that fails to write its output buffer silences the track it is on.
- **A12** — Enter ends a block. A single newline is a soft break in markdown; pressing Enter after a paragraph must start a new one.
- **A13** — no fonts are bundled. The plugin uses the fonts on the machine it runs on, and loads one for a new script at the moment text needs it.

## Work criteria

Original:

- **W1** — you will need to write a minimal VST3 host to allow plugin testing
- **W2** — you will need to come up with, and then use, a test runner that lets you load the plugin in the VST host and then perform test operations.
- **W3** — you will define a set of tests that reflect normal text writing that a human user would do using markdown, including but not limited to writing headings, paragraphs, lists including checkkbox lists, styled text, links, etc.
- **W4** — free to pick whichever language is best suited, however only AFTER determining whether this can be done using Rust. If it can, do it in Rust.

Added during development:

- **W5** — a document explaining how to use the VST host, and how to load the plugin — and really *any* VST3 plugin — in it.
- **W6** — the GUI must be verifiable by looking at it, not by asserting around it. Write something that can see the rendered output.
- **W7** — a manual build script for each platform: `build.bat` on Windows, `build.sh` on macOS.
- **W8** — CI runs the full suite on every pull request, on both Windows and macOS. Pushes to main build both platforms and publish the zips as a release, numbered by release count — no version number, no semver. Documentation-only pushes do not build and do not release.

## Status

Verified means checked by a test or by inspecting real output, not by reading the code and concluding it ought to work.

| | Criterion | Status |
|---|---|---|
| A1 | Universal VST3, Windows + macOS targets | **Partly.** Both platforms build and bundle in CI. Neither has been loaded in a DAW on macOS. Linux, which the criterion does not name, builds and bundles on one machine (x86_64), and the bundle loads in this project's host there. CI does not build it, and it has not been loaded in a DAW |
| A2 | Notes in plugin state | Verified |
| A3 | Typora-style conversion | Verified |
| A4 | WYSIWYG / raw toggle | Verified |
| A5 | Resizable, size in state | **Partly.** State verified; host→window resize unverified |
| A6 | Load .md from disk | **Partly.** Logic verified; dialog untested |
| A7 | Save | **Partly.** Logic verified; dialog untested |
| A8 | Save As | **Partly.** Logic verified; dialog untested |
| A9 | Theme light/dark/auto, in state, tested | Verified |
| A10 | Editor padding | Verified |
| A11 | Audio passes through untouched | Verified, including mono |
| A12 | Enter ends a block | Verified |
| A13 | System fonts, loaded on demand | **Partly.** Verified on Windows; the macOS family names compile but have never been resolved on a Mac |
| W1 | Minimal VST3 host | Verified |
| W2 | Test runner driving the plugin in the host | Verified |
| W3 | Tests reflecting real markdown writing | Verified |
| W4 | Rust, after establishing feasibility | Verified |
| W5 | Host documentation, any VST3 plugin | Verified |
| W6 | Something that can see the GUI | Verified |
| W7 | Per-platform build scripts | **Partly.** `build.bat` verified; `build.sh` has never run. On Linux `build.sh` has run, at the root and in each project, and leaves the four results in `binaries/`; `test.sh --full` has run there and passed |
| W8 | CI on pull requests, build and publish on main | **Partly.** Both build jobs pass; publishing has not run yet |

## Open problem

**DAWs listed the plugin as an instrument as well as an effect.** Whether that
is still true is unknown: the current build has never been loaded in a DAW.

The plugin declares one `Audio Module Class` with subcategory `Fx`, through all
three factory versions, with one audio input bus, one audio output bus, no event
buses, and it refuses any bus arrangement with no audio input. That last point
was a real bug — a host asking "can you run with no audio input?" got a yes,
which is how a host decides something can be a generator.

Ruled out as causes: the subcategory string, a missing `IPluginFactory3`,
DAW-side caching, and the enum values behind media type and bus direction.
Splitting into two classes was tried and reverted; it fixed nothing and is a lot
of COM machinery for a note-taking effect.

Next step: install `binaries/Markdown Notes.vst3`, rescan a DAW, report what it says.

## Known gaps

Things that are not done, or are done but unproven. None are hidden behind a
passing test.

1. **macOS builds but has never been run.** A Windows machine cannot link a
   Mach-O binary, so CI is the only way to build it. The bundle now compiles and
   packages there, GUI included, but no Mac has ever launched it: the window,
   the font resolution and the file dialogs are unproven on that platform.
2. **Barely exercised in a real DAW.** Every automated test drives the plugin
   through this project's own host, which does not attach a window, implement
   `IComponentHandler`, or use a connection proxy — it is forgiving in exactly
   the ways a DAW is not.
3. **The host→window resize path is unverified.** `onSize` updates the stored
   size and the GUI follows it via `ViewportCommand::InnerSize`, but proving it
   needs a real host window to drag.
4. **`IPlugFrame::resizeView` is not called.** If the editor wants to resize
   itself — restoring a project whose stored size differs while the window is
   open — a well-behaved plugin asks the host first. We keep the frame pointer
   and never use it.
5. **File dialogs are untested end to end.** `open_path`/`save`/`save_as` have
   unit tests, but nothing drives the native dialog.
6. **The GUI keyboard path is untested.** Scenarios drive `onKeyDown`, which is
   the only input path when no window exists. With a window, egui handles keys
   natively and that translation has no automated coverage.
7. **No HiDPI negotiation.** `IPlugViewContentScaleSupport` is not implemented,
   so a host on a scaled display cannot tell the plugin its scale factor.
8. **Bold is a colour, not a typeface.** egui ships no bold family.
9. **No mouse selection.** Clicking places the caret; selection is keyboard-only.
10. **The host is never told the notes changed.** VST3 has no "state is dirty"
    signal — the usual trick is a hidden parameter bumped on every edit. Without
    one a DAW still saves the notes, but may not mark the project modified.
11. **Script coverage depends on a list of font family names.** The names are
    resolved against the running machine, but a system whose fonts are not in
    the list gets no glyphs for that script. The platform per-character fallback
    APIs (`IDWriteFontFallback::MapCharacters`, `CTFontCreateForString`) would
    remove the list entirely.
12. **Linux is one machine.** The plugin, the host and every test have run on
    one Linux desktop: x86_64, GNOME on Wayland with the windows on XWayland,
    one monitor at a scale of 1. CI builds and tests Windows and macOS only.
    The plugin has not been loaded in a DAW on Linux.
13. **No drop target for files on Linux.** A picture pasted with Ctrl+V goes
    into the note there, and a file dropped on the window does not: `drop.rs`
    has a target for Windows and one for macOS.
14. **The Linux UI tests are written for that desktop.** The driver knows
    GNOME's file dialog (its `Save`, `Replace` and `Select` buttons, and that
    its path field takes only places that exist), finds a window's resize
    grip by the cursor GNOME shows over it, and needs the desktop portal to
    allow it the screen, the pointer and the keyboard. Its text reader answers
    only with the characters from the space to `~`, so no step can look for
    lettering outside them. How long one look at the screen takes was not
    measured.
15. **Not proven on Linux.**
    - Whether the plugin's colours are what they are on the other platforms.
      On that machine its window can only be made without an sRGB framebuffer,
      and the two were not compared.
    - Why the plugin keeps showing an arrow when the pointer is put on text in
      one jump. The driver moves the pointer there, as a hand does, and then
      the I-beam shows; the cause was not found.
    - That text keeps its size through every moment of a live resize. In the
      frames looked at, two a second, it does. In one of them, taken while
      the window was changing size, the plugin's part looks about a pixel
      lower than in the others, judged by eye.
    - The plugin's lettering is the desktop's sans-serif face, where Windows
      and macOS get Arial, so text is wider: a section tab is 109 wide where
      the step files assume about 102. The UI tests find the `+` after the
      tabs by looking for that reason. Nothing else was checked for what the
      wider face does to it.
