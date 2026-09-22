---
name: show-each-visual-result-before-the-next-change
description: For visual work, send every rendered result as it is made, and work from the reference picture's rules, not from guesses
metadata:
  type: feedback
---

When the work is how something looks, send the picture after every change,
before making the next one. When the user gives a reference picture, study it
first and write down the rules it follows (where lines turn, how far apart
things sit, what sits on what), then make the code follow those rules.

**Why:** in the mermaid work the user could not see anything between my edits,
and I iterated three times on my own idea of "better" while they were looking
at a reference that already answered every question. They had to point at the
reference five times. Each round I fixed one symptom and guessed at the rest.
A fix I reported as done (opaque label boxes) was never looked at in a picture
and did not work: a stylesheet rule overrode it.

**How to apply:** render through a saved test that writes a PNG, send it with
SendUserFile and a one-line caption of what changed, then look at it myself.
Measure the reference (sizes, gaps, which element carries the bump or label)
before choosing constants. A visual fix gets a pixel assertion, see
[[never-run-a-test-you-do-not-save]]. Do not stop to list what is still off:
fix it, see [[plain-language]].
