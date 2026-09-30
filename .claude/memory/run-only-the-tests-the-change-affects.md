---
name: run-only-the-tests-the-change-affects
description: Rerun the suite covering what changed, not everything else that already passed
metadata:
  type: feedback
---

After a change, run the tests that cover it. Do not rerun suites the change
cannot have touched, and do not rerun a suite that already passed in this
session unless something it depends on has changed since.

**Why:** the UI tests take the machine's mouse and keyboard for the length of
the run. Re-running the markdown-notes suite after a mini-host-only change costs the
user a minute of an unusable desktop and proves nothing.

**How to apply:** name the suite — `uitest mini-host`, `uitest markdown-notes` — or the
single test. The runner takes one name for exactly this. A change to the
driver or the harness is not a reason to run everything either: rerun the
tests that were failing, one at a time, and stop when each has passed. On
2026-09-29 I started `./test.sh --full` after every UI test had already
passed on its own, and was stopped: *"stop fucking running every goddamn test
if you only fix one or two"*. Related: [[never-run-a-test-you-do-not-save]],
[[redo-the-failed-step-not-the-procedure]].
