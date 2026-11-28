#!/bin/sh
# Cargo's runner on macOS. The app binary runs from inside a dev bundle (target/<profile>/Waffle Dev.app), so
# macOS has a bundle id, name and icon to post its notifications as; anything else (tests, examples) runs as is.
set -e
bin=$1
shift
[ "$(basename "$bin")" = waffle ] || exec "$bin" "$@"
app="$(dirname "$bin")/Waffle Dev.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
# A clone, so signing the bundle leaves cargo's own binary alone.
cp -c "$bin" "$app/Contents/MacOS/waffle" 2>/dev/null || cp "$bin" "$app/Contents/MacOS/waffle"
root=$(cd "$(dirname "$0")/.." && pwd)
cp "$root/assets/macos/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Waffle Dev</string>
  <key>CFBundleIdentifier</key><string>com.frstycodes.waffle.dev</string>
  <key>CFBundleExecutable</key><string>waffle</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Waffle uses the microphone for voice calls.</string>
  <key>NSCameraUsageDescription</key><string>Waffle uses the camera for video calls.</string>
</dict></plist>
PLIST
# Signed as a bundle, so the signature names the bundle id that notification permission is granted to; codesign
# refuses files carrying extended attributes, which copies pick up. LaunchServices then knows where it lives.
xattr -cr "$app"
codesign --force --sign - "$app" 2>/dev/null || echo "macos-run: couldn't sign $app; notifications may be refused" >&2
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app" 2>/dev/null || true
exec "$app/Contents/MacOS/waffle" "$@"
