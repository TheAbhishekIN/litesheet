#!/usr/bin/env bash
# Build a release LiteSheet.app in ./dist. Pass --universal for an
# Apple Silicon + Intel binary (needs both rustup targets installed).
#
# Signing: set SIGN_IDENTITY to a "Developer ID Application: …" identity in
# your keychain to sign with the hardened runtime (required for Apple
# notarization). Without it the app is ad-hoc signed, which runs locally but
# makes Gatekeeper warn on other Macs.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
APP=dist/LiteSheet.app

if [[ "${1:-}" == "--universal" ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  BIN=$(mktemp)
  lipo -create -output "$BIN" \
    target/aarch64-apple-darwin/release/litesheet \
    target/x86_64-apple-darwin/release/litesheet
else
  cargo build --release
  BIN=target/release/litesheet
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/LiteSheet"

# App icon: build an .icns from assets/icon.png with the system tools.
ICONSET=$(mktemp -d)/LiteSheet.iconset
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z $size $size assets/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) assets/icon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/LiteSheet.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>LiteSheet</string>
  <key>CFBundleDisplayName</key><string>LiteSheet</string>
  <key>CFBundleIdentifier</key><string>app.litesheet.LiteSheet</string>
  <key>CFBundleExecutable</key><string>LiteSheet</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>LiteSheet</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
</dict>
</plist>
PLIST
if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$APP"
  echo "Built $APP (signed by $SIGN_IDENTITY)"
else
  # Ad-hoc sign so Gatekeeper and the Keychain treat it as one app.
  codesign --force --deep --sign - "$APP" >/dev/null
  echo "Built $APP (ad-hoc signed)"
fi
