---
name: edit-existing-files-never-write-them
description: "Write is only for new files; changing an existing file is always Edit"
metadata:
  node_type: memory
  type: feedback
---

`Write` creates. `Edit` changes. A file that already exists is always edited,
whatever the size of the change — never rewritten whole, never re-emitted after
being read.

**Why:** I kept using `Write` on `run-markdown-notes.bat`,
`run-markdown-notes.sh`, `run.bat`
and `run.sh`, all of which existed. `Write` throws away the current contents,
so anything changed since the last read is lost silently, and the diff shows
the whole file instead of the part that actually changed. The user: *"WHY THE
FUCK ARE YOU 'CREATING RUN-NOTEPAD.SH' INSTEAD OF FUCKING EDITING IT"*.

Related: [[moves-are-moves-not-copies]] — same principle, do the operation that
matches the intent instead of a destructive substitute.
