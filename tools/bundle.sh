#!/bin/sh
# Build a double-clickable captainE.app (macOS): tools/bundle.sh [outdir]
set -e
cd "$(dirname "$0")/.."
out="${1:-dist}"
app="$out/captainE.app"
cargo build --release -p captaine-app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/bin"
cp target/release/captaine-app "$app/Contents/MacOS/captaine-app"
cp crates/vision/src/vision.py "$app/Contents/Resources/" 2>/dev/null || true
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
 <key>CFBundleName</key><string>captainE</string>
 <key>CFBundleDisplayName</key><string>captainE</string>
 <key>CFBundleIdentifier</key><string>dev.captaine.editor</string>
 <key>CFBundleVersion</key><string>0.0.1</string>
 <key>CFBundleShortVersionString</key><string>0.0.1</string>
 <key>CFBundleExecutable</key><string>captaine-app</string>
 <key>CFBundlePackageType</key><string>APPL</string>
 <key>LSMinimumSystemVersion</key><string>12.0</string>
 <key>NSHighResolutionCapable</key><true/>
 <key>NSMicrophoneUsageDescription</key><string>captainE records voice-over from the microphone.</string>
 <key>CFBundleDocumentTypes</key><array><dict>
  <key>CFBundleTypeName</key><string>captainE project</string>
  <key>CFBundleTypeExtensions</key><array><string>captaine</string></array>
  <key>CFBundleTypeRole</key><string>Editor</string>
 </dict></array>
</dict></plist>
EOF
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true
echo "$app"
