#!/bin/sh
# Install the official CPU runtime for tashkeel 0.1.0 (ONNX Runtime API 31).
set -eu
architecture=${1:?Pass amd64 or arm64}
destination=${2:?Pass an output directory}
version=1.31.0
case "$architecture" in
    amd64) asset=linux-x64; bytes=11408003; sha=cc5c72baf5ae5c8238a6841f0897227be2d02826b9cf98eaf02fdefaeeb03a57 ;;
    arm64) asset=linux-aarch64; bytes=10331104; sha=c5b8b3cca31f3d643a3b313b8f42e2d0f16a4fcefd02cda343d97bd5306afaab ;;
    *) echo 'Unsupported ONNX Runtime architecture' >&2; exit 1 ;;
esac
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
trap 'exit 1' HUP INT TERM
root="onnxruntime-$asset-$version"
archive="$staging/runtime.tgz"
curl --fail --location --silent --show-error --retry 3 --connect-timeout 30 --max-time 180 --max-filesize "$bytes" \
    "https://github.com/microsoft/onnxruntime/releases/download/v$version/$root.tgz" -o "$archive"
if [ "$(wc -c < "$archive")" -ne "$bytes" ] || ! printf '%s  %s\n' "$sha" "$archive" | sha256sum --check --status; then
    echo 'ONNX Runtime archive size or SHA-256 mismatch' >&2
    exit 1
fi
# Extract only the library and the notices from the checksum-verified archive.
tar -xzf "$archive" -C "$staging" \
    "$root/lib/libonnxruntime.so.$version" "$root/LICENSE" "$root/ThirdPartyNotices.txt"
mkdir -p "$destination"
install -m 644 "$staging/$root/lib/libonnxruntime.so.$version" "$destination/libonnxruntime.so"
install -m 644 "$staging/$root/LICENSE" "$destination/LICENSE"
install -m 644 "$staging/$root/ThirdPartyNotices.txt" "$destination/ThirdPartyNotices.txt"
