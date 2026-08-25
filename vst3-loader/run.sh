#!/usr/bin/env sh
# Load a VST3 plugin and report on it. Builds nothing: run ./build.sh first.
#   ./run.sh <path-to-plugin-or-bundle> [options]
set -e

loader=
for candidate in ../dist/vst3-loader .cache/release/vst3-loader; do
    if [ -x "$candidate" ]; then loader=$candidate; break; fi
done

if [ -z "$loader" ]; then
    echo "no vst3-loader built - run ./build.sh first" >&2
    exit 1
fi
if [ $# -eq 0 ]; then
    echo "usage: ./run.sh <path-to-plugin-or-bundle> [options]" >&2
    exit 1
fi

exec "$loader" "$@"
