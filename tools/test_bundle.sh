#!/bin/sh
# Bundle the app, check the plist and signature, and check it starts with a bare Finder-like environment.
set -e
cd "$(dirname "$0")/.."
out=$(mktemp -d)
app=$(tools/bundle.sh "$out")
plutil -lint "$app/Contents/Info.plist"
codesign -v "$app"
env -i HOME="$HOME" PATH=/usr/bin:/bin "$app/Contents/MacOS/captaine-app" >/dev/null 2>&1 &
pid=$!
sleep 4
kill -0 $pid && echo "bundle OK: starts and stays up" && kill $pid
rm -rf "$out"
