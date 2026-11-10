#!/usr/bin/env bash
# Regenerates packaging/macos/file-icons/*.icns, the Finder document icons
# Info.plist names per format, from the SVG sources in assets/file-icons/svg
# (the same pack as the Windows .ico files). Run by hand when the artwork
# changes; the result is committed, so a release needs no image tools.
#
#   packaging/macos/file-icons.sh
#
# Needs ImageMagick 7 (`magick`) with an SVG delegate. Runs anywhere: an
# .icns is a list of PNGs behind 8-byte headers, written here directly,
# so Apple's iconutil is not needed.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
src="$root/assets/file-icons/svg"
out="$here/file-icons"
# The formats Info.plist gives an icon of their own. .aif and .oga use the
# aiff and ogg icons; .mp4 is not declared on macOS.
formats=(flac mp3 wav aiff ogg opus m4a aac dsf dff m3u m3u8)
# (type, pixels) for each entry: 16 pt and 32 pt @2x (ic11, ic12), then
# 128 to 512 pt, each @1x and @2x. No 16 or 32 pixel frame: Finder (macOS
# 26) draws the PNG types for them, icp4 and icp5, as noise, and iconutil
# reads them wrong too; without them it scales ic11 and ic12 down cleanly.
# So the simplified W of the .ico files' smallest frames is not used here.
entries=(ic11:32 ic12:64 ic07:128 ic13:256 ic08:256 ic14:512 ic09:512 ic10:1024)

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# PNG of `svg` at `px` pixels, rendered at that size (the viewBox is 256)
# rather than scaled down from a large one. No timestamps or text chunks,
# so the same artwork gives the same bytes.
render() {
  local svg=$1 px=$2 png=$3
  magick -background none -density "$(awk "BEGIN { print 96 * $px / 256 }")" "$svg" \
    -resize "${px}x${px}!" -strip -define png:exclude-chunks=date,time,tEXt,zTXt,iTXt \
    -define png:color-type=6 "$png"
}

# Big-endian 32-bit integer.
be32() {
  local n=$1
  printf "\\$(printf %03o $(((n >> 24) & 255)))\\$(printf %03o $(((n >> 16) & 255)))\\$(printf %03o $(((n >> 8) & 255)))\\$(printf %03o $((n & 255)))"
}

mkdir -p "$out"
for format in "${formats[@]}"; do
  body="$tmp/$format.body"
  : >"$body"
  for entry in "${entries[@]}"; do
    type=${entry%%:*}
    px=${entry##*:}
    svg="$src/$format.svg"
    png="$tmp/$format-$type.png"
    render "$svg" "$px" "$png"
    size=$(wc -c <"$png")
    { printf '%s' "$type"; be32 $((size + 8)); cat "$png"; } >>"$body"
  done
  total=$(($(wc -c <"$body") + 8))
  { printf 'icns'; be32 "$total"; cat "$body"; } >"$out/$format.icns"
  echo "$out/$format.icns ($total bytes)"
done
