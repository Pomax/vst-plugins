---
name: never-write-sectioning-comments
description: Never divide a file with banner or section comments; split the file or leave it alone
metadata:
  type: feedback
---

Never write a sectioning comment. No `// ---- tabs ----`, no `# ---- the
dialogs ----`, no banner of dashes or equals signs, no `// ---- accessors`, in
any language and in any file: source, config, tests, step files, workflows.

**Why:** it is decoration standing in for structure. A file long enough to want
signposts is a file that wants splitting, and one short enough not to need
splitting does not need signposts either: `fn save_as` is already under the
heading "save_as". The user has said this in session after session and it keeps
coming back, which makes it a habit to break rather than a preference to weigh.

**How to apply:** if a file feels like it has parts, split it into modules and
let the file names carry the names. Otherwise write nothing at all between the
items. Comments explain what code does and what a reader must know to change it
safely, which is [[no-chat-prose-in-source-files]]; a divider does neither.
Remove these on sight in any file being edited for another reason.
