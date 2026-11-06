#!/usr/bin/env bash
# Copies a release onto Cloudflare R2, the updater's second host
# (src/update.rs), in WaveFlow's bucket under quickplay/:
#
#   quickplay/releases/<tag>/<every file>   same names, same bytes
#   quickplay/latest.json                   the manifest, pointing there
#
#   publish-r2.sh --check                  prints "configured" or "absent"
#   publish-r2.sh <files dir> <latest.json>
#
# <files dir> holds the release's files under their published names (a
# `gh release download` of the tag, or the GitLab build's packages);
# <latest.json> is its manifest, wherever its addresses point. The copy's
# manifest is the same with each address moved to quickplay/releases/<tag>/.
# The signatures do not change: they cover the package's bytes, its version
# and its name, not where it is downloaded from.
#
# The manifest goes up last, once every package it names answers on the
# public domain with the size of the local file: it is what makes a client
# download, so it never points at something that is not there yet.
#
# Environment:
#   R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY  an R2 API token's S3 keys
#   R2_ACCOUNT_ID                          the Cloudflare account id, or its
#                                          S3 endpoint
#   R2_BUCKET                              the bucket's name
#   R2_PUBLIC_URL   the bucket's public domain, as compiled into the app
#                   (default https://download.waveflow.app)
#   TAG_NAME        the release's tag, v<version>
#   R2_DRY_RUN=1    touch no network: print the uploads, write the manifest
#                   to R2_DRY_RUN_OUT (default r2-latest.json)
#   R2_FORCE=1      replace the manifest even when the one published is
#                   newer (a deliberate rollback), or the same version for
#                   more platforms
#
# A pre-release tag (v1.5.0-beta.1) has its files copied, never its
# manifest.
#
# None of the four R2_* secrets set means no copy: the script says so and
# succeeds, so a fork still releases. Some but not all of them is an error.
# Needs the AWS CLI, curl and jq.
set -euo pipefail

r2_state() {
  local set=0
  for v in R2_ACCESS_KEY_ID R2_SECRET_ACCESS_KEY R2_ACCOUNT_ID R2_BUCKET; do
    [ -n "${!v:-}" ] && set=$((set + 1))
  done
  case "$set" in
    0) echo absent ;;
    4) echo configured ;;
    *) echo partial ;;
  esac
}

state="$(r2_state)"
if [ "$state" = "partial" ]; then
  echo "::error::R2 half configured: set all four of R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY, R2_ACCOUNT_ID, R2_BUCKET, or none" >&2
  exit 1
fi
if [ "${1:-}" = "--check" ]; then
  echo "$state"
  exit 0
fi

files="${1:?usage: publish-r2.sh <files dir> <latest.json>}"
manifest="${2:?usage: publish-r2.sh <files dir> <latest.json>}"
dry_run="${R2_DRY_RUN:-0}"
: "${TAG_NAME:?TAG_NAME is required}"

if [ "$state" = "absent" ]; then
  if [ "$dry_run" != "1" ]; then
    echo "No R2_* secret set: the R2 copy is not configured, nothing to publish."
    exit 0
  fi
  # A dry run needs names to print, not credentials.
  R2_ACCOUNT_ID="<account-id>"
  R2_BUCKET="<bucket>"
else
  # The account id, or the S3 endpoint the dashboard shows right beside it,
  # https://<id>.r2.cloudflarestorage.com: a secret set to the second once
  # made the upload fail on DNS, at https://https://…, with nothing saying
  # why. Anything else is refused by name.
  account="${R2_ACCOUNT_ID#https://}"
  account="${account%%.*}"
  if ! [[ "$account" =~ ^[0-9a-f]{32}$ ]]; then
    echo "::error::R2_ACCOUNT_ID is neither an account id nor https://<id>.r2.cloudflarestorage.com" >&2
    exit 1
  fi
  R2_ACCOUNT_ID="$account"
fi

public="${R2_PUBLIC_URL:-https://download.waveflow.app}"
public="${public%/}"
prefix="quickplay/releases/$TAG_NAME"
channel="quickplay/latest.json"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

version="$(jq -er '.version' "$manifest")"
if [ "v$version" != "$TAG_NAME" ]; then
  echo "::error::the manifest is for $version, the tag is $TAG_NAME" >&2
  exit 1
fi
# Every package the manifest names must be one of the files copied.
while read -r name; do
  if [ ! -f "$files/$name" ]; then
    echo "::error::the manifest names $name, which is not in $files" >&2
    exit 1
  fi
done < <(jq -r '.platforms[].url | split("/") | last' "$manifest" | tr -d '\r')
jq --arg base "$public/$prefix" \
  '.platforms |= map_values(.url = ($base + "/" + (.url | split("/") | last)))' \
  "$manifest" >"$work/latest.json"

