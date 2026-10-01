# Linux build and UI tests: plan

## 1. Goal

`./build.sh` and `./test.sh` (including `./test.sh --full`) work on Linux the
way they work on Windows and macOS:

- every project builds and its result lands in `binaries/`;
- the unit tests, the scenarios and the headless pixel tests pass;
- the UI tests run from the same step files, against the real host window,
  with input posted to the desktop's own keyboard and pointer, the way
  `SendInput` and `CGEvent` post to theirs, and pictures taken of the screen;
- Linux has the same three tools the other platforms have: a window driver, a
  screen photographer and a text finder.

Target: GNOME 50 on Wayland, x86_64, one 1920x1080 monitor. The host and the
plugin are XWayland windows, because the plugin's only Linux window type is
`kPlatformTypeX11EmbedWindowID` (`markdown-notes-plugin/src/lib.rs:618`).

Fixed choices:

- Screen reading: the xdg-desktop-portal ScreenCast interface.
- Text recognition: the `tesseract` program.
- Input: the xdg-desktop-portal RemoteDesktop interface, which posts key and
  pointer events to the session's existing keyboard and pointer. No device is
  created, virtual or otherwise.

## 2. Current breakdown

### 2.1 Entry points

| Script | What it runs |
|---|---|
| `build.sh` | `build.sh` of `tools/find-text`, `tools/mini-host`, `tools/vst3-loader`, `markdown-notes`, in that order |
| `test.sh` | `test.sh` of `tools/mini-host`, `tools/vst3-loader`, `markdown-notes` |
| `markdown-notes/build.sh` | `cargo run -p xtask -- bundle --release --target aarch64-apple-darwin` |
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
| Keys | `SendInput` with real scan codes, one key at a time, a Shift of its own per character, key found with `VkKeyScanW` | `CGEvent` posted at the HID tap |
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

Steps the drivers implement: `type click press showing hidden dialog nodialog
window nowindow dragtext hold moveto letgo drag cursor shortcut kill restart
row copies dragto dragedge geometry remove picture clipboard written shot film
endfilm wiggle wait resize`.

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
| M1 | `markdown-notes/build.sh` | Hardcodes `--target aarch64-apple-darwin`. Fails on Linux. |
| M2 | `tools/find-text/build.sh` | Runs `swiftc`. `./build.sh` stops at its first project on Linux. |
| M3 | `tools/find-text/src/main.rs`, `Cargo.toml` | Windows-only code and dependencies. No Linux reader. |
| M4 | `xtask/src/main.rs:195`, `write_binary` | The Linux bundle is not the documented one. The VST3 Plugin Format page: binary at `<Name>.vst3/Contents/x86_64-linux/<Name>.so`, folder and `.so` share the name, single-file plug-ins deprecated since 3.6.10. The code names the inner file `.vst3` and copies a bare file to `binaries/`. |
| M5 | `markdown-notes/run.sh` | Looks for `.dylib` only. |
| M6 | `mini-host/src/main.rs:397`, `vst3-loader/src/lib.rs:109` | `native_handle` accepts `Xcb` only. winit's X11 window gives `Xlib` (`winit-0.30.13/src/platform_impl/linux/x11/window.rs:1884`). The editor is never attached. |
| M7 | `mini-host` `Cargo.toml` | eframe has `wayland` and `x11`. On a Wayland session winit picks Wayland, and an X11 plugin window cannot be parented into a Wayland surface. |
| M8 | `mini-host/src/app/place.rs:378` | Every Linux function is empty: the editor is not put under the strip, not resized, not given the keyboard, not measured. |
| M9 | `xtask/src/uitest.rs:545` | No Linux `drive`. |
| M10 | `xtask/src/main.rs:25` | `mod png` is macOS only. |
| M11 | Linux tooling | No screen photographer, no input driver, no window lookup. |
| M12 | `docs/` | Nothing about Linux. |

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
  output to `parse_tsv`.
- `fn parse_tsv(text: &str, scale: u32) -> Vec<Line>`: level 5 rows are words
  (`left top width height conf text`); words sharing block, paragraph and line
  numbers form one `Line`; boxes are divided by `scale`; rows with no text or a
  negative confidence are dropped.
- `tesseract` not on `PATH`: exit 2 with a message that names it.

The Windows build of this crate changes (cfg attributes, target table) and
cannot be compiled here. The Windows code itself is not edited.

### 4.3 vst3-loader (M6)

`src/lib.rs` `native_handle`: add
`RawWindowHandle::Xlib(h) => Some(h.window as usize as *mut c_void)`.

