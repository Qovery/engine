# VPA

A chart to install the [Kubernetes Vertical Pod Autoscaler](https://github.com/kubernetes/autoscaler/tree/master/vertical-pod-autoscaler)

This chart is entirely based on the manifests and various scripts in the `deploy` and `hack` directories of the VPA repository.

## Tests and Debugging

There are a few tests included with this chart that can help debug why your installation of VPA isn't working as expected. You can run `helm test -n <Release Namespace> <Release Name>` to run them.

* `checkpoint-crd-available` - Checks for the verticalpodautoscalercheckpoints CRD
* `crd-available` - Checks for the verticalpodautoscalers CRD
* `metrics-api-available` - Checks to make sure that the metrics api endpoint is available. If it's not, install [metrics-server](https://github.com/kubernetes-sigs/metrics-server) in your cluster
* `create-vpa` - A simple check to make sure that VPA objects can be created in your cluster. Does not check for functionality of that VPA.

## Components

There are three primary components to the Vertical Pod Autoscaler that can be enabled individually here.

* recommender
* updater
* admissionController

The admissionController is the only one that poses a stability consideration because it will create a mutatingwebhookconfiguration in your cluster. This _could_ cause the cluster to stop accepting pod creation requests if it is not configured correctly. Because of this, it is disabled by default in this chart. The recommender and updater are enabled by default.

For more details, please see the values below, and the vertical pod autosclaer documentation.

## Installation

```bash
helm repo add fairwinds-stable https://charts.fairwinds.com/stable
helm install vpa fairwinds-stable/vpa --namespace vpa --create-namespace
```

## Utilize Prometheus for History

In order to utilize prometheus for recommender history, you will need to pass some extra flags to the recommender. If you use prometheus operator installed in the `prometheus-operator` namespace, these values will do the trick.

```
recommender:
  extraArgs:
    prometheus-address: |
      http://prometheus-operator-prometheus.prometheus-operator.svc.cluster.local:9090
    storage: prometheus
```

## Values

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| imagePullSecrets | list | `[]` | A list of image pull secrets to be used for all pods |
| priorityClassName | string | `""` | To set the priorityclass for all pods |
| nameOverride | string | `""` | A template override for the name |
| fullnameOverride | string | `""` | A template override for the fullname |
| rbac.create | bool | `true` | If true, then rbac resources (clusterroles and clusterrolebindings) will be created for the selected components. |
| serviceAccount.create | bool | `true` | Specifies whether a service account should be created for each component |
| serviceAccount.annotations | object | `{}` | Annotations to add to the service accounts for each component |
| serviceAccount.name | string | `""` | The base name of the service account to use (appended with the component). If not set and create is true, a name is generated using the fullname template and appended for each component |
| serviceAccount.automountServiceAccountToken | bool | `true` | Automount API credentials for the Service Account |
| recommender.enabled | bool | `true` | If true, the vpa recommender component will be installed. |
| recommender.extraArgs | object | `{"pod-recommendation-min-cpu-millicores":15,"pod-recommendation-min-memory-mb":100,"v":"4"}` | A set of key-value flags to be passed to the recommender |
| recommender.replicaCount | int | `1` |  |
| recommender.maxUnavailable | int | `1` | This is the max unavailable setting for the pod disruption budget |
| recommender.image.repository | string | `"us.gcr.io/k8s-artifacts-prod/autoscaling/vpa-recommender"` | The location of the recommender image |
| recommender.image.pullPolicy | string | `"Always"` | The pull policy for the recommender image. Recommend not changing this |
| recommender.image.tag | string | `""` | Overrides the image tag whose default is the chart appVersion |
| recommender.podAnnotations | object | `{}` | Annotations to add to the recommender pod |
| recommender.podSecurityContext.runAsNonRoot | bool | `true` |  |
| recommender.podSecurityContext.runAsUser | int | `65534` |  |
| recommender.securityContext | object | `{}` | The security context for the containers inside the recommender pod |
| recommender.resources | object | `{"limits":{"cpu":"200m","memory":"1000Mi"},"requests":{"cpu":"50m","memory":"500Mi"}}` | The resources block for the recommender pod |
| recommender.nodeSelector | object | `{}` |  |
| recommender.tolerations | list | `[]` |  |
| recommender.affinity | object | `{}` |  |
| updater.enabled | bool | `true` | If true, the updater component will be deployed |
| updater.extraArgs | object | `{}` | A key-value map of flags to pass to the updater |
| updater.replicaCount | int | `1` |  |
| updater.maxUnavailable | int | `1` | This is the max unavailable setting for the pod disruption budget |
| updater.image.repository | string | `"us.gcr.io/k8s-artifacts-prod/autoscaling/vpa-updater"` | The location of the updater image |
| updater.image.pullPolicy | string | `"Always"` | The pull policy for the updater image. Recommend not changing this |
| updater.image.tag | string | `""` | Overrides the image tag whose default is the chart appVersion |
| updater.podAnnotations | object | `{}` | Annotations to add to the updater pod |
| updater.podSecurityContext.runAsNonRoot | bool | `true` |  |
| updater.podSecurityContext.runAsUser | int | `65534` |  |
| updater.securityContext | object | `{}` | The security context for the containers inside the updater pod |
| updater.resources | object | `{"limits":{"cpu":"200m","memory":"1000Mi"},"requests":{"cpu":"50m","memory":"500Mi"}}` | The resources block for the updater pod |
| updater.nodeSelector | object | `{}` |  |
| updater.tolerations | list | `[]` |  |
| updater.affinity | object | `{}` |  |
| admissionController.enabled | bool | `false` | If true, will install the admission-controller component of vpa |
| admissionController.generateCertificate | bool | `true` | If true and admissionController is enabled, a pre-install hook will run to create the certificate for the webhook |
| admissionController.certGen.image.repository | string | `"quay.io/reactiveops/ci-images"` | An image that contains certgen for creating certificates. Only used if admissionController.generateCertificate is true |
| admissionController.certGen.image.tag | string | `"v11-alpine"` | An image tag for the admissionController.certGen.image.repository image. Only used if admissionController.generateCertificate is true |
| admissionController.certGen.image.pullPolicy | string | `"Always"` | The pull policy for the certgen image. Recommend not changing this |
| admissionController.certGen.env | object | `{}` | Additional environment variables to be added to the certgen container. Format is KEY: Value format |
| admissionController.cleanupOnDelete | bool | `true` | If true, a post-delete job will remove the mutatingwebhookconfiguration and the tls secret for the admission controller |
| admissionController.replicaCount | int | `1` |  |
| admissionController.image.repository | string | `"us.gcr.io/k8s-artifacts-prod/autoscaling/vpa-admission-controller"` | The location of the vpa admission controller image |
| admissionController.image.pullPolicy | string | `"Always"` | The pull policy for the admission controller image. Recommend not changing this |
| admissionController.image.tag | string | `""` | Overrides the image tag whose default is the chart appVersion |
| admissionController.podAnnotations | object | `{}` | Annotations to add to the admission controller pod |
| admissionController.podSecurityContext.runAsNonRoot | bool | `true` |  |
| admissionController.podSecurityContext.runAsUser | int | `65534` |  |
| admissionController.securityContext | object | `{}` | The security context for the containers inside the admission controller pod |
| admissionController.resources | object | `{"limits":{"cpu":"200m","memory":"500Mi"},"requests":{"cpu":"50m","memory":"200Mi"}}` | The resources block for the admission controller pod |
| admissionController.nodeSelector | object | `{}` |  |
| admissionController.tolerations | list | `[]` |  |
| admissionController.affinity | object | `{}` |  |
