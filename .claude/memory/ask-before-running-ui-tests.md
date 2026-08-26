---
name: ask-before-running-ui-tests
description: Ask first, every time, before running anything that takes over the mouse, keyboard or screen
metadata:
  type: feedback
---

Never start a UI test, a screenshot run, a screen recording, or anything else
that drives the pointer, the keyboard or a window, without asking first and
getting a yes. Ask once per set of runs, not once per session: the answer
depends on what the user is doing at that moment, and they are usually at the
machine.

A yes covers the whole set. Once it is given, keep running, fixing and
rerunning until everything passes, and only then stop. Asking again between
attempts of the same set is asking twice for the same thing.

Say when a run is starting, every single time, including every rerun inside a
set that was already agreed to. Permission is not notice: the user is at the
machine, and a run that begins unannounced takes the pointer out from under
their hand. A pointer or a keystroke of theirs landing mid-run also makes the
result meaningless, so a run they did not know about can fail for a reason
that is not in the code.

Capturing the screen outside a test run is forbidden outright. No probe, no
"just checking a flag", no capture of any kind from the working shell, ever.
A capture flashes the whole desktop and lights the recording indicator even
when it fails, and the user has forbidden it in those words after exactly such
a probe. A question about how a capture tool behaves goes into the capture
tool and is answered by the next permitted test run.

This covers `xtask uitest` in any form (one test, a suite, or `test --full`),
`tools/capture-window.ps1` and its macOS counterpart, and launching the host or
the plugin window to look at it.

**Why:** these tests take the real mouse and keyboard for the length of the run.
Anything the user was typing goes into the test's window instead, and their
machine is unusable until it finishes.

And the result is worthless either way. The user is at the keyboard, so their
keystrokes and pointer land in the window under test alongside the driver's: a
pass means nothing and a failure says nothing about the code. A UI test run
without asking is not just rude, it is a run whose outcome cannot be believed. The user has said so repeatedly, in these
words: *"STOP FUCKING INTERFERING WITH MY FUCKING DESKTOP"*, *"I AM DOING WORK
HERE, STOP HIJACKING MY MOUSE AND KEYBOARD"*, and after doing it again,
*"FUCKING ASK BEFORE YOU RUN SESSION HIJACKING CODE"*.

**How to apply:** say which test or suite wants to run and why, then wait. While
waiting, run the tests that do not touch the desktop — unit tests, the
scenarios, the headless pixel tests — and report those. A failing UI test is
still a saved test in the suite: it can wait for a yes, and does not license a
second run to "have a look". Related:
[[run-only-the-tests-the-change-affects]],
[[never-run-destructive-commands-unasked]],
[[blanket-permission-means-just-run-it]] — which is about ordinary commands, not
about taking over the machine.
