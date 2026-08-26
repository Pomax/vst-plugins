#!/usr/bin/env sh
# Build, then put the executable in the shared dist directory.
set -e
cargo build --release
mkdir -p ../dist
# Removed first, never overwritten in place: macOS remembers an approved
# executable by its file, and a file whose contents change under it is killed
# at launch without a word.
rm -f ../dist/mini-host
cp .cache/release/mini-host ../dist/mini-host
echo "dist:   ../dist/mini-host"
