---
name: moves-are-moves-not-copies
description: "Rename and move with mv; never copy-then-delete, and never create a new file to stand in for an edit"
metadata:
  node_type: memory
  type: feedback
---

Renaming is `mv`. Moving is `mv`. Never `cp` followed by `rm`, and never write
a new file with the new name and delete the old one.

The same goes for edits: change the file in place. Do not create a replacement
file and remove the original.

If `mv` fails with "Device or resource busy", the shell is sitting in that
directory or something has it open. `cd` somewhere else and try again. Do not
work around it by copying.

**Why:** renaming `vst3-inspect` to `vst3-loader`, `mv` failed because the
shell's own working directory was inside it. I copied instead, then ran
`rm -rf` on the original — which deleted every file in it but left the locked
directory behind. Moving that now-empty directory produced an empty
`vst3-loader`, and the source file was destroyed. A copy-and-delete has a
window where the data exists in one place and is about to be removed from the
other; `mv` does not.

See [[never-attribute-words-to-the-user]] and
[[unexpected-file-changes-are-the-users]].
