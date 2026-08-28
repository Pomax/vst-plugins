---
name: window-shot-rebuild-resets-its-grant
description: Rebuilding Window Shot voids its screen recording grant; the build script resets the stale entry itself, and the user is never told to flip the toggle
metadata:
  type: project
---

Window Shot is signed ad hoc, so every rebuild is a new signature and the
screen recording grant dies with the old one. The stale settings entry then
blocks the new build: it cannot be fixed by flipping the toggle, only by
deleting the entry.

**Why:** the user was sent through flip-the-toggle cycles four times over
(2026-08-26) before saying: run the removal yourself when you update the tool.
Toggling re-enables the old record; it never rekeys to the new binary.

**How to apply:** `tools/window-shot/build.sh` runs
`tccutil reset ScreenCapture com.markdown-notes.window-shot` after signing, so
a rebuild leaves no stale entry. After a rebuild the sequence is: run the test
once (Window Shot prompts), the user allows it, run again. Never instruct the
user to flip the toggle, and never rebuild Window Shot without need: each
rebuild costs one prompt-and-allow round. Related:
[[ask-before-running-ui-tests]], [[never-touch-anything-outside-the-repo]]
(the reset is a sanctioned, named exception the user ordered).
