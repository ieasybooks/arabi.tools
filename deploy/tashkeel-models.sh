#!/bin/sh
# Provision the exact model revisions in tashkeel 0.1.0; never run on a job request.
set -eu
umask 022
destination=${1:?Pass a persistent model cache directory}
script_directory=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
staging=
trap '[ -z "$staging" ] || rm -rf "$staging"' EXIT
trap 'exit 1' HUP INT TERM

verify() {
    file=$1
    bytes=$2
    sha=$3
    if [ ! -f "$file" ] || [ "$(wc -c < "$file")" -ne "$bytes" ] || ! printf '%s  %s\n' "$sha" "$file" | sha256sum --check --status; then
        printf 'Missing or invalid pinned model file: %s\nUse a fresh cache directory; existing files were not replaced.\n' "$file" >&2
        exit 1
    fi
}

verify_shakkelha() {
    verify "$1/model.onnx" 10777454 41ed03075e80847aaef2bfdb65f123b7f225ff6a179a507b1ec3e6152184fa71
}

verify_catt() {
    verify "$1/encoder.onnx" 77915147 437a99c032ec7de4249280efbc4dce2d0532f95c9fe8356debf24ce05a34ba6a
    verify "$1/decoder.onnx" 37294 e6575bdba8623bf51a6d459fc4955e8a98aa3c6c2ffc5abffac26e883792bc80
}

mkdir -p "$destination/shakkelha" "$destination/catt"
if [ -e "$destination/shakkelha/v1" ]; then
    verify_shakkelha "$destination/shakkelha/v1"
else
    staging=$(mktemp -d "$destination/shakkelha/.download-XXXXXX")
    curl --fail --location --silent --show-error --retry 3 --connect-timeout 30 --max-time 900 --max-filesize 10777454 \
        https://github.com/ieasybooks/tashkeel-rs/releases/download/models-v1/shakkelha_normalized_big_avg20_sigmoid.onnx -o "$staging/model.onnx"
    verify_shakkelha "$staging"
    chmod 755 "$staging"
    mv -T "$staging" "$destination/shakkelha/v1"
    staging=
fi

if [ -e "$destination/catt/v1" ]; then
    verify_catt "$destination/catt/v1"
else
    staging=$(mktemp -d "$destination/catt/.download-XXXXXX")
    curl --fail --location --silent --show-error --retry 3 --connect-timeout 30 --max-time 900 --max-filesize 72109998 \
        https://github.com/abjadai/catt/releases/download/v2/eo_model_onnx.zip -o "$staging/eo_model_onnx.zip"
    verify "$staging/eo_model_onnx.zip" 72109998 5343bfdfda70c981b587811b79348b8dde91f65c0a551ca98f232ea5cb62ceac
    # Select known members instead of trusting archive paths.
    unzip -p "$staging/eo_model_onnx.zip" encoder.onnx > "$staging/encoder.onnx"
    unzip -p "$staging/eo_model_onnx.zip" decoder.onnx > "$staging/decoder.onnx"
    verify_catt "$staging"
    rm "$staging/eo_model_onnx.zip"
    chmod 755 "$staging"
    mv -T "$staging" "$destination/catt/v1"
    staging=
fi

mkdir -p "$destination/licenses"
cp -R "$script_directory/licenses/tashkeel/." "$destination/licenses/"
printf 'Verified tashkeel 0.1.0 models in %s\n' "$destination"
