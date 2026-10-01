# Linux build and UI tests: plan

## 1. Goal

`./build.sh` and `./test.sh` (including `./test.sh --full`) work on Linux the
way they work on Windows and macOS:

- every project `./build.sh` names builds and its result lands in `binaries/`
  (`tools/window-shot` is macOS only and is not one of them);
- the unit tests, the scenarios and the headless pixel tests pass;
- the UI tests run from the same step files, against the real host window,
  with input posted to the desktop's own keyboard and pointer, the way
  `SendInput` and `CGEvent` post to theirs, and pictures taken of the screen;
- Linux has what the other platforms have for this: a window driver that can
  photograph the screen, and a text finder. On Windows the driver and the
  photographer are one program, on macOS two; on Linux they are one.

The first UI run raises GNOME's permission dialog, which only the person at
the machine can answer.

Not part of this plan: files dropped onto the plugin's window. `drop.rs` has
no Linux drop target, and no test drives a real drop onto the window on any
platform. The headless `pictures_in_notes` tests put files into the same
queue through egui, and those run on Linux in T3.

Also not part of this plan:

- CI and releases. The workflows build and test on Windows and macOS only,
  and this plan does not add a Linux job or a Linux release.
- `markdown-notes/README.md` and the root `README.md`, which are the user's.
  Line 27 of the first says there are only Windows and macOS releases
  "because Linux does not support VST3". A Linux build of the plugin is at
  odds with that sentence, and the sentence is the user's to change.
- `markdown-notes/.cargo/config.toml`, whose `CFLAGS` lines trim the
  tree-sitter grammar out of the Windows and macOS binaries. Linux gets no
  such line, so its binary keeps the grammar. The lines are about size, and
  T3 is what shows the Linux build works without one.

Target: GNOME 50 on Wayland, x86_64, one 1920x1080 monitor. The host and the
plugin are XWayland windows, because the plugin's only Linux window type is
`kPlatformTypeX11EmbedWindowID` (`markdown-notes-plugin/src/lib.rs:618`).

Fixed choices:

- Screen reading: the xdg-desktop-portal ScreenCast interface.
- Text recognition: the `tesseract` program.
- Input: the xdg-desktop-portal RemoteDesktop interface. The driver hands key
  and pointer events to the desktop, which delivers them through its own
  pointer and keyboard focus, the way `SendInput` and `CGEvent` posting do.
  The driver creates no input device. How GNOME carries those events inside
  itself is not checked: its source is not on this machine.

## 2. Current breakdown

Sections 2 and 3 describe the code as it was before any task of section 5 was
done. Line numbers are from then.

### 2.1 Entry points

| Script | What it runs |
|---|---|
| `build.sh` | `build.sh` of `tools/find-text`, `tools/mini-host`, `tools/vst3-loader`, `markdown-notes`, in that order |
| `test.sh` | `test.sh` of `tools/mini-host`, `tools/vst3-loader`, `markdown-notes` |
| `markdown-notes/build.sh` | `cargo run --release --quiet -p xtask -- bundle --release --target aarch64-apple-darwin` |
| `markdown-notes/test.sh` | `cargo run -p xtask -- test "$@"` |
| `tools/find-text/build.sh` | `swiftc find-text.swift` into `binaries/find-text` |
| `tools/mini-host/build.sh`, `tools/vst3-loader/build.sh` | `cargo build --release`, copy into `binaries/` |

### 2.2 The xtask (`markdown-notes/src/xtask/src`)

- `main.rs`: `bundle` (build the cdylib, assemble `Markdown Notes.vst3`, write
  `moduleinfo.json`, copy the result to `binaries/`, load it back through
  `vst3-loader`), `test` (plugin build, `cargo test --workspace`, scenarios,
  the fifteen pixel tests, and with `--full` the UI tests), `uitest`.
- `uitest.rs`: reads `order.txt` and the step files of two suites
  (`markdown-notes/src/tools/uitests`, `tools/mini-host/src/tools/uitests`),
  rewrites `plugin:` steps and the `%CACHE%` and `%PRESETS%` markers, writes
  `<test>.steps`, calls the platform's `drive`, then checks the `expect:` lines
  against the state file the host wrote.
- `drive` on Windows shells out to `tools/capture-window.ps1`. On macOS it is
  `macos.rs`. Everywhere else it returns an error (`uitest.rs:545`).
- `png.rs`: writes the one-colour PNG a `picture:` step asks for. macOS only.

### 2.3 How the two existing drivers work

| Job | Windows (`tools/capture-window.ps1`) | macOS (`xtask/src/macos.rs`) |
|---|---|---|
| Keys | characters: `SendInput` with real scan codes, one key at a time, a Shift of its own per character, key found with `VkKeyScanW`. Named keys: `SendKeys`. Shortcuts: `SendInput` | `CGEvent` posted at the HID tap, the character set on the event |
| Mouse | `SetCursorPos`, `mouse_event`, glides in 16 ms steps | `CGEvent` mouse events, glides in 16 ms steps |
| Picture | `CopyFromScreen` of the window's region | `Window Shot.app` running `screencapture -R` on the region |
| Film | ffmpeg `gdigrab`, frames counted with `mpdecimate` | stills at 30 a second joined by ffmpeg, same count |
| Text finding | `binaries/find-text.exe` (Windows OCR) | `binaries/find-text` (Vision) |
| Window lookup | `EnumWindows` by process id and title | System Events by process id |
| Host window coordinates | painted frame, title bar included | window position and size, title bar included |
| Dialog coordinates | the dialog's client area | the dialog window |
| File dialog detection | a second window of the host's process | the front window is not the host's |
| Cursor check | compare the cursor handle with the stock ones | compare the cursor picture with the stock ones |
| Clipboard picture | `Clipboard.SetImage` | `NSPasteboard` PNG and TIFF |
| Closing | `WM_CLOSE` posted to the window | click on the close button |
| Geometry | measured by the driver from the child window | written by the host (`--geometry`), copied by the driver |
| Permission | none | Accessibility for input, Screen Recording for Window Shot |

Both read the screen and not the window, so a dialog on top is in the picture.
Both send input to the system and not to the window.

Steps both drivers implement: `type click press showing hidden dialog nodialog
window nowindow dragtext hold moveto letgo cursor shortcut kill restart row
copies dragto geometry remove picture clipboard written shot film endfilm
wiggle`. Only the macOS driver has `wait`, `drag`, `dragedge` and `resize`.
The step files of the two suites use every shared step except `kill` and
`wiggle`, and none of the macOS-only ones.

### 2.4 The host, the loader and the plugin

- `tools/vst3-loader`: loads the binary, resolves a bundle directory to the
  file under `Contents/<arch>-<os>/`, calls `ModuleEntry` on Linux, attaches
  the view with `kPlatformTypeX11EmbedWindowID` on Linux.
- `tools/mini-host`: an eframe window with a 26 px strip (`chrome::HEIGHT`),
  the plugin's editor below it, two preset dialogs as egui viewports.
  `app/place.rs` holds the per-platform code that places, resizes, focuses and
  measures the plugin's window.
- `markdown-notes-plugin`: baseview window parented into the host's
  (`gui::ParentWindow` builds an `Xcb` handle on Linux), file dialogs through
  `rfd` (the xdg portal on Linux), clipboard pictures through `arboard`.

## 3. What is missing

