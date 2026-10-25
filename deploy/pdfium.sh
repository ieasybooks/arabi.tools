#!/bin/sh
# Install the same pinned PDFium runtime as tahweel; no downloads during cargo builds.
set -eu
architecture=${1:?Pass amd64 or arm64}
destination=${2:?Pass an output directory}
case "$architecture" in
    amd64) asset=linux-x64; sha=bdc60126399f39fe768566a18d4be055a7b3681bc9e248e8a6656b4f6cae0aa2 ;;
    arm64) asset=linux-arm64; sha=042de247138f96341534cccf24ed21c2550a7245b936bb4a60d2340120000f05 ;;
    *) echo 'Unsupported PDFium architecture' >&2; exit 1 ;;
esac
mkdir -p "$destination"
archive=$(mktemp)
trap 'rm -f "$archive"' EXIT HUP INT TERM
curl --fail --location --silent --show-error --max-time 120 \
    "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8111/pdfium-$asset.tgz" -o "$archive"
printf '%s  %s\n' "$sha" "$archive" | sha256sum --check --status
tar -xzf "$archive" -C "$destination" lib/libpdfium.so LICENSE licenses
