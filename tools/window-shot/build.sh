#!/usr/bin/env sh
# Build the window photographer and assemble it into an application bundle.
#
# It has to be a bundle: macOS remembers the right to read the screen against
# an application, and refuses to ask about a bare executable at all.
set -e

here=$(cd "$(dirname "$0")" && pwd)
dist=$here/../../dist
app="$dist/Window Shot.app"

cargo build --release

rm -rf "$app"
mkdir -p "$app/Contents/MacOS"

cp "$here/.cache/release/window-shot" "$app/Contents/MacOS/window-shot"
cp "$here/Info.plist" "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

codesign --force --sign - "$app"

# Every build is a new signature, and the system's screen recording grant is
# tied to the old one: a stale entry blocks the new build from even asking.
# Dropping the entry here means the next run prompts fresh instead of failing
# against a grant that can never match.
tccutil reset ScreenCapture com.markdown-notes.window-shot

echo "dist:   $app"
