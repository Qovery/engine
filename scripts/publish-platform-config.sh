#!/usr/bin/env bash
# Publish platform component config bundles as OCI artifacts (Engine v2).
#
# For each selected component, pushes platform-catalog/components/<name>/config/
# with ORAS as a single tar+gzip layer to:
#   $PLATFORM_CONFIG_REGISTRY/platform-config/<name>:<version>
# where <version> comes from platform-catalog/catalog.yaml.
#
# Writes platform-config-publish.json (component, version, ref, digest) — the
# digest is the pin q-core records. See platform-catalog/README.md.
#
# Usage:
#   PLATFORM_CONFIG_REGISTRY=<registry> ./scripts/publish-platform-config.sh [component ...]
#   PLATFORM_CONFIG_REGISTRY=<registry> PLATFORM_CONFIG_COMPONENTS="loki cluster-agent" ./scripts/publish-platform-config.sh
#
# With no argument (or "all"), publishes every component whose config/ directory
# is non-empty. Authentication: oras reuses docker credentials (~/.docker/config.json),
# so any prior `docker login` / `ci_helper docker_login_*` applies.
#
# PLATFORM_CONFIG_ORAS_FLAGS: extra flags passed to every oras call
# (e.g. --plain-http to test against a local registry).

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CATALOG_FILE="$ROOT_DIR/platform-catalog/catalog.yaml"
COMPONENTS_DIR="$ROOT_DIR/platform-catalog/components"
OUTPUT_FILE="$ROOT_DIR/platform-config-publish.json"
ARTIFACT_TYPE="application/vnd.qovery.platform-config.v1"
LAYER_MEDIA_TYPE="application/vnd.qovery.platform-config.layer.v1.tar+gzip"

fatal() { echo "ERROR: $*" >&2; exit 1; }

command -v oras >/dev/null 2>&1 || fatal "oras CLI is required (https://oras.land)"
command -v jq >/dev/null 2>&1 || fatal "jq is required"
[ -n "${PLATFORM_CONFIG_REGISTRY:-}" ] || fatal "PLATFORM_CONFIG_REGISTRY is not set (e.g. public.ecr.aws/r3m4q3r9 — see platform-catalog/README.md)"

# ECR does not auto-create repositories on push. They are deliberately NOT
# created here either: registry repositories are declared in the infra
# Terraform, like the rest of the AWS infra.
push_failed() {
  fatal "push failed for $1 — if the error above is 'repository does not exist': ECR repositories are not auto-created, declare it in the infra Terraform"
}

# Extracts the `version` of a component from the `components:` section of
# catalog.yaml (scoped: a name may also appear in the `charts:` section). The
# file is a flat, fixed-shape list, so plain awk beats a yq dependency the CI
# image may not have.
catalog_version() {
  awk -v name="$1" '
    /^components:/ { in_components = 1; next }
    in_components && /^[^ #]/ { in_components = 0 }
    in_components && $1 == "-" && $2 == "name:" { current = $3 }
    in_components && $1 == "version:" && current == name { print $2; exit }
  ' "$CATALOG_FILE"
}

# Resolve the component list: explicit args > env var > all.
# "none" skips config bundles entirely (the CI job also runs the chart mirror).
components="${*:-${PLATFORM_CONFIG_COMPONENTS:-all}}"
components="${components//,/ }"
if [ "$components" = "none" ]; then
  echo "--- PLATFORM_CONFIG_COMPONENTS=none, skipping config bundle publication"
  echo '[]' > "$OUTPUT_FILE"
  exit 0
fi
if [ "$components" = "all" ]; then
  components=$(basename -a "$COMPONENTS_DIR"/*/)
fi

revision="${CI_COMMIT_SHA:-$(git -C "$ROOT_DIR" rev-parse HEAD)}"
# Pin the `created` annotation to the commit date: oras would otherwise stamp
# the push time, giving a different manifest digest for identical content.
created="$(git -C "$ROOT_DIR" show -s --format=%cI "$revision" 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)"
# Intentionally word-split ($oras_flags used unquoted): flags never contain spaces,
# and empty arrays break under `set -u` on macOS bash 3.2.
oras_flags="${PLATFORM_CONFIG_ORAS_FLAGS:-}"
results=()

for component in $components; do
  config_dir="$COMPONENTS_DIR/$component/config"
  [ -d "$config_dir" ] || fatal "unknown component '$component' ($config_dir not found)"

  if [ -z "$(find "$config_dir" -type f -not -name '.gitkeep' | head -1)" ]; then
    echo "--- $component: config/ is empty (q-core manifest wiring only), skipping"
    continue
  fi

  version="$(catalog_version "$component")"
  [ -n "$version" ] || fatal "component '$component' has no version in $CATALOG_FILE"

  ref="$PLATFORM_CONFIG_REGISTRY/platform-config/$component:$version"

  # Immutable tags — decision pending (mutable-v0 for now, q-core pins by digest).
  # Uncomment to make re-publishing an existing version fail:
  # if oras manifest fetch $oras_flags --descriptor "$ref" >/dev/null 2>&1; then
  #   fatal "$ref already exists and tags are immutable — bump the version in catalog.yaml"
  # fi

  echo "--- $component: pushing $ref"
  # Pushing the directory lets `oras pull` restore its exact content (oras tars
  # it and marks the layer for unpack). --disable-path-validation because we
  # push from an absolute path; the stored path stays relative ("config").
  (cd "$COMPONENTS_DIR/$component" && oras push $oras_flags "$ref" \
    --artifact-type "$ARTIFACT_TYPE" \
    --annotation "org.opencontainers.image.created=$created" \
    --annotation "org.opencontainers.image.revision=$revision" \
    --annotation "org.opencontainers.image.source=https://gitlab.com/qovery/backend/engine" \
    "config:$LAYER_MEDIA_TYPE") || push_failed "$ref"

  digest="$(oras manifest fetch $oras_flags --descriptor "$ref" | jq -r '.digest')"
  echo "--- $component: published $ref@$digest"
  results+=("$(jq -n --arg c "$component" --arg v "$version" --arg r "$ref" --arg d "$digest" \
    '{component: $c, version: $v, ref: $r, digest: $d}')")
done

printf '%s\n' "${results[@]:-}" | jq -s '.' > "$OUTPUT_FILE"
echo "--- wrote $OUTPUT_FILE"
jq '.' "$OUTPUT_FILE"
