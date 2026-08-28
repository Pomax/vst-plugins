---
name: windows-redraw-during-resize
description: A window inside another must be moved from the resize message and asked to paint at once, or a drag shows black
metadata:
  type: project
---

The mini host puts the plugin's editor window inside its own. Two things are
needed for a drag of the host's corner to redraw cleanly, and both were missing
until 2026-08-24:

1. The plugin's window is moved from inside the host's `WM_WINDOWPOSCHANGED` /
   `WM_SIZE`, by a subclass on the host's window, not on the next frame of
   eframe's drawing loop. Doing it on the next frame is one frame late for every
   frame of a drag. `WS_CLIPCHILDREN` on the host's window then keeps the host
   from painting over the area the plugin owns.
2. The plugin is asked to paint immediately, in that same message:
   `RedrawWindow(child, RDW_INVALIDATE | RDW_UPDATENOW)`, and then a
   `WM_TIMER` with baseview's frame timer id, 4242.

The second is the one that is easy to miss. baseview draws only on that timer,
and Windows only synthesises `WM_TIMER` when the message queue is empty. During
a drag the queue never empties, so the plugin is never asked for a frame, and
the area it has just been stretched into shows what the graphics card left
there: black. It looks like a repaint failure in the host, and it is not.

**How to apply:** this lives in
`tools/mini-host/src/crates/mini-host/src/app/place.rs`
as `track_editor` and `draw_now`. macOS needs none of it: an `NSView` with an
autoresizing mask is resized by AppKit inside the resize. Do not move the
tracking back into the frame loop, and do not drop the paint nudge because it
looks like a hack — without it the drag is black. `host-resize.txt` is the test,
and it drags the real corner: a version that only set the size never touched
this path at all. Related: [[ask-before-running-ui-tests]].
