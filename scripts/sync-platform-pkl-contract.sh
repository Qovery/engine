#!/usr/bin/env bash
# Vendor the canonical q-core/Pkl contract into every executable component bundle.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CANONICAL_CONTRACT="$ROOT_DIR/platform-catalog/pkl/component-contract.pkl"
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

model_count=0
status=0
while IFS= read -r model; do
  contract="$(dirname "$model")/contract.pkl"
  component="$(basename "$(dirname "$(dirname "$(dirname "$model")")")")"
  model_count=$((model_count + 1))

  if [[ "$MODE" == "--write" ]]; then
    cp "$CANONICAL_CONTRACT" "$contract"
    echo "--- $component: synchronized shared Pkl contract"
  elif [[ ! -f "$contract" ]] || ! cmp -s "$CANONICAL_CONTRACT" "$contract"; then
    echo "ERROR: $component evaluator contract differs from platform-catalog/pkl/component-contract.pkl" >&2
    echo "Run ./scripts/sync-platform-pkl-contract.sh and commit the result." >&2
    status=1
  fi
done < <(find "$COMPONENTS_DIR" -path '*/config/runtime-values/model.pkl' -type f | sort)

if [[ "$model_count" -eq 0 ]]; then
  echo "ERROR: no executable platform configuration model was found" >&2
  exit 1
fi

exit "$status"
