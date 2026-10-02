---
name: warnings-mean-not-done
description: A build or test run that prints a compiler warning is not finished work; search the whole output for warnings and fix each before calling a task done
metadata:
  type: feedback
---

Code that builds or tests with a compiler warning in the output is not done.
A warning is fixed where it comes from, not filtered out of what is read.

**Why:** on 2026-10-01, at the end of the Linux plan, `./test.sh --full`
printed "function `take` is never used" for `drop.rs` nine times a run. It
had been there since T3. I read past it in every run, filtered it out of the
output I looked at as "the one known warning", and called T8 done. The user:
*"WHY, THE FUCK, DOES RUNNING `./test.sh --full` SHOW FUCKING RUST
WARNINGS?!?!?! DID YOU NOT FUCKING LOOK AT THE GODDAMN OUTPUT!??!? CODE THAT
HAS WARNINGS IS NOT FUCKING DONE"*.

**How to apply:** after any build or test run, count the lines with
`warning` in the whole of stdout and stderr. The count has to be 0 before a
task is called done. Never write a filter that drops a warning from the
output being read: see [[never-tail-command-output]]. A warning that was
there before the work started is still fixed, and said. My reading, not the
user's words: this holds for every platform's code that this machine can
build, and what cannot be built here is stated as not shown.
