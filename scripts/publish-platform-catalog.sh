#!/usr/bin/env bash
# Publish the complete Engine v2 platform catalog in dependency order:
# config bundles, frozen charts, then verified root platform templates.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CATALOG_FILE="$ROOT_DIR/platform-catalog/catalog.yaml"
CONFIG_OUTPUT="$ROOT_DIR/platform-config-publish.json"
CHART_OUTPUT="$ROOT_DIR/frozen-charts-publish.json"
TEMPLATE_OUTPUT="$ROOT_DIR/platform-templates-publish.json"
TEMPLATE_ARTIFACT_TYPE="application/vnd.qovery.platform-template.v1"
TEMPLATE_LAYER_MEDIA_TYPE="application/vnd.qovery.platform-template.layer.v1+yaml"

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
        if (seen[key] != 1) {
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
        if (seen[key] != 1) {
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
