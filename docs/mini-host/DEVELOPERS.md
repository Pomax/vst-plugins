# mini-host

A minimal VST3 host. It loads any VST3 binary, instantiates it the way a DAW
would, and gives the plugin's editor a window to live in. Reporting what a
plugin declares, without a window, is the loader's job: `tools/vst3-loader`,
which this host is built on.

It exists because testing a plugin against a real DAW is slow, manual, and
tells you nothing when it fails. This host talks to a plugin through the same
interfaces a DAW uses — `GetPluginFactory`, `IPluginFactory::createInstance`,
`IEditController::createView`, `IPlugView::attached`, `IPlugView::onKeyDown`,
`IComponent::get/setState` — so anything it can drive, a DAW can too.

Nothing here is specific to any one plugin.

## Layout

| | |
|---|---|
| `src/crates/mini-host` | The whole thing: one binary, `mini-host`. |
| `src/crates/mini-host/src/main.rs` | The command line, loading the plugin, and the window the editor is put in. |
| `src/crates/mini-host/src/app/chrome.rs` | The strip across the top, with **Save preset** and **Load preset**, and the two dialogs they open. |
| `src/crates/mini-host/src/app/place.rs` | Putting the plugin's window under the strip, sizing it with the host's window and giving it the keyboard, one way per platform. |
| `src/crates/mini-host/src/app/icon.rs` | The window's icon. |
| `src/crates/mini-host/src/presets.rs` | A plugin's state saved to and restored from a file. |
| `src/crates/mini-host/tests/` | Two tests that open the host's window. Linux only. |
| `src/tools/uitests/` | The host's own UI tests, which the markdown-notes test task runs. |

Loading, the factory, instances, buses, audio, state and the view are not
here: they are the library of `tools/vst3-loader`, and so is the command-line
inspector.

The project lives in `tools/mini-host`. Build output goes to `.cache/`, not
`target/`, and the finished executable is copied to `../../binaries`, shared
with the other projects here.

## Building and running

```bash
build.bat
```

```bash
./build.sh
```

Then point the windowed host at a plugin:

```bash
run.bat ..\..\binaries\Markdown Notes.vst3
```

```bash
./run.sh ../../binaries/Markdown Notes.vst3
```

`run` builds nothing. It uses `../../binaries/mini-host`, falling back to
`.cache/release/mini-host`, and fails if neither is there.

Besides the plugin's path the host takes `--preset FILE`, a preset to restore
before the editor is opened, and two things the UI tests use: `--state FILE`,
where the plugin's state is written when the window closes, and
`--geometry FILE`, where the host writes what its window and the plugin's
window inside it measure. With no path at all it asks for a plugin in a file
dialog.

To inspect a plugin instead of opening it, use the loader, in
`tools/vst3-loader`:

```bash
./run.sh "../../binaries/Markdown Notes.vst3"
```

Tests:

```bash
test.bat
```

```bash
./test.sh
```

Two more open the host's window, so they only run when asked for, and only on
Linux:

```bash
cargo test -- --ignored
```

## The window

**`mini-host`** opens a window with eframe, hands its native handle to
`IPlugView::attached`, and keeps the plugin alive until the window closes.
This is the only way to see a plugin's own editor without a DAW: `createView`
alone builds the view object and draws nothing.

Across the top of the window is the host's own strip, 26 high, with **Save
preset** and **Load preset**. A preset is the plugin's state as
`IComponent::getState` gives it, in a file under `presets/`, next to the
executable, in a directory named after the plugin's file. The plugin's editor
has everything below the strip.

Its strip names the plug-in twice: the name the factory reports, which is what
a DAW lists and which usually carries a version, and then in brackets the file
it was loaded from. The two differ, and renaming the file on disk changes only
the second.

The window remembers where it was left: eframe does that, through
`persist_window`. Under test, which is what `--geometry` or `--state` marks,
that memory is a file of the test's own beside the one named, so a test
starts at the size the plugin asks for and not where the last run left the
window. The title bar icon
is drawn in memory rather than shipped as a resource; on macOS an application
icon comes from a bundle, which a bare cargo build does not produce.

On Linux a plugin's editor is an X11 window, so the host's window is one as
well: on a Wayland desktop it is XWayland's. An editor there makes itself the
size of the window it is put in, so the host does not put it in its own
window. It makes a window of its own inside that one, under the strip and as
large as everything below it, and hands the plugin that. Sizing that window
is how the editor is sized: a thread watches the host's window and resizes
it the moment the X server says the host's has changed, and the editor
follows by itself. The window manager gives the keyboard to the host's window
whenever that comes to the front, and the host passes it on to the editor.
This is the Linux part of `src/crates/mini-host/src/app/place.rs`.

## Using the library

The library is the loader's. See [its guide](../vst3-loader/USING.md), which
covers the command-line tool in full and shows how to drive a plugin from
Rust — typing keys into it, reading its state back, pushing audio through it.

## What it does not do

- **No `IComponentHandler`.** A plugin that insists on one may refuse to draw
  or to report parameter changes.
- **No `IPlugFrame::resizeView`.** A plugin that wants to resize itself has
  nowhere to ask.
- **No parameter automation, no timing, no transport.** Audio is pushed through
  one block at a time with no host context.
- **No HiDPI negotiation.** `IPlugViewContentScaleSupport` is not implemented.

These are the ways it is more forgiving than a DAW, and therefore the ways a
plugin can pass here and still fail there.
