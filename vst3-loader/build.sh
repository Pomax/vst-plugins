#!/usr/bin/env sh
# Build, then put the executable in the shared dist directory.
set -e
cargo build --release
mkdir -p ../dist
cp .cache/release/vst3-loader ../dist/vst3-loader
echo "dist:   ../dist/vst3-loader"
