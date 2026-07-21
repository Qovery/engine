#!/usr/bin/env bash
# Contract tests for executable platform config models, plus Loki-specific behavior.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKL_BIN="${PKL_BIN:-pkl}"
LOKI_MODEL="$ROOT_DIR/platform-catalog/components/loki/config/runtime-values/model.pkl"
LOKI_TEST="$ROOT_DIR/platform-catalog/components/loki/tests/runtime-values.test.pkl"
CLUSTER_AGENT_MODEL="$ROOT_DIR/platform-catalog/components/cluster-agent/config/runtime-values/model.pkl"

command -v "$PKL_BIN" >/dev/null 2>&1 || {
  echo "ERROR: Pkl 0.32 is required (set PKL_BIN to its executable)" >&2
  exit 1
}
command -v jq >/dev/null 2>&1 || {
  echo "ERROR: jq is required" >&2
  exit 1
}

"$ROOT_DIR/scripts/sync-platform-pkl-contract.sh" --check

"$PKL_BIN" test "$LOKI_TEST"

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
  component_key="$(basename "$(dirname "$(dirname "$(dirname "$model")")")")"
  request="{\"operation\":\"DESCRIBE\",\"componentKey\":\"${component_key}\",\"profileConfig\":{},\"clusterContext\":null,\"clusterInputs\":{},\"componentOutputs\":{}}"
  response="$(evaluate "$model" "$request")"
  assert_result "$component_key DESCRIBE envelope" "$response" '
    (.fields | type) == "array" and
    (.requiredInputs | type) == "array" and
    (.violations | type) == "array" and
    (has("helmValues") | not)
  '
  model_count=$((model_count + 1))
done < <(find "$ROOT_DIR/platform-catalog/components" -path '*/config/runtime-values/model.pkl' -type f | sort)

if [[ "$model_count" -eq 0 ]]; then
  echo "ERROR: no executable platform configuration model was found" >&2
  exit 1
fi

cluster_agent_with_loki_request='{"operation":"COMPILE","componentKey":"cluster-agent","profileConfig":{},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{},"enabledComponents":["cluster-agent","loki"]}'
cluster_agent_with_loki="$(evaluate "$CLUSTER_AGENT_MODEL" "$cluster_agent_with_loki_request")"
assert_result "cluster-agent COMPILE with Loki" "$cluster_agent_with_loki" '
  .fields == [] and
  .requiredInputs == [] and
  .violations == [] and
  .helmValues.environmentVariables.LOKI_URL == "http://loki-gateway.qovery.svc"
'

cluster_agent_without_loki_request='{"operation":"COMPILE","componentKey":"cluster-agent","profileConfig":{},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{},"enabledComponents":["cluster-agent"]}'
cluster_agent_without_loki="$(evaluate "$CLUSTER_AGENT_MODEL" "$cluster_agent_without_loki_request")"
assert_result "cluster-agent COMPILE without Loki" "$cluster_agent_without_loki" '
  .fields == [] and
  .requiredInputs == [] and
  .violations == [] and
  .helmValues.environmentVariables.LOKI_URL == ""
'

# Component test suites (pkl:test): readable business-rule facts plus golden COMPILE outputs.
# Expected outputs live next to each suite as *.pkl-expected.pcf; when a model change is
# intentional, regenerate them with `pkl test --overwrite <suite>` and review the diff.
while IFS= read -r suite; do
  "$PKL_BIN" test "$suite"
done < <(find "$ROOT_DIR/platform-catalog/components" -path '*/tests/*.tests.pkl' -type f | sort)

