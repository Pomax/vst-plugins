---
name: resize-keeps-text-size
description: The target for a live resize, confirmed by the user 2026-08-26; text keeps its size while the window changes, never scales with it
metadata:
  type: project
---

During a live resize of the host window, the plugin's text stays the same
size while the window's dimensions change: more or less content becomes
visible, and nothing "gets bigger". The user confirmed this behaviour from
the host-resize film and named it the target.
`mini-host/src/tools/uitests/host-resize-target.mp4` is that film, beside
the test it belongs to and out of `.cache` so test runs cannot wipe it.

**Why:** the broken behaviour is the plugin not drawing during the drag, so
its last picture is stretched with the view and everything, text included,
scales like an image. That looked like a rendering or DPI problem and was
chased as one; it is a frame-starvation problem.

**How to apply:** each platform needs its own way of keeping the plugin
drawing through a drag. On Windows it is the `WM_TIMER` nudge in
`track_editor`, see [[windows-redraw-during-resize]]. On macOS it is
`let_editor_draw_in_drags` in
`mini-host/src/crates/mini-host/src/app/place.rs`: the plugin's frame timer
lives in the run loop's default mode, which a drag does not run, so the host
installs a common-modes timer that gives the default mode one non-blocking
pass while the view is in a live resize. Never run that pass from inside the
host's own frame: it re-enters the event handling and aborts the process.
The `host-resize` test drives the resize with the mouse and films it; judge
a change against the film, not just the pass, because the assertions cannot
see stretching.
