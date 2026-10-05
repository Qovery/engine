#!/usr/bin/env bash
# Link every executable component bundle to the canonical Pkl authoring SDK: contract.pkl and sdk
# under config/runtime-values/ are relative symbolic links to platform-catalog/pkl/{contract.pkl,sdk}.
# q-core deliberately resolves Pkl imports only inside one digest-pinned OCI bundle, so
# publish-platform-config.sh replaces both links with copies of the canonical files.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CANONICAL_CONTRACT="$ROOT_DIR/platform-catalog/pkl/contract.pkl"
CANONICAL_SDK_DIR="$ROOT_DIR/platform-catalog/pkl/sdk"
COMPONENTS_DIR="$ROOT_DIR/platform-catalog/components"
# Unpublished exchange fixtures exercise the same autonomous bundle layout and linking rules.
FIXTURES_DIR="$ROOT_DIR/platform-catalog/pkl/tests/fixtures"
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

relative_path() {
  local base="$1" prefix=""
  while [[ "$2" != "$base/"* ]]; do
    base="${base%/*}"
    prefix="../$prefix"
  done
  printf '%s%s\n' "$prefix" "${2#"$base/"}"
}

sync_link() {
  local link="$1" target actual problem=""
  target="$(relative_path "${link%/*}" "$2")"
  if [[ -L "$link" ]]; then
    actual="$(readlink "$link")"
    [[ "$actual" == "$target" ]] || problem="links to $actual instead of $target"
  elif [[ -e "$link" ]]; then
    problem="is a file or directory instead of a link to $target"
  else
    problem="is missing (expected a link to $target)"
  fi
  [[ -n "$problem" ]] || return 0

  if [[ "$MODE" == "--write" ]]; then
    rm -rf "$link"
    ln -s "$target" "$link"
    echo "--- ${link#"$ROOT_DIR/"}: linked to $target"
  else
    echo "ERROR: ${link#"$ROOT_DIR/"} $problem" >&2
    status=1
  fi
}

model_count=0
status=0
while IFS= read -r model; do
  runtime_values_dir="$(dirname "$model")"
  # Test fixtures must not satisfy the publisher's "at least one executable component" guard.
  if [[ "$model" == "$COMPONENTS_DIR/"* ]]; then
    model_count=$((model_count + 1))
  fi
  sync_link "$runtime_values_dir/contract.pkl" "$CANONICAL_CONTRACT"
  sync_link "$runtime_values_dir/sdk" "$CANONICAL_SDK_DIR"
done < <(find "$COMPONENTS_DIR" "$FIXTURES_DIR" -path '*/config/runtime-values/model.pkl' -type f | sort)

while IFS= read -r link; do
  runtime_values_dir="$(dirname "$link")"
  if [[ "$runtime_values_dir" == */config/runtime-values && -f "$runtime_values_dir/model.pkl" ]]; then
    case "$(basename "$link")" in
      contract.pkl | sdk) continue ;;
    esac
  fi
  echo "ERROR: ${link#"$ROOT_DIR/"} is an unmanaged symbolic link; remove it" >&2
  status=1
done < <(find "$COMPONENTS_DIR" "$FIXTURES_DIR" \( -path '*/config' -o -path '*/config/*' \) -type l | sort)

if [[ "$model_count" -eq 0 ]]; then
  echo "ERROR: no executable platform configuration model was found" >&2
  exit 1
fi

if [[ "$status" -ne 0 && "$MODE" == "--check" ]]; then
  echo "Run ./scripts/sync-platform-pkl-sdk.sh and commit the result." >&2
fi

exit "$status"
