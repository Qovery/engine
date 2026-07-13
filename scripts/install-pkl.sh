#!/usr/bin/env bash

# Installs the Pkl version used by the executable platform configuration contract tests.
# The version and checksums are pinned so CI never executes an unreviewed binary.

set -euo pipefail

PKL_VERSION="${PKL_VERSION:-0.32.0}"
PKL_INSTALL_DIR="${PKL_INSTALL_DIR:-/usr/local/bin}"

if command -v pkl >/dev/null 2>&1 && pkl --version | head -n 1 | grep -Fq "Pkl ${PKL_VERSION}"; then
  echo "Pkl ${PKL_VERSION} is already installed"
  exit 0
fi

case "$(uname -s):$(uname -m)" in
  Linux:x86_64 | Linux:amd64)
    pkl_asset="pkl-linux-amd64"
    pkl_sha256="15e7e7375c28b8542b3d13fe35bccaeb7b9542114008998708385489885f41e7"
    ;;
  Linux:aarch64 | Linux:arm64)
    # Pkl names its Linux ARM asset aarch64, unlike tools that publish an arm64 asset.
    pkl_asset="pkl-linux-aarch64"
    pkl_sha256="b1ba7ef5dec9287f8f843ce563911eba822fed5789a5baab8cc712615a9dbaf0"
    ;;
  Darwin:arm64 | Darwin:aarch64)
    pkl_asset="pkl-macos-aarch64"
    pkl_sha256="fb891856705f3dcb8589c74999e01ded3eeb6b77ad5c6a95c02579a848c13928"
    ;;
  Darwin:x86_64 | Darwin:amd64)
    pkl_asset="pkl-macos-amd64"
    pkl_sha256="190ad63fec4f81f40e75dba8d6309033dd30e51b3e042db3b846f67c7ab46e3d"
    ;;
  *)
    echo "ERROR: unsupported Pkl platform: $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

tmp_file="$(mktemp)"
trap 'rm -f "$tmp_file"' EXIT

curl -fsSL \
  -o "$tmp_file" \
  "https://github.com/apple/pkl/releases/download/${PKL_VERSION}/${pkl_asset}"

if command -v sha256sum >/dev/null 2>&1; then
  echo "${pkl_sha256}  ${tmp_file}" | sha256sum -c -
else
  echo "${pkl_sha256}  ${tmp_file}" | shasum -a 256 -c -
fi

mkdir -p "$PKL_INSTALL_DIR"
install -m 0755 "$tmp_file" "$PKL_INSTALL_DIR/pkl"
echo "Installed Pkl ${PKL_VERSION} from ${pkl_asset}"