| # | Where | What |
|---|---|---|
| M1 | `markdown-notes/build.sh` | Hardcodes `--target aarch64-apple-darwin`, so on Linux it does not build the Linux plugin. |
| M2 | `tools/find-text/build.sh` | Runs `swiftc`. `./build.sh` stops at its first project on Linux. |
| M3 | `tools/find-text/src/main.rs`, `Cargo.toml` | Windows-only code and dependencies. No Linux reader. |
| M4 | `xtask/src/main.rs:195`, `write_binary` | The Linux bundle is not the documented one. The VST3 Plugin Format page: binary at `<Name>.vst3/Contents/x86_64-linux/<Name>.so`, folder and `.so` share the name, single-file plug-ins deprecated since 3.6.10. The code names the inner file `.vst3` and copies a bare file to `binaries/`. |
| M5 | `markdown-notes/run.sh` | Looks for `.dylib` only. |
| M6 | `mini-host/src/main.rs:397` | `native_handle` accepts `Xcb` only. winit's X11 window gives `Xlib` (`winit-0.30.13/src/platform_impl/linux/x11/window.rs:1884`). The host gets no handle and the editor is never attached. |
| M7 | `mini-host` `Cargo.toml` | eframe has `wayland` and `x11`. On a Wayland session winit picks Wayland, and an X11 plugin window cannot be parented into a Wayland surface. |
| M8 | `mini-host/src/app/place.rs:378` | Every Linux function is empty: the editor is not put under the strip, not resized, not given the keyboard, not measured. |
| M9 | `xtask/src/uitest.rs:545` | No Linux `drive`. |
| M10 | `xtask/src/main.rs:25` | `mod png` is macOS only. |
| M11 | Linux tooling | No screen photographer, no input driver, no window lookup. |
| M12 | `docs/markdown-notes/DEVELOPERS.md`, `docs/mini-host/DEVELOPERS.md`, `docs/markdown-notes/PROJECT_DEFINITION.md` | Line 38 of the first says the Linux result in `binaries/` is the bare plugin binary, which stops being true with M4. Its install folders and its UI test tooling are given for Windows and macOS only. Nothing says what a Linux machine needs installed. `PROJECT_DEFINITION.md`, which `.claude/CLAUDE.md` names as a document to keep current and as where project history goes, lists the build targets, the per-platform build scripts, the status of every criterion and the known gaps, and says nothing of Linux. |

## 4. Implementation

### 4.1 Build scripts and bundle (M1, M2, M4, M5)

`markdown-notes/build.sh`:

```sh
target=
if [ "$(uname -s)" = Darwin ]; then
    target="--target aarch64-apple-darwin"
fi
exec cargo run --release --quiet -p xtask -- bundle --release $target
```

`tools/find-text/build.sh`: the two systems build different sources with
different compilers, so only the compile step is chosen by `uname -s`. Darwin
keeps the `swiftc` line. Linux runs `cargo build --release` in the project,
removes `binaries/find-text` and copies `.cache/release/find-text` there. The
`mkdir -p` before it and the `echo` after it stay shared.

`markdown-notes/run.sh`: add `.cache/release/libmarkdown_notes_plugin.so` and
`.cache/debug/libmarkdown_notes_plugin.so` to the plugin candidates.

`xtask/src/main.rs`:

- New `fn inner_binary_name(triple: &str) -> String`: `Markdown Notes.vst3`
  for Windows, `Markdown Notes.so` for Linux. `bundle` uses it at line 195.
- `write_binary`: copy the whole bundle (`copy_tree`) when the triple is macOS
  or Linux, the bare library otherwise.
- Doc comments on the module, on line 193 and on `write_binary` say what the
  code then does.

`vst3_loader::resolve_binary` already finds `Contents/x86_64-linux/*.so`, so
`verify_bundle`, `write_module_info`, the mini host and `run.sh` take the
bundle directory unchanged.

Untouched: every `.bat`, `capture-window.ps1`, `macos.rs`,
`tools/window-shot`, `find-text.swift`.

### 4.2 find-text on Linux (M3)

Output contract: the Windows one. With a needle: `width height`, then one
`x y width height` per match, exact matches first, exit 1 for none. Without:
`width height`, then `x y width height text` per line read.

`tools/find-text/Cargo.toml`:

```toml
[target.'cfg(windows)'.dependencies]
windows-future = "0.3.2"
windows = { version = "0.62.2", features = [ ...as now... ] }

[target.'cfg(target_os = "linux")'.dependencies]
image = { version = "0.25.10", default-features = false, features = ["png"] }

[target.'cfg(target_os = "linux")'.dev-dependencies]
font-kit = "0.14.3"
pathfinder_geometry = "0.5"
```

`tools/find-text/src/main.rs`:

- Shared and unchanged: `main`'s argument handling, matching and printing,
  `Line`, `Reading`, `words_matching`, `enlargements`.
- `#[cfg(windows)]` on the Windows imports, `wait`, `read`, `at_scale`,
  `deepen`, `one_line` and the `CoInitializeEx` call.
- `#[cfg(target_os = "linux")] mod linux;` and `use linux::read;`.

New `tools/find-text/src/linux.rs`:

- `pub fn read(path: &str) -> Result<Reading, String>`: decode the PNG, then
  for each scale from `enlargements` call `at_scale` and collect the lines.
- `fn at_scale(picture, scale) -> Result<Vec<Line>, String>`: enlarge with a
  cubic filter, pull the greys apart with the Windows reader's formula, encode
  as PNG, pipe it to `tesseract stdin stdout --psm 11 -l eng tsv`, hand the
  output to `parse_tsv`. `man tesseract`: "If FILE is stdin or - then the
  standard input is used", the same for `stdout`, `tsv` is one of its config
  files, and `-l` and `--psm` "must occur before any CONFIGFILE".
- `fn parse_tsv(text: &str, scale: u32) -> Vec<Line>`: level 5 rows are words
  (`left top width height conf text`); words sharing block, paragraph and line
  numbers form one `Line`; boxes are divided by `scale`; rows with no text or a
  negative confidence are dropped.
- `tesseract` not on `PATH`: exit 2 with a message that names it.

The Windows build of this crate changes (cfg attributes, target table) and
cannot be compiled here. No CI job builds find-text either, so that change
stays unverified until the crate is built on Windows. The Windows code itself
is not edited.

### 4.3 vst3-loader

No change. The host hands `Plugin::attach` the socket of 4.4, which is an
`Xcb` handle, and the loader's `native_handle` already accepts that.

### 4.4 mini-host (M6, M7, M8)

`Cargo.toml`:

- remove `"wayland"` from eframe's features (no effect on Windows or macOS);
- `[target.'cfg(target_os = "linux")'.dependencies] x11rb = "0.13"`.

`src/main.rs`:

- `native_handle`: add
  `RawWindowHandle::Xlib(h) => Some(h.window as usize as *mut c_void)`.
- Attaching, Linux only: `place::make_socket(parent, chrome::HEIGHT)` then
  `plugin.attach(&socket)`. Other platforms keep `plugin.attach(cc)`.
- `Host::ui`, Linux only (`#[cfg(target_os = "linux")]`): call
  `place::hold_keyboard(self.handle)` each frame while no dialog is open.

The Windows and macOS modules of `place.rs` are not edited.

`src/app/place.rs`, a new Linux module. The empty module there now is the
catch-all for every system that is not Windows or macOS; it stays for the
others, with its cfg changed to leave Linux out. `make_socket`, `Socket` and
`hold_keyboard` exist on Linux only and are only called from Linux-only code:

- State: one `x11rb::rust_connection::RustConnection`, the socket's window id
  and the strip height, in a `OnceLock<Mutex<_>>`.
- `pub struct Socket(u32)` implementing `HasWindowHandle` with an
  `XcbWindowHandle`.
- `pub fn make_socket(handle, top) -> Result<Socket, String>`: create a child
  window of the host's at `0, top`, sized to the host's width and its height
  less `top`, map it.
- `inset_editor`, `follow_resize`: `ConfigureWindow` on the socket to the
  host's current size less the strip.
- `track_editor`: a thread with an X connection of its own that selects
  `StructureNotify` on the host's window and resizes the socket on every
  `ConfigureNotify`, in that moment. The host draws only when an event
  arrives (nothing in it asks for a repaint), so sizing the socket from its
  frame would be a frame late for every step of a drag, which is what
  `host-resize.txt` and `.claude/memory/windows-redraw-during-resize.md`
  describe as the fault on Windows. baseview then resizes the plugin's window
  to the socket's size on the socket's `ConfigureNotify`
  (`baseview/src/platform/x11/event_loop.rs`, `handle_coalesced_resize_events`).
