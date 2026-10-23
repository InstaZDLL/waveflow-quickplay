#!/usr/bin/env bash
# Builds "WaveFlow QuickPlay.app" around a built binary, signs it ad hoc
# and wraps it in a .dmg with a link to /Applications.
#
#   packaging/macos/bundle.sh <binary> <version> <out-dir>
#
# Ad-hoc signing ("-") is the macOS counterpart of the self-signed
# Windows certificate: the bundle is sealed (tampering breaks it), but no
# Apple Developer ID vouches for it. Its identity is the hash of the code
# it signs, so every build has a new one. Gatekeeper blocks the first
# launch of a downloaded copy: on macOS 15 Sequoia and later the user
# allows it in System Settings › Privacy & Security (Open Anyway), on
# earlier versions with right-click › Open. Apple silicon refuses to run
# unsigned code at all, so the signature is not optional.
set -euo pipefail

binary=$1
version=$2
out=$3
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

app="$out/WaveFlow QuickPlay.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/waveflow-quickplay"
chmod 755 "$app/Contents/MacOS/waveflow-quickplay"
cp "$root/assets/icon.icns" "$app/Contents/Resources/icon.icns"
cp "$root/LICENSE" "$app/Contents/Resources/LICENSE.txt"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"

codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict --verbose=2 "$app"
codesign --display --verbose=2 "$app" 2>&1 | grep -E '^(Identifier|Format|Signature)='

stage="$out/dmg-stage"
rm -rf "$stage"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
dmg="$out/waveflow-quickplay_${version}_macos-universal.dmg"
rm -f "$dmg"
hdiutil create -volname "WaveFlow QuickPlay $version" -srcfolder "$stage" \
  -fs HFS+ -format UDZO -ov "$dmg"
rm -rf "$stage"
hdiutil verify "$dmg"
echo "$dmg"
