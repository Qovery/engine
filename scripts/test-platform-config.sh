#!/usr/bin/env bash
# Contract tests for executable platform config models.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKL_BIN="${PKL_BIN:-pkl}"
MODEL="$ROOT_DIR/platform-catalog/components/loki/config/model.pkl"

command -v "$PKL_BIN" >/dev/null 2>&1 || {
  echo "ERROR: Pkl 0.32 is required (set PKL_BIN to its executable)" >&2
  exit 1
}
command -v jq >/dev/null 2>&1 || {
  echo "ERROR: jq is required" >&2
  exit 1
}

evaluate() {
  "$PKL_BIN" eval -p "request=$1" "$MODEL"
}

describe_request='{"operation":"DESCRIBE","componentKey":"loki","profileConfig":{},"clusterContext":null,"clusterInputs":{},"componentOutputs":{}}'
describe="$(evaluate "$describe_request")"
jq -e '
  (.fields | map(.key)) == ["retentionWeeks", "highAvailability", "storage"] and
  .requiredInputs == [] and
  .violations == [] and
  (has("helmValues") | not)
' <<<"$describe" >/dev/null

resolve_request='{"operation":"RESOLVE_REQUIREMENTS","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{}}'
resolve="$(evaluate "$resolve_request")"
jq -e '
  (.requiredInputs | map(.key)) == ["infra.s3BucketName", "infra.lokiRoleArn"] and
  (.requiredInputs | map(.scope) | unique) == ["CLUSTER"] and
  .violations == []
' <<<"$resolve" >/dev/null

validate_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"pvc","retentionWeeks":0,"highAvailability":true},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{}}'
validate="$(evaluate "$validate_request")"
jq -e '
  (.violations | map(.code)) == ["VALUE_OUT_OF_RANGE", "HIGH_AVAILABILITY_REQUIRES_S3"] and
  (has("helmValues") | not)
' <<<"$validate" >/dev/null

compile_request='{"operation":"COMPILE","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{"infra.s3BucketName":"qovery-loki","infra.lokiRoleArn":"arn:aws:iam::123456789012:role/loki","infra.awsRegion":"eu-west-3"},"componentOutputs":{}}'
compile="$(evaluate "$compile_request")"
jq -e '
  .violations == [] and
  .helmValues.deploymentMode == "SingleBinary" and
  .helmValues.loki.commonConfig.replication_factor == 1 and
  .helmValues.loki.limits_config.retention_period == "2016h" and
  .helmValues.loki.storage.bucketNames == {"chunks":"qovery-loki","ruler":"qovery-loki","admin":"qovery-loki"} and
  .helmValues.loki.storage.s3.region == "eu-west-3" and
  .helmValues.serviceAccount.annotations["eks.amazonaws.com/role-arn"] == "arn:aws:iam::123456789012:role/loki"
' <<<"$compile" >/dev/null

echo "Platform config Pkl contract tests passed"
