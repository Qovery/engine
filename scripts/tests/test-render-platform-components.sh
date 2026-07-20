#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ALLOY_CHART="$ROOT_DIR/lib-engine/lib/common/bootstrap/charts/alloy"
ALLOY_VALUES="$ROOT_DIR/platform-catalog/components/alloy/config/static-values/base.yaml"
LOKI_CHART="$ROOT_DIR/lib-engine/lib/common/bootstrap/charts/loki"
LOKI_VALUES="$ROOT_DIR/platform-catalog/components/loki/config/static-values/base.yaml"
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

command -v helm >/dev/null 2>&1 || fail "helm is required"

alloy_manifest="$TEMP_DIR/alloy.yaml"
helm template alloy "$ALLOY_CHART" \
  --namespace qovery \
  --values "$ALLOY_VALUES" > "$alloy_manifest"

assert_missing "$alloy_manifest" "{% raw %}"
assert_missing "$alloy_manifest" "{% endraw %}"
assert_contains "$alloy_manifest" 'template = "{{ if .Value }}{{ ToLower .Value }}{{ end }}"'
assert_contains "$alloy_manifest" "qovery_com_deployment_id"
assert_contains "$alloy_manifest" "value: http://loki-gateway.qovery.svc/loki/api/v1/push"
assert_contains "$alloy_manifest" "name: GOMEMLIMIT"
assert_contains "$alloy_manifest" "value: 450MiB"
assert_contains "$alloy_manifest" "cpu: 100m"
assert_contains "$alloy_manifest" "memory: 128Mi"
assert_contains "$alloy_manifest" "memory: 512Mi"
if grep -Eq '^namespace:' "$ALLOY_VALUES"; then
  fail "$ALLOY_VALUES contains the unsupported top-level namespace key"
fi

single_binary_manifest="$TEMP_DIR/loki-single-binary.yaml"
helm template loki "$LOKI_CHART" \
  --namespace qovery \
  --values "$LOKI_VALUES" \
  --set deploymentMode=SingleBinary \
  --set backend.replicas=0 \
  --set read.replicas=0 \
  --set write.replicas=0 > "$single_binary_manifest"

assert_contains "$single_binary_manifest" "name: loki-gateway"
assert_contains "$single_binary_manifest" 'proxy_pass       http://loki.qovery.svc.cluster.local:3100$request_uri;'

simple_scalable_manifest="$TEMP_DIR/loki-simple-scalable.yaml"
helm template loki "$LOKI_CHART" \
  --namespace qovery \
  --values "$LOKI_VALUES" \
  --values "$LOKI_CHART/simple-scalable-values.yaml" \
  --set loki.storage.type=s3 > "$simple_scalable_manifest"

assert_contains "$simple_scalable_manifest" "name: loki-gateway"
assert_contains "$simple_scalable_manifest" 'proxy_pass       http://loki-write.qovery.svc.cluster.local:3100$request_uri;'

echo "Platform component render tests passed"
