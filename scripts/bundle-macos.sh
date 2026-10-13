#!/bin/sh
# Build target/release/Shard.app, so Shard opens from Finder and the Dock under its own name.
#   BIN=<path>          bundle this binary instead of building one (CI passes a universal build)
#   SIGN_IDENTITY=<id>  sign with this Developer ID, hardened runtime on (needed for notarising);
#                       without it the bundle is signed ad hoc, which is enough on this machine
set -eu
cd "$(dirname "$0")/.."
if [ -z "${BIN:-}" ]; then
	cargo build --release
	BIN=target/release/shard
fi
app=target/release/Shard.app
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$BIN" "$app/Contents/MacOS/shard"
cp assets/icons/Shard.icns "$app/Contents/Resources/Shard.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Shard</string>
  <key>CFBundleDisplayName</key><string>Shard</string>
  <key>CFBundleIdentifier</key><string>dev.shard.app</string>
  <key>CFBundleExecutable</key><string>shard</string>
  <key>CFBundleIconFile</key><string>Shard</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
if [ -n "${SIGN_IDENTITY:-}" ]; then
	codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$app"
else
	codesign --force --sign - "$app" # ad-hoc: macOS wants a signed bundle, even a local one
fi
echo "built $app"
