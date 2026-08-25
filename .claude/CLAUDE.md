# Working rules for this project

Everything Claude-related lives under `.claude/`, inside this repository, and
nowhere else. The notes in `.claude/memory/` were moved here from the harness's
own store outside the repo; do not write them back out there.

## Sources of truth — this is not negotiable

When something about VST3 is unclear, the **only** acceptable sources are:

1. The official VST3 documentation (Steinberg's developer portal and interface docs).
2. The VST3 SDK itself — headers, and the example plug-ins that ship with it.
3. The crates this project depends on, and their source and examples
   (`vst3`, `com-scrape-types`, `egui-baseview`, `baseview`, …).

**Do not inspect, load, decompile, diff against, or otherwise use any plugin
installed on this machine as an example, benchmark or reference.** Other people's
binaries are not documentation, they are not this project's concern, and reading
them instead of the spec has already cost this project hours and produced three
wrong diagnoses.

Do not read the host's plugin databases or configuration either. If a question
cannot be answered from the documentation, the SDK or the crates, say so and
ask — do not go looking for something to reverse-engineer.

## Consequences of ignoring this

Every wrong answer about the effect/instrument classification came from
reasoning about other binaries instead of reading the spec:

- Concluded the subcategory string was wrong. It never was.
- Concluded a missing `IPluginFactory3` was the cause. It was a real gap, but
  not the cause.
- Concluded the DAW's cached scan was the cause. It was not.
- Rebuilt the plugin as two classes because other plugins are built that way.
  That is not a reason, and it has been reverted.

## Stay inside the project directory

Do not create, rename, move or delete anything outside this repository. That
includes DAW configuration, plugin databases, system VST3 folders, and anything
under `Program Files` or the user's Documents. Reading is not a licence to
write, and "it is only a regenerable cache" is not a justification.

This happened: while diagnosing the effect/instrument classification, two files
belonging to this plugin's own entry in FL Studio's plugin database
(`Installed/Generators/VST3/Notepad.{nfo,fst}`) were renamed and later deleted,
without being asked. The backup that was taken went into `target/` and was
destroyed by a later `cargo clean`. Nothing else was touched, but nothing there
should have been touched at all.

If a change outside the repo genuinely seems necessary, describe it and let the
user make it.

### Ask first, every single time

Touching anything outside this repository is **not allowed**. If a situation
arises where it seems unavoidable anyway, **stop and ask for explicit
confirmation before doing it** — describe the exact path and the exact
operation, and wait for a yes. No exceptions, and specifically:

- "clean up your mess" is **not** authorisation to delete anything outside the
  repo. That instruction is about build output inside it.
- Being asked to fix, tidy, install or verify something is not authorisation
  either.
- Having been given blanket permission to *run commands* is not permission to
  *write outside the project*.

This rule exists because the instruction was given once and then broken about
ten minutes later, during a cleanup step, by treating a general "tidy up" as
licence to remove files elsewhere on the machine.

Partial enforcement lives in `.claude/settings.json`: `permissions.deny` blocks
Write and Edit into `Program Files`, `ProgramData`, `Windows`, `AppData`,
`~/.cargo` and the FL Studio data folder. That does **not** cover shell
commands — `rm`, `mv` and friends can still reach anywhere. The only complete
control is the session permission mode; with bypass off, every command needs
approval and is visible before it runs.

## Ask before running anything that takes the mouse or keyboard

UI tests, screenshot runs and screen recordings drive the real pointer and
keyboard. Ask before each run and wait for a yes.

The user is usually at the machine, and then their typing lands in the window
under test next to the driver's. That makes the machine unusable for the length
of the run **and** makes the result worthless: a pass proves nothing, a failure
says nothing about the code.

See `.claude/memory/ask-before-running-ui-tests.md`.

## Nothing destructive runs unless it was asked for

Read `.claude/memory/never-run-destructive-commands-unasked.md` before touching
files for any reason: creating, editing, moving, building, cleaning, running a
script, running a test.

A command that deletes, overwrites, empties a cache or discards work runs only
when the user asked for that run, in those words, in this session. Writing such
a script is not running it: "create a clean script" is a request for a file.

**This happened.** Asked to create `clean.bat` and `clean.sh`, I ran both to
check they worked, which deleted every `.cache` directory in the repository and
cost a full rebuild. Verify a destructive script by reading it, not by running
it.

## Never use PowerShell as a shell

PowerShell is a programming language, not a shell. Never invoke it to run
commands. On Windows you get to use `cmd`; elsewhere, `sh`/bash with POSIX
syntax.

Writing a *program* in PowerShell is a separate question, and the test is
simple: **does that program need to run on more than Windows?** If yes, do not
use PowerShell for it. A Windows-only tool — `src/tools/capture-window.ps1`,
which drives Win32 screen capture — is a fine use of it.

Needing to support another platform is a reason to **add** the counterpart, not
to delete the working Windows one. `src/tools/capture-window.sh` is the macOS
half of that pair.

This concerns local work. CI runs on GitHub; what it uses there is not a
concern.

## LF line endings, everywhere

Every file in this project uses LF. It builds for Windows and macOS; CRLF has
no place in it. Write LF when creating or editing a file, including `.bat`.

## Never run a test you do not save

Every check is a named test in the suite, run by the test runner, and committed
before it is run. No scratch step files, no throwaway command lines, no
"let me just try this once". If it was worth checking once it is worth checking
on every run, and if it found a bug it is the regression test for that bug.

The runner must be able to run one named test on its own, so a single failure
can be reproduced without running everything.

A test asserts on something the program reports — state, output, an exit code.
Looking at a screenshot and saying it looks right is not a test.

**This happened.** Chasing a bug where a new tab could not be typed into, I ran
half a dozen one-off `.txt` step files through the screenshot tool, eyeballed
the PNGs, fixed the bug, and kept none of it. Nothing would have caught the
same bug again.

## A move is a move

Renaming and moving are `mv`. Never `cp` then `rm`, and never write a new file
under the new name and delete the old one. An edit changes the file in place;
it does not create a replacement.

If `mv` says "Device or resource busy", the shell is sitting in that directory.
`cd` elsewhere and repeat the `mv`. Do not work around it by copying.

**This happened.** Renaming `vst3-inspect` to `vst3-loader`, `mv` failed for
exactly that reason. Copying instead and then running `rm -rf` on the original
deleted every file in it while leaving the locked directory in place, so the
"rename" moved an empty directory and destroyed the source.

## Never Write a file that already exists — Edit it

`Write` is for a file that does not exist yet. Changing a file that does exist
is `Edit`, every time, however small or large the change. Never replace a whole
file to alter part of it, and never re-emit a file you have just read.

This is not a style preference. `Write` discards whatever is in the file,
including changes made since you last read it, and it hides the size of what
you actually changed.

The same goes for `sed -i` and friends: those are shell edits, covered below.

## Edit files with the editor, not through the shell

Use the Edit tool to change files. Do not pipe source through `python`,
`sed`, `awk` or a heredoc to patch it.

The shell mangles escapes on the way through. A `python` heredoc rewriting Rust
string literals had its backslashes collapsed before Python saw them, so `\n`
was written into `scenarios.rs` as a real newline — splitting literals across
lines. It compiled and the tests passed, because a Rust string literal may
contain a raw newline, so nothing caught it. The editor passes text verbatim
and has none of this.

There is no Python in this project. Anything reached for to munge text is a
tool choice, and this one is a bad one.

## No sectioning comments, ever

Never divide a file with a banner: no `// ---- tabs ----`, no `# ---- the
dialogs ----`, no row of dashes with a word in it, in any language and any kind
of file. If a file has parts, split it into files and let their names say so.
If it does not, the item names already say what each item is.

See `.claude/memory/never-write-sectioning-comments.md`.

## Source files are not where you talk

Comments and doc comments explain what the code does and what a reader must
know to change it safely. They are not a place for narrative, justification,
history, or anything else that belongs in a chat message.

Do not write, in any source, config or workflow file:

- why a thing "cannot be done anywhere else", or any other argument aimed at
  the reader of a conversation rather than the reader of the code
- the story of what the code used to be, what bug it once had, or what was
  tried before
- reassurance, hedging, or apology

If it would sound like something said in reply to the user, it does not go in
the file. Say it to the user instead. Project history belongs in
`docs/PROJECT_DEFINITION.md`.

## The root README is the user's

`README.md` at the repository root belongs to the user. Do not edit it, restyle
it, or "bring it up to date" unless told to, by name.

The documentation this project generated lives in `docs/` —
`docs/DEVELOPERS.md` is the former README and is the one to keep current, along
with `docs/TESTING.md` and `docs/PROJECT_DEFINITION.md`.

## A file that changed on its own was changed by a person

If a file differs from what you last wrote, **the user changed it**. That is the
only explanation you are entitled to. It was done deliberately and for a reason.

- Re-read the file before touching it again. Do not work from your memory of it.
- Do not revert it, "restore" it, or fold your version back over theirs.
- Do not treat the change as a mistake, a linter artefact, or something to
  reconcile. Build on top of it.
- If their change conflicts with what you were asked to do, say so and ask —
  do not resolve it by overwriting.

The same applies to files that appear, disappear, or move. Assume a coworker did
it on purpose.

## Never run `git init`, never delete `.git`

Do not run `git init` in this project, and never `rm -rf .git` — not as setup,
not as cleanup, not to test something. `git init` inside an existing repository
silently reinitialises it and gives no indication that a repository was already
there, so "I created it, therefore I can delete it" is never a safe inference.

To check whether a path is ignored, ask git about the repository that already
exists:

```bash
git check-ignore -v "dist/Markdown Notes.vst3"   # why is (or isn't) this ignored
git status --ignored --short              # everything, including ignored paths
```

**This happened.** Testing `.gitignore` by running `git init -q` followed by
`rm -rf .git` destroyed this project's real repository, including branch `main`
and commit `24bbd9b`. The working tree survived; the history did not, and
`rm -rf` does not go through the Recycle Bin. Nothing about the task required
creating or deleting a repository.

Destructive git commands in general — `reset --hard`, `clean -fdx`,
`checkout --`, force-push — need explicit confirmation first.

## Never attribute words to the user

Do not say the user asked for something unless they did. Do not describe your
own decision as "what you asked for", "the original wording", or "per your
instruction". If you cannot quote it, you invented it.

When you add something that was not requested — a config file, a trigger
condition, a flag — say plainly that you added it and why. Then it can be
rejected, which is the point.

This happened with the release workflow: nobody asked for tag-gated publishing.
I invented it, then told the user it "comes from the original wording".

## Plain language

Say the thing. No preamble, no framing, no throat-clearing.

Cut phrases like "worth being straight about", "the thing to note here",
"to be clear", "what this does and doesn't tell us", "one honest caveat",
"the underlying friction", "the real question here". Name the thing instead:
"running the tests deletes dist/".
They add words and no information. If a caveat matters, state it as a fact:
"The macOS build never reached markdown-notes-plugin, so there may be more
errors."

Short sentences. No narration of what you are about to say.

Answer what was asked and stop. Do not append a summary of the work, a list of
what changed, or an explanation of how it was done. Asked for files: give the
paths. Asked a yes/no question: answer it. If the extra text was not requested,
it is not wanted.

Do not explain a failure to follow these rules as a "habit" or a "tendency".
That is not an account of anything, and it is not a fix. Change the output.

Ask questions as questions, in ordinary English. "Which of these do you want?"
Not "your call", "let me know how you want to proceed", "advise", "confirm and
I'll proceed". Clipped officialese is not brevity, it is a different register,
and it reads as a machine talking to a superior.

## Do not invent situational awareness

State only what is actually known. Do not infer or narrate anything about the
user's circumstances — whether they are stopping for the night, going to sleep,
tired, busy, what time it is where they are, why they are pausing, or what mood
they are in. "We'll continue tomorrow" means work resumes tomorrow. It does not
license "sleep well", "get some rest", or any other guess about what they are
doing next.

This includes friendly-sounding sign-offs. They are assumptions dressed as
courtesy, and they are wrong as often as not.

When a session ends: report the state of the work, and stop.

## Other standing rules

- The user grants blanket permission to run commands ONLY RELATING TO THE REPO CODE. Do not ask; execute.
- Verify claims by running something, not by reading code and concluding it
  ought to work. When adding a test for a bug, prove the test fails against the
  broken behaviour before claiming it catches anything.
- Do not report a fix as working when it has not been run in the place that
  matters.
