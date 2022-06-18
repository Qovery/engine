<!--- app-name: Metrics Server -->

# Metrics Server packaged by Bitnami

Metrics Server aggregates resource usage data, such as container CPU and memory usage, in a Kubernetes cluster and makes it available via the Metrics API.

[Overview of Metrics Server](https://github.com/kubernetes-incubator/metrics-server)

Trademarks: This software listing is packaged by Bitnami. The respective trademarks mentioned in the offering are owned by the respective companies, and use of them does not imply any affiliation or endorsement.
                           
## TL;DR

```console
$ helm repo add bitnami https://charts.bitnami.com/bitnami
$ helm install my-release bitnami/metrics-server
```

## Introduction

This chart bootstraps a [Metrics Server](https://github.com/bitnami/bitnami-docker-metrics-server) deployment on a [Kubernetes](https://kubernetes.io) cluster using the [Helm](https://helm.sh) package manager.

Bitnami charts can be used with [Kubeapps](https://kubeapps.com/) for deployment and management of Helm Charts in clusters. This Helm chart has been tested on top of [Bitnami Kubernetes Production Runtime](https://kubeprod.io/) (BKPR). Deploy BKPR to get automated TLS certificates, logging and monitoring for your applications.

## Prerequisites

- Kubernetes 1.19+
- Helm 3.2.0+

## Installing the Chart

To install the chart with the release name `my-release`:

```console
$ helm repo add bitnami https://charts.bitnami.com/bitnami
$ helm install my-release bitnami/metrics-server
```

These commands deploy Metrics Server on the Kubernetes cluster in the default configuration. The [Parameters](#parameters) section lists the parameters that can be configured during installation.

> **Tip**: List all releases using `helm list`

## Uninstalling the Chart

To uninstall/delete the `my-release` deployment:

```console
$ helm delete my-release
```

The command removes all the Kubernetes components associated with the chart and deletes the release.

## Parameters

### Global parameters

| Name                      | Description                                     | Value |
| ------------------------- | ----------------------------------------------- | ----- |
| `global.imageRegistry`    | Global Docker image registry                    | `""`  |
| `global.imagePullSecrets` | Global Docker registry secret names as an array | `[]`  |


### Common parameters

| Name                     | Description                                                                                  | Value          |
| ------------------------ | -------------------------------------------------------------------------------------------- | -------------- |
| `kubeVersion`            | Force target Kubernetes version (using Helm capabilities if not set)                         | `""`           |
| `nameOverride`           | String to partially override common.names.fullname template (will maintain the release name) | `""`           |
| `fullnameOverride`       | String to fully override common.names.fullname template                                      | `""`           |
| `namespaceOverride`      | String to fully override common.names.namespace                                              | `""`           |
| `commonLabels`           | Add labels to all the deployed resources                                                     | `{}`           |
| `commonAnnotations`      | Add annotations to all the deployed resources                                                | `{}`           |
| `extraDeploy`            | Array of extra objects to deploy with the release                                            | `[]`           |
| `diagnosticMode.enabled` | Enable diagnostic mode (all probes will be disabled and the command will be overridden)      | `false`        |
| `diagnosticMode.command` | Command to override all containers in the the deployment(s)/statefulset(s)                   | `["sleep"]`    |
| `diagnosticMode.args`    | Args to override all containers in the the deployment(s)/statefulset(s)                      | `["infinity"]` |


### Metrics Server parameters

| Name                                              | Description                                                                                                                                                              | Value                    |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------ |
| `image.registry`                                  | Metrics Server image registry                                                                                                                                            | `docker.io`              |
| `image.repository`                                | Metrics Server image repository                                                                                                                                          | `bitnami/metrics-server` |
| `image.tag`                                       | Metrics Server image tag (immutable tags are recommended)                                                                                                                | `0.6.1-debian-10-r67`    |
| `image.pullPolicy`                                | Metrics Server image pull policy                                                                                                                                         | `IfNotPresent`           |
| `image.pullSecrets`                               | Metrics Server image pull secrets                                                                                                                                        | `[]`                     |
| `hostAliases`                                     | Add deployment host aliases                                                                                                                                              | `[]`                     |
| `replicas`                                        | Number of metrics-server nodes to deploy                                                                                                                                 | `1`                      |
| `updateStrategy.type`                             | Set up update strategy for metrics-server installation.                                                                                                                  | `RollingUpdate`          |
| `rbac.create`                                     | Enable RBAC authentication                                                                                                                                               | `true`                   |
| `serviceAccount.create`                           | Specifies whether a ServiceAccount should be created                                                                                                                     | `true`                   |
| `serviceAccount.name`                             | The name of the ServiceAccount to create                                                                                                                                 | `""`                     |
| `serviceAccount.automountServiceAccountToken`     | Automount API credentials for a service account                                                                                                                          | `true`                   |
| `serviceAccount.annotations`                      | Annotations for service account. Evaluated as a template. Only used if `create` is `true`.                                                                               | `{}`                     |
| `apiService.create`                               | Specifies whether the v1beta1.metrics.k8s.io API service should be created. You can check if it is needed with `kubectl get --raw "/apis/metrics.k8s.io/v1beta1/nodes"`. | `false`                  |
| `apiService.insecureSkipTLSVerify`                | Specifies whether to skip self-verifying self-signed TLS certificates. Set to "false" if you are providing your own certificates.                                        | `true`                   |
| `apiService.caBundle`                             | A base64-encoded string of concatenated certificates for the CA chain for the APIService.                                                                                | `""`                     |
| `containerPorts.https`                            | Port where metrics-server will be running                                                                                                                                | `8443`                   |
| `hostNetwork`                                     | Enable hostNetwork mode                                                                                                                                                  | `false`                  |
| `dnsPolicy`                                       | Default dnsPolicy setting                                                                                                                                                | `ClusterFirst`           |
| `command`                                         | Override default container command (useful when using custom images)                                                                                                     | `[]`                     |
| `args`                                            | Override default container args (useful when using custom images)                                                                                                        | `[]`                     |
| `lifecycleHooks`                                  | for the metrics-server container(s) to automate configuration before or after startup                                                                                    | `{}`                     |
| `extraEnvVars`                                    | Array with extra environment variables to add to metrics-server nodes                                                                                                    | `[]`                     |
| `extraEnvVarsCM`                                  | Name of existing ConfigMap containing extra env vars for metrics-server nodes                                                                                            | `""`                     |
| `extraEnvVarsSecret`                              | Name of existing Secret containing extra env vars for metrics-server nodes                                                                                               | `""`                     |
| `extraArgs`                                       | Extra arguments to pass to metrics-server on start up                                                                                                                    | `[]`                     |
| `sidecars`                                        | Add additional sidecar containers to the metrics-server pod(s)                                                                                                           | `[]`                     |
| `initContainers`                                  | Add additional init containers to the metrics-server pod(s)                                                                                                              | `[]`                     |
| `podLabels`                                       | Pod labels                                                                                                                                                               | `{}`                     |
| `podAnnotations`                                  | Pod annotations                                                                                                                                                          | `{}`                     |
| `priorityClassName`                               | Priority class for pod scheduling                                                                                                                                        | `""`                     |
| `schedulerName`                                   | Name of the k8s scheduler (other than default)                                                                                                                           | `""`                     |
| `terminationGracePeriodSeconds`                   | In seconds, time the given to the metrics-server pod needs to terminate gracefully                                                                                       | `""`                     |
| `podAffinityPreset`                               | Pod affinity preset. Ignored if `affinity` is set. Allowed values: `soft` or `hard`                                                                                      | `""`                     |
| `podAntiAffinityPreset`                           | Pod anti-affinity preset. Ignored if `affinity` is set. Allowed values: `soft` or `hard`                                                                                 | `soft`                   |
| `pdb.create`                                      | Create a PodDisruptionBudget                                                                                                                                             | `false`                  |
| `pdb.minAvailable`                                | Minimum available instances                                                                                                                                              | `""`                     |
| `pdb.maxUnavailable`                              | Maximum unavailable instances                                                                                                                                            | `""`                     |
| `nodeAffinityPreset.type`                         | Node affinity preset type. Ignored if `affinity` is set. Allowed values: `soft` or `hard`                                                                                | `""`                     |
| `nodeAffinityPreset.key`                          | Node label key to match. Ignored if `affinity` is set.                                                                                                                   | `""`                     |
| `nodeAffinityPreset.values`                       | Node label values to match. Ignored if `affinity` is set.                                                                                                                | `[]`                     |
| `affinity`                                        | Affinity for pod assignment                                                                                                                                              | `{}`                     |
| `topologySpreadConstraints`                       | Topology spread constraints for pod                                                                                                                                      | `[]`                     |
| `nodeSelector`                                    | Node labels for pod assignment                                                                                                                                           | `{}`                     |
| `tolerations`                                     | Tolerations for pod assignment                                                                                                                                           | `[]`                     |
| `service.type`                                    | Kubernetes Service type                                                                                                                                                  | `ClusterIP`              |
| `service.ports.https`                             | Kubernetes Service port                                                                                                                                                  | `443`                    |
| `service.nodePorts.https`                         | Kubernetes Service port                                                                                                                                                  | `""`                     |
| `service.clusterIP`                               | metrics-server service Cluster IP                                                                                                                                        | `""`                     |
| `service.loadBalancerIP`                          | LoadBalancer IP if Service type is `LoadBalancer`                                                                                                                        | `""`                     |
| `service.loadBalancerSourceRanges`                | metrics-server service Load Balancer sources                                                                                                                             | `[]`                     |
| `service.externalTrafficPolicy`                   | metrics-server service external traffic policy                                                                                                                           | `Cluster`                |
| `service.extraPorts`                              | Extra ports to expose (normally used with the `sidecar` value)                                                                                                           | `[]`                     |
| `service.annotations`                             | Annotations for the Service                                                                                                                                              | `{}`                     |
| `service.labels`                                  | Labels for the Service                                                                                                                                                   | `{}`                     |
| `service.sessionAffinity`                         | Session Affinity for Kubernetes service, can be "None" or "ClientIP"                                                                                                     | `None`                   |
| `service.sessionAffinityConfig`                   | Additional settings for the sessionAffinity                                                                                                                              | `{}`                     |
| `resources.limits`                                | The resources limits for the container                                                                                                                                   | `{}`                     |
| `resources.requests`                              | The requested resources for the container                                                                                                                                | `{}`                     |
| `startupProbe.enabled`                            | Enable startupProbe                                                                                                                                                      | `false`                  |
| `startupProbe.initialDelaySeconds`                | Initial delay seconds for startupProbe                                                                                                                                   | `0`                      |
| `startupProbe.periodSeconds`                      | Period seconds for startupProbe                                                                                                                                          | `10`                     |
| `startupProbe.timeoutSeconds`                     | Timeout seconds for startupProbe                                                                                                                                         | `1`                      |
| `startupProbe.failureThreshold`                   | Failure threshold for startupProbe                                                                                                                                       | `3`                      |
| `startupProbe.successThreshold`                   | Success threshold for startupProbe                                                                                                                                       | `1`                      |
| `livenessProbe.enabled`                           | Enable livenessProbe                                                                                                                                                     | `true`                   |
| `livenessProbe.initialDelaySeconds`               | Initial delay seconds for livenessProbe                                                                                                                                  | `0`                      |
| `livenessProbe.periodSeconds`                     | Period seconds for livenessProbe                                                                                                                                         | `10`                     |
| `livenessProbe.timeoutSeconds`                    | Timeout seconds for livenessProbe                                                                                                                                        | `1`                      |
| `livenessProbe.failureThreshold`                  | Failure threshold for livenessProbe                                                                                                                                      | `3`                      |
| `livenessProbe.successThreshold`                  | Success threshold for livenessProbe                                                                                                                                      | `1`                      |
| `readinessProbe.enabled`                          | Enable readinessProbe                                                                                                                                                    | `true`                   |
| `readinessProbe.initialDelaySeconds`              | Initial delay seconds for readinessProbe                                                                                                                                 | `0`                      |
| `readinessProbe.periodSeconds`                    | Period seconds for readinessProbe                                                                                                                                        | `10`                     |
| `readinessProbe.timeoutSeconds`                   | Timeout seconds for readinessProbe                                                                                                                                       | `1`                      |
| `readinessProbe.failureThreshold`                 | Failure threshold for readinessProbe                                                                                                                                     | `3`                      |
| `readinessProbe.successThreshold`                 | Success threshold for readinessProbe                                                                                                                                     | `1`                      |
| `customStartupProbe`                              | Custom liveness probe for the Web component                                                                                                                              | `{}`                     |
| `customLivenessProbe`                             | Custom Liveness probes for metrics-server                                                                                                                                | `{}`                     |
| `customReadinessProbe`                            | Custom Readiness probes metrics-server                                                                                                                                   | `{}`                     |
| `containerSecurityContext.enabled`                | Enable Container security context                                                                                                                                        | `true`                   |
| `containerSecurityContext.readOnlyRootFilesystem` | ReadOnlyRootFilesystem for the container                                                                                                                                 | `false`                  |
| `containerSecurityContext.runAsNonRoot`           | Run containers as non-root users                                                                                                                                         | `true`                   |
| `containerSecurityContext.runAsUser`              | Set containers' Security Context runAsUser                                                                                                                               | `1001`                   |
| `podSecurityContext.enabled`                      | Pod security context                                                                                                                                                     | `false`                  |
| `podSecurityContext.fsGroup`                      | Set %%MAIN_CONTAINER_NAME%% pod's Security Context fsGroup                                                                                                               | `1001`                   |
| `extraVolumes`                                    | Extra volumes                                                                                                                                                            | `[]`                     |
| `extraVolumeMounts`                               | Mount extra volume(s)                                                                                                                                                    | `[]`                     |


Specify each parameter using the `--set key=value[,key=value]` argument to `helm install`. For example,

```console
$ helm install my-release \
  --set rbac.create=true bitnami/metrics-server
```

The above command enables RBAC authentication.

Alternatively, a YAML file that specifies the values for the parameters can be provided while installing the chart. For example,

```console
$ helm install my-release -f values.yaml bitnami/metrics-server
```

> **Tip**: You can use the default [values.yaml](values.yaml)

## Configuration and installation details

### [Rolling vs Immutable tags](https://docs.bitnami.com/containers/how-to/understand-rolling-tags-containers/)

It is strongly recommended to use immutable tags in a production environment. This ensures your deployment does not change automatically if the same tag is updated with a different image.

Bitnami will release a new chart updating its containers if a new version of the main container, significant changes, or critical vulnerabilities exist.

### Enable RBAC security

In order to enable Role-Based Access Control (RBAC) for Metrics Server, use the following parameter: `rbac.create=true`.

### Configure certificates

If you are providing your own certificates for the API Service, set `insecureSkipTLSVerify` to `"false"`, and provide a `caBundle` consisting of the base64-encoded certificate chain.

### Set Pod affinity

This chart allows you to set custom Pod affinity using the `affinity` parameter. Find more information about Pod affinity in the [Kubernetes documentation](https://kubernetes.io/docs/concepts/configuration/assign-pod-node/#affinity-and-anti-affinity).

As an alternative, you can use one of the preset configurations for pod affinity, pod anti-affinity, and node affinity available at the [bitnami/common](https://github.com/bitnami/charts/tree/master/bitnami/common#affinities) chart. To do so, set the `podAffinityPreset`, `podAntiAffinityPreset`, or `nodeAffinityPreset` parameters.

## Troubleshooting

Find more information about how to deal with common errors related to Bitnami's Helm charts in [this troubleshooting guide](https://docs.bitnami.com/general/how-to/troubleshoot-helm-chart-issues).

## Upgrading

### To 6.0.0

This major release renames several values in this chart and adds missing features, in order to be aligned with the rest of the assets in the Bitnami charts repository.

Affected values:

- `service.port` was deprecated. We recommend using `service.ports.http` instead.
- `service.nodePort` was deprecated. We recommend using `service.nodePorts.https` instead.
- `extraArgs` is now interpreted as an array.

### To 5.2.0

This version introduces `bitnami/common`, a [library chart](https://helm.sh/docs/topics/library_charts/#helm) as a dependency. More documentation about this new utility could be found [here](https://github.com/bitnami/charts/tree/master/bitnami/common#bitnami-common-library-chart). Please, make sure that you have updated the chart dependencies before executing any upgrade.

### To 5.0.0

[On November 13, 2020, Helm v2 support formally ended](https://github.com/helm/charts#status-of-the-project). This major version is the result of the required changes applied to the Helm Chart to be able to incorporate the different features added in Helm v3 and to be consistent with the Helm project itself regarding the Helm v2 EOL.

[Learn more about this change and related upgrade considerations](https://docs.bitnami.com/kubernetes/infrastructure/metrics-server/administration/upgrade-helm3/).

### To 4.0.0

Backwards compatibility is not guaranteed unless you modify the labels used on the chart's deployments.
Use the workaround below to upgrade from versions previous to 4.0.0. The following example assumes that the release name is metrics-server:

```console
$ kubectl delete deployment metrics-server --cascade=false
$ helm upgrade metrics-server bitnami/metrics-server
```

### To 2.0.0

Backwards compatibility is not guaranteed unless you modify the labels used on the chart's deployments.
Use the workaround below to upgrade from versions previous to 2.0.0. The following example assumes that the release name is metrics-server:

```console
$ kubectl patch deployment metrics-server --type=json -p='[{"op": "remove", "path": "/spec/selector/matchLabels/chart"}]'
```

## License

Copyright &copy; 2022 Bitnami

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.