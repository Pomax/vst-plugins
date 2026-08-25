---
name: never-use-git-without-being-asked
description: git is not available to me here — not even to read status, log or diffs
metadata:
  type: feedback
---

Do not run git. Not `status`, not `log`, not `diff`, and above all not
`checkout`, `restore` or `reset`. Nobody granted git; blanket permission to run
commands does not include it.

**Why:** the user's repository and its history are theirs. Reaching for git to
work out what changed, or to undo something, puts their working tree and their
commits at risk — this project has already lost a repository that way.

**How to apply:** to know what a file used to be, use the scratchpad copies —
see [[snapshot-before-editing]]. To undo, restore from those. If something
genuinely needs git, describe the command and let the user run it. Related:
[[never-touch-anything-outside-the-repo]].
