---
name: never-run-destructive-commands-unasked
description: Never run anything that deletes, overwrites or discards unless asked for that run in those words; writing such a script is not running it
metadata:
  type: feedback
---

Read this before touching files for any reason: creating, editing, moving,
building, cleaning, running a script, running a test.

Never run a command that destroys anything unless the user asked for that run,
in those words, in this session. Destroys means: deletes files or directories
(`rm`, `rmdir /s`, `del`, `clean`, a task that empties a cache), overwrites or
truncates a file, discards work (`git reset --hard`, `checkout --`, `clean
-fdx`, force-push), kills a process holding the user's state, or empties a
build cache someone will have to rebuild.

Writing such a script is not running it. "Create a clean script" is a request
for a file. Nothing about it authorises a run, and a request to fix, tidy,
verify or check is not authorisation either.

**Why:** told to create `clean.bat` and `clean.sh`, I ran both to see that they
worked, and deleted all four `.cache` directories in the repository, costing a
full rebuild of every project. The rule I was following — verify by running
rather than by reading — does not hold when running the thing *is* the damage.
There was nothing to learn from the run that reading the two scripts does not
tell you.

**How to apply:** before any command, ask what it removes. If the answer is not
"nothing", either the user asked for exactly that, or it does not run: say what
the command would do, name the exact paths, and wait. Verify a destructive
script by reading it, by `--dry-run`, or by pointing it at a directory made for
the purpose in the scratchpad. Blanket permission to run commands is permission
to skip the prompt, not permission to destroy: see
[[blanket-permission-means-just-run-it]] and
[[never-touch-anything-outside-the-repo]]. Related:
[[snapshot-before-editing]], [[never-use-git-without-being-asked]],
[[run-only-the-tests-the-change-affects]].
