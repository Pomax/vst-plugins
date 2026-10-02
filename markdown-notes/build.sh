#!/usr/bin/env sh
set -e
target=
if [ "$(uname -s)" = Darwin ]; then
    target="--target aarch64-apple-darwin"
fi
exec cargo run --release --quiet -p xtask -- bundle --release $target