### 4.4 mini-host (M6, M7, M8)

`Cargo.toml`:

- remove `"wayland"` from eframe's features (no effect on Windows or macOS);
- `[target.'cfg(target_os = "linux")'.dependencies] x11rb = "0.13"`.

`src/main.rs`:

- `native_handle`: the same `Xlib` arm as the loader.
- Attaching, Linux only: `place::make_socket(parent, chrome::HEIGHT)` then
  `plugin.attach(&socket)`. Other platforms keep `plugin.attach(cc)`.
- `Host::ui`: call `place::hold_keyboard(self.handle)` each frame while no
  dialog is open. Empty on Windows and macOS.

`src/app/place.rs`, Linux module (replaces the empty one; the catch-all for
other systems stays):

- State: one `x11rb::rust_connection::RustConnection`, the socket's window id
  and the strip height, in a `OnceLock<Mutex<_>>`.
- `pub struct Socket(u32)` implementing `HasWindowHandle` with an
  `XcbWindowHandle`.
- `pub fn make_socket(handle, top) -> Result<Socket, String>`: create a child
  window of the host's at `0, top`, sized to the host's width and its height
  less `top`, map it.
- `inset_editor`, `follow_resize`: `ConfigureWindow` on the socket to the
  host's current size less the strip.
- `track_editor`: nothing. baseview resizes the plugin's window to its
  parent's size on the parent's `ConfigureNotify`
  (`baseview/src/platform/x11/event_loop.rs`, `handle_coalesced_resize_events`),
  and the parent is the socket.
- `focus_editor`: `SetInputFocus` on the plugin's window (the socket's largest
  child, from `QueryTree`).
- `hold_keyboard`: when `GetInputFocus` names the host's own window, give the
  focus to the plugin's window. The window manager puts the focus on the
  top-level window every time it is activated, which is after every dialog.
- `editor_in_host`: the socket's position plus the plugin window's geometry.
  With this, `Host::report_geometry` writes `window=`, `host=`, `editor=` and
  `inset=` as it does on macOS.
- `set_enabled`: nothing, as on macOS.

Why a socket window: with the host's whole window as the parent, baseview's
follow-the-parent rule makes the plugin cover the strip. This reading is of
baseview 0.3.5; the lock file has 0.3.4, so task T3.1 confirms it there first.

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
| `linux.rs` | `pub fn drive(root, host, plugin, steps, state, shot)`, `struct Run` (child, pid, restarts, launch arguments, `reported` geometry file, `host` and current `rect`, `holding`, `title`, `killed`, film, the clipboard owner), `launch`, `finish`, `play`, `step` |
| `linux/portal.rs` | `struct Portal`: the one portal session that carries both input and the screen stream. `open(cache)`, the stream's node and size, the restore token |
| `linux/input.rs` | Input through the `Portal`, and the last pointer position. `move_to`, `glide`, `click`, `take_hold`, `let_go`, `drag`, `wheel`, `tap`, `shortcut`, `write`, `send_keys` |
| `linux/keys.rs` | `keysym_of(char)`, `named(&str)` (the keysym of `ENTER`, `ESC`, `BS`, `END`, `DOWN`, `TAB`), `modifier(&str)`, `parse_keys(step) -> Vec<Stroke>` |
| `linux/xwindows.rs` | `struct Desktop`: X connection and atoms. `window_of(pid, title)`, `frame_rect`, `client_rect`, `activate`, `close`, `set_size`, `pointer`, `screen_size`, `cursor_image` |
| `linux/cursor.rs` | `stock(theme, size)` and `name_of(shown, stock)` giving `ibeam`, `arrow`, `hand`, `grabbing` or `other` |
| `linux/screen.rs` | Pictures and films from the `Portal`'s stream. `shot(area, to)`, `film(area, to)`, `Film::stop`, `crop(area, stream, screen)` |
| `linux/look.rs` | `finder()`, `find_all(picture, text)`, `read_all(picture)`, `nearest(boxes, near)`, `find_on_screen`, `find_box_on_screen` |

The portal session (`portal.rs`):

- One session per xtask process, made with `RemoteDesktop::create_session`.
  `RemoteDesktop::select_devices` asks for the keyboard and the pointer,
  `Screencast::select_sources` adds the monitor to the same session, and
  `RemoteDesktop::start` raises GNOME's one permission dialog for both.
- Persist mode "until revoked". The restore token is read from and written to
  `markdown-notes/.cache/portal-token`, so the dialog is answered once.
- While the dialog is up the driver prints one line saying it is waiting.
- A refusal, or no portal on the bus, stops the run with a message, as
  `may_post_input` does on macOS.

