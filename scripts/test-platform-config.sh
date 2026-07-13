#!/usr/bin/env bash
# Contract tests for executable platform config models, plus Loki-specific behavior.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKL_BIN="${PKL_BIN:-pkl}"
LOKI_MODEL="$ROOT_DIR/platform-catalog/components/loki/config/model.pkl"

command -v "$PKL_BIN" >/dev/null 2>&1 || {
  echo "ERROR: Pkl 0.32 is required (set PKL_BIN to its executable)" >&2
  exit 1
}
command -v jq >/dev/null 2>&1 || {
  echo "ERROR: jq is required" >&2
  exit 1
}

evaluate() {
  local model="$1"
  local request="$2"

  "$PKL_BIN" eval -p "request=$request" "$model"
}

assert_result() {
  local operation="$1"
  local response="$2"
  local assertion="$3"

  if jq -e "$assertion" <<<"$response" >/dev/null; then
    return 0
  fi

  echo "ERROR: platform configuration assertion failed for $operation" >&2
  echo "Actual response:" >&2
  jq . <<<"$response" >&2 || echo "$response" >&2
  return 1
}

# Every executable model must at least support the shared DESCRIBE envelope. Component-specific
# behavior stays below, where failures can name the operation and print the actual JSON response.
model_count=0
while IFS= read -r model; do
  component_key="$(basename "$(dirname "$(dirname "$model")")")"
  request="{\"operation\":\"DESCRIBE\",\"componentKey\":\"${component_key}\",\"profileConfig\":{},\"clusterContext\":null,\"clusterInputs\":{},\"componentOutputs\":{}}"
  response="$(evaluate "$model" "$request")"
  assert_result "$component_key DESCRIBE envelope" "$response" '
    (.fields | type) == "array" and
    (.requiredInputs | type) == "array" and
    (.violations | type) == "array" and
    (has("helmValues") | not)
  '
  model_count=$((model_count + 1))
done < <(find "$ROOT_DIR/platform-catalog/components" -path '*/config/model.pkl' -type f | sort)

if [[ "$model_count" -eq 0 ]]; then
  echo "ERROR: no executable platform configuration model was found" >&2
  exit 1
fi

describe_request='{"operation":"DESCRIBE","componentKey":"loki","profileConfig":{},"clusterContext":null,"clusterInputs":{},"componentOutputs":{}}'
describe="$(evaluate "$LOKI_MODEL" "$describe_request")"
assert_result "Loki DESCRIBE" "$describe" '
  (.fields | map(.key)) == ["retentionWeeks", "highAvailability", "storage"] and
  (.fields[] | select(.key == "retentionWeeks") | .description | startswith("Whole number")) and
  .requiredInputs == [] and
  .violations == [] and
  (has("helmValues") | not)
'

resolve_request='{"operation":"RESOLVE_REQUIREMENTS","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{}}'
resolve="$(evaluate "$LOKI_MODEL" "$resolve_request")"
assert_result "Loki RESOLVE_REQUIREMENTS on AWS" "$resolve" '
  (.requiredInputs | map(.key)) == ["infra.s3BucketName", "infra.lokiRoleArn"] and
  (.requiredInputs | map(.scope) | unique) == ["CLUSTER"] and
  .violations == []
'

gcp_resolve_request='{"operation":"RESOLVE_REQUIREMENTS","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"GCP"},"clusterInputs":{},"componentOutputs":{}}'
gcp_resolve="$(evaluate "$LOKI_MODEL" "$gcp_resolve_request")"
assert_result "Loki RESOLVE_REQUIREMENTS on GCP" "$gcp_resolve" '
  .requiredInputs == [] and
  (.violations | map(.code)) == ["S3_AWS_ONLY"]
'

gcp_validate_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"GCP"},"clusterInputs":{},"componentOutputs":{}}'
gcp_validate="$(evaluate "$LOKI_MODEL" "$gcp_validate_request")"
assert_result "Loki VALIDATE on GCP" "$gcp_validate" '
  (.violations | map(.code)) == ["S3_AWS_ONLY"]
'

validate_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"pvc","retentionWeeks":0,"highAvailability":true},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{}}'
validate="$(evaluate "$LOKI_MODEL" "$validate_request")"
assert_result "Loki VALIDATE product settings" "$validate" '
  (.violations | map(.code)) == ["VALUE_OUT_OF_RANGE", "HIGH_AVAILABILITY_REQUIRES_S3"] and
  (has("helmValues") | not)
'

