# gateway-crds-helm

![Version: v0.0.0-latest](https://img.shields.io/badge/Version-v0.0.0--latest-informational?style=flat-square) ![Type: application](https://img.shields.io/badge/Type-application-informational?style=flat-square) ![AppVersion: latest](https://img.shields.io/badge/AppVersion-latest-informational?style=flat-square)

A Helm chart for Envoy Gateway CRDs

**Homepage:** <https://gateway.envoyproxy.io/>

## Maintainers

| Name | Email | Url |
| ---- | ------ | --- |
| envoy-gateway-steering-committee |  | <https://github.com/envoyproxy/gateway/blob/main/GOVERNANCE.md> |
| envoy-gateway-maintainers |  | <https://github.com/envoyproxy/gateway/blob/main/CODEOWNERS> |

## Source Code

* <https://github.com/envoyproxy/gateway>

## Usage

[Helm](https://helm.sh) must be installed to use the charts.
Please refer to Helm's [documentation](https://helm.sh/docs) to get started.

If you want to manage the CRDs outside of the Envoy Gateway Helm chart, you can use this chart to install the CRDs separately.
If you do, make sure that you don't install the CRDs again when installing the Envoy Gateway Helm chart, by using `--skip-crds` flag.

### Install from DockerHub

Once Helm has been set up correctly, install the chart from dockerhub:

``` shell
helm template eg-crds oci://docker.io/envoyproxy/gateway-crds-helm --set 'crds.gatewayAPI.enabled=true' --set 'crds.envoyGateway.enabled=true' \
    --version v0.0.0-latest | kubectl apply --server-side -f -
```

**Note**: We're using `helm template` piped into `kubectl apply` instead of `helm install` due to a [known Helm limitation](https://github.com/helm/helm/pull/12277)
related to large CRDs in the `templates/` directory.

You can find all helm chart release in [Dockerhub](https://hub.docker.com/r/envoyproxy/gateway-crds-helm/tags)

To uninstall the chart:

``` shell
helm template eg-crds oci://docker.io/envoyproxy/gateway-crds-helm \
    --version v0.0.0-latest | kubectl delete --server-side -f -
```

## Values

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| crds.envoyGateway.enabled | bool | `false` |  |
| crds.gatewayAPI.channel | string | `"experimental"` |  |
| crds.gatewayAPI.enabled | bool | `false` |  |

---

## Qovery Patches

⚠️ **IMPORTANT**: The following files have been **manually patched** by Qovery:

- `templates/standard-gatewayapi-crds.yaml`
- `templates/experimental-gatewayapi-crds.yaml`

### Removed: ValidatingAdmissionPolicy

The upstream Gateway API CRDs include a `ValidatingAdmissionPolicy` named `safe-upgrades.gateway.networking.k8s.io` that blocks installing experimental/RC CRDs on top of stable versions.

**Problem on GKE Autopilot:**
- GKE Autopilot pre-installs Gateway API v1.3.0 (stable channel)
- We need experimental channel for `ListenerSets` support
- The policy would block upgrades to v1.5.0-rc.1 experimental CRDs

**Solution:**
Both ValidatingAdmissionPolicy and its binding have been removed from the CRD templates.

### Updating Gateway API CRDs

When updating to new Gateway API versions from upstream:

1. Download new CRDs from https://github.com/kubernetes-sigs/gateway-api/releases
2. **Before committing**, remove these sections from both `standard` and `experimental` CRD files:
   ```yaml
   ---
   # config/crd/.../gateway.networking.k8s.io_vap_safeupgrades.yaml
   apiVersion: admissionregistration.k8s.io/v1
   kind: ValidatingAdmissionPolicy
   metadata:
     name: "safe-upgrades.gateway.networking.k8s.io"
   ...
   ---
   apiVersion: admissionregistration.k8s.io/v1
   kind: ValidatingAdmissionPolicyBinding
   metadata:
     name: safe-upgrades.gateway.networking.k8s.io
   ...
   ```
3. Replace with a comment block (see existing files for reference)

### ValidatingAdmissionPolicy Deletion

The Rust pre-execute action in `envoy_gateway_crd_chart.rs` automatically:
- Deletes any existing `safe-upgrades.gateway.networking.k8s.io` ValidatingAdmissionPolicy before applying CRDs
- Verifies the policy is fully deleted (retries up to 60 seconds)
- Waits an additional 5 seconds for API server cache propagation

This ensures experimental CRDs can be installed even if a previous installation included the policy.

