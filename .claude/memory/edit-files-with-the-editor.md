---
name: edit-files-with-the-editor
description: "Change files with the Edit/Write tools — never patch source through python, sed or shell heredocs; escapes get mangled"
metadata:
  node_type: memory
  type: feedback
---

Use the Edit and Write tools to change files. Do not pipe source through
`python`, `sed`, `awk`, or a shell heredoc to patch it.

**Why:** a `python3` heredoc rewriting Rust string literals had its backslashes
collapsed by the shell before Python ran, so `\n` was written into
`scenarios.rs` as an actual newline, splitting string literals across lines. It
compiled and every test passed — a Rust string literal may contain a raw
newline — so nothing caught it; I only found it by reading the bytes. The
editor passes text verbatim and cannot do this.

The user's reaction on seeing Python mentioned at all: *"...what? We're not
using Python, we're using Rust."* There is no Python in the project; reaching
for it to munge text is a tool choice, and a poor one.
