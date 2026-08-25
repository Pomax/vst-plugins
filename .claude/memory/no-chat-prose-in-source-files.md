---
name: no-chat-prose-in-source-files
description: "Comments explain the code, not the conversation — no narrative, justification or history in source, config or workflow files"
metadata:
  node_type: memory
  type: feedback
---

Comments and doc comments state what the code does and what a reader needs to
know to change it safely. Nothing else.

Never put in a source, config or workflow file:

- arguments aimed at the reader of a conversation ("this cannot be done
  anywhere else", "this is why we need X")
- the history of the code — what it used to be, what bug it had, what was tried
- reassurance, hedging, or apology

Test: if it reads like a reply to the user, it belongs in the reply, not the
file. Project history goes in `docs/PROJECT_DEFINITION.md`.

**Why:** I opened a CI workflow with a paragraph explaining why a macOS runner
was necessary. The user: *"Do not put text that you would put here inside
source code, including the workflow file(s). No one gives a fuck that 'it
cannot be done anywhere else'."* The same habit had been filling Rust doc
comments with narrative about earlier versions of the plugin.
