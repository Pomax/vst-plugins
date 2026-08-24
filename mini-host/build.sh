#!/usr/bin/env sh
# Build, then put the executable in the shared dist directory.
set -e
cargo build --release
mkdir -p ../dist
cp .cache/release/mini-host ../dist/mini-host
echo "dist:   ../dist/mini-host"
