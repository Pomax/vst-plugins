#!/usr/bin/env sh
# Build, then put the executable in the shared dist directory.
set -e
cargo build --release
mkdir -p ../dist
# Removed first, never overwritten in place: macOS kills a launched executable
# whose file contents changed after it was first approved.
rm -f ../dist/vst3-loader
cp .cache/release/vst3-loader ../dist/vst3-loader
echo "dist:   ../dist/vst3-loader"
