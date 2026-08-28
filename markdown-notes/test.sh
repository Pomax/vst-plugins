#!/usr/bin/env sh
set -e
exec cargo run -p xtask -- test "$@"
