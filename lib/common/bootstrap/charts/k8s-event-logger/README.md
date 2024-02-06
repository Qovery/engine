# k8s-event-logger

![Version: 1.1.6](https://img.shields.io/badge/Version-1.1.6-informational?style=flat-square) ![AppVersion: 2.1](https://img.shields.io/badge/AppVersion-2.1-informational?style=flat-square)

This chart runs a pod that simply watches Kubernetes Events and logs them to stdout in JSON to be collected and stored by your logging solution, e.g. [fluentd](https://github.com/helm/charts/tree/master/stable/fluentd) or [fluent-bit](https://github.com/helm/charts/tree/master/stable/fluent-bit).

https://github.com/max-rocket-internet/k8s-event-logger

Events in Kubernetes log very important information. If are trying to understand what happened in the past then these events show clearly what your Kubernetes cluster was thinking and doing. Some examples:

- Pod events like failed probes, crashes, scheduling related information like `TriggeredScaleUp` or `FailedScheduling`
- HorizontalPodAutoscaler events like scaling up and down
- Deployment events like scaling in and out of ReplicaSets
- Ingress events like create and update

The problem is that these events are simply API objects in Kubernetes and are only stored for about 1 hour. Without some way of storing these events, debugging a problem in the past very tricky.

**Homepage:** <https://github.com/max-rocket-internet/k8s-event-logger>

## How to install this chart

Add Delivery Hero public chart repo:

```console
helm repo add deliveryhero https://charts.deliveryhero.io/
```

A simple install with default values:

```console
helm install deliveryhero/k8s-event-logger
```

To install the chart with the release name `my-release`:

```console
helm install my-release deliveryhero/k8s-event-logger
```

To install with some set values:

```console
helm install my-release deliveryhero/k8s-event-logger --set values_key1=value1 --set values_key2=value2
```

To install with custom values file:

```console
helm install my-release deliveryhero/k8s-event-logger -f values.yaml
```

## Source Code

* <https://github.com/max-rocket-internet/k8s-event-logger>

## Values

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| affinity | object | `{}` |  |
| annotations | object | `{}` |  |
| containerName | string | `"k8s-event-logger"` |  |
| env | object | `{}` | A map of environment variables |
| fullnameOverride | string | `""` |  |
| image.pullPolicy | string | `"IfNotPresent"` |  |
| image.repository | string | `"maxrocketinternet/k8s-event-logger"` |  |
| imagePullSecrets | list | `[]` |  |
| nameOverride | string | `""` |  |
| nodeSelector | object | `{}` |  |
| podAnnotations | object | `{}` |  |
| podLabels | object | `{}` |  |
| podSecurityContext.allowPrivilegeEscalation | bool | `false` |  |
| podSecurityContext.capabilities.drop[0] | string | `"ALL"` |  |
| podSecurityContext.readOnlyRootFilesystem | bool | `true` |  |
| podSecurityContext.runAsGroup | int | `10001` |  |
| podSecurityContext.runAsNonRoot | bool | `true` |  |
| podSecurityContext.runAsUser | int | `10001` |  |
| podSecurityContext.seccompProfile.type | string | `"RuntimeDefault"` |  |
| resources.limits.cpu | string | `"100m"` |  |
| resources.limits.memory | string | `"128Mi"` |  |
| resources.requests.cpu | string | `"10m"` |  |
| resources.requests.memory | string | `"128Mi"` |  |
| securityContext | object | `{}` |  |
| tolerations | list | `[]` |  |

## Maintainers

| Name | Email | Url |
| ---- | ------ | --- |
| max-rocket-internet | <max.williams@deliveryhero.com> |  |
