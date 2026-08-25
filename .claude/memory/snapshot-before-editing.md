---
name: snapshot-before-editing
description: Copy a file to the scratchpad before changing it, so any change can be undone without git
metadata:
  type: feedback
---

Before editing a file, copy it to the session scratchpad. Keep every version,
not just the first, so any edit can be reversed on its own.

**Why:** git is not available for this — it is not allowed here, and the user
should not have to hand back a working tree because an experiment went wrong.
Without a copy there is nothing to restore from: "undo what you just did" then
means reconstructing the file from memory of the edits, which is slow and
wrong. Code written in a sandbox still needs revision control.

**How to apply:** the first time a file is touched in a session, copy it to
`<scratchpad>/before/<path>.<n>`; bump `<n>` on every later edit. To undo, copy
the wanted version back. Never reach for git to undo — see
[[never-use-git-without-being-asked]] — and never let the scratchpad copies
leave the sandbox: they are not part of the repository. Related:
[[edit-existing-files-never-write-them]],
[[unexpected-file-changes-are-the-users]].
