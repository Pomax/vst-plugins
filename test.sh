#!/usr/bin/env sh
# Run every project's tests. Stops at the first failure.
set -e

for project in mini-host vst3-loader notepad; do
    echo "=== $project ==="
    (cd "$project" && ./test.sh "$@")
done

echo "=== all projects passed ==="