invalid_s3_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{"infra.s3BucketName":"INVALID_BUCKET","infra.lokiRoleArn":"not-an-arn"},"componentOutputs":{}}'
invalid_s3="$(evaluate "$LOKI_MODEL" "$invalid_s3_request")"
assert_result "Loki VALIDATE malformed S3 inputs" "$invalid_s3" '
  (.violations | map(.code)) == ["INPUT_PATTERN_MISMATCH", "INPUT_PATTERN_MISMATCH"] and
  (.violations | map(.fieldPath)) == ["clusterInputs.infra.s3BucketName", "clusterInputs.infra.lokiRoleArn"]
'

non_string_input_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{"infra.s3BucketName":123,"infra.lokiRoleArn":"arn:aws:iam::123456789012:role/loki"},"componentOutputs":{}}'
non_string_input="$(evaluate "$LOKI_MODEL" "$non_string_input_request")"
assert_result "Loki VALIDATE non-string runtime input" "$non_string_input" '
  (.violations | map(.code)) == ["REQUIRED_INPUT_MISSING"] and
  .violations[0].fieldPath == "clusterInputs.infra.s3BucketName"
'

decimal_retention_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"pvc","retentionWeeks":12.5,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{}}'
decimal_retention="$(evaluate "$LOKI_MODEL" "$decimal_retention_request")"
assert_result "Loki VALIDATE decimal retention" "$decimal_retention" '
  (.violations | map(.code)) == ["INVALID_TYPE"] and
  .violations[0].message == "Loki configuration field retentionWeeks must be a whole number"
'

missing_region_request='{"operation":"COMPILE","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{"infra.s3BucketName":"qovery-loki","infra.lokiRoleArn":"arn:aws:iam::123456789012:role/loki"},"componentOutputs":{}}'
missing_region="$(evaluate "$LOKI_MODEL" "$missing_region_request")"
assert_result "Loki COMPILE without q-core region" "$missing_region" '
  (.violations | map(.fieldPath)) == ["clusterInputs.infra.awsRegion"] and
  (has("helmValues") | not)
'

compile_request='{"operation":"COMPILE","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{"infra.s3BucketName":"qovery-loki","infra.lokiRoleArn":"arn:aws:iam::123456789012:role/loki","infra.awsRegion":"eu-west-3"},"componentOutputs":{}}'
compile="$(evaluate "$LOKI_MODEL" "$compile_request")"
assert_result "Loki COMPILE" "$compile" '
  .violations == [] and
  .helmValues.deploymentMode == "SingleBinary" and
  .helmValues.loki.commonConfig.replication_factor == 1 and
  .helmValues.loki.limits_config.retention_period == "2016h" and
  .helmValues.loki.storage.bucketNames == {"chunks":"qovery-loki","ruler":"qovery-loki","admin":"qovery-loki"} and
  .helmValues.loki.storage.s3.region == "eu-west-3" and
  .helmValues.singleBinary.persistence.enabled == false and
  .helmValues.singleBinary.extraVolumes == [{"name":"storage","emptyDir":{}}] and
  .helmValues.singleBinary.extraVolumeMounts == [{"name":"storage","mountPath":"/var/loki"}] and
  .helmValues.write.persistence.volumeClaimsEnabled == false and
  .helmValues.backend.persistence.volumeClaimsEnabled == false and
  .helmValues.serviceAccount.annotations["eks.amazonaws.com/role-arn"] == "arn:aws:iam::123456789012:role/loki"
'

ha_compile_request='{"operation":"COMPILE","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":4,"highAvailability":true},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{"infra.s3BucketName":"qovery-loki","infra.lokiRoleArn":"arn:aws:iam::123456789012:role/loki","infra.awsRegion":"eu-west-3"},"componentOutputs":{}}'
ha_compile="$(evaluate "$LOKI_MODEL" "$ha_compile_request")"
assert_result "Loki COMPILE high availability" "$ha_compile" '
  .violations == [] and
  .helmValues.deploymentMode == "SimpleScalable" and
  .helmValues.loki.commonConfig.replication_factor == 3 and
  .helmValues.loki.limits_config.retention_period == "672h" and
  .helmValues.singleBinary.replicas == 0 and
  .helmValues.write.replicas == 3 and
  .helmValues.read.replicas == 3 and
  .helmValues.backend.replicas == 3 and
  .helmValues.write.persistence.volumeClaimsEnabled == false and
  .helmValues.backend.persistence.volumeClaimsEnabled == false
'

echo "Platform config Pkl contract tests passed"