- `focus_editor`: `SetInputFocus` on the plugin's window (the socket's largest
  child, from `QueryTree`).
- `hold_keyboard`: when `GetInputFocus` names the host's own window, give the
  focus to the plugin's window. The window manager puts the focus on the
  top-level window every time it is activated: at launch, after a restart,
  and when a dialog closes. The baseline test types with nothing clicked, so
  the first of those is what it depends on.
- `editor_in_host`: the socket's position plus the plugin window's geometry.
  With this, `Host::report_geometry` writes `window=`, `host=`, `editor=` and
  `inset=` as it does on macOS.
- `set_enabled`: nothing, as on macOS.

Why a socket window: with the host's whole window as the parent, baseview's
follow-the-parent rule makes the plugin cover the strip. Checked in the
version the lock file names, baseview 0.3.4: the rule is at
`platform/x11/event_loop.rs:175` and `:422`, a parent given as `Xlib` or `Xcb`
is accepted (`platform/x11/mod.rs`), and the window draws on a 15 ms timer in
a thread of its own (`event_loop.rs:125`), whatever the host is doing.

### 4.5 The Linux driver (M9, M10, M11)

`xtask/Cargo.toml`:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
x11rb = { version = "0.13", features = ["xfixes", "resource_manager"] }
ashpd = { version = "0.11", default-features = false, features = ["async-std"] }
pollster = "0.4"
xcursor = "0.3"
arboard = { version = "3.6", default-features = false, features = ["image-data"] }
image = { version = "0.25.10", default-features = false, features = ["png"] }
libc = "0.2"
```

`xtask/src/main.rs`: `#[cfg(target_os = "linux")] mod linux;`, and `mod png`
under `any(target_os = "macos", target_os = "linux")`.

`xtask/src/uitest.rs`: a `#[cfg(target_os = "linux")] fn drive` that calls
`crate::linux::drive`; the fallback excludes Linux.

New files under `xtask/src/`:

| File | Holds |
|---|---|
| `linux.rs` | `pub fn drive(root, host, plugin, steps, state, shot)`, `struct Run` (child, pid, restarts, launch arguments, `reported` geometry file, `host` and current `rect`, `holding`, `title`, film, the clipboard owner), `launch`, `finish`, `play`, `step` |
| `linux/portal.rs` | `struct Portal`: the one portal session that carries both input and the screen stream. `open(cache)`, the stream's node and size, the restore token. `request_open(pid)`: whether a process has a portal request open, which is what a file dialog is |
| `linux/input.rs` | Input through the `Portal`, and the last pointer position. `move_to`, `glide`, `click`, `take_hold`, `let_go`, `wheel`, `tap`, `shortcut`, `write`, `send_keys` |
| `linux/keys.rs` | `keysym_of(char)`, `named(&str)` (the keysym of `ENTER`, `ESC`, `BS`, `END`, `DOWN`, `TAB`), `modifier(&str)`, `parse_keys(step) -> Vec<Stroke>` |
| `linux/xwindows.rs` | `struct Desktop`: X connection and atoms. `window_of(pid, title)`, `frame_rect`, `client_rect`, `activate`, `close`, `pointer`, `cursor_image` |
| `linux/cursor.rs` | `stock(theme)`, every size of each named cursor in the theme, and `name_of(shown, stock)` giving `ibeam`, `arrow`, `hand`, `grabbing` or `other` |
| `linux/screen.rs` | Pictures and films from the `Portal`'s stream. `shot(area, to)`, `film(area, to)`, `Film::stop`, `cut(area, stream, frame_size)` (the region of a frame, in its pixels) |
| `linux/look.rs` | `finder()`, `find_all(picture, text)`, `read_all(picture)`, `nearest(boxes, near)`, `find_on_screen`, `find_box_on_screen` |

The portal session (`portal.rs`):

- The session is opened before the first host is started, so GNOME's dialog
  is answered before there is a window for it to take the focus from.
- One session per xtask process, made with `RemoteDesktop::create_session`.
  `RemoteDesktop::select_devices` asks for the keyboard and the pointer,
  `Screencast::select_sources` adds the monitor to the same session, and
  `RemoteDesktop::start` raises GNOME's one permission dialog for both.
- Persistence is asked for in `select_devices` only: persist mode "until
  revoked", with the restore token read from and written to
  `markdown-notes/.cache/portal-token`. `select_sources` asks for none and
  gets no token. The portal documentation requires this for a combined
  session: "the persistence options in ScreenCast.SelectSources must not be
  used with a remote desktop session".
- This desktop's portal is RemoteDesktop version 2 and ScreenCast version 5,
  so the stream is addressed by its PipeWire node id, which only version 6
  deprecates.
- The driver cannot see GNOME's dialog. It prints one line before `start`
  saying the dialog may be up, and one after saying how long `start` took.
  An answer within two seconds is the grant having been restored with nobody
  touching the machine. `start` is waited on for up to 200 seconds, the time
  the macOS driver gives its photographer's first prompt.
- A refusal, or no portal on the bus, stops the run with a message, as
  `may_post_input` does on macOS.

Input (`input.rs`, `keys.rs`):

- What Windows does with `SendInput` and macOS with `CGEvent` at the HID tap
  is post events into the system's input stream for the keyboard and pointer
  the session already has. The portal's `Notify*` calls are that on this
  desktop: GNOME moves its own pointer and delivers the keys to whichever
  window has the focus. Nothing is sent to the window itself.
- Pointer: `notify_pointer_motion_absolute(stream, x, y)`. The portal
  documentation puts x and y in "the stream's logical coordinate space", so a
  point in a window is its X position less the stream's reported position.
  `notify_pointer_button(BTN_LEFT, pressed/released)` takes the evdev button
  code, and `notify_pointer_axis_discrete(Vertical, steps)` is the wheel.
- Keys: `notify_keyboard_keysym(keysym, pressed/released)`, one character per
  keystroke. The keysym carries the character, so the layout in use does not
  change what arrives (the macOS driver sets the character on the event for
  the same reason).
- Named keys: `ENTER`, `ESC`, `BS`/`BACKSPACE`, `END`, `DOWN`, `TAB`, with
  `{NAME n}` repeats and `{(}` `{)}` literals, the notation the step files use.
- `shortcut:`: the modifier's keysym (`Control_L`, `Shift_L`, `Alt_L`) held
  over the key's. No remapping.
- Timing follows the existing drivers: 16 ms glide steps, a settle before a
  press, a hold before a release, about 20 ms per keystroke.
- Input must never land in another program's window. Before a step clicks,
  the root's `_NET_ACTIVE_WINDOW` has to name the addressed window. Before a
  step types, one of two things has to hold: that same check, or the host's
  process has a portal request open, which is a file dialog being up. In the
  second case what is typed is for the dialog: the path, and the `{ENTER}`
  the test confirms it with, which is a plain `type:` step. When neither
  holds, the driver asks once for the addressed window to be brought to the
  front, the way `window:` does, and checks again: a file dialog that has
  just closed may not have handed the focus back. If the window is still not
  the active one the step fails and nothing is sent. Whether the dialog
  itself has the keyboard cannot be read from outside it (U14).
- Nothing is left pressed. When a step fails, or the run ends, the driver
  releases the mouse button if `hold:` left it down and any modifier a
  `shortcut:` was holding, before it returns. A button or a Ctrl left down
  would stay down on the desktop of the person at the machine.
- At the end of each move onto one of the host's windows the driver reads
  `QueryPointer` and fails if the pointer is not where it was sent. X only
  knows the pointer while it is over an X window, so a move onto a file
  dialog, which is not one, is not checked this way.

Windows and geometry (`xwindows.rs`):

