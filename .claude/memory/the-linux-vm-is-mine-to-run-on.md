---
name: the-linux-vm-is-mine-to-run-on
description: On the dedicated Linux VM, UI runs are started without asking; the rule to ask first is for machines the user is working at
metadata:
  type: feedback
---

On the dedicated Linux VM (hostname `ubuntu`, user `ubuntu`, GNOME on Wayland)
run whatever the task needs, UI tests and windows included, without asking
first.

**Why:** on 2026-10-01, during T4 of the Linux plan, I asked before a test
that opens the host's window. The user: *"YOU ARE RUNNING ON A DEDICATED VM
WITH FULL CONTROL, FUCKING RUN WHAT YOU NEED TO IN ORDER TO COMPLETE TASKS"*.
Nobody is working at that desktop, so there is nobody whose pointer or typing
a run can get in the way of.

**How to apply:** this is about that VM only. [[ask-before-running-ui-tests]]
still holds on a machine the user is sitting at (the Windows and macOS
machines), and so does everything else: nothing outside the repository is
written, and nothing destructive runs unasked. My reading, not the user's
words: a run on the VM is still announced in one line before it starts, so
the log of the session says when the desktop was in use.