describe_request='{"operation":"DESCRIBE","componentKey":"loki","profileConfig":{},"clusterContext":null,"clusterInputs":{},"componentOutputs":{}}'
describe="$(evaluate "$LOKI_MODEL" "$describe_request")"
assert_result "Loki DESCRIBE" "$describe" '
  (.fields | map(.key)) == ["retentionWeeks", "highAvailability", "storage"] and
  (.fields[] | select(.key == "retentionWeeks") | .description | startswith("Whole number")) and
  (.fields[] | select(.key == "storage") | .constraints.allowedValues) == ["pvc", "s3", "gcs", "azureBlob", "s3Compatible"] and
  .requiredInputs == [] and
  .violations == [] and
  (has("helmValues") | not)
'

gcp_describe_request='{"operation":"DESCRIBE","componentKey":"loki","profileConfig":{},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"GCP"},"clusterInputs":{},"componentOutputs":{}}'
gcp_describe="$(evaluate "$LOKI_MODEL" "$gcp_describe_request")"
assert_result "Loki contextual DESCRIBE on GCP" "$gcp_describe" '
  (.fields[] | select(.key == "storage") | .constraints.allowedValues) == ["pvc", "gcs"] and
  .requiredInputs == [] and
  .violations == []
'

resolve_request='{"operation":"RESOLVE_REQUIREMENTS","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{}}'
resolve="$(evaluate "$LOKI_MODEL" "$resolve_request")"
assert_result "Loki RESOLVE_REQUIREMENTS on AWS" "$resolve" '
  (.fields[] | select(.key == "storage") | .constraints.allowedValues) == ["pvc", "s3"] and
  (.requiredInputs | map(.key)) == ["infra.s3BucketName", "infra.lokiRoleArn"] and
  (.requiredInputs | map(.scope) | unique) == ["CLUSTER"] and
  .violations == []
'

gcp_resolve_request='{"operation":"RESOLVE_REQUIREMENTS","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"GCP"},"clusterInputs":{},"componentOutputs":{}}'
gcp_resolve="$(evaluate "$LOKI_MODEL" "$gcp_resolve_request")"
assert_result "Loki RESOLVE_REQUIREMENTS on GCP" "$gcp_resolve" '
  (.fields[] | select(.key == "storage") | .constraints.allowedValues) == ["pvc", "gcs"] and
  .requiredInputs == [] and
  (.violations | map(.code)) == ["STORAGE_PROVIDER_MISMATCH"]
'

managed_resolve_request='{"operation":"RESOLVE_REQUIREMENTS","componentKey":"loki","profileConfig":{"storage":"pvc","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"QOVERY_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{}}'
managed_resolve="$(evaluate "$LOKI_MODEL" "$managed_resolve_request")"
assert_result "Loki RESOLVE_REQUIREMENTS on a Qovery-managed cluster" "$managed_resolve" '
  (.fields[] | select(.key == "storage") | .constraints.allowedValues) == ["pvc"] and
  .requiredInputs == [] and
  .violations == []
'

gcp_validate_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"s3","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"GCP"},"clusterInputs":{},"componentOutputs":{}}'
gcp_validate="$(evaluate "$LOKI_MODEL" "$gcp_validate_request")"
assert_result "Loki VALIDATE on GCP" "$gcp_validate" '
  (.violations | map(.code)) == ["STORAGE_PROVIDER_MISMATCH"]
'

validate_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"pvc","retentionWeeks":0,"highAvailability":true},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AWS"},"clusterInputs":{},"componentOutputs":{}}'
validate="$(evaluate "$LOKI_MODEL" "$validate_request")"
assert_result "Loki VALIDATE product settings" "$validate" '
  (.violations | map(.code)) == ["VALUE_OUT_OF_RANGE", "HIGH_AVAILABILITY_REQUIRES_OBJECT_STORAGE"] and
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