- Launch: the host is started with the plugin, `--state` and `--geometry`. Its
  window is waited for by process id and title for up to 30 seconds, brought to
  the front, and then looked at until text can be read below the strip, which
  is the plugin having drawn. This is the Windows driver's `Wait-Loaded`.
- A window is found in `_NET_CLIENT_LIST` by `_NET_WM_PID` and its title.
  This desktop's window manager lists `_NET_CLIENT_LIST`, `_NET_WM_PID`,
  `_NET_ACTIVE_WINDOW` and `_NET_FRAME_EXTENTS` in `_NET_SUPPORTED` on the X
  root, and winit sets `_NET_WM_PID` on its windows.
- The window under test is addressed by its frame (client area grown by
  `_NET_FRAME_EXTENTS`), the Windows and macOS convention. A dialog is
  addressed by its client area, the Windows convention.
- Front: `_NET_ACTIVE_WINDOW` client message, then wait until the root's
  `_NET_ACTIVE_WINDOW` names the window.
- Close: `WM_DELETE_WINDOW` client message, then wait for a clean exit as
  `Run::finish` does on macOS. A crash on the way out fails the test.
- Sizes for `dragto:` and `geometry:` come from the host's `--geometry` file,
  the macOS way.
- Read from this desktop's X resources: `Xft.dpi` is 96, so the host and the
  plugin both draw at a scale of 1 and a step's coordinates are X pixels.
  `Xcursor.theme` is `Yaru`, and that theme has `text`, `xterm`, `left_ptr`,
  `hand2`, `hand1`, `closedhand` and `grabbing`, the names baseview 0.3.4
  loads (`platform/x11/cursor.rs`).

Screen (`screen.rs`, `look.rs`):

- The stream is the monitor stream of the `Portal`'s session, cursor hidden.
- A picture: `open_pipe_wire_remote`, then
  `gst-launch-1.0 -q pipewiresrc fd=N path=NODE num-buffers=1 ! videoconvert !
  pngenc ! filesink location=FRAME`, five second limit. That is the whole
  monitor. The driver cuts the region out of it with the `image` crate and
  writes the PNG the step asked for.
- The portal reports the stream's position and size in the compositor's
  logical space and says the size "may not be equivalent to a size in a pixel
  coordinate space". A frame is in pixels. So the cut is scaled by the frame's
  width over the stream's logical width, and a scale other than 1 is handled
  and not assumed.
- A film: `gst-launch-1.0 -e pipewiresrc ... ! videorate !
  video/x-raw,framerate=30/1 ! videoconvert ! videocrop ... ! x264enc ! mp4mux !
  filesink`, ended with SIGINT. `videocrop` needs its numbers in pixels before
  the recording starts, so the scale comes from one still taken when `film:`
  begins. Frames are then counted with ffmpeg and `mpdecimate` and written as
  `frames=` and `distinct_frames=`, as on the other platforms.
- Text finding calls `binaries/find-text`, built through its `build.sh` when
  it is not there, as `macos.rs` does.

Steps:

| Step | Linux behaviour |
|---|---|
| `type:` | `send_keys`. A value starting with `/` is a path for a file dialog, see below |
| `click:` `hold:` `moveto:` `letgo:` | the pointer functions of `input.rs`, in the addressed window's coordinates |
| `press:LABEL\|X,Y` | put the pointer on the title bar first when the window is the host's (a pointer left on a control draws it highlighted, the Windows driver's reason), photograph the window and take the match nearest X,Y, looking again until it is there or five seconds have gone; then click it. No match fails, keeps the picture and says what was read |
| `showing:` `hidden:` | photograph the window, look below Y until it is as the step says, five second limit; a failure says what was read below Y |
| `dragtext:` | find the text below Y, looking until it is there or five seconds have gone; press on its first glyph, let go at X,Y |
| `window:` `nowindow:` | X11 window of the host's process by title, waited for up to four seconds (`window:`) or waited to go for up to three (`nowindow:`), the Windows driver's limits. `window:` brings it to the front, as both drivers do; a new dialog is waited for until text can be read in it |
| `dialog:WORD` `nodialog:WORD` | the file dialog is the portal's window, not the host's, and not an X window. Whether one is up is asked of the portal, not read off the screen: a dialog is a request object at `/org/freedesktop/portal/desktop/request/SENDER/TOKEN` that "will stay alive for the duration of the user interaction" (portal documentation), the portal's object tree lists them by sender (seen here for its `session` node), and `GetConnectionUnixProcessID` says which process a sender is. `dialog:` passes when the host's process has one, `nodialog:` when it has none. This is the Windows driver's test, a second window of the host's process, in the form this desktop has. Words on the monitor cannot decide it: any other window showing `Cancel`, this conversation included, would hold `nodialog:` up for ever. WORD is then read off the monitor, on the same row as `Cancel`, to say which dialog it is. `dialog:` waits up to ten seconds, `nodialog:` up to three, the Windows driver's limits |
| `cursor:X,Y\|NAME` | move there, compare the XFixes cursor picture with the theme's pictures for the names baseview loads (`text`/`xterm`, `left_ptr`, `hand2`/`hand1`, `closedhand`/`grabbing`), two second limit |
| `shortcut:` | modifier held over the key |
| `row:DIR\|NAME` | look for the row, click it; wheel down and look again when it is not on screen |
| `dragto:W,H,MS` | take the frame's bottom right corner, glide by the difference from the reported size over MS milliseconds, let go, wait up to two seconds for the reported size to be W by H |
| `geometry:PATH` | copy the host's report once the editor fills the host, or as it stands after two seconds, the Windows driver's limit |
| `picture:` | `png::solid`, the colour the other drivers use |
| `clipboard:PATH` | `arboard::Clipboard::set_image`, owner kept in `Run` |
| `shot:` `film:` `endfilm:` | the functions of `screen.rs`. `shot:` is of the addressed window. `film:PATH\|W,H` records a region of that size from the top left of the window under test, or the window's own size with no `\|W,H`, as both drivers do; `endfilm:` writes `PATH.txt` with `frames=` and `distinct_frames=` |
| `restart:` `copies:` `remove:` `written:` | as `macos.rs`. `written:` looks for up to five seconds; `restart:` waits ten seconds for the host to close and thirty for its new window |

Not implemented on Linux: `kill`, `wiggle`, `wait`, `drag`, `dragedge`,
`resize`. No test in either suite uses them, so nothing could show them
working. A step file that uses one fails on Linux with `unknown step`.

After the last step the window under test is photographed to the test's PNG,
whichever window the steps were addressing, as both existing drivers do. A
film still rolling after a failed step is stopped, as the macOS driver does;
the Windows driver leaves it.

The host is never left running. Whatever a step failed on, the window not
appearing at launch included, the driver asks the host to close, waits ten
seconds, and kills it if it is still there.

File dialog paths: stage pictures `dialog-<n>-<stage>.png` in
`.cache/uitests`, as `macos.rs` takes them. Precondition: the host's process
has a portal request open, so nothing is typed into the editor by mistake.
Then Ctrl+L, the path typed, and the test's
own `{ENTER}` confirms. Whether a Save dialog takes the whole path in one go
or needs the directory and the name separately is unknown U6.

Unknowns, each settled by a named test during an allowed UI run:

| # | Unknown | Settled by | If it does not hold |
|---|---|---|---|
| U1 | A new reader of the portal stream gets a frame while the screen is still | `typing-goes-into-the-document` (`showing:`) | keep one reader attached for the whole run and take its latest frame |
| U2 | `open_pipe_wire_remote` can be called once per picture | same | one reader for the run, as U1 |
| U3 | A position given to the portal in stream coordinates is the position X reports for the pointer | `editing-the-document`: the `QueryPointer` check on its first `cursor:` step. The baseline test never moves the pointer | convert between the two with the measured ratio and offset |
| U9 | A modifier pressed by keysym is held when the next key arrives, and a keysym that needs Shift arrives as that character | `editing-the-document` (`shortcut:` steps, and the `X` it types) | send keys with `notify_keyboard_keycode`: the key's evdev code, inside a Shift of its own when the layout X reports says so, which is the Windows driver's way |
| U4 | Raw step coordinates hit their targets with GNOME's title bar height | `editing-the-document` | stop; the choice touches step files shared with Windows and macOS and is the user's |
| U5 | The frame's bottom right corner is a resize grip | `sections-and-files`, whose `dragto:` is the first one run | move the grab point along the frame edge, found from the test's pictures |
| U6 | How GNOME's Open and Save dialogs take a typed path | `sections-and-files` stage pictures | directory first, then the name, as `macos.rs` does |
| U7 | XWayland reports the cursor's picture through XFixes | `editing-the-document` (`cursor:`) | read the cursor from the picture of the screen with the portal's cursor mode on |
| U8 | The plugin has the keyboard once its host's window is at the front, with nothing clicked | `typing-goes-into-the-document`, which types without clicking. No test types straight after a dialog closes: each clicks or restarts first | adjust `hold_keyboard` |
| U10 | Reading a picture through `tesseract` at each of its enlargements (three for a window, two for the whole monitor, by the rule `enlargements` already has) is quick enough for the steps that look until a deadline | `typing-goes-into-the-document` (`showing:`) | read large pictures at fewer enlargements |
| U11 | The portal lists the host's request while its file dialog is up, and the dialog shows `Cancel` and the word the step names on one row. The second part reads the whole monitor, so another window showing both words on one row could pass it wrongly | `sections-and-files` (`dialog:Save`, `nodialog:Save`) | for the first: fall back to reading the monitor and report that other windows can fool it. For the second: use the words its stage pictures show |
| U13 | GNOME brings the window to the front on an `_NET_ACTIVE_WINDOW` request. It declares the property supported; whether it honours a request from a program that is not a pager is not known | `typing-goes-into-the-document` (the first `type:`) | stop and report it. The driver does not click a title bar it cannot tell is on top: the click could land in another program |
| U14 | A file dialog has the keyboard when it opens | `sections-and-files` (the first path typed, by its stage pictures) | stop and report it. The driver does not click where it only read words: they could be another program's |
| U15 | GNOME restores the grant from the token, so its dialog comes up once and not on every run | the second UI run of T5: `start` answers within two seconds | stop and report it: every run would then need the dialog answered |
| U18 | `tesseract` reads the plugin's and the host's lettering: dark on light, light on dark, and white on the host's blue strip | `typing-goes-into-the-document` for the plugin, `opening-a-note` (`press:Save preset`) for the strip | change the enlargements, the grey-pulling or the page mode in `linux.rs`. If a word in a shared step file still cannot be read, stop: the file is shared with Windows and macOS |
| U16 | The host writes its geometry report with no input arriving | T4's `the_editor_fills_the_window_below_the_strip` | on Linux the host asks for a repaint ten times a second while `--geometry` is given |
| U17 | The plugin keeps drawing while the host's corner is dragged, so its text keeps its size | `host-resize`, by the frames of its film | stop and write the fix into this plan as an item of its own |

### 4.6 Documentation (M12)

`docs/markdown-notes/DEVELOPERS.md`, three places:

- line 38: what `binaries/` holds on Linux;
- the install folders: Linux, as `docs/vst3-loader/USING.md` gives it;
- the UI test section: the Linux tooling beside the Windows and macOS ones.
  The portal's permission dialog, how input and pictures work, and what has
  to be installed (`tesseract-ocr`, GStreamer with `pipewiresrc`, ffmpeg).

`docs/mini-host/DEVELOPERS.md`: how the host holds the plugin's window on
Linux.

`docs/markdown-notes/PROJECT_DEFINITION.md`:

- Status: what Linux adds to A1 (build targets) and W7 (per-platform build
  scripts), each stated as verified only for what a test or a run showed.
- Known gaps: what is not done or not proven on Linux by the end of this
  plan. Known already: no drop target for files, and any unknown of 4.5 that
  ended in "stop and report".
- The criteria lists are the user's. No criterion is added or reworded there
  by this work.

Nothing else in these files is rewritten. The root `README.md` is not edited.

### 4.7 What changes for Windows and macOS

Files those platforms also build or run, and what shows the change is safe
there. None of it can be run on this machine.

| File | Change | Shown safe by |
|---|---|---|
| `markdown-notes/build.sh`, `markdown-notes/run.sh`, `tools/find-text/build.sh` | Darwin keeps the commands it had; Linux gets its own | nothing until they are run on a Mac. CI builds the plugin by calling cargo itself and runs none of these three scripts |
| `xtask/src/main.rs` | `inner_binary_name`, `write_binary`, the `frames` task, module lines | CI: `cargo run -p xtask -- test` on Windows and macOS compiles it and runs its tests |
| `xtask/src/uitest.rs`, `xtask/Cargo.toml` | a Linux `drive` and Linux-only dependencies | the same CI job compiles them out |
| `mini-host` `main.rs`, `place.rs`, `Cargo.toml` | an `Xlib` arm in a match, Linux-only code and a Linux-only dependency, eframe's `wayland` feature dropped | CI: `cargo test` in `tools/mini-host` on Windows and macOS |
| `tools/find-text` `main.rs`, `Cargo.toml` | cfg attributes and target tables | nothing: no CI job builds find-text |
| `markdown-notes-plugin/src/fonts.rs` (T3.4, T3.5) | the two loaders call `name_of(font)`, which on Windows and macOS is `font.full_name()`, the call they made before | the same CI job, which runs the plugin's tests |
| `markdown-notes-plugin/src/gui.rs` (T3.7) | the document's scroll area is built in two statements so that one Linux-only line can go between them. Windows and macOS make the calls they made before | the same CI job: the pixel tests draw through this code |
| `markdown-notes-plugin/src/gui.rs` (T4.6) | `open` keeps the result of making the window in a variable so that a Linux-only second try can go after it. Windows and macOS make the calls they made before | the same CI job compiles it. No test there opens the plugin's window: that is the UI tests, which CI does not run |
| `xtask/src/main.rs` (T3.12) | two Linux-only lines that set `RUST_TEST_THREADS` | the same CI job compiles them out |
| `markdown-notes-plugin/tests/caret_in_view.rs` (T3.7, T3.8) | Linux-only lines, and for every platform: `changing_the_view_arrives_at_the_caret` keeps its rendering when it fails | the same CI job runs the test. It passes there by the same assertion as before |
| `markdown-notes-plugin/tests/selection_rendering.rs` (T3.10), `title_field.rs` (T3.15) | Linux-only lines | the same CI job compiles them out |
| `markdown-notes-plugin/tests/source_view.rs` (T3.13) | the heading may measure `SLACK` rows taller than the body. `SLACK` is 0 on Windows and macOS, which is the check they had | the same CI job runs the test |

### 4.8 Added by this plan and not asked for

Each can be struck out.

- The host's socket window and the thread that resizes it (4.4). Without the
  socket the plugin covers the host's strip; without the thread it trails the
  host's window by a frame during a drag.
- `the_editor_fills_the_window_below_the_strip` (T4.1). Without it the host
  task has nothing of its own to show it works.
- The tests in find-text (T2.4, T2.5, T2.7) and the driver's unit tests (T5).
  The existing find-text and drivers have none.
- The `frames` task of xtask (T7.3). Without it the frames of a film are
  taken out by a command typed once and thrown away, which the rules forbid.
- Two checks in the driver (4.5): that the window about to get input is the
  active one, and that no button or modifier is left pressed. Neither existing
  driver makes them.

### 4.9 Rules that bind the work

From `.claude/CLAUDE.md` and `.claude/memory/`:

- Nothing is created, changed or deleted outside the repository. System
  packages are not installed by this work. `tesseract` 5.5.0 with the `eng`
  language is on the machine, as are `gst-launch-1.0` with `pipewiresrc` and
  ffmpeg.
