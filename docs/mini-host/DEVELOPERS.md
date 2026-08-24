# mini-host

A minimal VST3 host. It loads any VST3 binary, instantiates it the way a DAW
would, and either reports what it found or gives the plugin's editor a window
to live in.

It exists because testing a plugin against a real DAW is slow, manual, and
tells you nothing when it fails. This host talks to a plugin through the same
interfaces a DAW uses — `GetPluginFactory`, `IPluginFactory::createInstance`,
`IEditController::createView`, `IPlugView::attached`, `IPlugView::onKeyDown`,
`IComponent::get/setState` — so anything it can drive, a DAW can too.

Nothing here is specific to any one plugin.

## Layout

| | |
|---|---|
| `src/crates/mini-host` | The whole thing: the library, plus both binaries. |
| `src/crates/mini-host/src/lib.rs` | Loading, the factory, instances, buses, audio, state, the view. |
| `src/crates/mini-host/src/keys.rs` | The host's own `Key` and `Mods`, so driving a plugin does not mean importing that plugin's editor model. |
| `src/crates/mini-host/src/stream.rs` | An in-memory `IBStream`, which is how plugin state is read and written. |
| `src/crates/mini-host/src/bin/vst3-host.rs` | Command-line inspector. |
| `src/crates/mini-host/src/bin/mini-host/` | The windowed host: window, icon, saved position. |

Build output goes to `.cache/`, not `target/`. The finished executables are
copied to `../dist`, shared with the other projects here.

## Building and running

```bash
build.bat
```

```bash
./build.sh
```

Then point the windowed host at a plugin:

```bash
run.bat ..\dist\Notepad.vst3
```

```bash
./run.sh ../dist/Notepad.vst3
```

`run` builds nothing. It uses `../dist/mini-host`, falling back to
`.cache/release/mini-host`, and fails if neither is there.

To inspect a plugin instead of opening it:

```bash
cargo run --bin vst3-host -- ../dist/Notepad.vst3
```

Tests:

```bash
test.bat
```

```bash
./test.sh
```

## The two binaries

**`vst3-host`** prints what a plugin declares: the factory version it supports,
its classes and their categories, bus counts and arrangements, parameters, the
editor's size, and the bytes of its saved state. It never opens a window.

**`mini-host`** opens one. It creates the window with baseview, hands the
native handle to `IPlugView::attached`, and keeps the plugin alive until the
window closes. This is the only way to see a plugin's own editor without a DAW:
`createView` alone builds the view object and draws nothing.

The window remembers where it was left. baseview has no position API, so that
goes through the platform — `SetWindowPos` on Windows, `NSWindow` on macOS —
and the position is kept in `.cache/mini-host-window.txt`. The title bar icon
is drawn in memory rather than shipped as a resource; on macOS an application
icon comes from a bundle, which a bare cargo build does not produce.

## Using the library

See [USING.md](USING.md), which covers the command-line tool in full and shows
how to drive a plugin from Rust — typing keys into it, reading its state back,
pushing audio through it.

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
