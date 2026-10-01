#!/usr/bin/env sh
# Build the text finder into the shared binaries directory.
set -e

here=$(cd "$(dirname "$0")" && pwd)
binaries=$here/../../binaries

mkdir -p "$binaries"
if [ "$(uname -s)" = Darwin ]; then
    swiftc -O -o "$binaries/find-text" "$here/find-text.swift"
else
    (cd "$here" && cargo build --release)
    rm -f "$binaries/find-text"
    cp "$here/.cache/release/find-text" "$binaries/find-text"
fi

echo "binary: $binaries/find-text"
