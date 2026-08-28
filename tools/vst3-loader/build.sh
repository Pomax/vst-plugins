#!/usr/bin/env sh
# Build, then put the executable in the shared binaries directory.
set -e
cargo build --release
mkdir -p ../../binaries
# Removed first, never overwritten in place: macOS kills a launched executable
# whose file contents changed after it was first approved.
rm -f ../../binaries/vst3-loader
cp .cache/release/vst3-loader ../../binaries/vst3-loader
echo "binary: ../../binaries/vst3-loader"
