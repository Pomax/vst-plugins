#!/usr/bin/env sh
# Open the notepad plugin in the mini host.
set -e
cd notepad
exec ./run.sh "$@"