Input (`input.rs`, `keys.rs`):

- What Windows does with `SendInput` and macOS with `CGEvent` at the HID tap
  is post events into the system's input stream for the keyboard and pointer
  the session already has. The portal's `Notify*` calls are that on this
  desktop: GNOME moves its own pointer and delivers the keys to whichever
  window has the focus. Nothing is sent to the window itself.
- Pointer: `notify_pointer_motion_absolute(stream, x, y)` in the monitor
  stream's coordinates, `notify_pointer_button(BTN_LEFT, pressed/released)`,
  `notify_pointer_axis_discrete(Vertical, steps)` for the wheel.
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
- After every pointer move onto a window the driver reads `QueryPointer` and
  fails if the pointer is not where it was sent.

Windows and geometry (`xwindows.rs`):

- A window is found in `_NET_CLIENT_LIST` by `_NET_WM_PID` and its title.
- The window under test is addressed by its frame (client area grown by
  `_NET_FRAME_EXTENTS`), the Windows and macOS convention. A dialog is
  addressed by its client area, the Windows convention.
- Front: `_NET_ACTIVE_WINDOW` client message, then wait until the root's
  `_NET_ACTIVE_WINDOW` names the window.
- Close: `WM_DELETE_WINDOW` client message, then wait for a clean exit as
  `Run::finish` does on macOS. A crash on the way out fails the test.
- Sizes for `dragto:` and `geometry:` come from the host's `--geometry` file,
  the macOS way.

Screen (`screen.rs`, `look.rs`):

- The stream is the monitor stream of the `Portal`'s session, cursor hidden.
- A picture: `open_pipe_wire_remote`, then
  `gst-launch-1.0 -q pipewiresrc fd=N path=NODE num-buffers=1 ! videoconvert !
  videocrop ... ! pngenc ! filesink location=OUT`, five second limit.
- The crop is computed from the stream's size against the X screen's size, so
  a scale other than 1 is handled and not assumed.
- A film: `gst-launch-1.0 -e pipewiresrc ... ! videorate !
  video/x-raw,framerate=30/1 ! videoconvert ! videocrop ... ! x264enc ! mp4mux !
  filesink`, ended with SIGINT. Frames are then counted with ffmpeg and
  `mpdecimate` and written as `frames=` and `distinct_frames=`, as on the other
  platforms.
- Text finding calls `binaries/find-text`, built through its `build.sh` when
  it is not there, as `macos.rs` does.

Steps:

| Step | Linux behaviour |
|---|---|
| `type:` | `send_keys`. A value starting with `/` is a path for a file dialog, see below |
| `click:` `hold:` `moveto:` `letgo:` `drag:` `wiggle:` | `Hands`, in the addressed window's coordinates |
| `press:LABEL\|X,Y` | photograph the window, take the match nearest X,Y, click it; no match fails and says what was read |
| `showing:` `hidden:` | photograph the window, look below Y until it is as the step says, five second limit |
| `dragtext:` | find the text below Y, press on its first glyph, let go at X,Y |
| `window:` `nowindow:` | X11 window of the host's process by title; a new dialog is waited for until text can be read in it |
| `dialog:WORD` `nodialog:WORD` | the file dialog is another program's window, so it is found by looking at the monitor: up when `Cancel` and WORD are both read, gone when `Cancel` is not |
| `cursor:X,Y\|NAME` | move there, compare the XFixes cursor picture with the theme's pictures for the names baseview loads (`text`/`xterm`, `left_ptr`, `hand2`/`hand1`, `closedhand`/`grabbing`), two second limit |
| `shortcut:` | modifier held over the key |
| `row:DIR\|NAME` | look for the row, click it; wheel down and look again when it is not on screen |
| `dragto:W,H,MS` | take the frame's bottom right corner, glide by the difference from the reported size, let go, wait for the reported size |
| `dragedge:` | the same with a given distance |
| `resize:W,H` | `ConfigureWindow`, corrected from the reported size |
| `geometry:PATH` | copy the host's report once the editor fills the host |
| `picture:` | `png::solid`, the colour the other drivers use |
| `clipboard:PATH` | `arboard::Clipboard::set_image`, owner kept in `Run` |
| `shot:` `film:` `endfilm:` | `Screen` |
| `restart:` `kill:` `copies:` `remove:` `written:` `wait:` | as `macos.rs` |

