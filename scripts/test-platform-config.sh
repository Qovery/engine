#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PKL_BIN=${PKL_BIN:-pkl}

if ! command -v "$PKL_BIN" >/dev/null 2>&1; then
  echo "error: $PKL_BIN is required" >&2
  exit 1
fi

"$ROOT_DIR/scripts/sync-platform-pkl-contract.sh" --check

suite_count=0
while IFS= read -r suite; do
  "$PKL_BIN" test "$suite"
  suite_count=$((suite_count + 1))
done < <(
  find "$ROOT_DIR/platform-catalog/components" -type f \
    \( -name '*.test.pkl' -o -name '*.tests.pkl' \) \
    | sort
)

if [[ $suite_count -eq 0 ]]; then
  echo "error: no platform catalogue Pkl test suite found" >&2
  exit 1
fi

echo "Platform configuration model tests passed ($suite_count suites)."