- A UI run needs a yes first, per set of runs, and each run is announced. No
  screen capture and no input outside a test run.
- Every check is a saved, named test. No scratch step files and no throwaway
  command lines: taking the frames out of a film is an xtask task (T7.3), not
  an ffmpeg line typed by hand.
- A file that differs from what was last written was changed by the user. It
  is read again before it is touched, and never put back.
- Only the tests a change affects are run, by name. A failed step is redone,
  not the procedure.
- A deleting command runs only when asked for. The ones this plan runs:
  - `./build.sh` and each project's own `build.sh` replace that project's
    result in `binaries/`: `find-text`, `mini-host`, `vst3-loader`,
    `Markdown Notes.vst3`;
  - `./test.sh` and `markdown-notes/test.sh` remove
    `binaries/Markdown Notes.vst3` before building;
  - a UI run rebuilds the plugin and the host (replacing both in `binaries/`),
    empties `markdown-notes/.cache/uitests` and carries out the step files'
    `remove:` and `cleanup:` lines there and in
    `binaries/presets/Markdown Notes/`.
- No git. A file is copied to the scratchpad before each edit.
- Existing files are edited, new ones written, moves are `mv`.
- LF line endings, no em dashes, no sectioning comments, no conversation in
  source files.
- Command output is read whole.
- VST3 questions are answered from the VST3 documentation, the SDK and the
  crates.
- Counterparts are added. Nothing that works on Windows or macOS is removed,
  and the shared step files are not edited.
- The picture of each UI test is sent after its run.

## 5. Task list

A task is one `T` heading. Its items are done in the order written, and the
task ends with a check that shows it works.

Work stops after each task is demonstrably completed, so that the user can
form a git commit. The next task does not start until that has been done.

Order and what each task needs: T1, then T2, then T3 (needs T1 and T2), then
T4 (needs the plugin T3 builds), then T5 (needs T2, T3 and T4), then T6, T7,
T8, T9.

### T1. Build scripts and the bundle layout (M1, M4, M5)

- [x] T1.1 `markdown-notes/build.sh`: the target in a variable, set on Darwin
      only.
- [x] T1.2 `markdown-notes/run.sh`: `.so` candidates.
- [x] T1.3 `xtask/src/main.rs`: `inner_binary_name`, `write_binary` copies the
      bundle on Linux, doc comments.
- [x] T1.4 Unit test `the_linux_bundle_holds_a_so_named_after_the_bundle` in
      `xtask/src/main.rs`.

Done when: `cargo test -p xtask the_linux_bundle` in `markdown-notes` fails
against the old naming and passes after T1.3. The scripts themselves are run
in T3.

### T2. find-text on Linux (M2, M3)

- [x] T2.1 `Cargo.toml`: target tables, description, and for Linux only the
      dev-dependencies T2.5 draws with: `font-kit` and `pathfinder_geometry`,
      whose types font-kit's drawing calls take and font-kit does not
      re-export.
- [x] T2.2 `src/main.rs`: cfg attributes, `mod linux`.
- [x] T2.3 `src/linux.rs`: `read`, `at_scale`, `parse_tsv`.
- [x] T2.4 Unit tests in `src/linux.rs`:
      `a_word_row_becomes_a_word_with_its_box_scaled_back`,
      `words_on_one_line_are_one_line_in_reading_order`,
      `rows_that_are_not_words_are_dropped`; in `src/main.rs`:
      `a_run_of_words_is_boxed_on_its_own`.
- [x] T2.5 Tests that run the program on a picture the test makes itself, in
      `tools/find-text/tests/the_program.rs`, Linux only. The test draws three
      lines of words at places it chooses, in the system's sans-serif face
      (loaded with `font-kit`, a dev-dependency), writes the PNG under
      `tools/find-text/.cache/tests/`, runs `find-text` on it, and checks what
      the program prints and its exit code:
      `words_drawn_into_a_picture_are_read_where_they_were_drawn` (exit 0, the
      picture's size, and every box within a few pixels of where the words
      were drawn), `words_that_were_not_drawn_are_not_found` (exit 1 and
      nothing printed, on a picture in which a drawn word is found),
      `an_exact_match_is_printed_before_one_that_holds_the_text`,
      `with_no_text_every_line_read_is_printed`. No picture that existed
      before the test is read.
- [x] T2.6 `tools/find-text/build.sh`: the compile step chosen by `uname -s`
      (edit made during T1).
- [x] T2.7 Tests of the program's errors, in the same file, each checking
      exit 2 and the message: `a_file_that_is_not_there_is_an_error`,
      `a_file_that_is_not_a_picture_is_an_error`,
      `without_tesseract_the_error_says_so` (the program is run with a `PATH`
      that holds no programs).
- [x] T2.8 Every test of T2.4, T2.5 and T2.7 is seen to fail. The code the
      test covers is broken, the test is run by name and fails, and the code
      is put back. The breaks:
  - [x] `parse_tsv` does not divide by the scale:
        `a_word_row_becomes_a_word_with_its_box_scaled_back`,
        `words_drawn_into_a_picture_are_read_where_they_were_drawn`.
  - [x] `parse_tsv` ignores the line number:
        `words_on_one_line_are_one_line_in_reading_order`.
  - [x] `parse_tsv` never joins words:
        `with_no_text_every_line_read_is_printed`.
  - [x] `parse_tsv` keeps rows of other levels, keeps rows with no text,
        keeps rows with a negative confidence, one at a time:
        `rows_that_are_not_words_are_dropped`, three times.
  - [x] `parse_tsv` reports a word that is not in the table:
        `words_that_were_not_drawn_are_not_found`.
  - [x] `words_matching` keeps the first run it finds:
        `a_run_of_words_is_boxed_on_its_own`.
  - [x] `main` prints the containing matches first:
        `an_exact_match_is_printed_before_one_that_holds_the_text`.
  - [x] `read` calls every failure "not a picture":
        `a_file_that_is_not_there_is_an_error`.
  - [x] `read` calls every failure "could not read":
        `a_file_that_is_not_a_picture_is_an_error`.
  - [x] `at_scale` has no message of its own for a missing `tesseract`:
        `without_tesseract_the_error_says_so`.

Done when: every break of T2.8 has made its test fail, `cargo test` in
`tools/find-text` passes with the code put back, and
`tools/find-text/build.sh` ends with its `binary:` line and
`binaries/find-text` exists.

Not shown by T2:

- The Windows build of find-text (4.2).
- `binaries/find-text` itself. The tests run the build of the program that
  `cargo test` makes. `binaries/find-text` is the release build of the same
  source, and the first thing to run it is the driver, in T5.

### T3. Everything builds and the tests without a desktop pass (goal)

- [x] T3.1 `./build.sh` from the root. Done when: it ends with "all projects
      built", `binaries/Markdown Notes.vst3/Contents/x86_64-linux/Markdown Notes.so`
      exists and `bundle` prints its `loads:` line.
- [x] T3.2 `./test.sh` from the root, no `--full`: unit tests, scenarios,
      headless pixel tests. Done when: it ends with "all projects passed".
- [x] T3.3 Every failure in T3.1 or T3.2 is written into this list as an item
      of its own before it is worked on, and only the failing test is rerun.
      A failure that needs something installed on the system is reported and
      not worked around.
- [x] T3.4 Failure in T3.2, `markdown-notes-plugin`:
      `fonts::tests::bold_is_a_different_face_from_the_regular_one`. "bold
      measured 192.0625 against a regular 192.0625: the same width means the
      same face, drawn in another colour".
