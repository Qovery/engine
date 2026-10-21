#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

command -v helm >/dev/null || {
  echo "helm must be installed and available on PATH" >&2
  exit 1
}

cargo build --manifest-path app/Cargo.toml --bin engine_post_renderer

export HELM_PLUGINS="$repository_root/target/helm-plugins"
plugin_dir="$HELM_PLUGINS/qovery-engine-post-renderer"
mkdir -p "$plugin_dir"
cp docker/helm-plugins/engine-post-renderer/plugin.yaml "$plugin_dir/plugin.yaml"
ln -sfn "$repository_root/target/debug/engine_post_renderer" "$plugin_dir/engine_post_renderer"

helm plugin list | awk '$1 == "qovery-engine-post-renderer" && $3 == "postrenderer/v1" { found = 1 } END { exit !found }' || {
  echo "failed to register the Helm post-renderer" >&2
  exit 1
}

exec cargo run --package app --bin engine "$@"
