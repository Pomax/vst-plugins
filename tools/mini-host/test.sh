#!/usr/bin/env sh
# --all and --ui are accepted and ignored: this project's UI tests are run by
# the markdown-notes test task.
set -e
args=""
for arg in "$@"; do
    case "$arg" in --all | --ui) continue ;; esac
    args="$args $arg"
done
exec cargo test $args
