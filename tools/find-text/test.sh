#!/usr/bin/env sh
set -e
if [ "$(uname -s)" = Darwin ]; then
    echo "find-text: the macOS finder has no tests"
    exit 0
fi
args=""
for arg in "$@"; do
    [ "$arg" = "--full" ] && continue
    args="$args $arg"
done
exec cargo test $args
