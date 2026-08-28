---
name: answer-by-checking-not-by-running
description: "Did I break X" is answered by reading the changes, not by saying the tests cannot be run here
metadata:
  type: feedback
---

When asked whether something is broken, work it out from the changes and say
yes or no. "I cannot run those" is not an answer to "did you break them": what
was changed is known, and so is what reads it.

Go through each change and name what consumes it. A file only one platform
compiles, a step only one driver parses, a coordinate another test already
relies on: each of those settles the question without running anything.

**Why:** asked whether the macOS UI tests still worked, I said I had not run
them and could not, and stopped. Reading the changes took under a minute and
gave a clean answer: every edit was either keyboard text both drivers parse or
a Windows-only file. The user: *"WHY DIDN'T YOU FUCKING SAY THAT, THEN, WHY DO
I NEED TO FUCKING KEEP PUSHING YOU FOR ANSWERS?"*

**How to apply:** answer the question that was asked. If running is the only
way to be sure, say what reading already settles first, then name the one
thing that is left. Related: [[answer-with-all-of-it]],
[[say-when-it-cannot-be-done]].
