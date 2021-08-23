# Metrics Server

[Metrics Server](https://github.com/kubernetes-incubator/metrics-server) is a cluster-wide aggregator of resource usage data. Metrics Server collects metrics from the Summary API, exposed by Kubelet on each node.

## TL;DR

```console
$ helm repo add bitnami https://charts.bitnami.com/bitnami
$ helm install my-release bitnami/metrics-server
```

## Introduction

This chart bootstraps a [Metrics Server](https://github.com/bitnami/bitnami-docker-metrics-server) deployment on a [Kubernetes](http://kubernetes.io) cluster using the [Helm](https://helm.sh) package manager.

Bitnami charts can be used with [Kubeapps](https://kubeapps.com/) for deployment and management of Helm Charts in clusters. This Helm chart has been tested on top of [Bitnami Kubernetes Production Runtime](https://kubeprod.io/) (BKPR). Deploy BKPR to get automated TLS certificates, logging and monitoring for your applications.

## Prerequisites

- Kubernetes 1.12+
- Helm 3.1.0

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

| Name               | Description                                                                                  | Value |
| ------------------ | -------------------------------------------------------------------------------------------- | ----- |
| `nameOverride`     | String to partially override common.names.fullname template (will maintain the release name) | `""`  |
| `fullnameOverride` | String to fully override common.names.fullname template                                      | `""`  |


### Metrics Server parameters

| Name                                              | Description                                                                                                                                                              | Value                    |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------ |
| `image.registry`                                  | Metrics Server image registry                                                                                                                                            | `docker.io`              |
| `image.repository`                                | Metrics Server image repository                                                                                                                                          | `bitnami/metrics-server` |
| `image.tag`                                       | Metrics Server image tag (immutable tags are recommended)                                                                                                                | `0.5.0-debian-10-r32`    |
| `image.pullPolicy`                                | Metrics Server image pull policy                                                                                                                                         | `IfNotPresent`           |
| `image.pullSecrets`                               | Metrics Server image pull secrets                                                                                                                                        | `[]`                     |
| `hostAliases`                                     | Add deployment host aliases                                                                                                                                              | `[]`                     |
| `replicas`                                        | Number of metrics-server nodes to deploy                                                                                                                                 | `1`                      |
| `updateStrategy.type`                             | Set up update strategy for metrics-server installation.                                                                                                                  | `RollingUpdate`          |
| `rbac.create`                                     | Enable RBAC authentication                                                                                                                                               | `true`                   |
| `serviceAccount.create`                           | Specifies whether a ServiceAccount should be created                                                                                                                     | `true`                   |
| `serviceAccount.name`                             | The name of the ServiceAccount to create                                                                                                                                 | `""`                     |
| `serviceAccount.automountServiceAccountToken`     | Automount API credentials for a service account                                                                                                                          | `true`                   |
| `apiService.create`                               | Specifies whether the v1beta1.metrics.k8s.io API service should be created. You can check if it is needed with `kubectl get --raw "/apis/metrics.k8s.io/v1beta1/nodes"`. | `false`                  |
| `securePort`                                      | Port where metrics-server will be running                                                                                                                                | `8443`                   |
| `hostNetwork`                                     | Enable hostNetwork mode                                                                                                                                                  | `false`                  |
| `command`                                         | Override default container command (useful when using custom images)                                                                                                     | `[]`                     |
| `extraArgs`                                       | Extra arguments to pass to metrics-server on start up                                                                                                                    | `{}`                     |
| `podLabels`                                       | Pod labels                                                                                                                                                               | `{}`                     |
| `podAnnotations`                                  | Pod annotations                                                                                                                                                          | `{}`                     |
| `priorityClassName`                               | Priority class for pod scheduling                                                                                                                                        | `""`                     |
| `podAffinityPreset`                               | Pod affinity preset. Ignored if `affinity` is set. Allowed values: `soft` or `hard`                                                                                      | `""`                     |
| `podAntiAffinityPreset`                           | Pod anti-affinity preset. Ignored if `affinity` is set. Allowed values: `soft` or `hard`                                                                                 | `soft`                   |
| `podDisruptionBudget.enabled`                     | Create a PodDisruptionBudget                                                                                                                                             | `false`                  |
| `podDisruptionBudget.minAvailable`                | Minimum available instances                                                                                                                                              | `""`                     |
| `podDisruptionBudget.maxUnavailable`              | Maximum unavailable instances                                                                                                                                            | `""`                     |
| `nodeAffinityPreset.type`                         | Node affinity preset type. Ignored if `affinity` is set. Allowed values: `soft` or `hard`                                                                                | `""`                     |
| `nodeAffinityPreset.key`                          | Node label key to match. Ignored if `affinity` is set.                                                                                                                   | `""`                     |
| `nodeAffinityPreset.values`                       | Node label values to match. Ignored if `affinity` is set.                                                                                                                | `[]`                     |
| `affinity`                                        | Affinity for pod assignment                                                                                                                                              | `{}`                     |
| `topologySpreadConstraints`                       | Topology spread constraints for pod                                                                                                                                      | `[]`                     |
| `nodeSelector`                                    | Node labels for pod assignment                                                                                                                                           | `{}`                     |
| `tolerations`                                     | Tolerations for pod assignment                                                                                                                                           | `[]`                     |
| `service.type`                                    | Kubernetes Service type                                                                                                                                                  | `ClusterIP`              |
| `service.port`                                    | Kubernetes Service port                                                                                                                                                  | `443`                    |
| `service.nodePort`                                | Kubernetes Service port                                                                                                                                                  | `""`                     |
| `service.loadBalancerIP`                          | LoadBalancer IP if Service type is `LoadBalancer`                                                                                                                        | `""`                     |
| `service.annotations`                             | Annotations for the Service                                                                                                                                              | `{}`                     |
| `service.labels`                                  | Labels for the Service                                                                                                                                                   | `{}`                     |
| `resources.limits`                                | The resources limits for the container                                                                                                                                   | `{}`                     |
| `resources.requests`                              | The requested resources for the container                                                                                                                                | `{}`                     |
| `livenessProbe.enabled`                           | Enable livenessProbe                                                                                                                                                     | `true`                   |
| `livenessProbe.httpGet.path`                      | Request path for livenessProbe                                                                                                                                           | `/livez`                 |
| `livenessProbe.httpGet.port`                      | Port for livenessProbe                                                                                                                                                   | `https`                  |
| `livenessProbe.httpGet.scheme`                    | Scheme for livenessProbe                                                                                                                                                 | `HTTPS`                  |
| `livenessProbe.periodSeconds`                     | Period seconds for livenessProbe                                                                                                                                         | `10`                     |
| `livenessProbe.failureThreshold`                  | Failure threshold for livenessProbe                                                                                                                                      | `3`                      |
| `readinessProbe.enabled`                          | Enable readinessProbe                                                                                                                                                    | `true`                   |
| `readinessProbe.httpGet.path`                     | Request path for readinessProbe                                                                                                                                          | `/readyz`                |
| `readinessProbe.httpGet.port`                     | Port for readinessProbe                                                                                                                                                  | `https`                  |
| `readinessProbe.httpGet.scheme`                   | Scheme for livenessProbe                                                                                                                                                 | `HTTPS`                  |
| `readinessProbe.periodSeconds`                    | Period seconds for readinessProbe                                                                                                                                        | `10`                     |
| `readinessProbe.failureThreshold`                 | Failure threshold for readinessProbe                                                                                                                                     | `3`                      |
| `customLivenessProbe`                             | Custom Liveness probes for metrics-server                                                                                                                                | `{}`                     |
| `customReadinessProbe`                            | Custom Readiness probes metrics-server                                                                                                                                   | `{}`                     |
| `containerSecurityContext.enabled`                | Enable Container security context                                                                                                                                        | `true`                   |
| `containerSecurityContext.readOnlyRootFilesystem` | ReadOnlyRootFilesystem for the container                                                                                                                                 | `false`                  |
| `containerSecurityContext.runAsNonRoot`           | Run containers as non-root users                                                                                                                                         | `true`                   |
| `podSecurityContext.enabled`                      | Pod security context                                                                                                                                                     | `false`                  |
| `extraVolumes`                                    | Extra volumes                                                                                                                                                            | `[]`                     |
| `extraVolumeMounts`                               | Mount extra volume(s)                                                                                                                                                    | `[]`                     |
| `extraContainers`                                 | Extra containers to run within the pod                                                                                                                                   | `{}`                     |


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

### [Rolling VS Immutable tags](https://docs.bitnami.com/containers/how-to/understand-rolling-tags-containers/)

It is strongly recommended to use immutable tags in a production environment. This ensures your deployment does not change automatically if the same tag is updated with a different image.

Bitnami will release a new chart updating its containers if a new version of the main container, significant changes, or critical vulnerabilities exist.

### Enable security for Metrics Server. Configuring RBAC

In order to enable Role-based access control for Metrics Servier you can use the following parameter: `rbac.create=true`

### Setting Pod's affinity

This chart allows you to set your custom affinity using the `affinity` parameter. Find more information about Pod's affinity in the [kubernetes documentation](https://kubernetes.io/docs/concepts/configuration/assign-pod-node/#affinity-and-anti-affinity).

As an alternative, you can use of the preset configurations for pod affinity, pod anti-affinity, and node affinity available at the [bitnami/common](https://github.com/bitnami/charts/tree/master/bitnami/common#affinities) chart. To do so, set the `podAffinityPreset`, `podAntiAffinityPreset`, or `nodeAffinityPreset` parameters.

## Troubleshooting

Find more information about how to deal with common errors related to Bitnami’s Helm charts in [this troubleshooting guide](https://docs.bitnami.com/general/how-to/troubleshoot-helm-chart-issues).

## Upgrading

### To 5.2.0

This version introduces `bitnami/common`, a [library chart](https://helm.sh/docs/topics/library_charts/#helm) as a dependency. More documentation about this new utility could be found [here](https://github.com/bitnami/charts/tree/master/bitnami/common#bitnami-common-library-chart). Please, make sure that you have updated the chart dependencies before executing any upgrade.

### To 5.0.0

[On November 13, 2020, Helm v2 support was formally finished](https://github.com/helm/charts#status-of-the-project), this major version is the result of the required changes applied to the Helm Chart to be able to incorporate the different features added in Helm v3 and to be consistent with the Helm project itself regarding the Helm v2 EOL.

**What changes were introduced in this major version?**

- Previous versions of this Helm Chart use `apiVersion: v1` (installable by both Helm 2 and 3), this Helm Chart was updated to `apiVersion: v2` (installable by Helm 3 only). [Here](https://helm.sh/docs/topics/charts/#the-apiversion-field) you can find more information about the `apiVersion` field.
- The different fields present in the *Chart.yaml* file has been ordered alphabetically in a homogeneous way for all the Bitnami Helm Charts

**Considerations when upgrading to this version**

- If you want to upgrade to this version from a previous one installed with Helm v3, you shouldn't face any issues
- If you want to upgrade to this version using Helm v2, this scenario is not supported as this version doesn't support Helm v2 anymore
- If you installed the previous version with Helm v2 and wants to upgrade to this version with Helm v3, please refer to the [official Helm documentation](https://helm.sh/docs/topics/v2_v3_migration/#migration-use-cases) about migrating from Helm v2 to v3

**Useful links**

- https://docs.bitnami.com/tutorials/resolve-helm2-helm3-post-migration-issues/
- https://helm.sh/docs/topics/v2_v3_migration/
- https://helm.sh/blog/migrate-from-helm-v2-to-helm-v3/

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
