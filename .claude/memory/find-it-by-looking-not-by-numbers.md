---
name: find-it-by-looking-not-by-numbers
description: When a UI test's fixed coordinates miss their target, the driver finds the target with the screenshot and text reader and clicks where it is; it is not a question for the user
metadata:
  type: feedback
---

When a click, a press or a drag in a UI test misses because the thing is not
where a number says it is, find the thing on the screen and act where it is.
The driver has a screenshot and a text reader for exactly this.

**Why:** on 2026-10-01, during T6 of the Linux plan, `plugin:239,87` missed
the `+` button by 4 pixels because a section tab is wider in the Linux face.
I stopped and offered three choices (change the face, change the shared step
file, fix the tab's width). The user: *"You have a screenshot and OCR tool:
why are you relying on hardcoded values, just fucking find the thing you
need, then click where you now know it exists"*.

**How to apply:** my reading, not the user's words: the shared step files
stay as they are and the looking goes in the platform's driver. In that
session it became: the Linux `click:` reads the window around its point and
goes to a lone `+` close by; the window's resize grip is found by where the
pointer turns into the corner's arrow; a dialog's button is found by what it
says. A step whose number has nothing readable behind it is still a reason to
stop and say so, see [[say-when-it-cannot-be-done]]. Related:
[[a-question-stops-the-work]], [[ask-instead-of-assuming-intent]].