- [x] T3.5 Failure in T3.2, `markdown-notes-plugin`:
      `gui::tests::the_caret_lands_after_bold_text_not_inside_it`. "the bold
      face should be wider: galley 111, one font 111.78125".

      The cause of both, read from the code: `install_base` in
      `markdown-notes-plugin/src/fonts.rs` files each face under
      `font.full_name()` and keeps the first face filed under a name. On
      FreeType, font-kit's `full_name` only reads name records of the Apple
      Unicode platform (`loaders/freetype.rs`, `get_type_1_or_sfnt_name`) and
      otherwise gives the family name. This machine's sans-serif is Noto Sans,
      with `NotoSans-Regular.ttf` and `NotoSans-Bold.ttf`, and both come back
      named "Noto Sans". The bold face is dropped and bold text is drawn in
      the regular one.

      The fix, in `fonts.rs`: a face is filed under `name_of(font)`. On
      Windows and macOS that is `font.full_name()`, as before. Everywhere else
      (`#[cfg(not(any(target_os = "windows", target_os = "macos")))]`, the
      systems the FreeType loader serves) it is the face's PostScript name,
      which the regular and the bold do not share. The two failing tests are
      the tests of it: both failed before it and pass with it.
- [x] T3.6 The first run of T3.2 stopped at those two failures. What comes
      after the plugin's unit tests in `markdown-notes/test.sh` has not run
      yet: the rest of `cargo test --workspace`, the scenarios and the
      headless pixel tests. They are run with `markdown-notes/test.sh` once
      T3.4 and T3.5 pass.
- [x] T3.7 Failure in the run of T3.6, `markdown-notes-plugin`, pixel test
      `caret_in_view`: `changing_the_view_arrives_at_the_caret`. "the
      formatted view is not at the caret".
- [x] T3.8 Failure in the run of T3.6, same file:
      `the_caret_at_the_end_of_a_tall_document_is_in_view`. "the caret is at
      the end of a document 60 lines long and nothing is drawn on screen, so
      the document was not scrolled to it".

      The cause of T3.8, shown by the test passing once it was removed: the
      test's harness draws a frame as it is built, before the test installs
      the system's fonts. That frame is laid out in egui's built-in font, and
      the document is scrolled to its caret then and not again. Noto Sans has
      taller rows than the built-in font, so when it arrives the caret is
      below the window. A real window installs the fonts before its first
      frame (`gui.rs`, `build`). The fix is in the test file, for Linux only
      (`#[cfg(target_os = "linux")]`): the harness's first frame installs the
      fonts and draws nothing. What Windows and macOS run in this test is as
      it was.

      The cause of T3.7, read off the scroll position on each pass: after a
      caret move `document` asks the scroll area for the caret's place and
      has the pass drawn again, so that the pass shown has the caret in it.
      egui's scroll area, left at its default of animated scrolling, takes
      the new offset up at the start of the next pass, after that pass's
      contents are placed, so the pass drawn again still shows the old place
      and the caret arrives one frame later. The test looks at the first
      frame after the click. On this machine the formatted view is taller
      than the source view (Noto Sans against the monospace face), so the
      caret is out of sight in that frame. The fix is in `gui.rs`:
      `.animated(false)` on the document's scroll area, which makes a scroll
      target take effect in the pass that asks for it. It is the only use
      egui makes of that setting. It is compiled for Linux only
      (`#[cfg(target_os = "linux")]`), so Windows and macOS build what they
      built before. The one-frame delay is not particular to Linux: it is in
      egui. Keeping the fix to Linux is a choice made here, because nothing
      on this machine can show what it does to the other two. Taking the
      `cfg` line away applies it everywhere.

      Also changed in the test file, for every platform: when this test fails
      it now writes its rendering to `.cache/uitests` and says where, as the
      other two tests in the file do.
- [x] T3.9 The run of T3.6 stopped at those two. The pixel tests after
      `caret_in_view` have not run yet. They are run with
      `markdown-notes/test.sh` once T3.7 and T3.8 pass.
- [x] T3.10 Failure in the run of T3.9, `markdown-notes-plugin`, pixel test
      `selection_rendering`:
      `a_selection_ending_mid_line_leaves_the_words_after_it_alone`. "the
      selection ends on line 0 of 3, which is not a line with text above and
      below it".

      The rendering the test kept shows the selection drawn as it should be.
      The cause is in how the test finds lines: it takes rows of dark pixels
      with a blank row between them as separate lines, and the caret is as
      dark as the words and a whole row tall. In Noto Sans its top touches
      the descenders of the row above, so the two rows are read as one line.
      Shown by the test passing with the caret not drawn. The fix is in the
      test file, for Linux only (`#[cfg(target_os = "linux")]`): the caret is
      given a colour with no opacity. What Windows and macOS run in this test
      is as it was.
- [x] T3.11 The run of T3.9 stopped at that one. The pixel tests after
      `selection_rendering` have not run yet. They are run with
      `markdown-notes/test.sh` once T3.10 passes.
- [x] T3.12 Failure in the run of T3.11, `markdown-notes-plugin`, pixel test
      program `section_dragging`: the program died with "signal: 11, SIGSEGV:
      invalid memory reference" straight after printing "running 9 tests",
      before any test reported. The same program passed all nine tests in the
      run of T3.9, and nothing it is built from changed in between.

      The cause, from the backtrace of the crash under `gdb`: the fault is
      inside `/usr/lib/x86_64-linux-gnu/libvulkan.so.1` (the system's Vulkan
      loader, package `libvulkan1` 1.4.341.0-1), called from wgpu naming an
      object (`set_debug_utils_object_name`) while it makes a device. Every
      pixel test makes a device of its own, and the test harness runs the
      tests of one program on as many threads as there are tests, so several
      devices are made at the same moment. Run again on its own the program
      passed; under `gdb` it crashed on the second try.

      The crash is in a system library and is not fixed here. What is
      changed, in `xtask/src/main.rs`, for Linux only
      (`#[cfg(target_os = "linux")]`): the pixel tests are run with
      `RUST_TEST_THREADS=1`, one test at a time, so no two devices are made
      at once. Windows and macOS do not use this loader and run the tests as
      before.

      Shown by running the program 20 times under `gdb` each way: on many
      threads it crashed 7 times, on one thread not once. Run 30 times
      without `gdb` it did not crash either way, so the crash is rare in an
      ordinary run and one clean run does not show it is gone.
- [x] T3.13 Failure in the next run of `markdown-notes/test.sh`,
      `markdown-notes-plugin`, pixel test `source_view`:
      `a_heading_is_no_larger_than_body_text_in_the_source_view`. "in the
      source view the heading is 14 rows tall and the body 11 ([(100, 113),
      (139, 149)]), so it is still drawn as a heading".

      The rendering the test kept shows both lines at one size. The cause is
      in how the test measures a line: from its first row with dark pixels to
      its last, where dark is a pixel four fifths covered or more. Counted
      row by row, the two lines have the same eight rows of small letters,
      thirty-six rows apart. The three rows above them are dark in the
      heading (`H`, `d`) and not in the body (`b`, `d`): in this machine's
      monospaced face the stems are thin, and whether one is dark enough
      depends on where its letter falls on the pixel grid. The fix is in the
      test file, for Linux only (`#[cfg(target_os = "linux")]`): the heading
      may measure up to four rows taller than the body. Windows and macOS
      keep the check they had. Shown to still catch the fault it is for: with
      the source view made to draw headings large, the test failed with the
      heading 28 rows tall against 15.
- [x] T3.14 That run stopped there. The pixel tests after `source_view` have
      not run yet: `text_area`, `theme_rendering`, `title_field` and
      `view_mode_button`. They are run with `markdown-notes/test.sh` once
      T3.13 passes.
- [x] T3.15 Failure in the run of T3.14, `markdown-notes-plugin`, pixel test
      `title_field`:
      `the_caret_stays_in_view_while_a_name_longer_than_fits_is_typed`. "no
      caret is in view after typing past the end of the field".

      The rendering the test kept shows the caret at the end of the field,
      where it should be. The cause is in where the test looks for it: in the
      columns from the field's left edge up to its right edge with the
      fraction dropped. Read off the picture, the field here runs from 18.0
      to 379.7 and the caret is on columns 379 and 380, so the columns looked
      at stop one short of it. The fix is in the test file, for Linux only
      (`#[cfg(target_os = "linux")]`): the column the right edge falls in
      counts as the field's. Windows and macOS keep the range they had.
