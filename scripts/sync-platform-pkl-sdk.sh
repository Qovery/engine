#!/usr/bin/env bash
# Vendor the canonical Pkl authoring SDK (contract.pkl + sdk/) into every executable component
# bundle. q-core deliberately resolves Pkl imports only inside one digest-pinned OCI bundle, so
# each bundle carries a byte-identical copy of platform-catalog/pkl/{contract.pkl,sdk}.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CANONICAL_CONTRACT="$ROOT_DIR/platform-catalog/pkl/contract.pkl"
CANONICAL_SDK_DIR="$ROOT_DIR/platform-catalog/pkl/sdk"
COMPONENTS_DIR="$ROOT_DIR/platform-catalog/components"
MODE="${1:---write}"

if [[ "$MODE" != "--write" && "$MODE" != "--check" ]]; then
  echo "Usage: $0 [--write|--check]" >&2
  exit 2
fi

[[ -f "$CANONICAL_CONTRACT" ]] || {
  echo "ERROR: canonical Pkl contract is missing: $CANONICAL_CONTRACT" >&2
  exit 1
}
[[ -d "$CANONICAL_SDK_DIR" ]] || {
  echo "ERROR: canonical Pkl SDK directory is missing: $CANONICAL_SDK_DIR" >&2
  exit 1
}

model_count=0
status=0
while IFS= read -r model; do
  runtime_values_dir="$(dirname "$model")"
  component="$(basename "$(dirname "$(dirname "$runtime_values_dir")")")"
  model_count=$((model_count + 1))

  if [[ "$MODE" == "--write" ]]; then
    cp "$CANONICAL_CONTRACT" "$runtime_values_dir/contract.pkl"
    rm -rf "$runtime_values_dir/sdk"
    cp -R "$CANONICAL_SDK_DIR" "$runtime_values_dir/sdk"
    echo "--- $component: synchronized shared Pkl SDK"
    continue
  fi

  if [[ ! -f "$runtime_values_dir/contract.pkl" ]] \
    || ! cmp -s "$CANONICAL_CONTRACT" "$runtime_values_dir/contract.pkl"; then
    echo "ERROR: $component vendored contract differs from platform-catalog/pkl/contract.pkl" >&2
    status=1
  fi
  # diff -r reports missing, stale, and extraneous vendored SDK files alike.
  if ! diff -r "$CANONICAL_SDK_DIR" "$runtime_values_dir/sdk" >/dev/null 2>&1; then
    echo "ERROR: $component vendored sdk/ differs from platform-catalog/pkl/sdk" >&2
    status=1
  fi
done < <(find "$COMPONENTS_DIR" -path '*/config/runtime-values/model.pkl' -type f | sort)

if [[ "$model_count" -eq 0 ]]; then
  echo "ERROR: no executable platform configuration model was found" >&2
  exit 1
fi

if [[ "$status" -ne 0 ]]; then
  echo "Run ./scripts/sync-platform-pkl-sdk.sh and commit the result." >&2
fi

exit "$status"
