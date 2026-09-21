---
name: a-question-stops-the-work
description: "A question stops whatever is running: acknowledge it, then answer it, before anything else"
metadata:
  node_type: memory
  type: feedback
---

When the user asks a question, stop what you are doing. Acknowledge the
question, then answer it. Do not finish the tool call you were making, do not
answer it three messages later, and do not answer it as a footnote to a report
about something else.

**Why:** a question is asked because the answer changes what happens next.
Carrying on and answering afterwards means the work in between was done without
the answer, and it reads as ignoring them.

**How to apply:** the reply that follows a question contains the answer and
nothing else. Whatever was in flight waits. See [[ask-instead-of-assuming-intent]]
and [[plain-language]].
