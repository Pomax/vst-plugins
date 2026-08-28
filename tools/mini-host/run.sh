#!/usr/bin/env sh
# Open a VST3 plugin in the host's window. Builds nothing: run ./build.sh first.
#   ./run.sh <path-to-plugin-or-bundle>
set -e

host=
for candidate in ../../binaries/mini-host .cache/release/mini-host; do
    if [ -x "$candidate" ]; then host=$candidate; break; fi
done

if [ -z "$host" ]; then
    echo "no mini-host built - run ./build.sh first" >&2
    exit 1
fi
if [ $# -eq 0 ]; then
    echo "usage: ./run.sh <path-to-plugin-or-bundle>" >&2
    exit 1
fi

exec "$host" "$@"
