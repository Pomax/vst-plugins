---
name: never-tail-command-output
description: Never pipe a command through tail; read the whole log, every time
metadata:
  type: feedback
---

Never run `2>&1 | tail`, or `| tail` in any other form, on a command whose
output matters. Do not write `2>&1` either. Run the command plainly and read
all of it: the tool already captures stderr.

**Why:** the user forbade it outright. Tailing throws away the beginning of the
log, which is where the first error is. A build or test run that fails early
and then prints a hundred lines of unrelated output looks fine from the last
twenty lines, so the actual cause is never seen and the next answer is a guess.

**How to apply:** run the command on its own, with no redirection and no pipe,
and read the output in full. If it is genuinely too large, write it to a file inside
the repository and read that file, rather than discarding the front of it. The
same applies to `head`, `-n`, and any other truncation of output that is being
read to decide what to do next. Related: [[plain-language]],
[[answer-with-all-of-it]].
