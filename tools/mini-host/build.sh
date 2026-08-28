#!/usr/bin/env sh
# Build, then put the executable in the shared binaries directory.
set -e
cargo build --release
mkdir -p ../../binaries
# Removed first, never overwritten in place: macOS remembers an approved
# executable by its file, and a file whose contents change under it is killed
# at launch without a word.
rm -f ../../binaries/mini-host
cp .cache/release/mini-host ../../binaries/mini-host
echo "binary: ../../binaries/mini-host"
