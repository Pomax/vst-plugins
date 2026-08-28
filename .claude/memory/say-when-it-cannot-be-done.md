---
name: say-when-it-cannot-be-done
description: When the task cannot be done inside the scope it was given, say so first and stop; never widen the scope or substitute a workaround
metadata:
  type: feedback
---

Work out whether the thing asked for is possible within exactly what was named
before touching anything. If it is not, say so in the first reply, say what
the limit is, and stop. Do not reach for a different file, a different
language, or a clever substitute.

**Why:** told to make `run-markdown-notes.bat` start without a terminal
window, I edited Rust source and `Cargo.toml` that were never mentioned, then
put a hidden-relaunch ActiveX call in the batch file. Neither was the
assignment. A batch file always gets a console from Windows and cmd cannot
hide its own, so the honest first answer was "cmd cannot do this, here is
why". The user: *"IF YOU CAN'T DO A FUCKING THING, SAY THAT INSTEAD OF FUCKING
INVENTING SHIT THAT ISN'T WHAT THE ASSIGNMENT WAS"*.

**How to apply:** name the file or the tool that was specified, and check the
limit against it. Report the limit, name the options that stay inside the
scope, and ask which one. Nothing is edited until the answer arrives. See
[[ask-instead-of-assuming-intent]] and [[never-attribute-words-to-the-user]].
