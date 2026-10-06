#!/usr/bin/env sh
# Open Markdown Notes in the mini host.
set -e
cd "$(dirname "$0")/../markdown-notes"
exec ./run.sh "$@"
