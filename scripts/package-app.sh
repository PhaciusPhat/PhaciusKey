#!/usr/bin/env bash
set -euo pipefail

# Builds a distributable phacius_vnkey.dmg from the Rust binary. End users just
# drag the .app to /Applications — no Rust, no toolchain. Rust is only needed
# HERE (build time); the whole app is a single self-contained Rust executable.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$REPO_ROOT/apps/vnkey"
DIST="$REPO_ROOT/dist"
APP="$DIST/PhaciusKey.app"
VERSION="$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP_DIR/Info.plist")"

echo "==> Building Rust app (release)..."
cargo build --release -p vnkey

echo "==> Assembling .app bundle..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$REPO_ROOT/target/release/vnkey" "$APP/Contents/MacOS/vnkey"
cp "$APP_DIR/Info.plist" "$APP/Contents/Info.plist"

echo "==> Generating app icon..."
ICONSET="$DIST/AppIcon.iconset"
rm -rf "$ICONSET"
"$REPO_ROOT/target/release/vnkey" --export-iconset "$ICONSET"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

# Ad-hoc sign so the bundle keeps a stable identity across launches —
# required for Accessibility permission to persist. Replace "-" with your
# Developer ID to ship notarized.
echo "==> Ad-hoc code signing..."
codesign --force --deep --sign - "$APP"

echo "==> Building .dmg..."
DMG="$DIST/PhaciusKey-$VERSION.dmg"
rm -f "$DMG"
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "PhaciusKey" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

echo "==> Done: $DMG"
