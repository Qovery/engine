# beyla

![Version: 1.8.0](https://img.shields.io/badge/Version-1.8.0-informational?style=flat-square) ![Type: application](https://img.shields.io/badge/Type-application-informational?style=flat-square) ![AppVersion: 2.2.3](https://img.shields.io/badge/AppVersion-2.2.3-informational?style=flat-square)

eBPF-based autoinstrumentation HTTP, HTTP2 and gRPC services, as well as network metrics.

**Homepage:** <https://grafana.com/oss/beyla-ebpf/>

## Maintainers

| Name | Email | Url |
| ---- | ------ | --- |
| mariomac |  | <https://github.com/mariomac> |
| grcevski |  | <https://github.com/grcevski> |
| marctc |  | <https://github.com/marctc> |
| rafaelroquetto |  | <https://github.com/rafaelroquetto> |

## Source Code

* <https://github.com/grafana/beyla>

## Values

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| affinity | object | `{}` | used for scheduling of pods based on affinity rules |
| config.create | bool | `true` | set to true, to use the below default configurations |
| config.data | object | `{"attributes":{"kubernetes":{"enable":true}},"filter":{"network":{"k8s_dst_owner_name":{"not_match":"{kube*,*jaeger-agent*,*prometheus*,*promtail*,*grafana-agent*}"},"k8s_src_owner_name":{"not_match":"{kube*,*jaeger-agent*,*prometheus*,*promtail*,*grafana-agent*}"}}},"prometheus_export":{"path":"/metrics","port":9090}}` | default value of beyla configuration |
| config.name | string | `""` |  |
| config.skipConfigMapCheck | bool | `false` | set to true, to skip the check around the ConfigMap creation |
| contextPropagation | object | `{"enabled":true}` | Enables context propagation support. |
| dnsPolicy | string | `"ClusterFirstWithHostNet"` | Determines how DNS resolution is handled for that pod. If `.Values.preset` is set to `network` or `.Values.config.data.network` is enabled, Beyla requires `hostNetwork` access, causing cluster service DNS resolution to fail. It is recommended not to change this if Beyla sends traces and metrics to Grafana components via k8s service. |
| env | object | `{}` | extra environment variables |
| envValueFrom | object | `{}` | extra environment variables to be set from resources such as k8s configMaps/secrets |
| extraCapabilities | list | `[]` | Extra capabilities for unprivileged / less privileged setup. |
| extraObjects | list | `[]` | Extra k8s manifests to deploy |
| fullnameOverride | string | `""` | Overrides the chart's computed fullname. |
| global.image.pullSecrets | list | `[]` | Optional set of global image pull secrets. |
| global.image.registry | string | `""` | Global image registry to use if it needs to be overridden for some specific use cases (e.g local registries, custom images, ...) |
| image.digest | string | `nil` | Beyla image's SHA256 digest (either in format "sha256:XYZ" or "XYZ"). When set, will override `image.tag`. |
| image.pullPolicy | string | `"IfNotPresent"` | Beyla image pull policy. |
| image.pullSecrets | list | `[]` | Optional set of image pull secrets. |
| image.registry | string | `"docker.io"` | Beyla image registry (defaults to docker.io) |
| image.repository | string | `"grafana/beyla"` | Beyla image repository. |
| image.tag | string | `nil` | Beyla image tag. When empty, the Chart's appVersion is used. |
| k8sCache | object | `{"annotations":{},"env":{},"envValueFrom":{},"image":{"digest":null,"pullPolicy":"IfNotPresent","pullSecrets":[],"registry":"docker.io","repository":"grafana/beyla-k8s-cache","tag":null},"internalMetrics":{"path":"/metrics","port":0,"portName":"metrics"},"podAnnotations":{},"podLabels":{},"profilePort":0,"replicas":0,"resources":{},"service":{"annotations":{},"labels":{},"name":"beyla-k8s-cache","port":50055}}` | Options to deploy the Kubernetes metadata cache as a separate service |
| k8sCache.annotations | object | `{}` | Deployment annotations. |
| k8sCache.env | object | `{}` | extra environment variables |
| k8sCache.envValueFrom | object | `{}` | extra environment variables to be set from resources such as k8s configMaps/secrets |
| k8sCache.image.digest | string | `nil` | K8s Cache image's SHA256 digest (either in format "sha256:XYZ" or "XYZ"). When set, will override `image.tag`. |
| k8sCache.image.pullPolicy | string | `"IfNotPresent"` | K8s Cache image pull policy. |
| k8sCache.image.pullSecrets | list | `[]` | Optional set of image pull secrets. |
| k8sCache.image.registry | string | `"docker.io"` | K8s Cache image registry (defaults to docker.io) |
| k8sCache.image.repository | string | `"grafana/beyla-k8s-cache"` | K8s Cache image repository. |
| k8sCache.image.tag | string | `nil` | K8s Cache image tag. When empty, the Chart's appVersion is used. |
| k8sCache.podAnnotations | object | `{}` | Adds custom annotations to the Beyla Kube Cache Pods. |
| k8sCache.podLabels | object | `{}` | Adds custom labels to the Beyla Kube Cache Pods. |
| k8sCache.profilePort | int | `0` | Enables the profile port for the Beyla cache |
| k8sCache.replicas | int | `0` | Number of replicas for the Kubernetes metadata cache service. 0 disables the service. |
| k8sCache.service.annotations | object | `{}` | Service annotations. |
| k8sCache.service.labels | object | `{}` | Service labels. |
| k8sCache.service.name | string | `"beyla-k8s-cache"` | Name of both the Service and Deployment |
| k8sCache.service.port | int | `50055` | Port of the Kubernetes metadata cache service. |
| nameOverride | string | `""` | Overrides the chart's name |
| namespaceOverride | string | `""` | Override the deployment namespace |
| nodeSelector | object | `{}` | The nodeSelector field allows user to constrain which nodes your DaemonSet pods are scheduled to based on labels on the node |
| podAnnotations | object | `{}` | Adds custom annotations to the Beyla Pods. |
| podLabels | object | `{}` | Adds custom labels to the Beyla Pods. |
| podSecurityContext | object | `{}` |  |
| preset | string | `"application"` | Preconfigures some default properties for network or application observability. Accepted values are "network" or "application". |
| priorityClassName | string | `""` |  |
| privileged | bool | `true` | If set to false, deploys an unprivileged / less privileged setup. |
| rbac.create | bool | `true` | Whether to create RBAC resources for Belya |
| rbac.extraClusterRoleRules | list | `[]` | Extra custer roles to be created for Belya |
| resources | object | `{}` |  |
| securityContext | object | `{"privileged":true}` | Security context for privileged setup. |
| service.annotations | object | `{}` | Service annotations. |
| service.appProtocol | string | `""` | Adds the appProtocol field to the service. This allows to work with istio protocol selection. Ex: "http" or "tcp" |
| service.clusterIP | string | `""` | cluster IP |
| service.enabled | bool | `false` | whether to create a service for metrics |
| service.internalMetrics.appProtocol | string | `""` | Adds the appProtocol field to the service. This allows to work with istio protocol selection. Ex: "http" or "tcp" |
| service.internalMetrics.port | int | `8080` | internal metrics service port |
| service.internalMetrics.portName | string | `"int-metrics"` | name of the port for internal metrics. |
| service.internalMetrics.targetPort | string | `nil` | targetPort overrides the internal metrics port. It defaults to the value of `internal_metrics.prometheus.port` from the Beyla configuration file. |
| service.labels | object | `{}` | Service labels. |
| service.loadBalancerClass | string | `""` | loadbalancer class name |
| service.loadBalancerIP | string | `""` | loadbalancer IP |
| service.loadBalancerSourceRanges | list | `[]` | source ranges for loadbalancer |
| service.port | int | `80` | Prometheus metrics service port |
| service.portName | string | `"metrics"` | name of the port for Prometheus metrics. |
| service.targetPort | string | `nil` | targetPort overrides the Prometheus metrics port. It defaults to the value of `prometheus_export.port` from the Beyla configuration file. |
| service.type | string | `"ClusterIP"` | type of the service |
| serviceAccount.annotations | object | `{}` | Annotations to add to the service account |
| serviceAccount.automount | bool | `true` | Automatically mount a ServiceAccount's API credentials? |
| serviceAccount.create | bool | `true` | Specifies whether a service account should be created |
| serviceAccount.labels | object | `{}` | ServiceAccount labels. |
| serviceAccount.name | string | `""` | The name of the service account to use. If not set and create is true, a name is generated using the fullname template |
| serviceMonitor | object | `{"additionalLabels":{},"annotations":{},"enabled":false,"internalMetrics":{"endpoint":{"interval":"15s"}},"jobLabel":"","metrics":{"endpoint":{"interval":"15s"}}}` | Enable creation of ServiceMonitor for scraping of prometheus HTTP endpoint |
| serviceMonitor.additionalLabels | object | `{}` | Add custom labels to the ServiceMonitor resource |
| serviceMonitor.annotations | object | `{}` | ServiceMonitor annotations |
| serviceMonitor.internalMetrics.endpoint | object | `{"interval":"15s"}` | ServiceMonitor internal metrics scraping endpoint. Target port and path is set based on service and `internal_metrics` values. For additional values, see the ServiceMonitor spec |
| serviceMonitor.jobLabel | string | `""` | Prometheus job label. If empty, chart release name is used |
| serviceMonitor.metrics.endpoint | object | `{"interval":"15s"}` | ServiceMonitor Prometheus scraping endpoint. Target port and path is set based on service and `prometheus_export` values. For additional values, see the ServiceMonitor spec |
| tolerations | list | `[]` | Tolerations allow pods to be scheduled on nodes with specific taints |
| updateStrategy.type | string | `"RollingUpdate"` | update strategy type |
| volumeMounts | list | `[]` | Additional volumeMounts on the output daemonset definition. |
| volumes | list | `[]` | Additional volumes on the output daemonset definition. |

----------------------------------------------
Autogenerated from chart metadata using [helm-docs v1.13.1](https://github.com/norwoodj/helm-docs/releases/v1.13.1)