credentialed_endpoint_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"s3Compatible","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"SCW"},"clusterInputs":{"infra.s3CompatibleBucketName":"qovery-loki-scw","infra.s3CompatibleEndpoint":"https://access:secret@s3.fr-par.scw.cloud","infra.s3CompatibleRegion":"fr-par","infra.s3CompatibleCredentialsSecretName":"loki-object-storage"},"componentOutputs":{}}'
credentialed_endpoint="$(evaluate "$LOKI_MODEL" "$credentialed_endpoint_request")"
assert_result "Loki VALIDATE S3-compatible endpoint without embedded credentials" "$credentialed_endpoint" '
  (.violations | map(.code)) == ["INPUT_PATTERN_MISMATCH"] and
  .violations[0].fieldPath == "clusterInputs.infra.s3CompatibleEndpoint"
'

newline_endpoint_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"s3Compatible","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"SCW"},"clusterInputs":{"infra.s3CompatibleBucketName":"qovery-loki-scw","infra.s3CompatibleEndpoint":"https://s3.fr-par.scw.cloud\n","infra.s3CompatibleRegion":"fr-par","infra.s3CompatibleCredentialsSecretName":"loki-object-storage"},"componentOutputs":{}}'
newline_endpoint="$(evaluate "$LOKI_MODEL" "$newline_endpoint_request")"
assert_result "Loki VALIDATE S3-compatible endpoint with trailing newline" "$newline_endpoint" '
  (.violations | map(.code)) == ["INPUT_PATTERN_MISMATCH"] and
  .violations[0].fieldPath == "clusterInputs.infra.s3CompatibleEndpoint"
'

short_azure_container_request='{"operation":"VALIDATE","componentKey":"loki","profileConfig":{"storage":"azureBlob","retentionWeeks":12,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AZURE"},"clusterInputs":{"infra.azureStorageAccountName":"qoveryloki","infra.azureContainerName":"a","infra.azureManagedIdentityClientId":"12345678-1234-1234-1234-123456789abc"},"componentOutputs":{}}'
short_azure_container="$(evaluate "$LOKI_MODEL" "$short_azure_container_request")"
assert_result "Loki VALIDATE Azure container minimum length" "$short_azure_container" '
  (.violations | map(.code)) == ["INPUT_PATTERN_MISMATCH"] and
  .violations[0].fieldPath == "clusterInputs.infra.azureContainerName"
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
  .helmValues.gateway.replicas == 3 and
  .helmValues.write.persistence.volumeClaimsEnabled == false and
  .helmValues.backend.persistence.volumeClaimsEnabled == false
'

gcs_compile_request='{"operation":"COMPILE","componentKey":"loki","profileConfig":{"storage":"gcs","retentionWeeks":8,"highAvailability":true},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"GCP"},"clusterInputs":{"infra.gcsBucketName":"qovery-loki-gcp","infra.gcpServiceAccountEmail":"loki@qovery-project.iam.gserviceaccount.com"},"componentOutputs":{}}'
gcs_compile="$(evaluate "$LOKI_MODEL" "$gcs_compile_request")"
assert_result "Loki COMPILE GCS" "$gcs_compile" '
  .violations == [] and
  (.fields[] | select(.key == "storage") | .constraints.allowedValues) == ["pvc", "gcs"] and
  (.requiredInputs | map(.key)) == ["infra.gcsBucketName", "infra.gcpServiceAccountEmail"] and
  .helmValues.deploymentMode == "SimpleScalable" and
  .helmValues.loki.storage.type == "gcs" and
  .helmValues.loki.storage.bucketNames == {"chunks":"qovery-loki-gcp","ruler":"qovery-loki-gcp","admin":"qovery-loki-gcp"} and
  .helmValues.loki.schemaConfig.configs[0].object_store == "gcs" and
  .helmValues.loki.compactor.delete_request_store == "gcs" and
  .helmValues.serviceAccount.annotations["iam.gke.io/gcp-service-account"] == "loki@qovery-project.iam.gserviceaccount.com" and
  .helmValues.write.persistence.volumeClaimsEnabled == false and
  .helmValues.backend.persistence.volumeClaimsEnabled == false
'

