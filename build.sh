#!/usr/bin/env sh
# Build every project. Stops at the first failure.
set -e

for project in mini-host vst3-loader markdown-notes; do
    echo "=== $project ==="
    (cd "$project" && ./build.sh "$@")
done

echo "=== all projects built ==="
