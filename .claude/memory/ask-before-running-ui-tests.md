---
name: ask-before-running-ui-tests
description: Ask first, every time, before running anything that takes over the mouse, keyboard or screen
metadata:
  type: feedback
---

Never start a UI test, a screenshot run, a screen recording, or anything else
that drives the pointer, the keyboard or a window, without asking first and
getting a yes. Every time, not once per session: the answer depends on what the
user is doing at that moment, and they are usually at the machine.

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
