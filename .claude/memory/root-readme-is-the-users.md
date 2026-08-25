---
name: root-readme-is-the-users
description: "README.md at the repo root belongs to the user — never edit it unless told to by name; docs/DEVELOPERS.md is the one to maintain"
metadata:
  node_type: memory
  type: feedback
---

`README.md` at the repository root is the user's own file. Do not edit,
restyle, or update it unless explicitly told to, by name.

The documentation I wrote lives in `docs/`. `docs/DEVELOPERS.md` is the former
README, renamed by the user, and is the file to keep current — along with
`docs/TESTING.md` and `docs/PROJECT_DEFINITION.md`.

**Why:** the user reorganised the docs themselves and said plainly: *"remember
that the readme was renamed to developer in the docs dir, the new readme is
mine, not yours. do not touch it unless specifically told to"*. See
[[unexpected-file-changes-are-the-users]].
