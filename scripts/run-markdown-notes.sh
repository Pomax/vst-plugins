#!/usr/bin/env sh
# Open Markdown Notes in the mini host.
set -e
cd "$(dirname "$0")/.."
exec ./scripts/build.sh
cd "markdown-notes"
exec ./run.sh --demo "$@"
