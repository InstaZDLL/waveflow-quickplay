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

if [ $# -ne 3 ]; then
  echo "usage: $0 <binary> <version> <out-dir>" >&2
  exit 2
fi
binary=$1
version=$2
out=$3
# The version goes into a sed replacement, the plist and file names.
if ! [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "bundle.sh: not a version: $version" >&2
  exit 2
fi
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

# The image is staged, built and verified in a folder of this run's own,
# then moved over the previous one: a failed build leaves that image alone
# and no half-written one behind.
stage=$(mktemp -d "$out/dmg-stage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/root"
cp -R "$app" "$stage/root/"
ln -s /Applications "$stage/root/Applications"
dmg="$out/waveflow-quickplay_${version}_macos-universal.dmg"
hdiutil create -volname "WaveFlow QuickPlay $version" -srcfolder "$stage/root" \
  -fs HFS+ -format UDZO -ov "$stage/image.dmg"
hdiutil verify "$stage/image.dmg"
mv -f "$stage/image.dmg" "$dmg"
echo "$dmg"