File dialog paths: stage pictures `dialog-<n>-<stage>.png` in
`.cache/uitests`, as `macos.rs` takes them. Precondition: `Cancel` is read on
screen before anything is typed. Then Ctrl+L, the path typed, and the test's
own `{ENTER}` confirms. Whether a Save dialog takes the whole path in one go
or needs the directory and the name separately is unknown U6.

Unknowns, each settled by a named test during an allowed UI run:

| # | Unknown | Settled by | If it does not hold |
|---|---|---|---|
| U1 | A new reader of the portal stream gets a frame while the screen is still | `typing-goes-into-the-document` (`showing:`) | keep one reader attached for the whole run and take its latest frame |
| U2 | `open_pipe_wire_remote` can be called once per picture | same | one reader for the run, as U1 |
| U3 | A position given to the portal in stream coordinates is the position X reports for the pointer | the `QueryPointer` check on the first click | convert between the two with the measured ratio and offset |
| U9 | A modifier pressed by keysym is held when the next key arrives | `editing-the-document` (`shortcut:` steps) | send shortcuts with `notify_keyboard_keycode` and the keys' evdev codes |
| U4 | Raw step coordinates hit their targets with GNOME's title bar height | `editing-the-document` | stop; the choice touches step files shared with Windows and macOS and is the user's |
| U5 | The frame's bottom right corner is a resize grip | `host-resize` | move the grab point along the frame edge, found from the test's pictures |
| U6 | How GNOME's Open and Save dialogs take a typed path | `sections-and-files` stage pictures | directory first, then the name, as `macos.rs` does |
| U7 | XWayland reports the cursor's picture through XFixes | `editing-the-document` (`cursor:`) | read the cursor from the picture of the screen with the portal's cursor mode on |
| U8 | The editor keeps the keyboard after a preset dialog closes | `opening-a-note` | adjust `hold_keyboard` |

### 4.6 Documentation (M12)

`docs/markdown-notes/DEVELOPERS.md` and `docs/mini-host/DEVELOPERS.md`: a
Linux part covering what has to be installed (`tesseract-ocr`, GStreamer with
`pipewiresrc`, ffmpeg), the portal's permission dialog, how input and
pictures work. The root `README.md` is not edited.

### 4.7 Rules that bind the work

From `.claude/CLAUDE.md` and `.claude/memory/`:

- Nothing is created, changed or deleted outside the repository. `tesseract`
  is installed by the user (`sudo apt install tesseract-ocr`).
- A UI run needs a yes first, per set of runs, and each run is announced. No
  screen capture and no input outside a test run.
- Every check is a saved, named test. No scratch step files.
- Only the tests a change affects are run, by name. A failed step is redone,
  not the procedure.
- A deleting command runs only when asked for. The ones this plan runs:
  - `./build.sh` replaces `binaries/find-text`, `binaries/mini-host`,
    `binaries/vst3-loader`, `binaries/Markdown Notes.vst3`;
  - `./test.sh` removes `binaries/Markdown Notes.vst3` before building;
  - a UI run empties `markdown-notes/.cache/uitests` and carries out the step
    files' `remove:` and `cleanup:` lines there and in
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

Each task ends with the check that says it is done.

Work stops after each task is demonstrably completed, so that the user can
form a git commit. The next task does not start until that has been done.

### T1. Build on Linux (M1, M2, M4, M5)

- [ ] T1.1 `markdown-notes/build.sh`: `uname` case. Done when: read back.
- [ ] T1.2 `markdown-notes/run.sh`: `.so` candidates.
- [ ] T1.3 `xtask/src/main.rs`: `inner_binary_name`, `write_binary` copies the
      bundle on Linux, doc comments.
- [ ] T1.4 Unit test `the_linux_bundle_holds_a_so_named_after_the_bundle` in
      `xtask/src/main.rs`. Done when: it fails before T1.3 and passes after
      (`cargo test -p xtask the_linux_bundle`).
- [ ] T1.5 `tools/find-text/build.sh`: `uname` case (needs T2).
- [ ] T1.6 Run `./build.sh`. Done when: it ends with "all projects built",
      `binaries/Markdown Notes.vst3/Contents/x86_64-linux/Markdown Notes.so`
      exists and `bundle` prints its `loads:` line.

### T2. find-text on Linux (M3)

- [ ] T2.1 `Cargo.toml`: target tables.
- [ ] T2.2 `src/main.rs`: cfg attributes, `mod linux`.
- [ ] T2.3 `src/linux.rs`: `read`, `at_scale`, `parse_tsv`.
- [ ] T2.4 Unit tests in `src/linux.rs`:
      `a_word_row_becomes_a_word_with_its_box_scaled_back`,
      `words_on_one_line_are_one_line_in_reading_order`,
      `rows_that_are_not_words_are_dropped`; in `src/main.rs`:
      `a_run_of_words_is_boxed_on_its_own`. Done when: `cargo test` in
      `tools/find-text` passes.