- [x] T3.16 The run of T3.14 stopped there. `view_mode_button` has not run
      yet. It is run with `markdown-notes/test.sh` once T3.15 passes.
- [x] T3.17 The plugin's code changed after the build of T3.1 (`fonts.rs`,
      `gui.rs`), and so did the xtask. `markdown-notes/build.sh` is run again
      on the code as it now is. Done when: it prints its `loads:` line and
      the `.so` is in the bundle in `binaries/`. The other three projects of
      `./build.sh` have not changed since T3.1.

What T3 changed that was not written in this plan before it started, and
which Windows and macOS also build, is listed in 4.7.

### T4. The host on Linux (M6, M7, M8)

- [x] T4.1 New test file
      `tools/mini-host/src/crates/mini-host/tests/editor_under_the_strip.rs`,
      Linux only, `#[ignore]` because it opens a window:
      `the_editor_fills_the_window_below_the_strip` starts the host on
      `binaries/Markdown Notes.vst3` with `--geometry`, waits up to 30 seconds
      for a report with `editor=0,26,` and `inset=0,26,0,0`, then kills the
      host. The bundle is built first with `markdown-notes/build.sh`, since
      T3.2 removed it.
- [x] T4.2 Run it against the host as it is. Needs a yes: a window opens.
      Expected: it fails, no report is written. It failed with "the host
      wrote no report within 30 seconds", and the host printed "this window
      has no handle the plugin can use".
- [x] T4.3 `mini-host/Cargo.toml`: drop `wayland`, add `x11rb`.
- [x] T4.4 `mini-host/src/main.rs`: `Xlib` arm, Linux attach through the
      socket, Linux-only `hold_keyboard` call.
- [x] T4.5 `mini-host/src/app/place.rs`: the Linux module of 4.4, the
      `track_editor` thread included. The catch-all module's cfg excludes
      Linux. The Windows and macOS modules are not edited.
- [x] T4.6 Found by the test of T4.1 once T4.3 to T4.5 were in: the host had
      a window for the plugin, and the plugin's `attached` failed with
      `tresult 4`. With the reason printed, it was baseview's "Could not find
      a valid Framebuffer configuration". baseview asks GLX for a framebuffer
      that is sRGB capable, and this machine's X server (XWayland, drawing in
      software) has none: with the plugin asking for one that is not sRGB
      capable the window opened and the test passed. The fix is in the
      plugin's `gui.rs`, `open`, for Linux only
      (`#[cfg(target_os = "linux")]`): when the window cannot be made with
      baseview's own settings it is made again asking for a framebuffer that
      is not sRGB capable. egui draws the same into either. Windows and macOS
      build what they built before. The test of T4.1 is the test of it.

Done when: `./test.sh` in `tools/mini-host` passes, and
`cargo test --test editor_under_the_strip -- --ignored` in `tools/mini-host`
passes (needs a yes; settles U16).

Both pass. U16 holds: the host wrote its report with no input arriving, a
fifth of a second after it was started. The test has been seen to fail three
ways: with no report from the host as it was (T4.2), with no report while the
plugin's window could not be made (T4.6), and, with the socket put at the top
of the host's window on purpose, with the report `editor=0,0,900x620` and
`inset=0,0,0,26`.

Not shown by T4, and shown by the named tests of later tasks:

- `track_editor`, the thread that sizes the socket as the host's window is
  dragged: `host-resize` (T7.4, U17).
- `hold_keyboard` and `focus_editor`: `typing-goes-into-the-document` (T5,
  U8).
- `follow_resize`: it runs when the host's window changes size, which this
  test never does. `sections-and-files` drags the window (T6.2).

### T5. The Linux driver (M9, M10, M11)

- [ ] T5.1 `xtask/Cargo.toml`: Linux dependencies, description.
- [ ] T5.2 `xtask/src/main.rs`: module lines.
- [ ] T5.3 `linux/keys.rs` with tests
      `a_character_is_sent_as_its_own_keysym`,
      `named_keys_and_repeats_are_read_out_of_braces`,
      `bracket_escapes_are_literal_brackets`,
      `a_name_that_is_not_a_key_is_refused`.
- [ ] T5.4 `linux/portal.rs`: the session, the token file, the refusal message,
      `request_open`, with test
      `the_senders_with_open_requests_are_read_out_of_the_portals_tree`.
      `linux/input.rs` on top of it, with the release of whatever a failed
      step left pressed.
- [ ] T5.5 `linux/xwindows.rs` with test
      `a_frame_is_the_client_area_grown_by_its_extents`.
- [ ] T5.6 `linux/cursor.rs` with tests
      `the_shown_cursor_is_named_by_its_picture`,
      `a_cursor_no_stock_picture_matches_is_other`.
- [ ] T5.7 `linux/screen.rs` with tests
      `a_region_is_cut_out_at_the_frames_scale`,
      `a_region_hanging_off_the_monitor_is_clamped`.
- [ ] T5.8 `linux/look.rs` with tests
      `find_text_output_is_read_as_boxes`,
      `the_box_nearest_the_expected_spot_wins`.
- [ ] T5.9 `linux.rs`: `Run`, `launch`, `finish`, `play`, every step in the
      table of 4.5, the portal session opened before the first launch, the
      active-window check before input, and the host never left running.
- [ ] T5.10 `uitest.rs`: Linux `drive`.

Done when: `cargo test -p xtask` in `markdown-notes` passes, and
`./test.sh --ui typing-goes-into-the-document` in `markdown-notes` passes
(needs a yes; settles U1, U2, U8, U10, U13, U18 for the plugin). The
first run raises GNOME's dialog, which the person at the machine answers. It
then passes a second time with `start` answering within two seconds (U15).
Its picture is sent. The baseline uses `type:`, `showing:` and `shot:` only
and never moves the pointer. Every other step, and the pointer, are proven by
T6 and T7.

### T6. The markdown-notes UI suite

Needs a yes before the set of runs. Each test is run alone with
`./test.sh --ui <name>` in `markdown-notes`, fixed and rerun alone until it
passes, and its picture is sent.

- [ ] T6.1 `editing-the-document` (U3, U4, U7, U9).
- [ ] T6.2 `sections-and-files` (U5, U6, U11, U14).
- [ ] T6.3 `opening-a-note` (U18 for the strip).
- [ ] T6.4 `pictures-in-a-note`.

Done when: each of the four has passed.

### T7. The mini-host UI suite

As T6.

- [ ] T7.1 `host-preset-across-runs`.
- [ ] T7.2 `host-preset-dialogs`.
- [ ] T7.3 A task in `xtask/src/main.rs`, `frames FILM DIR`: it runs ffmpeg to
      write two frames a second of FILM into DIR as PNGs. It is the same code
      on every platform, and ffmpeg is already what all three drivers count a
      film's frames with. The task is added to xtask's `help` text.
- [ ] T7.4 `host-resize` (U17). The film the test recorded is read as what it
      is, frames: `cargo run -p xtask -- frames` takes them out of it, and out
      of `host-resize-target.mp4`, into `.cache/uitests`, and they are looked
      at as images. Text keeps its size in every frame while the window
      changes size.

Done when: each of the three tests has passed and the frames of T7.4 have
been looked at.

A bug found in T5, T6 or T7 gets a saved test that fails before its fix.

### T8. The whole run

- [ ] T8.1 `./test.sh --full` from the root, once. Needs a yes.

Done when: it ends with "all projects passed".

### T9. Documentation (M12)

- [ ] T9.1 `docs/markdown-notes/DEVELOPERS.md`: the three places of 4.6.
- [ ] T9.2 `docs/mini-host/DEVELOPERS.md`: the one place of 4.6.
- [ ] T9.3 `docs/markdown-notes/PROJECT_DEFINITION.md`: the Status rows and
      the Known gaps of 4.6.

Done when: all three read back.
