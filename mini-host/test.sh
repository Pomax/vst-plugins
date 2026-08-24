#!/usr/bin/env sh
# --full is accepted and ignored: this project has no UI tests to add.
set -e
args=""
for arg in "$@"; do
    [ "$arg" = "--full" ] && continue
    args="$args $arg"
done
exec cargo test $args
