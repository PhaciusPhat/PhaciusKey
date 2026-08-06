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

# A *stable* signing identity is what lets macOS keep the Accessibility grant
# across auto-updates — the grant is keyed to the certificate (its SHA-1), NOT
# the app's name or path. Ad-hoc signatures ("-") change every build, which is
# why the permission used to reset after each update.
#
#   • phaciuskey-release — the ONE shared identity every published release must
#     use. Created once, exported as a .p12, imported on each release machine /
#     CI. See CONTRIBUTING.md > Releasing. Note a self-signed cert's identity is
#     its HASH, not its name: two certs both named "phaciuskey-release" are
#     DIFFERENT identities, so the canonical cert is pinned by SHA-1 below and a
#     DMG signed by anything else aborts.
#
# For a personal build from source (no release cert), set PHACIUSKEY_ALLOW_ADHOC=1;
# such a build works fine but resets its own Accessibility grant on every update.
RELEASE_CERT_SHA="${RELEASE_CERT_SHA:-3A75751FF7A48B380B37C1DB0148F9BABE054B14}"
SIGN_KEYCHAIN="${SIGN_KEYCHAIN:-}"   # optional: keychain file holding the identity
                                     # (must also be in the user search list and unlocked)

# Echo a code-signing identity's SHA-1 by (sub)name, or nothing. Matched without
# -v: a self-signed cert is usable for signing even when it isn't a trusted root
# (-v would hide it).
find_identity() {
    # shellcheck disable=SC2086
    security find-identity -p codesigning ${SIGN_KEYCHAIN:+"$SIGN_KEYCHAIN"} 2>/dev/null \
        | awk -v name="$1" '$0 ~ name { print $2; exit }' || true
}

SIGN_ID="$(find_identity 'phaciuskey-release')"
if [ -n "$SIGN_ID" ]; then
    echo "==> Code signing with 'phaciuskey-release' ($SIGN_ID)"
    if [ -n "$RELEASE_CERT_SHA" ] && [ "$SIGN_ID" != "$RELEASE_CERT_SHA" ]; then
        echo "!!! This 'phaciuskey-release' cert ($SIGN_ID) is NOT the canonical release" >&2
        echo "    cert ($RELEASE_CERT_SHA). Shipping it would reset every user's" >&2
        echo "    Accessibility grant. Aborting — see CONTRIBUTING.md > Releasing." >&2
        exit 1
    fi
elif [ "${PHACIUSKEY_ALLOW_ADHOC:-0}" = "1" ]; then
    SIGN_ID="-"
    echo "==> Ad-hoc code signing (PHACIUSKEY_ALLOW_ADHOC=1) — personal build only;"
    echo "    the Accessibility grant will NOT survive auto-updates of this build."
else
    echo "!!! No 'phaciuskey-release' signing identity found." >&2
    echo "    Releases must be signed with the shared identity so users keep their" >&2
    echo "    Accessibility grant across auto-updates (CONTRIBUTING.md > Releasing)." >&2
    echo "    For a personal build from source, re-run with PHACIUSKEY_ALLOW_ADHOC=1." >&2
    exit 1
fi
codesign --force --deep --sign "$SIGN_ID" "$APP"

echo "==> Building .dmg..."
DMG="$DIST/PhaciusKey-$VERSION.dmg"
rm -f "$DMG"
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "PhaciusKey" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

echo "==> Done: $DMG"
