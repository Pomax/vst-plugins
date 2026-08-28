#!/usr/bin/env sh
# Run every project's tests. Stops at the first failure.
set -e

for project in tools/mini-host tools/vst3-loader markdown-notes; do
    echo "=== $project ==="
    (cd "$project" && ./test.sh "$@")
done

echo "=== all projects passed ==="
