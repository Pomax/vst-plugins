---
name: redo-the-failed-step-not-the-procedure
description: "When something fails, work out the one missing step and do that; never re-run the whole procedure"
metadata:
  node_type: memory
  type: feedback
---

When a run fails part way, read what actually failed and do the smallest thing
that finishes it. Do not re-run the script that produced it.

- The build compiled and only the final move was refused: move the binary, do
  not build again.
- A measurement disagrees with what the user sees: look at the real thing, do
  not re-run the harness.
- One test failed: run that test, not the suite.
- Something is unclear: read the code that decides it, do not gather more
  output.

**Why:** a re-run is not cheap. It is billed the whole time it runs, on top of
the minutes it takes, and it produces the same result as the run before it. A
five minute build re-run to fix a one second move costs five minutes of real
money for nothing.

**How to apply:** before running anything, say what state you expect and what
the run would tell you that you do not already know. If the answer is nothing,
do not run it. See [[answer-by-checking-not-by-running]] and
[[run-only-the-tests-the-change-affects]].