azure_compile_request='{"operation":"COMPILE","componentKey":"loki","profileConfig":{"storage":"azureBlob","retentionWeeks":6,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"AZURE"},"clusterInputs":{"infra.azureStorageAccountName":"qoveryloki","infra.azureContainerName":"loki-data","infra.azureManagedIdentityClientId":"12345678-1234-1234-1234-123456789abc"},"componentOutputs":{}}'
azure_compile="$(evaluate "$LOKI_MODEL" "$azure_compile_request")"
assert_result "Loki COMPILE Azure Blob" "$azure_compile" '
  .violations == [] and
  (.fields[] | select(.key == "storage") | .constraints.allowedValues) == ["pvc", "azureBlob"] and
  (.requiredInputs | map(.key)) == ["infra.azureStorageAccountName", "infra.azureContainerName", "infra.azureManagedIdentityClientId"] and
  .helmValues.deploymentMode == "SingleBinary" and
  .helmValues.loki.storage.type == "azure" and
  .helmValues.loki.storage.bucketNames == {"chunks":"loki-data","ruler":"loki-data","admin":"loki-data"} and
  .helmValues.loki.storage.azure.accountName == "qoveryloki" and
  .helmValues.loki.storage.azure.useFederatedToken == true and
  .helmValues.loki.podLabels["azure.workload.identity/use"] == "true" and
  .helmValues.serviceAccount.name == "qovery-storage" and
  .helmValues.serviceAccount.labels["azure.workload.identity/use"] == "true" and
  .helmValues.serviceAccount.annotations["azure.workload.identity/client-id"] == "12345678-1234-1234-1234-123456789abc"
'

s3_compatible_compile_request='{"operation":"COMPILE","componentKey":"loki","profileConfig":{"storage":"s3Compatible","retentionWeeks":10,"highAvailability":false},"clusterContext":{"mode":"CUSTOMER_MANAGED","provider":"SCW"},"clusterInputs":{"infra.s3CompatibleBucketName":"qovery-loki-scw","infra.s3CompatibleEndpoint":"https://s3.fr-par.scw.cloud","infra.s3CompatibleRegion":"fr-par","infra.s3CompatibleCredentialsSecretName":"loki-object-storage"},"componentOutputs":{}}'
s3_compatible_compile="$(evaluate "$LOKI_MODEL" "$s3_compatible_compile_request")"
assert_result "Loki COMPILE S3-compatible" "$s3_compatible_compile" '
  .violations == [] and
  (.fields[] | select(.key == "storage") | .constraints.allowedValues) == ["pvc", "s3Compatible"] and
  (.requiredInputs | map(.key)) == ["infra.s3CompatibleBucketName", "infra.s3CompatibleEndpoint", "infra.s3CompatibleRegion", "infra.s3CompatibleCredentialsSecretName"] and
  .helmValues.loki.storage.type == "s3" and
  .helmValues.loki.storage.s3.endpoint == "https://s3.fr-par.scw.cloud" and
  .helmValues.loki.storage.s3.region == "fr-par" and
  .helmValues.loki.storage.s3.accessKeyId == "${S3_ACCESS_KEY_ID}" and
  .helmValues.loki.storage.s3.secretAccessKey == "${S3_SECRET_ACCESS_KEY}" and
  .helmValues.loki.storage.s3.s3ForcePathStyle == true and
  .helmValues.singleBinary.extraArgs == ["-config.expand-env=true"] and
  .helmValues.singleBinary.extraEnvFrom == [{"secretRef":{"name":"loki-object-storage"}}] and
  .helmValues.write.extraEnvFrom == [{"secretRef":{"name":"loki-object-storage"}}] and
  .helmValues.read.extraEnvFrom == [{"secretRef":{"name":"loki-object-storage"}}] and
  .helmValues.backend.extraEnvFrom == [{"secretRef":{"name":"loki-object-storage"}}] and
  (.helmValues | has("global") | not)
'

echo "Platform config Pkl contract tests passed"
