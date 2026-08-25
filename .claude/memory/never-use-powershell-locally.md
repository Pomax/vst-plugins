---
name: never-use-powershell-locally
description: "Never invoke PowerShell as a shell — use sh/bash. Writing a program in PowerShell is fine when the task is Windows-only."
metadata:
  node_type: memory
  type: feedback
---

Two different things, and I conflated them:

**As a shell — never.** Do not use the PowerShell tool or PowerShell one-liners
to run commands. PowerShell is a programming language, not a shell. Use
`sh`/bash with POSIX syntax, on Windows as everywhere else.

**As a language — it depends on one question:** does the program need to run on
more than Windows? If yes, do not write it in PowerShell. If it is Windows-only,
PowerShell is a fine choice. `src/tools/capture-window.ps1` drives Win32 screen
capture and is Windows-only, so it stays.

**Cross-platform need means ADD the counterpart, never delete the working one.**
I deleted that script because the project targets macOS too. Wrong: the answer
was to write `src/tools/capture-window.sh` alongside it. *"JUST BECAUSE WE NEED
CROSS PLATFORM DOESN'T MEAN YOU DELETE WINDOWS ONLY SHIT, IF THAT THING WAS
NECESSARY AND IT WORKED THEN YOU FUCKING KEEP IT AND ADD THE FUCKING THING YOU
NEED FOR MACOS."*

Applies to local work. CI runs on GitHub and what it uses there is not a
concern — do not edit workflow files on these grounds.

**Why:** told *"NEVER USE POWERSHELL ON WINDOWS, IT'S A PROGRAMMING LANGUAGE,
NOT A SHELL"*, I over-applied it twice: first by rewriting a CI step, then by
offering to delete a Windows-only PowerShell tool. *"I SAID NOT TO USE
POWERSHELL FOR SHELL CALLS, THE SCRIPT YOU'RE TALKING ABOUT IS A FUCKING PROGRAM
WRITTEN IN PS ... DOES IT NEED TO RUN ON MORE THAN WINDOWS? THEN YES, DON'T
FUCKING USE IT."*
