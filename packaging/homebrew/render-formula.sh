#!/usr/bin/env sh
set -eu

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
  echo "usage: $0 VERSION CHECKSUMS OUTPUT [RELEASE_BASE_URL]" >&2
  exit 2
fi

version=$1
checksums=$2
output=$3
base_url=${4:-https://github.com/contextlimit/cx/releases/download/v${version}}
script_dir=$(CDPATH='' cd -- "$(dirname "$0")" && pwd)
template=${script_dir}/Formula/cx.rb.in

case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *)
    echo "render-formula: VERSION must be semantic, for example 0.1.0" >&2
    exit 2
    ;;
esac

case "$base_url" in
  https://*) ;;
  file://*)
    if [ "${CX_HOMEBREW_ALLOW_FILE_URL:-0}" != "1" ]; then
      echo "render-formula: file URLs require CX_HOMEBREW_ALLOW_FILE_URL=1" >&2
      exit 2
    fi
    ;;
  *)
    echo "render-formula: URL must use https" >&2
    exit 2
  ;;
esac

if [ ! -f "$template" ]; then
  echo "render-formula: missing template at $template" >&2
  exit 1
fi

if [ ! -f "$checksums" ]; then
  echo "render-formula: missing checksum manifest at $checksums" >&2
  exit 1
fi

base_url=${base_url%/}

checksum_for() {
  asset=$1
  rows=$(awk -v asset="$asset" '$2 == asset { count += 1; value = $1 } END { print count + 0, value }' "$checksums")
  count=${rows%% *}
  sha256=${rows#* }

  if [ "$count" -ne 1 ]; then
    echo "render-formula: expected exactly one checksum for $asset, found $count" >&2
    exit 2
  fi

  case "$sha256" in
    *[!0-9a-f]*)
      echo "render-formula: checksum for $asset must use lowercase hexadecimal" >&2
      exit 2
      ;;
  esac

  if [ "${#sha256}" -ne 64 ]; then
    echo "render-formula: checksum for $asset must contain 64 hexadecimal characters" >&2
    exit 2
  fi

  printf '%s' "$sha256"
}

darwin_arm64_asset=cx-v${version}-darwin-arm64
darwin_x64_asset=cx-v${version}-darwin-x64
linux_arm64_asset=cx-v${version}-linux-arm64
linux_x64_asset=cx-v${version}-linux-x64

darwin_arm64_sha256=$(checksum_for "$darwin_arm64_asset")
darwin_x64_sha256=$(checksum_for "$darwin_x64_asset")
linux_arm64_sha256=$(checksum_for "$linux_arm64_asset")
linux_x64_sha256=$(checksum_for "$linux_x64_asset")

mkdir -p "$(dirname "$output")"
awk \
  -v version="$version" \
  -v darwin_arm64_url="$base_url/$darwin_arm64_asset" \
  -v darwin_arm64_sha256="$darwin_arm64_sha256" \
  -v darwin_x64_url="$base_url/$darwin_x64_asset" \
  -v darwin_x64_sha256="$darwin_x64_sha256" \
  -v linux_arm64_url="$base_url/$linux_arm64_asset" \
  -v linux_arm64_sha256="$linux_arm64_sha256" \
  -v linux_x64_url="$base_url/$linux_x64_asset" \
  -v linux_x64_sha256="$linux_x64_sha256" \
  '{
    gsub(/@VERSION@/, version)
    gsub(/@DARWIN_ARM64_URL@/, darwin_arm64_url)
    gsub(/@DARWIN_ARM64_SHA256@/, darwin_arm64_sha256)
    gsub(/@DARWIN_X64_URL@/, darwin_x64_url)
    gsub(/@DARWIN_X64_SHA256@/, darwin_x64_sha256)
    gsub(/@LINUX_ARM64_URL@/, linux_arm64_url)
    gsub(/@LINUX_ARM64_SHA256@/, linux_arm64_sha256)
    gsub(/@LINUX_X64_URL@/, linux_x64_url)
    gsub(/@LINUX_X64_SHA256@/, linux_x64_sha256)
    print
  }' \
  "$template" > "$output"

if grep -q '@[A-Z0-9_][A-Z0-9_]*@' "$output"; then
  echo "render-formula: unresolved template placeholder in $output" >&2
  exit 1
fi

echo "rendered $output"