export AWS_ACCESS_KEY_ID="${R2_ACCESS_KEY_ID:-}"
export AWS_SECRET_ACCESS_KEY="${R2_SECRET_ACCESS_KEY:-}"
export AWS_DEFAULT_REGION=auto
# AWS CLI 2.23 started sending CRC checksums R2 does not take on every
# request; Cloudflare's guidance is to send them only where required.
export AWS_REQUEST_CHECKSUM_CALCULATION=when_required
export AWS_RESPONSE_CHECKSUM_VALIDATION=when_required
endpoint="https://$R2_ACCOUNT_ID.r2.cloudflarestorage.com"

run() {
  if [ "$dry_run" = "1" ]; then
    printf '[dry-run]'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

# Five minutes at the edge, not a year: a rebuild of the tag replaces the
# packages under the same names, and a cached old copy would no longer match
# the new manifest's signature.
uploaded=0
for f in "$files"/*; do
  [ -f "$f" ] || continue
  name="$(basename "$f")"
  [ "$name" = "latest.json" ] && continue # goes up rewritten, below
  run aws s3 cp "$f" "s3://$R2_BUCKET/$prefix/$name" \
    --endpoint-url "$endpoint" \
    --cache-control "public, max-age=300" \
    --only-show-errors
  uploaded=$((uploaded + 1))
done
if [ "$uploaded" -eq 0 ]; then
  echo "::error::no file in $files" >&2
  exit 1
fi
echo "Uploaded $uploaded files to $prefix/."

# QuickPlay has one channel, and latest.json is what every copy installs
# from: a pre-release (1.5.0-beta.1) is copied but never offered.
semver='^[0-9]+\.[0-9]+\.[0-9]+$'
if ! [[ "$version" =~ $semver ]]; then
  echo "::notice::$version is a pre-release: its files are under $prefix/, $channel is left as it is"
  exit 0
fi

if [ "$dry_run" != "1" ]; then
  # Through the public domain, as a client will ask: the S3 API accepting a
  # file says nothing about the domain serving it. The query string is a
  # cache key the edge has not seen.
  while read -r url; do
    name="${url##*/}"
    expected="$(wc -c <"$files/$name" | tr -d ' ')"
    got="$(curl -fsSIL "$url?v=$RANDOM$RANDOM" | tr -d '\r' |
      awk -F': ' 'tolower($1) == "content-length" { n = $2 } END { print n }')"
    if [ "$got" != "$expected" ]; then
      echo "::error::$url answers with ${got:-no} bytes, the package has $expected; the manifest stays unpublished" >&2
      exit 1
    fi
    echo "verified $url ($got bytes)"
  done < <(jq -r '.platforms[].url' "$work/latest.json" | tr -d '\r')

  # Never move the manifest backwards: a rebuild of an older tag would
  # otherwise put an older version in place, and every installation newer
  # than it would stop seeing updates from this host. The same version again
  # is a rebuild, and goes through. R2_FORCE=1 overrides.
  if [ "${R2_FORCE:-0}" != "1" ]; then
    code="$(curl -sSL -o "$work/current.json" -w '%{http_code}' \
      "$public/$channel?v=$RANDOM$RANDOM" || true)"
    case "$code" in
      200)
        published="$(jq -r '.version // empty' "$work/current.json" 2>/dev/null || true)"
        if ! [[ "$published" =~ $semver ]]; then
          echo "::error::cannot compare the published ${published:-manifest} with $version; set R2_FORCE=1 to replace it" >&2
          exit 1
        fi
        newest="$(printf '%s\n%s\n' "$published" "$version" | sort -V | tail -1)"
        if [ "$published" != "$version" ] && [ "$newest" = "$published" ]; then
          echo "::warning::$channel already offers $published, newer than $version: left in place (the files stay under $prefix/)"
          exit 0
        fi
        # The same version with fewer platforms (a Linux-only rebuild that
        # found no manifest to start from) would take the update away from
        # the copies of the platforms it lacks.
        if [ "$published" = "$version" ]; then
          missing="$(jq -r --slurpfile new "$work/latest.json" \
            '(.platforms | keys) - ($new[0].platforms | keys) | join(", ")' "$work/current.json")"
          if [ -n "$missing" ]; then
            echo "::error::$channel offers $version for $missing too, which this manifest lacks; set R2_FORCE=1 to replace it" >&2
            exit 1
          fi
        fi
        ;;
      404) ;; # the first manifest
      *)
        echo "::error::could not read $public/$channel (HTTP ${code:-none}); set R2_FORCE=1 to publish without the check" >&2
        exit 1
        ;;
    esac
  fi
fi

run aws s3 cp "$work/latest.json" "s3://$R2_BUCKET/$channel" \
  --endpoint-url "$endpoint" \
  --content-type application/json \
  --cache-control "no-cache" \
  --only-show-errors

if [ "$dry_run" = "1" ]; then
  out="${R2_DRY_RUN_OUT:-r2-latest.json}"
  cp "$work/latest.json" "$out"
  echo "Dry run: the manifest would be $public/$channel, written to $out"
  exit 0
fi

served="$(curl -fsSL "$public/$channel?v=$RANDOM$RANDOM" | jq -r '.version')"
if [ "$served" != "$version" ]; then
  echo "::error::$public/$channel serves $served after publishing $version" >&2
  exit 1
fi
echo "Published $version on $public/$channel"
