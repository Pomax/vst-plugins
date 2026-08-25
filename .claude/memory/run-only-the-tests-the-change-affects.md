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
single test. The runner takes one name for exactly this. Only run everything
when the change is shared: the driver in `tools/`, the plugin bundle, or the
test harness itself. Related: [[never-run-a-test-you-do-not-save]].
