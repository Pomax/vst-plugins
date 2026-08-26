#!/usr/bin/env sh
# Build the text finder into the shared dist.
set -e

here=$(cd "$(dirname "$0")" && pwd)
dist=$here/../../dist

mkdir -p "$dist"
swiftc -O -o "$dist/find-text" "$here/find-text.swift"

echo "dist:   $dist/find-text"
