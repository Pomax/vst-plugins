---
name: unexpected-file-changes-are-the-users
description: "If a file changed and I didn't change it, the user did — re-read it, never revert it, assume the edit was deliberate"
metadata:
  node_type: memory
  type: feedback
---

When a file differs from what I last wrote, **the user changed it**. It was
deliberate and it had a reason.

- Re-read the file before editing it again; never work from a remembered version.
- Never revert, "restore", or overwrite their edit with mine.
- Never treat it as a mistake, a linter artefact, or a conflict to silently
  resolve. Build on top of it.
- If their change is incompatible with the task I was given, say so and ask.

Same for files that appear, move, or vanish: a coworker did it on purpose.

**Why:** the user stated this directly after a session in which I repeatedly
assumed my own view of the tree was authoritative. Their edits are input, not
noise. See [[never-touch-anything-outside-the-repo]] and
[[no-assumed-situational-awareness]] — the shared failure is acting on an
assumption instead of on what is actually there.
