#!/usr/bin/env sh
# Open Markdown Notes in the mini host.
set -e
cd markdown-notes
exec ./run.sh "$@"
