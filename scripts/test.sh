#!/usr/bin/env sh
# Run every project's tests. Stops at the first failure.
# With a project's name first, run that project's tests alone and hand it the
# rest of the arguments:
#     scripts/test.sh markdown-notes --only mermaid_blocks
set -e
cd "$(dirname "$0")/.."

case "$1" in
    mini-host | vst3-loader) one="tools/$1" ;;
    markdown-notes) one="$1" ;;
    *) one="" ;;
esac
if [ -n "$one" ]; then
    shift
    cd "$one"
    exec ./test.sh "$@"
fi

for project in tools/mini-host tools/vst3-loader markdown-notes; do
    echo "=== $project ==="
    (cd "$project" && ./test.sh "$@")
done

echo "=== all projects passed ==="
