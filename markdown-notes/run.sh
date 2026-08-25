#!/usr/bin/env sh
# Open the built plugin in the mini host. Builds nothing: run ./build.sh first.
# Prefers the shared dist, then a bundle in the cache, then the bare plugin
# binary a test run leaves behind.
set -e

host=
for candidate in ../dist/mini-host ../mini-host/.cache/release/mini-host; do
    if [ -x "$candidate" ]; then host=$candidate; break; fi
done

plugin=
for candidate in \
    "../dist/Markdown Notes.vst3" \
    ".cache/release/bundle/Markdown Notes.vst3" \
    ".cache/debug/bundle/Markdown Notes.vst3" \
    .cache/release/libmarkdown_notes_plugin.dylib \
    .cache/debug/libmarkdown_notes_plugin.dylib
do
    if [ -e "$candidate" ]; then plugin=$candidate; break; fi
done

if [ -z "$host" ]; then
    echo "no mini-host built - run ./build.sh first" >&2
    exit 1
fi
if [ -z "$plugin" ]; then
    echo "no Markdown Notes plugin built - run ./build.sh or ./test.sh first" >&2
    exit 1
fi

exec "$host" "$plugin"
