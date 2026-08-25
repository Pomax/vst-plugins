---
name: lf-line-endings-everywhere
description: "Every file in this project uses LF, never CRLF — including .bat; write LF when creating or editing files"
metadata:
  node_type: memory
  type: feedback
---

Use LF line endings for every file, including `.bat`. The project targets
Windows and macOS; CRLF has no place in it.

Eleven files ended up with CRLF because edits made on Windows defaulted to it.
The user found them and was not pleased.

**How to apply:** write LF when creating or editing a file. If CRLF appears,
convert it — do not add tooling to manage it. Asked to fix line endings I also
added a `.gitattributes`, which was not requested: *"I DID NOT FUCKING TELL YOU
TO CREATE A GITATTRIBUTES FILE. JUST FUCKING USE LF INSTEAD OF CRLF."* Do the
thing asked, nothing adjacent to it.
