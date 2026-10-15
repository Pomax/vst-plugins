---
name: a-named-change-is-made-first
description: When the user names the change to make, make exactly that change before anything else; no investigating, alternatives, new tests or reading elsewhere
metadata:
  type: feedback
---

When the user says what the change is ("save and restore the cursor as part
of the checkbox click"), make exactly that change, first. Do not investigate
why first, do not propose a different fix, do not add a test for it, do not go
reading another crate's source. Then run the named test that covers it; that
run is what says whether it worked.

**Why:** the checkbox click in `clicking-the-controls` left the clipboard
empty. The user named the fix in one sentence. What followed instead was a
focus change that did nothing, a request to read egui's source, a proposed
headless test file, a proposed change to the test, and three questions, before
the named change was finally made and turned out to be the right one. Each of
those was an instruction ignored while something else was done, and the user
had to say so each time.

**How to apply:** the user's sentence is the task. If it names the place and
the change, that is the whole brief: open the place, make the change, ask for
the run. A reason to doubt it is said in one sentence after the change is made
and before the run, not acted on instead of it. See
[[ask-instead-of-assuming-intent]] for the case where the sentence is a
question rather than a change, and [[never-run-a-test-you-do-not-save]] for
why a headless check is not a substitute for the UI test that already covers
the behaviour.
