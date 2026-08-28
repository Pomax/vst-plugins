#!/usr/bin/env sh
# Build the text finder into the shared binaries directory.
set -e

here=$(cd "$(dirname "$0")" && pwd)
binaries=$here/../../binaries

mkdir -p "$binaries"
swiftc -O -o "$binaries/find-text" "$here/find-text.swift"

echo "binary: $binaries/find-text"
