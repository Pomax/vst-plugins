---
name: never-run-a-test-you-do-not-save
description: "Every check is a saved, named test in the suite — no one-off scripts, no eyeballed screenshots"
metadata:
  node_type: memory
  type: feedback
---

Every check is a named test, committed to the suite, run by the test runner.
Never invent a check, run it once, and throw it away. If it was worth running
once it is worth running every time, and if it found a bug it is that bug's
regression test.

The runner must be able to run a single named test, so one failure can be
reproduced without running the whole battery.

A test asserts on something the program reports — state, output, an exit code.
Looking at a screenshot and judging it correct is not a test.

**Why:** while chasing a bug where a newly created tab could not be typed into,
I wrote half a dozen throwaway step files, drove the GUI with them, eyeballed
the screenshots, fixed the bug and saved none of it. The user: *"YOU SHOULD NOT
RUN ONE-OFFS, EVER"* and *"NEVER FUCKING JUST 'INVENT A TEST, RUN IT, AND NOT
SAVE IT'"*.

Related: [[edit-existing-files-never-write-them]].
