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

echo "dist:   $app"
