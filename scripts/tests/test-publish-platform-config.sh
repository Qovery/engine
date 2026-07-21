#!/usr/bin/env bash
# Verifies that executable component publication stages and injects the canonical Pkl contract.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_dir="$(mktemp -d "${TMPDIR:-/tmp}/platform-config-publish-test.XXXXXX")"
trap 'rm -rf "$test_dir"' EXIT

mock_bin="$test_dir/bin"
marker="$test_dir/oras-push-inspected"
output="$test_dir/platform-config-publish.json"
mkdir -p "$mock_bin"

cat > "$mock_bin/oras" <<'MOCK_ORAS'
#!/usr/bin/env bash
set -euo pipefail

case "$1" in
  push)
    [[ "$PWD" != "$EXPECTED_COMPONENT_DIR" ]] || {
      echo "ERROR: executable component was pushed without an isolated staging directory" >&2
      exit 1
    }
    [[ -f config/runtime-values/model.pkl ]]
    cmp "$EXPECTED_CONTRACT" config/runtime-values/contract.pkl
    touch "$MOCK_MARKER"
    ;;
  manifest)
    printf '{"digest":"sha256:%064d"}\n' 0
    ;;
  *)
    echo "ERROR: unexpected oras command: $*" >&2
    exit 1
    ;;
esac
MOCK_ORAS
chmod +x "$mock_bin/oras"

PATH="$mock_bin:$PATH" \
EXPECTED_COMPONENT_DIR="$ROOT_DIR/platform-catalog/components/cluster-agent" \
EXPECTED_CONTRACT="$ROOT_DIR/platform-catalog/pkl/component-contract.pkl" \
MOCK_MARKER="$marker" \
PLATFORM_CONFIG_REGISTRY="registry.invalid/qovery" \
PLATFORM_CONFIG_OUTPUT_FILE="$output" \
  "$ROOT_DIR/scripts/publish-platform-config.sh" cluster-agent

[[ -f "$marker" ]] || {
  echo "ERROR: mocked ORAS push was not called" >&2
  exit 1
}

jq -e '
  .[0] as $publication |
  length == 1 and
  $publication.component == "cluster-agent" and
  ($publication.version | test("^v[0-9]+$")) and
  $publication.ref == ("registry.invalid/qovery/platform-config/cluster-agent:" + $publication.version) and
  $publication.digest == ("sha256:" + ("0" * 64))
' "$output" >/dev/null

echo "Platform config publication staging test passed"
