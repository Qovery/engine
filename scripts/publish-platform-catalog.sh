#!/usr/bin/env bash
# Publish the complete Engine v2 platform catalog in dependency order:
# config bundles, frozen charts, then verified root platform templates.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CATALOG_FILE="$ROOT_DIR/platform-catalog/catalog.yaml"
CONFIG_OUTPUT="$ROOT_DIR/platform-config-publish.json"
CHART_OUTPUT="$ROOT_DIR/frozen-charts-publish.json"
TEMPLATE_OUTPUT="$ROOT_DIR/platform-templates-publish.json"
CATALOG_OUTPUT="$ROOT_DIR/platform-catalog-publish.json"
TEMPLATE_ARTIFACT_TYPE="application/vnd.qovery.platform-template.v1"
TEMPLATE_LAYER_MEDIA_TYPE="application/vnd.qovery.platform-template.layer.v1+yaml"
CATALOG_ARTIFACT_TYPE="application/vnd.qovery.platform-catalog.v1"
CATALOG_LAYER_MEDIA_TYPE="application/vnd.qovery.platform-catalog.layer.v1+yaml"

fatal() { echo "ERROR: $*" >&2; exit 1; }

catalog_templates() {
  awk '
    /^templates:/ { in_section = 1; next }
    in_section && /^[^ #]/ { in_section = 0 }
    in_section && $1 == "-" && $2 == "key:" { key = $3 }
    in_section && $1 == "version:" { version = $2 }
    in_section && $1 == "path:" { print key, version, $2 }
  ' "$CATALOG_FILE"
}

catalog_default() {
  awk '
    /^defaultTemplate:/ { in_section = 1; next }
    in_section && /^[^ #]/ { in_section = 0 }
    in_section && $1 == "key:" { key = $2 }
    in_section && $1 == "version:" { print key, $2; exit }
  ' "$CATALOG_FILE"
}

render_catalog() (
  set -euo pipefail
  local template_output="$1"
  local destination="$2"
  local catalog_version="$3"
  local registry="${4%/}"
  local render_dir expected_coordinates actual_coordinates default_key default_version
  render_dir="$(mktemp -d)"
  trap 'rm -rf "$render_dir"' EXIT
  expected_coordinates="$render_dir/expected.tsv"
  actual_coordinates="$render_dir/actual.tsv"

  [ -f "$template_output" ] || fatal "template publication output $template_output does not exist"
  jq -e --arg registry "$registry" '
    type == "array" and length > 0 and
    all(.[];
      (.key | type) == "string" and
      (.version | type) == "string" and
      (.ref | type) == "string" and
      (.digest | type) == "string" and
      (.digest | test("^sha256:[0-9a-f]{64}$")) and
      .ref == ($registry + "/platform-templates/" + .key + ":" + .version)
    ) and
    (group_by([.key, .version]) | all(.[]; length == 1))
  ' "$template_output" >/dev/null || fatal "$template_output is not a valid complete template publication output"

  catalog_templates | awk '{ print $1 "\t" $2 }' | sort > "$expected_coordinates"
  jq -r '.[] | [.key, .version] | @tsv' "$template_output" | sort > "$actual_coordinates"
  cmp -s "$expected_coordinates" "$actual_coordinates" || {
    echo "ERROR: catalog snapshot requires every declared template release" >&2
    diff -u "$expected_coordinates" "$actual_coordinates" >&2 || true
    exit 1
  }

  read -r default_key default_version < <(catalog_default)
  [ -n "$default_key" ] && [ -n "$default_version" ] || fatal "defaultTemplate is missing from $CATALOG_FILE"
  jq -e --arg key "$default_key" --arg version "$default_version" \
    'any(.[]; .key == $key and .version == $version)' "$template_output" >/dev/null ||
    fatal "default template $default_key:$default_version is absent from the complete publication output"

  mkdir -p "$(dirname "$destination")"
  {
    echo 'apiVersion: platform.qovery.com/v1alpha1'
    echo 'kind: PlatformTemplateCatalog'
    printf 'version: %s\n' "$(jq -Rn --arg value "$catalog_version" '$value')"
    echo 'defaultRelease:'
    printf '  key: %s\n' "$(jq -Rn --arg value "$default_key" '$value')"
    printf '  version: %s\n' "$(jq -Rn --arg value "$default_version" '$value')"
    echo 'releases:'
    jq -r '
      .[] |
      "  - key: " + (.key | @json) + "\n" +
      "    version: " + (.version | @json) + "\n" +
      "    repository: " + (.ref | sub(":[^:]+$"; "") | @json) + "\n" +
      "    digest: " + (.digest | @json)
    ' "$template_output"
  } > "$destination"
)

# Kept as a subcommand so the complete-graph renderer can be tested without a registry.
render_template() (
  set -euo pipefail
  local source="$1"
  local config_output="$2"
  local chart_output="$3"
  local destination="$4"
  local expected_key="$5"
  local expected_version="$6"
  local registry="${7%/}"
  local render_dir
  render_dir="$(mktemp -d)"
  trap 'rm -rf "$render_dir"' EXIT

  [ -f "$source" ] || fatal "template source $source does not exist"
  [ -f "$config_output" ] || fatal "config publish output $config_output does not exist"
  [ -f "$chart_output" ] || fatal "chart publish output $chart_output does not exist"

  local actual_key actual_version
  actual_key="$(awk '/^platformTemplateRelease:/ { release = 1; next } release && $1 == "key:" { print $2; exit }' "$source")"
  actual_version="$(awk '/^platformTemplateRelease:/ { release = 1; next } release && $1 == "version:" { print $2; exit }' "$source")"
  [ "$actual_key" = "$expected_key" ] && [ "$actual_version" = "$expected_version" ] ||
    fatal "template source declares $actual_key:$actual_version; catalog expects $expected_key:$expected_version"

  jq -e '
    type == "array" and
    all(.[];
      (.component | type) == "string" and
      (.version | type) == "string" and
      (.ref | type) == "string" and
      (.digest | type) == "string" and
      (.digest | test("^sha256:[0-9a-f]{64}$"))
    ) and
    (group_by([.component, .version]) | all(.[]; length == 1))
  ' "$config_output" >/dev/null || fatal "$config_output is not a valid unique config publication output"
  jq -r '.[] | [.component, .version, .ref, .digest] | @tsv' "$config_output" > "$render_dir/config.tsv"

  awk -v map_file="$render_dir/config.tsv" -v registry="$registry" '
    function leading_spaces(value, prefix) {
      prefix = value
      sub(/[^ ].*$/, "", prefix)
      return length(prefix)
    }
    BEGIN {
      while ((getline line < map_file) > 0) {
        split(line, fields, "\t")
        key = fields[1] SUBSEP fields[2]
        refs[key] = fields[3]
        digests[key] = fields[4]
        labels[key] = fields[1] ":" fields[2]
      }
      close(map_file)
    }
    {
      trimmed = $0
      sub(/^[ ]*/, "", trimmed)
      if (in_ref && trimmed != "" && trimmed !~ /^#/ && leading_spaces($0) <= ref_indent) {
        in_ref = 0
      }
      if ($0 ~ /^[ ]*configRef:[ ]*$/) {
        in_ref = 1
        ref_indent = leading_spaces($0)
        component = ""
        version = ""
      } else if (in_ref && leading_spaces($0) == ref_indent + 2) {
        field = trimmed
        sub(/[ ]+#.*$/, "", field)
        if (field ~ /^chart:[ ]*/) {
          component = field
          sub(/^chart:[ ]*/, "", component)
        } else if (field ~ /^version:[ ]*/) {
          version = field
          sub(/^version:[ ]*/, "", version)
        } else if (field ~ /^digest:[ ]*/) {
          key = component SUBSEP version
          if (!(key in digests)) {
            print "ERROR: configRef " component ":" version " has no verified publication" > "/dev/stderr"
            errors++
          } else if (refs[key] != registry "/platform-config/" component ":" version) {
            print "ERROR: configRef " component ":" version " publication ref is " refs[key] > "/dev/stderr"
            errors++
          } else {
            sub(/digest:[ ].*$/, "digest: " digests[key])
            seen[key]++
          }
        }
      }
      print
    }
    END {
      for (key in digests) {
        if (seen[key] > 1) {
          print "ERROR: verified config publication " labels[key] " is referenced " (seen[key] + 0) " time(s)" > "/dev/stderr"
          errors++
        }
      }
      if (errors > 0) exit 2
    }
  ' "$source" > "$render_dir/template.yaml"

  jq -e '
    type == "array" and
    all(.[];
      (.chart | type) == "string" and
      (.version | type) == "string" and
      (.ref | type) == "string" and
      (.digest | type) == "string" and
      (.digest | test("^sha256:[0-9a-f]{64}$"))
    ) and
    (group_by([.chart, .version]) | all(.[]; length == 1))
  ' "$chart_output" >/dev/null || fatal "$chart_output is not a valid unique chart publication output"
  jq -r '.[] | [.chart, .version, .ref, .digest] | @tsv' "$chart_output" > "$render_dir/charts.tsv"

  awk -v map_file="$render_dir/charts.tsv" -v registry="$registry" '
    function leading_spaces(value, prefix) {
      prefix = value
      sub(/[^ ].*$/, "", prefix)
      return length(prefix)
    }
    function verify_chart(key) {
      if (name == "" || version == "" || repository == "") {
        print "ERROR: incomplete chart descriptor" > "/dev/stderr"
        errors++
        return
      }
      key = name SUBSEP version
      if (!(key in refs)) {
        print "ERROR: chart " name ":" version " has no verified publication" > "/dev/stderr"
        errors++
      } else if (repository != "oci://" registry "/charts/") {
        print "ERROR: chart " name ":" version " repository is " repository > "/dev/stderr"
        errors++
      } else if (refs[key] != registry "/charts/" name ":" version) {
        print "ERROR: chart " name ":" version " publication ref is " refs[key] > "/dev/stderr"
        errors++
      } else {
        seen[key]++
      }
    }
    BEGIN {
      while ((getline line < map_file) > 0) {
        split(line, fields, "\t")
        key = fields[1] SUBSEP fields[2]
        refs[key] = fields[3]
        labels[key] = fields[1] ":" fields[2]
      }
      close(map_file)
    }
    {
      trimmed = $0
      sub(/^[ ]*/, "", trimmed)
      if (in_chart && trimmed != "" && trimmed !~ /^#/ && leading_spaces($0) <= chart_indent) {
        verify_chart()
        in_chart = 0
      }
      if ($0 ~ /^[ ]*chart:[ ]*$/) {
        in_chart = 1
        chart_indent = leading_spaces($0)
        repository = ""
        name = ""
        version = ""
      } else if (in_chart && leading_spaces($0) == chart_indent + 2) {
        field = trimmed
        sub(/[ ]+#.*$/, "", field)
        if (field ~ /^repository:[ ]*/) {
          repository = field
          sub(/^repository:[ ]*/, "", repository)
        } else if (field ~ /^name:[ ]*/) {
          name = field
          sub(/^name:[ ]*/, "", name)
        } else if (field ~ /^version:[ ]*/) {
          version = field
          sub(/^version:[ ]*/, "", version)
        }
      }
    }
    END {
      if (in_chart) verify_chart()
      for (key in refs) {
        if (seen[key] > 1) {
          print "ERROR: verified chart publication " labels[key] " is referenced " (seen[key] + 0) " time(s)" > "/dev/stderr"
          errors++
        }
      }
      if (errors > 0) exit 2
    }
  ' "$render_dir/template.yaml"

  mkdir -p "$(dirname "$destination")"
  mv "$render_dir/template.yaml" "$destination"
)

if [ "${1:-}" = "render" ]; then
  [ "$#" -eq 8 ] || fatal "usage: $0 render <source> <config-output> <chart-output> <destination> <key> <version> <registry>"
  shift
  render_template "$@"
  exit 0
fi

if [ "${1:-}" = "render-catalog" ]; then
  [ "$#" -eq 5 ] || fatal "usage: $0 render-catalog <template-output> <destination> <catalog-version> <registry>"
  shift
  render_catalog "$@"
  exit 0
fi

command -v oras >/dev/null 2>&1 || fatal "oras CLI is required (https://oras.land)"
command -v helm >/dev/null 2>&1 || fatal "helm CLI is required"
command -v jq >/dev/null 2>&1 || fatal "jq is required"
[ -n "${PLATFORM_CONFIG_REGISTRY:-}" ] || fatal "PLATFORM_CONFIG_REGISTRY is not set"

"$ROOT_DIR/scripts/publish-platform-config.sh"
"$ROOT_DIR/scripts/publish-frozen-charts.sh"

templates="${PLATFORM_TEMPLATES:-all}"
templates="${templates//,/ }"
if [ "$templates" = "none" ]; then
  echo "--- PLATFORM_TEMPLATES=none, skipping root template publication"
  echo '[]' > "$TEMPLATE_OUTPUT"
  exit 0
fi

revision="${CI_COMMIT_SHA:-$(git -C "$ROOT_DIR" rev-parse HEAD)}"
created="$(git -C "$ROOT_DIR" show -s --format=%cI "$revision" 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)"
oras_flags="${PLATFORM_CONFIG_ORAS_FLAGS:-}"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
results=()
matched=0

while read -r key version path; do
  if [ "$templates" != "all" ]; then
    case " $templates " in
      *" $key "*) ;;
      *) continue ;;
    esac
  fi
  matched=$((matched + 1))
  source="$ROOT_DIR/$path"
  template_dir="$work_dir/$key"
  rendered="$template_dir/template.yaml"
  render_template "$source" "$CONFIG_OUTPUT" "$CHART_OUTPUT" "$rendered" "$key" "$version" "$PLATFORM_CONFIG_REGISTRY"

  ref="$PLATFORM_CONFIG_REGISTRY/platform-templates/$key:$version"
  echo "--- $key: pushing verified root template $ref"
  (cd "$template_dir" && oras push $oras_flags "$ref" \
    --artifact-type "$TEMPLATE_ARTIFACT_TYPE" \
    --annotation "org.opencontainers.image.created=$created" \
    --annotation "org.opencontainers.image.revision=$revision" \
    --annotation "org.opencontainers.image.source=https://gitlab.com/qovery/backend/engine" \
    "template.yaml:$TEMPLATE_LAYER_MEDIA_TYPE") || fatal "push failed for $ref; ensure its ECR repository is declared by infrastructure"

  digest="$(oras manifest fetch $oras_flags --descriptor "$ref" | jq -r '.digest')"
  [ -n "$digest" ] && [ "$digest" != "null" ] || fatal "cannot read published manifest digest for $ref"
  results+=("$(jq -n --arg k "$key" --arg v "$version" --arg r "$ref" --arg d "$digest" \
    '{key: $k, version: $v, ref: $r, digest: $d}')")
done < <(catalog_templates)

[ "$matched" -gt 0 ] || fatal "nothing published — no catalog template matches '$templates'"
printf '%s\n' "${results[@]}" | jq -s '.' > "$TEMPLATE_OUTPUT"
echo "--- wrote $TEMPLATE_OUTPUT"
jq '.' "$TEMPLATE_OUTPUT"

catalog_version="${PLATFORM_CATALOG_VERSION:-${CI_COMMIT_SHA:-$(git -C "$ROOT_DIR" rev-parse HEAD)}}"
catalog_dir="$work_dir/platform-catalog"
catalog_yaml="$catalog_dir/catalog.yaml"
render_catalog "$TEMPLATE_OUTPUT" "$catalog_yaml" "$catalog_version" "$PLATFORM_CONFIG_REGISTRY"
catalog_tag_ref="$PLATFORM_CONFIG_REGISTRY/platform-catalog/catalog:$catalog_version"
echo "--- platform catalog: pushing complete snapshot $catalog_tag_ref"
(cd "$catalog_dir" && oras push $oras_flags "$catalog_tag_ref" \
  --artifact-type "$CATALOG_ARTIFACT_TYPE" \
  --annotation "org.opencontainers.image.created=$created" \
  --annotation "org.opencontainers.image.revision=$revision" \
  --annotation "org.opencontainers.image.source=https://gitlab.com/qovery/backend/engine" \
  "catalog.yaml:$CATALOG_LAYER_MEDIA_TYPE") || fatal "push failed for $catalog_tag_ref; ensure its ECR repository is declared by infrastructure"

catalog_digest="$(oras manifest fetch $oras_flags --descriptor "$catalog_tag_ref" | jq -r '.digest')"
[ -n "$catalog_digest" ] && [ "$catalog_digest" != "null" ] || fatal "cannot read published manifest digest for $catalog_tag_ref"
catalog_canonical_ref="$PLATFORM_CONFIG_REGISTRY/platform-catalog/catalog@$catalog_digest"
jq -n \
  --arg version "$catalog_version" \
  --arg ref "$catalog_tag_ref" \
  --arg digest "$catalog_digest" \
  --arg canonicalRef "$catalog_canonical_ref" \
  '{version: $version, ref: $ref, digest: $digest, canonicalRef: $canonicalRef, activated: false}' > "$CATALOG_OUTPUT"

set_service_version() {
  local api_host="$1"
  local service_type="$2"
  local version="$3"
  local action="$4"
  local api_url="$api_host"
  case "$api_url" in
    http://*|https://*) ;;
    *) api_url="https://$api_url" ;;
  esac
  echo "--- $action on $api_url"
  curl --fail-with-body --silent --show-error --request PUT \
    --header 'Content-Type: application/json' \
    --header "X-Qovery-Signature: $CI_ENGINE_VERSION_CONTROLLER_TOKEN" \
    --get "$api_url/engine/serviceVersion" \
    --data-urlencode "serviceType=$service_type" \
    --data-urlencode "version=$version" >/dev/null
}

published_operator_chart_version() {
  jq -er '
    [.[] | select(.chart == "qovery-operator") | .version] as $versions
    | if (($versions | length) == 1
          and ($versions[0] | type) == "string"
          and ($versions[0] | length) > 0)
      then $versions[0]
      else error("expected exactly one published qovery-operator chart version")
      end
  ' "$CHART_OUTPUT"
}

activate_environment() {
  local api_host="$1"
  local operator_chart_version="$2"

  set_service_version \
    "$api_host" \
    "PLATFORM_CATALOG" \
    "$catalog_canonical_ref" \
    "activating catalog $catalog_canonical_ref"

  set_service_version \
    "$api_host" \
    "QOVERY_OPERATOR_CHART" \
    "$operator_chart_version" \
    "declaring Qovery Operator chart $operator_chart_version"
}

if [ "${PLATFORM_CATALOG_ACTIVATE:-false}" = "true" ]; then
  [ -n "${CI_ENGINE_VERSION_CONTROLLER_TOKEN:-}" ] || fatal "CI_ENGINE_VERSION_CONTROLLER_TOKEN is required to activate the catalog"
  operator_chart_version="$(published_operator_chart_version)"
  activate_environment "${QOVERY_ADMIN_DEV_API:-api-admin-dev.qovery.com}" "$operator_chart_version"
  activate_environment "${QOVERY_ADMIN_API:-api-admin.qovery.com}" "$operator_chart_version"
  jq '.activated = true' "$CATALOG_OUTPUT" > "$work_dir/catalog-output.json"
  mv "$work_dir/catalog-output.json" "$CATALOG_OUTPUT"
fi

echo "--- wrote $CATALOG_OUTPUT"
jq '.' "$CATALOG_OUTPUT"
