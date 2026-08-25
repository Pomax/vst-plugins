#!/usr/bin/env sh
# Put the standing rules in front of Claude on every single message.
#
# Reading them once at the start of a session is not enough: they are followed
# for a while and then quietly dropped. A hook is run by the harness, not by
# Claude, so this cannot be forgotten or skipped.
#
# Wired up as a UserPromptSubmit hook in settings.json. Whatever it prints on
# stdout is added to the context for that turn.
here=$(dirname "$0")

echo "STANDING RULES, checked before every reply. Full text in .claude/CLAUDE.md,"
echo "one file per rule in .claude/memory/."
echo
echo "- Answer what was asked and stop. No summary of the work, no list of what"
echo "  changed, no account of how it was done. A question gets an answer, not"
echo "  an action."
echo "- Ask before running anything that drives the mouse, keyboard or screen:"
echo "  UI tests, screenshots, recordings. Every time, and wait for a yes."
echo "- Nothing that deletes, overwrites, empties a cache or discards work runs"
echo "  unless it was asked for in those words. Writing such a script is not"
echo "  running it."
echo "- Run only the tests the change affects, by name."
echo "- Never use git. No status, log, diff, restore, init, or rm of .git."
echo "- Never write outside this repository."
echo "- cmd on Windows, sh elsewhere. Never PowerShell as a shell."
echo "- Edit existing files with the editor. Write is for new files; sed, awk,"
echo "  python and heredocs are not editors. A move is mv."
echo "- LF line endings. No em dashes. No sectioning comments. No chat prose in"
echo "  source files."
echo "- Snapshot a file to the scratchpad before changing it."
echo "- Never attribute words to the user that they did not say."
echo
echo "Memory index:"
cat "$here/memory/MEMORY.md"