- [ ] T2.5 Check on a real picture: a test
      `the_words_in_a_rendered_picture_are_read` that draws known text into a
      PNG and reads it back through `tesseract`. Done when: it passes with
      tesseract installed and fails with the tesseract call stubbed out.

### T3. Host and loader on Linux (M6, M7, M8)

- [ ] T3.1 Fetch baseview 0.3.4 (`cargo fetch` in `markdown-notes`) and read
      its `platform/x11` for the parent handling. Done when: the socket design
      is confirmed against 0.3.4 or section 4.4 is corrected.
- [ ] T3.2 `vst3-loader/src/lib.rs`: `Xlib` arm, with unit tests
      `an_xlib_window_is_one_a_plugin_can_use` and
      `an_xcb_window_is_one_a_plugin_can_use`. Done when:
      `cargo test` in `tools/vst3-loader` passes.
- [ ] T3.3 `mini-host/Cargo.toml`: drop `wayland`, add `x11rb`.
- [ ] T3.4 `mini-host/src/main.rs`: `Xlib` arm, Linux attach through the
      socket, `hold_keyboard` call.
- [ ] T3.5 `mini-host/src/app/place.rs`: the Linux module of 4.4, and an empty
      `hold_keyboard` in the Windows, macOS and catch-all modules.
- [ ] T3.6 Done when: `./test.sh` in `tools/mini-host` passes and
      `tools/mini-host/build.sh` builds. What it does on screen is checked
      by T5.

### T4. The Linux driver (M9, M10, M11)

- [ ] T4.1 `xtask/Cargo.toml`: Linux dependencies.
- [ ] T4.2 `xtask/src/main.rs`: module lines.
- [ ] T4.3 `linux/keys.rs` with tests
      `a_character_is_sent_as_its_own_keysym`,
      `named_keys_and_repeats_are_read_out_of_braces`,
      `bracket_escapes_are_literal_brackets`,
      `a_name_that_is_not_a_key_is_refused`.
- [ ] T4.4 `linux/portal.rs`: the session, the token file, the refusal message.
      `linux/input.rs` on top of it.
- [ ] T4.5 `linux/xwindows.rs` with test
      `a_frame_is_the_client_area_grown_by_its_extents`.
- [ ] T4.6 `linux/cursor.rs` with tests
      `the_shown_cursor_is_named_by_its_picture`,
      `a_cursor_no_stock_picture_matches_is_other`.
- [ ] T4.7 `linux/screen.rs` with tests
      `a_region_is_cropped_in_stream_pixels`,
      `a_region_hanging_off_the_monitor_is_clamped`.
- [ ] T4.8 `linux/look.rs` with tests
      `find_text_output_is_read_as_boxes`,
      `the_box_nearest_the_expected_spot_wins`.
- [ ] T4.9 `linux.rs`: `Run`, `launch`, `finish`, `play`, every step of 4.5.
- [ ] T4.10 `uitest.rs`: Linux `drive`.
- [ ] T4.11 Done when: `./test.sh` in `markdown-notes` (no `--full`) passes,
      which runs these unit tests with the rest of the workspace.

### T5. UI tests, one at a time (settles U1 to U9)

Each needs a yes before its set of runs. Each is run with
`./test.sh --ui <name>` in `markdown-notes`, fixed and rerun alone until it
passes, and its picture is sent.

- [ ] T5.1 `typing-goes-into-the-document` (U1, U2, U3).
- [ ] T5.2 `editing-the-document` (U4, U7, U9).
- [ ] T5.3 `sections-and-files` (U6, and `dragto:`).
- [ ] T5.4 `opening-a-note` (U8).
- [ ] T5.5 `pictures-in-a-note`.
- [ ] T5.6 `host-preset-across-runs`.
- [ ] T5.7 `host-preset-dialogs`.
- [ ] T5.8 `host-resize` (U5). The film is compared with
      `host-resize-target.mp4`: text keeps its size during the drag.
- [ ] T5.9 `./test.sh --full` from the root, once. Done when: it ends with
      "all projects passed".

A bug found in T5 gets a saved test that fails before its fix.

### T6. Documentation (M12)

- [ ] T6.1 `docs/markdown-notes/DEVELOPERS.md`: Linux part.
- [ ] T6.2 `docs/mini-host/DEVELOPERS.md`: Linux part.
