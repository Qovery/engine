#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PUBLISH_SCRIPT="$ROOT_DIR/scripts/publish-platform-catalog.sh"
REGISTRY="public.ecr.aws/r3m4q3r9"
DIGEST="sha256:$(printf 'a%.0s' {1..64})"
CATALOG_VERSION="2026-07-20.1"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

fail() {
  echo "ERROR: $*" >&2
  exit 1
}

assert_contains() {
  local file="$1"
  local expected="$2"
  grep -F -- "$expected" "$file" >/dev/null || fail "$file does not contain: $expected"
}

assert_missing() {
  local file="$1"
  local unexpected="$2"
  if grep -F -- "$unexpected" "$file" >/dev/null; then
    fail "$file unexpectedly contains: $unexpected"
  fi
}

write_template_output() {
  local destination="$1"
  local version="$2"
  jq -n \
    --arg key "qovery-cluster-v0" \
    --arg version "$version" \
    --arg ref "$REGISTRY/platform-templates/qovery-cluster-v0:$version" \
    --arg digest "$DIGEST" \
    '[{key: $key, version: $version, ref: $ref, digest: $digest}]' > "$destination"
}

valid_output="$TEMP_DIR/valid-templates.json"
valid_catalog="$TEMP_DIR/valid-catalog.yaml"
write_template_output "$valid_output" "0.1.0"
"$PUBLISH_SCRIPT" render-catalog "$valid_output" "$valid_catalog" "$CATALOG_VERSION" "$REGISTRY"
assert_contains "$valid_catalog" "apiVersion: platform.qovery.com/v1alpha1"
assert_contains "$valid_catalog" "kind: PlatformTemplateCatalog"
assert_contains "$valid_catalog" "version: \"$CATALOG_VERSION\""
assert_contains "$valid_catalog" "defaultRelease:"
assert_contains "$valid_catalog" "repository: \"$REGISTRY/platform-templates/qovery-cluster-v0\""
assert_contains "$valid_catalog" "digest: \"$DIGEST\""
assert_missing "$valid_catalog" "repository: \"$REGISTRY/platform-templates/qovery-cluster-v0:0.1.0\""

partial_output="$TEMP_DIR/partial-templates.json"
partial_catalog="$TEMP_DIR/partial-catalog.yaml"
partial_error="$TEMP_DIR/partial-error.log"
printf '[]\n' > "$partial_output"
if "$PUBLISH_SCRIPT" render-catalog "$partial_output" "$partial_catalog" "$CATALOG_VERSION" "$REGISTRY" 2> "$partial_error"; then
  fail "a partial template publication produced a catalog snapshot"
fi
assert_contains "$partial_error" "not a valid complete template publication output"
[[ ! -e "$partial_catalog" ]] || fail "a partial catalog file was written"

mismatched_output="$TEMP_DIR/mismatched-templates.json"
mismatched_catalog="$TEMP_DIR/mismatched-catalog.yaml"
mismatched_error="$TEMP_DIR/mismatched-error.log"
write_template_output "$mismatched_output" "0.2.0"
if "$PUBLISH_SCRIPT" render-catalog "$mismatched_output" "$mismatched_catalog" "$CATALOG_VERSION" "$REGISTRY" 2> "$mismatched_error"; then
  fail "mismatched template coordinates produced a catalog snapshot"
fi
assert_contains "$mismatched_error" "catalog snapshot requires every declared template release"
[[ ! -e "$mismatched_catalog" ]] || fail "a mismatched catalog file was written"

echo "Platform catalog render tests passed"
