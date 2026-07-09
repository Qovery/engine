#!/usr/bin/env bash
# Mirror frozen Helm charts to the OCI registry (Engine v2).
#
# For each chart listed under `charts:` in platform-catalog/catalog.yaml,
# packages its in-repo (helm-freeze vendored or Qovery-authored) directory and
# pushes it to:
#   oci://$PLATFORM_CONFIG_REGISTRY/charts/<name>:<Chart.yaml version>
# so the copy reviewed in this repo is the artifact actually executed, instead
# of an upstream pull at install time. To mirror another chart, add a
# name+path entry to catalog.yaml — nothing else to change.
#
# Writes frozen-charts-publish.json (chart, version, ref, digest).
# See platform-catalog/README.md.
#
# Usage:
#   PLATFORM_CONFIG_REGISTRY=<registry> ./scripts/publish-frozen-charts.sh [chart ...]
#   PLATFORM_CONFIG_REGISTRY=<registry> PLATFORM_CHARTS="loki qovery-shell-agent" ./scripts/publish-frozen-charts.sh
#
# With no argument (or "all"), mirrors every chart listed in catalog.yaml.
# Authentication: helm reuses docker credentials (~/.docker/config.json), so
# any prior `docker login` / `ci_helper docker_login_*` applies.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CATALOG_FILE="$ROOT_DIR/platform-catalog/catalog.yaml"
OUTPUT_FILE="$ROOT_DIR/frozen-charts-publish.json"

fatal() { echo "ERROR: $*" >&2; exit 1; }

command -v helm >/dev/null 2>&1 || fatal "helm CLI is required"
command -v jq >/dev/null 2>&1 || fatal "jq is required"
[ -n "${PLATFORM_CONFIG_REGISTRY:-}" ] || fatal "PLATFORM_CONFIG_REGISTRY is not set (e.g. public.ecr.aws/r3m4q3r9 — see platform-catalog/README.md)"

# Lists "name path" pairs from the `charts:` section of catalog.yaml. The file
# is a flat, fixed-shape list, so plain awk beats a yq dependency the CI image
# may not have.
catalog_charts() {
  awk '
    /^charts:/ { in_section = 1; next }
    in_section && /^[^ #]/ { in_section = 0 }
    in_section && $1 == "-" && $2 == "name:" { name = $3 }
    in_section && $1 == "path:" { print name, $2 }
  ' "$CATALOG_FILE"
}

# ECR does not auto-create repositories on push. They are deliberately NOT
# created here either: registry repositories are declared in the infra
# Terraform, like the rest of the AWS infra.
push_failed() {
  fatal "push failed for $1 — if the error above is 'repository does not exist': ECR repositories are not auto-created, declare it in the infra Terraform"
}

# Resolve the chart list: explicit args > env var > all.
# "none" skips the chart mirror entirely (the CI job also publishes config bundles).
selected="${*:-${PLATFORM_CHARTS:-all}}"
selected="${selected//,/ }"
if [ "$selected" = "none" ]; then
  echo "--- PLATFORM_CHARTS=none, skipping chart mirror"
  echo '[]' > "$OUTPUT_FILE"
  exit 0
fi

package_dir="$(mktemp -d)"
trap 'rm -rf "$package_dir"' EXIT
results=()

while read -r name path; do
  if [ "$selected" != "all" ]; then
    case " $selected " in
      *" $name "*) ;;
      *) continue ;;
    esac
  fi

  chart_dir="$ROOT_DIR/$path"
  [ -f "$chart_dir/Chart.yaml" ] || fatal "chart '$name': no Chart.yaml in $path"

  # Anchored to column 0: dependency entries also carry indented `version:` lines.
  version="$(awk '/^version:/ { print $2; exit }' "$chart_dir/Chart.yaml")"
  [ -n "$version" ] || fatal "chart '$name': no version in $path/Chart.yaml"

  ref="$PLATFORM_CONFIG_REGISTRY/charts/$name:$version"

  # Immutable tags — decision pending (mutable-v0 for now, consumers pin by digest).
  # Uncomment to make re-publishing an existing version fail:
  # if helm show chart "oci://$PLATFORM_CONFIG_REGISTRY/charts/$name" --version "$version" ${PLATFORM_CHARTS_HELM_FLAGS:-} >/dev/null 2>&1; then
  #   fatal "$ref already exists and tags are immutable — bump the chart version"
  # fi

  echo "--- $name: packaging $path ($version)"
  helm package "$chart_dir" -d "$package_dir" >/dev/null

  echo "--- $name: pushing $ref"
  if ! push_output="$(helm push "$package_dir/$name-$version.tgz" "oci://$PLATFORM_CONFIG_REGISTRY/charts" ${PLATFORM_CHARTS_HELM_FLAGS:-} 2>&1)"; then
    echo "$push_output" >&2
    push_failed "$ref"
  fi
  echo "$push_output"
  digest="$(echo "$push_output" | awk '$1 == "Digest:" { print $2 }')"
  [ -n "$digest" ] || fatal "chart '$name': could not extract digest from helm push output"

  results+=("$(jq -n --arg c "$name" --arg v "$version" --arg r "$ref" --arg d "$digest" \
    '{chart: $c, version: $v, ref: $r, digest: $d}')")
done < <(catalog_charts)

[ ${#results[@]} -gt 0 ] || fatal "nothing published — no catalog chart matches '$selected'"

printf '%s\n' "${results[@]}" | jq -s '.' > "$OUTPUT_FILE"
echo "--- wrote $OUTPUT_FILE"
jq '.' "$OUTPUT_FILE"
