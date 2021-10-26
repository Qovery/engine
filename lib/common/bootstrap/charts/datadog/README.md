# Datadog

![Version: 2.22.17](https://img.shields.io/badge/Version-2.22.17-informational?style=flat-square) ![AppVersion: 7](https://img.shields.io/badge/AppVersion-7-informational?style=flat-square)

[Datadog](https://www.datadoghq.com/) is a hosted infrastructure monitoring platform. This chart adds the Datadog Agent to all nodes in your cluster via a DaemonSet. It also optionally depends on the [kube-state-metrics chart](https://github.com/prometheus-community/helm-charts/tree/main/charts/kube-state-metrics). For more information about monitoring Kubernetes with Datadog, please refer to the [Datadog documentation website](https://docs.datadoghq.com/agent/basic_agent_usage/kubernetes/).

Datadog [offers two variants](https://hub.docker.com/r/datadog/agent/tags/), switch to a `-jmx` tag if you need to run JMX/java integrations. The chart also supports running [the standalone dogstatsd image](https://hub.docker.com/r/datadog/dogstatsd/tags/).

See the [Datadog JMX integration](https://docs.datadoghq.com/integrations/java/) to learn more.

## How to use Datadog Helm repository

You need to add this repository to your Helm repositories:

```
helm repo add datadog https://helm.datadoghq.com
helm repo update
```

## Prerequisites

Kubernetes 1.10+ or OpenShift 3.10+, note that:

- the Datadog Agent supports Kubernetes 1.4+
- The Datadog chart's defaults are tailored to Kubernetes 1.10+, see [Datadog Agent legacy Kubernetes versions documentation](https://github.com/DataDog/datadog-agent/tree/main/Dockerfiles/agent#legacy-kubernetes-versions) for adjustments you might need to make for older versions

## Requirements

| Repository | Name | Version |
|------------|------|---------|
| https://helm.datadoghq.com | datadog-crds | =0.3.2 |
| https://prometheus-community.github.io/helm-charts | kube-state-metrics | =2.13.2 |

## Quick start

By default, the Datadog Agent runs in a DaemonSet. It can alternatively run inside a Deployment for special use cases.

**Note:** simultaneous DaemonSet + Deployment installation within a single release will be deprecated in a future version, requiring two releases to achieve this.

### Installing the Datadog Chart

To install the chart with the release name `<RELEASE_NAME>`, retrieve your Datadog API key from your [Agent Installation Instructions](https://app.datadoghq.com/account/settings#agent/kubernetes) and run:

```bash
helm install --name <RELEASE_NAME> \
  --set datadog.apiKey=<DATADOG_API_KEY> datadog/datadog
```

By default, this Chart creates a Secret and puts an API key in that Secret.
However, you can use manually created secret by setting the `datadog.apiKeyExistingSecret` value. After a few minutes, you should see hosts and metrics being reported in Datadog.

**Note:** You can set your [Datadog site](https://docs.datadoghq.com/getting_started/site) using the `datadog.site` field.

```bash
helm install --name <RELEASE_NAME> \
    --set datadog.appKey=<DATADOG_APP_KEY> \
    --set datadog.site=<DATADOG_SITE> \
    datadog/datadog
```

#### Create and provide a secret that contains your Datadog API Key

To create a secret that contains your Datadog API key, replace the <DATADOG_API_KEY> below with the API key for your organization. This secret is used in the manifest to deploy the Datadog Agent.

```bash
DATADOG_SECRET_NAME=datadog-secret
kubectl create secret generic $DATADOG_SECRET_NAME --from-literal api-key="<DATADOG_API_KEY>" --namespace="default"
```

**Note**: This creates a secret in the default namespace. If you are in a custom namespace, update the namespace parameter of the command before running it.

Now, the installation command contains the reference to the secret.

```bash
helm install --name <RELEASE_NAME> \
  --set datadog.apiKeyExistingSecret=$DATADOG_SECRET_NAME datadog/datadog
```

**Note**: Provide a secret for the application key (AppKey) using the `datadog.appKeyExistingSecret` chart variable.

### Enabling the Datadog Cluster Agent

Read about the Datadog Cluster Agent in the [official documentation](https://docs.datadoghq.com/agent/kubernetes/cluster/).

Run the following if you want to deploy the chart with the Datadog Cluster Agent:

```bash
helm install --name datadog-monitoring \
    --set datadog.apiKey=<DATADOG_API_KEY> \
    --set datadog.appKey=<DATADOG_APP_KEY> \
    --set clusterAgent.enabled=true \
    --set clusterAgent.metricsProvider.enabled=true \
    datadog/datadog
```

**Note**: Specifying `clusterAgent.metricsProvider.enabled=true` enables the External Metrics Server.
If you want to learn to use this feature, you can check out this [Datadog Cluster Agent walkthrough](https://github.com/DataDog/datadog-agent/blob/main/docs/cluster-agent/CUSTOM_METRICS_SERVER.md).

The Leader Election is enabled by default in the chart for the Cluster Agent. Only the Cluster Agent(s) participate in the election, in case you have several replicas configured (using `clusterAgent.replicas`.

#### Cluster Agent Token

You can specify the Datadog Cluster Agent token used to secure the communication between the Cluster Agent(s) and the Agents with `clusterAgent.token`.

### Upgrading

#### From 1.x to 2.x

⚠️ Migrating from 1.x to 2.x requires a manual action.

The `datadog` chart has been refactored to regroup the `values.yaml` parameters in a more logical way.
Please follow the [migration guide](https://github.com/DataDog/helm-charts/blob/main/charts/datadog/docs/Migration_1.x_to_2.x.md) to update your `values.yaml` file.

#### From 1.19.0 onwards

Version `1.19.0` introduces the use of release name as full name if it contains the chart name(`datadog` in this case).
E.g. with a release name of `datadog`, this renames the `DaemonSet` from `datadog-datadog` to `datadog`.
The suggested approach is to delete the release and reinstall it.

#### From 1.0.0 onwards

Starting with version 1.0.0, this chart does not support deploying Agent 5.x anymore. If you cannot upgrade to Agent 6.x or later, you can use a previous version of the chart by calling helm install with `--version 0.18.0`.

See [0.18.1's README](https://github.com/helm/charts/blob/847f737479bb78d89f8fb650db25627558fbe1f0/datadog/datadog/README.md) to see which options were supported at the time.

### Uninstalling the Chart

To uninstall/delete the `<RELEASE_NAME>` deployment:

```bash
helm delete <RELEASE_NAME> --purge
```

The command removes all the Kubernetes components associated with the chart and deletes the release.

## Configuration

As a best practice, a YAML file that specifies the values for the chart parameters should be used to configure the chart. Any parameters not specified in this file will default to those set in [values.yaml](values.yaml).

1. Create an empty `datadog-values.yaml` file.
2. Create a Kubernetes `secret` to store your [Datadog API key](https://app.datadoghq.com/organization-settings/api-keys) and [App key](https://app.datadoghq.com/organization-settings/application-keys)

```bash
kubectl create secret generic datadog-secret --from-literal api-key=$DD_API_KEY --from-literal app-key=$DD_APP_KEY
```

3. Set the following parameters in your `datadog-values.yaml` file to reference the secret:

```yaml
datadog:
  apiKeyExistingSecret: datadog-secret
  appKeyExistingSecret: datadog-secret
```

3. Install or upgrade the Datadog Helm chart with the new `datadog-values.yaml` file:

```bash
helm install -f datadog-values.yaml <RELEASE_NAME> datadog/datadog
```

OR

```bash
helm upgrade -f datadog-values.yaml <RELEASE_NAME> datadog/datadog
```

See the [All configuration options](#all-configuration-options) section to discover all configuration possibilities in the Datadog chart.

### Configuring Dogstatsd in the agent
<a name="dsd-config"></a>
The agent will start a server running Dogstatsd in order to process custom metrics sent from your applications. Check out the [official documentation on Dogstatsd](https://docs.datadoghq.com/developers/dogstatsd/?tab=hostagent) for more details.

By default the agent will create a unix domain socket to process the datagrams (not supported on Windows, see [below](#windows-config)).

To disable the socket in favor of the hostPort, use the following configuration:

```yaml
datadog:
  #(...)
  dogstatsd:
    useSocketVolume: false
    useHostPort: true
```

### Enabling APM and Tracing

APM is enabled by default using a socket for communication in the out-of-the-box [values.yaml](values.yaml) file; more details about application configuration are available on the [official documentation](https://docs.datadoghq.com/agent/kubernetes/apm/?tab=helm).
Update your `datadog-values.yaml` file with the following configration to enable TCP communication using a `hostPort`:

```yaml
datadog:
  # (...)
  apm:
    portEnabled: true
```

To disable APM, set `socketEnabled` to `false` in your `datadog-values.yaml` file (`portEnabled` is `false` by default):

```yaml
datadog:
  # (...)
  apm:
    socketEnabled: false
```

### Enabling Log Collection

Update your `datadog-values.yaml` file with the following log collection configuration:

```yaml
datadog:
  # (...)
  logs:
    enabled: true
    containerCollectAll: true
```

then upgrade your Datadog Helm chart:

```bash
helm upgrade -f datadog-values.yaml <RELEASE_NAME> datadog/datadog
```

### Enabling Process Collection

Update your `datadog-values.yaml` file with the process collection configuration:

```yaml
datadog:
  # (...)
  processAgent:
    enabled: true
    processCollection: true
```

then upgrade your Datadog Helm chart:

```bash
helm upgrade -f datadog-values.yaml <RELEASE_NAME> datadog/datadog
```

### Enabling System Probe Collection

The system-probe agent only runs in dedicated container environment. Update your `datadog-values.yaml` file with the system-probe collection configuration:

```yaml
datadog:
  # (...)
  systemProbe:
    # (...)
    enabled: true

# (...)
```

then upgrade your Datadog Helm chart:

```bash
helm upgrade -f datadog-values.yaml <RELEASE_NAME> datadog/datadog
```

### Kubernetes event collection

Use the [Datadog Cluster Agent](#enabling-the-datadog-cluster-agent) to collect Kubernetes events. Please read [the official documentation](https://docs.datadoghq.com/agent/kubernetes/event_collection/) for more context.

Alternatively set the `datadog.leaderElection`, `datadog.collectEvents` and `rbac.create` options to `true` in order to enable Kubernetes event collection.

### conf.d and checks.d

The Datadog [entrypoint](https://github.com/DataDog/datadog-agent/blob/main/Dockerfiles/agent/entrypoint/89-copy-customfiles.sh) copies files with a `.yaml` extension found in `/conf.d` and files with `.py` extension in `/checks.d` to `/etc/datadog-agent/conf.d` and `/etc/datadog-agent/checks.d` respectively.

The keys for `datadog.confd` and `datadog.checksd` should mirror the content found in their respective ConfigMaps. Update your `datadog-values.yaml` file with the check configurations:

```yaml
datadog:
  confd:
    redisdb.yaml: |-
      ad_identifiers:
        - redis
        - bitnami/redis
      init_config:
      instances:
        - host: "%%host%%"
          port: "%%port%%"
    jmx.yaml: |-
      ad_identifiers:
        - openjdk
      instance_config:
      instances:
        - host: "%%host%%"
          port: "%%port_0%%"
    redisdb.yaml: |-
      init_config:
      instances:
        - host: "outside-k8s.example.com"
          port: 6379
```

then upgrade your Datadog Helm chart:

```bash
helm upgrade -f datadog-values.yaml <RELEASE_NAME> datadog/datadog
```

For more details, please refer to [the documentation](https://docs.datadoghq.com/agent/kubernetes/integrations/).

### Kubernetes Labels and Annotations

To map Kubernetes node labels and pod labels and annotations to Datadog tags, provide a dictionary with kubernetes labels/annotations as keys and Datadog tags key as values in your `datadog-values.yaml` file:

```yaml
nodeLabelsAsTags:
  beta.kubernetes.io/instance-type: aws_instance_type
  kubernetes.io/role: kube_role
```

```yaml
podAnnotationsAsTags:
  iam.amazonaws.com/role: kube_iamrole
```

```yaml
podLabelsAsTags:
  app: kube_app
  release: helm_release
```

then upgrade your Datadog Helm chart:

```bash
helm upgrade -f datadog-values.yaml <RELEASE_NAME> datadog/datadog
```

### CRI integration

As of the version 6.6.0, the Datadog Agent supports collecting metrics from any container runtime interface used in your cluster. Configure the location path of the socket with `datadog.criSocketPath`; default is the Docker container runtime socket. To deactivate this support, you just need to unset the `datadog.criSocketPath` setting.
Standard paths are:

- Docker socket: `/var/run/docker.sock`
- Containerd socket: `/var/run/containerd/containerd.sock`
- Cri-o socket: `/var/run/crio/crio.sock`

### Configuration required for Amazon Linux 2 based nodes

Amazon Linux 2 does not support apparmor profile enforcement.
Amazon Linux 2 is the default operating system for AWS Elastic Kubernetes Service (EKS) based clusters.
Update your `datadog-values.yaml` file to disable apparmor enforcement:

```yaml
agents:
  # (...)
  podSecurity:
    # (...)
    apparmor:
      # (...)
      enabled: false

# (...)
```

## All configuration options

The following table lists the configurable parameters of the Datadog chart and their default values. Specify each parameter using the `--set key=value[,key=value]` argument to `helm install`. For example,

```bash
helm install --name <RELEASE_NAME> \
  --set datadog.apiKey=<DATADOG_API_KEY>,datadog.logLevel=DEBUG \
  datadog/datadog
```

## Values

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| agents.additionalLabels | object | `{}` | Adds labels to the Agent daemonset and pods |
| agents.affinity | object | `{}` | Allow the DaemonSet to schedule using affinity rules |
| agents.containers.agent.env | list | `[]` | Additional environment variables for the agent container |
| agents.containers.agent.envFrom | list | `[]` | Set environment variables specific to agent container from configMaps and/or secrets |
| agents.containers.agent.healthPort | int | `5555` | Port number to use in the node agent for the healthz endpoint |
| agents.containers.agent.livenessProbe | object | Every 15s / 6 KO / 1 OK | Override default agent liveness probe settings |
| agents.containers.agent.logLevel | string | `nil` | Set logging verbosity, valid log levels are: trace, debug, info, warn, error, critical, and off |
| agents.containers.agent.ports | list | `[]` | Allows to specify extra ports (hostPorts for instance) for this container |
| agents.containers.agent.readinessProbe | object | Every 15s / 6 KO / 1 OK | Override default agent readiness probe settings |
| agents.containers.agent.resources | object | `{}` | Resource requests and limits for the agent container. |
| agents.containers.agent.securityContext | object | `{}` | Allows you to overwrite the default container SecurityContext for the agent container. |
| agents.containers.initContainers.resources | object | `{}` | Resource requests and limits for the init containers |
| agents.containers.processAgent.env | list | `[]` | Additional environment variables for the process-agent container |
| agents.containers.processAgent.envFrom | list | `[]` | Set environment variables specific to process-agent from configMaps and/or secrets |
| agents.containers.processAgent.logLevel | string | `nil` | Set logging verbosity, valid log levels are: trace, debug, info, warn, error, critical, and off |
| agents.containers.processAgent.ports | list | `[]` | Allows to specify extra ports (hostPorts for instance) for this container |
| agents.containers.processAgent.resources | object | `{}` | Resource requests and limits for the process-agent container |
| agents.containers.processAgent.securityContext | object | `{}` | Allows you to overwrite the default container SecurityContext for the process-agent container. |
| agents.containers.securityAgent.env | string | `nil` | Additional environment variables for the security-agent container |
| agents.containers.securityAgent.envFrom | list | `[]` | Set environment variables specific to security-agent from configMaps and/or secrets |
| agents.containers.securityAgent.logLevel | string | `nil` | Set logging verbosity, valid log levels are: trace, debug, info, warn, error, critical, and off |
| agents.containers.securityAgent.ports | list | `[]` | Allows to specify extra ports (hostPorts for instance) for this container |
| agents.containers.securityAgent.resources | object | `{}` | Resource requests and limits for the security-agent container |
| agents.containers.systemProbe.env | list | `[]` | Additional environment variables for the system-probe container |
| agents.containers.systemProbe.envFrom | list | `[]` | Set environment variables specific to system-probe from configMaps and/or secrets |
| agents.containers.systemProbe.logLevel | string | `nil` | Set logging verbosity, valid log levels are: trace, debug, info, warn, error, critical, and off. |
| agents.containers.systemProbe.ports | list | `[]` | Allows to specify extra ports (hostPorts for instance) for this container |
| agents.containers.systemProbe.resources | object | `{}` | Resource requests and limits for the system-probe container |
| agents.containers.systemProbe.securityContext | object | `{"capabilities":{"add":["SYS_ADMIN","SYS_RESOURCE","SYS_PTRACE","NET_ADMIN","NET_BROADCAST","NET_RAW","IPC_LOCK"]},"privileged":false}` | Allows you to overwrite the default container SecurityContext for the system-probe container. |
| agents.containers.traceAgent.env | string | `nil` | Additional environment variables for the trace-agent container |
| agents.containers.traceAgent.envFrom | list | `[]` | Set environment variables specific to trace-agent from configMaps and/or secrets |
| agents.containers.traceAgent.livenessProbe | object | Every 15s | Override default agent liveness probe settings |
| agents.containers.traceAgent.logLevel | string | `nil` | Set logging verbosity, valid log levels are: trace, debug, info, warn, error, critical, and off |
| agents.containers.traceAgent.ports | list | `[]` | Allows to specify extra ports (hostPorts for instance) for this container |
| agents.containers.traceAgent.resources | object | `{}` | Resource requests and limits for the trace-agent container |
| agents.containers.traceAgent.securityContext | object | `{}` | Allows you to overwrite the default container SecurityContext for the trace-agent container. |
| agents.customAgentConfig | object | `{}` | Specify custom contents for the datadog agent config (datadog.yaml) |
| agents.dnsConfig | object | `{}` | specify dns configuration options for datadog cluster agent containers e.g ndots |
| agents.enabled | bool | `true` | You should keep Datadog DaemonSet enabled! |
| agents.image.doNotCheckTag | string | `nil` | Skip the version<>chart compatibility check |
| agents.image.name | string | `"agent"` | Datadog Agent image name to use (relative to `registry`) |
| agents.image.pullPolicy | string | `"IfNotPresent"` | Datadog Agent image pull policy |
| agents.image.pullSecrets | list | `[]` | Datadog Agent repository pullSecret (ex: specify docker registry credentials) |
| agents.image.repository | string | `nil` | Override default registry + image.name for Agent |
| agents.image.tag | string | `"7.31.1"` | Define the Agent version to use |
| agents.image.tagSuffix | string | `""` | Suffix to append to Agent tag |
| agents.networkPolicy.create | bool | `false` | If true, create a NetworkPolicy for the agents. DEPRECATED. Use datadog.networkPolicy.create instead |
| agents.nodeSelector | object | `{}` | Allow the DaemonSet to schedule on selected nodes |
| agents.podAnnotations | object | `{}` | Annotations to add to the DaemonSet's Pods |
| agents.podLabels | object | `{}` | Sets podLabels if defined Note: These labels are also used as label selectors so they are immutable. |
| agents.podSecurity.apparmor.enabled | bool | `true` | If true, enable apparmor enforcement |
| agents.podSecurity.apparmorProfiles | list | `["runtime/default","unconfined"]` | Allowed apparmor profiles |
| agents.podSecurity.capabilities | list | `["SYS_ADMIN","SYS_RESOURCE","SYS_PTRACE","NET_ADMIN","NET_BROADCAST","NET_RAW","IPC_LOCK","AUDIT_CONTROL","AUDIT_READ"]` | Allowed capabilities |
| agents.podSecurity.podSecurityPolicy.create | bool | `false` | If true, create a PodSecurityPolicy resource for Agent pods |
| agents.podSecurity.privileged | bool | `false` | If true, Allow to run privileged containers |
| agents.podSecurity.seLinuxContext | object | Must run as spc_t | Provide seLinuxContext configuration for PSP/SCC |
| agents.podSecurity.seccompProfiles | list | `["runtime/default","localhost/system-probe"]` | Allowed seccomp profiles |
| agents.podSecurity.securityContextConstraints.create | bool | `false` | If true, create a SecurityContextConstraints resource for Agent pods |
| agents.podSecurity.volumes | list | `["configMap","downwardAPI","emptyDir","hostPath","secret"]` | Allowed volumes types |
| agents.priorityClassName | string | `nil` | Sets PriorityClassName if defineds |
| agents.rbac.create | bool | `true` | If true, create & use RBAC resources |
| agents.rbac.serviceAccountAnnotations | object | `{}` | Annotations to add to the ServiceAccount if agents.rbac.create is true |
| agents.rbac.serviceAccountName | string | `"default"` | Specify a preexisting ServiceAccount to use if agents.rbac.create is false |
| agents.tolerations | list | `[]` | Allow the DaemonSet to schedule on tainted nodes (requires Kubernetes >= 1.6) |
| agents.updateStrategy | object | `{"rollingUpdate":{"maxUnavailable":"10%"},"type":"RollingUpdate"}` | Allow the DaemonSet to perform a rolling update on helm update |
| agents.useConfigMap | string | `nil` | Configures a configmap to provide the agent configuration. Use this in combination with the `agents.customAgentConfig` parameter. |
| agents.useHostNetwork | bool | `false` | Bind ports on the hostNetwork |
| agents.volumeMounts | list | `[]` | Specify additional volumes to mount in all containers of the agent pod |
| agents.volumes | list | `[]` | Specify additional volumes to mount in the dd-agent container |
| clusterAgent.additionalLabels | object | `{}` | Adds labels to the Cluster Agent deployment and pods |
| clusterAgent.admissionController.enabled | bool | `false` | Enable the admissionController to be able to inject APM/Dogstatsd config and standard tags (env, service, version) automatically into your pods |
| clusterAgent.admissionController.mutateUnlabelled | bool | `false` | Enable injecting config without having the pod label 'admission.datadoghq.com/enabled="true"' |
| clusterAgent.advancedConfd | object | `{}` | Provide additional cluster check configurations |
| clusterAgent.affinity | object | `{}` | Allow the Cluster Agent Deployment to schedule using affinity rules |
| clusterAgent.command | list | `[]` | Command to run in the Cluster Agent container as entrypoint |
| clusterAgent.confd | object | `{}` | Provide additional cluster check configurations |
| clusterAgent.containers.clusterAgent.securityContext | object | `{}` | Specify securityContext on the cluster-agent container. |
| clusterAgent.createPodDisruptionBudget | bool | `false` | Create pod disruption budget for Cluster Agent deployments |
| clusterAgent.datadog_cluster_yaml | object | `{}` | Specify custom contents for the datadog cluster agent config (datadog-cluster.yaml) |
| clusterAgent.dnsConfig | object | `{}` | Specify dns configuration options for datadog cluster agent containers e.g ndots |
| clusterAgent.enabled | bool | `true` | Set this to false to disable Datadog Cluster Agent |
| clusterAgent.env | list | `[]` | Set environment variables specific to Cluster Agent |
| clusterAgent.envFrom | list | `[]` | Set environment variables specific to Cluster Agent from configMaps and/or secrets |
| clusterAgent.healthPort | int | `5556` | Port number to use in the Cluster Agent for the healthz endpoint |
| clusterAgent.image.name | string | `"cluster-agent"` | Cluster Agent image name to use (relative to `registry`) |
| clusterAgent.image.pullPolicy | string | `"IfNotPresent"` | Cluster Agent image pullPolicy |
| clusterAgent.image.pullSecrets | list | `[]` | Cluster Agent repository pullSecret (ex: specify docker registry credentials) |
| clusterAgent.image.repository | string | `nil` | Override default registry + image.name for Cluster Agent |
| clusterAgent.image.tag | string | `"1.15.1"` | Cluster Agent image tag to use |
| clusterAgent.livenessProbe | object | Every 15s / 6 KO / 1 OK | Override default Cluster Agent liveness probe settings |
| clusterAgent.metricsProvider.aggregator | string | `"avg"` | Define the aggregator the cluster agent will use to process the metrics. The options are (avg, min, max, sum) |
| clusterAgent.metricsProvider.createReaderRbac | bool | `true` | Create `external-metrics-reader` RBAC automatically (to allow HPA to read data from Cluster Agent) |
| clusterAgent.metricsProvider.enabled | bool | `false` | Set this to true to enable Metrics Provider |
| clusterAgent.metricsProvider.endpoint | string | `nil` | Override the external metrics provider endpoint. If not set, the cluster-agent defaults to `datadog.site` |
| clusterAgent.metricsProvider.service.port | int | `8443` | Set port of cluster-agent metrics server service (Kubernetes >= 1.15) |
| clusterAgent.metricsProvider.service.type | string | `"ClusterIP"` | Set type of cluster-agent metrics server service |
| clusterAgent.metricsProvider.useDatadogMetrics | bool | `false` | Enable usage of DatadogMetric CRD to autoscale on arbitrary Datadog queries |
| clusterAgent.metricsProvider.wpaController | bool | `false` | Enable informer and controller of the watermark pod autoscaler |
| clusterAgent.networkPolicy.create | bool | `false` | If true, create a NetworkPolicy for the cluster agent. DEPRECATED. Use datadog.networkPolicy.create instead |
| clusterAgent.nodeSelector | object | `{}` | Allow the Cluster Agent Deployment to be scheduled on selected nodes |
| clusterAgent.podAnnotations | object | `{}` | Annotations to add to the cluster-agents's pod(s) |
| clusterAgent.podSecurity.podSecurityPolicy.create | bool | `false` | If true, create a PodSecurityPolicy resource for Cluster Agent pods |
| clusterAgent.podSecurity.securityContextConstraints.create | bool | `false` | If true, create a SCC resource for Cluster Agent pods |
| clusterAgent.priorityClassName | string | `nil` | Name of the priorityClass to apply to the Cluster Agent |
| clusterAgent.rbac.create | bool | `true` | If true, create & use RBAC resources |
| clusterAgent.rbac.serviceAccountAnnotations | object | `{}` | Annotations to add to the ServiceAccount if clusterAgent.rbac.create is true |
| clusterAgent.rbac.serviceAccountName | string | `"default"` | Specify a preexisting ServiceAccount to use if clusterAgent.rbac.create is false |
| clusterAgent.readinessProbe | object | Every 15s / 6 KO / 1 OK | Override default Cluster Agent readiness probe settings |
| clusterAgent.replicas | int | `1` | Specify the of cluster agent replicas, if > 1 it allow the cluster agent to work in HA mode. |
| clusterAgent.resources | object | `{}` | Datadog cluster-agent resource requests and limits. |
| clusterAgent.securityContext | object | `{}` | Allows you to overwrite the default PodSecurityContext on the cluster-agent pods. |
| clusterAgent.strategy | object | `{"rollingUpdate":{"maxSurge":1,"maxUnavailable":0},"type":"RollingUpdate"}` | Allow the Cluster Agent deployment to perform a rolling update on helm update |
| clusterAgent.token | string | `""` | Cluster Agent token is a preshared key between node agents and cluster agent (autogenerated if empty, needs to be at least 32 characters a-zA-z) |
| clusterAgent.tokenExistingSecret | string | `""` | Existing secret name to use for Cluster Agent token |
| clusterAgent.useHostNetwork | bool | `false` | Bind ports on the hostNetwork |
| clusterAgent.volumeMounts | list | `[]` | Specify additional volumes to mount in the cluster-agent container |
| clusterAgent.volumes | list | `[]` | Specify additional volumes to mount in the cluster-agent container |
| clusterChecksRunner.additionalLabels | object | `{}` | Adds labels to the cluster checks runner deployment and pods |
| clusterChecksRunner.affinity | object | `{}` | Allow the ClusterChecks Deployment to schedule using affinity rules. |
| clusterChecksRunner.createPodDisruptionBudget | bool | `false` | Create the pod disruption budget to apply to the cluster checks agents |
| clusterChecksRunner.dnsConfig | object | `{}` | specify dns configuration options for datadog cluster agent containers e.g ndots |
| clusterChecksRunner.enabled | bool | `false` | If true, deploys agent dedicated for running the Cluster Checks instead of running in the Daemonset's agents. |
| clusterChecksRunner.env | list | `[]` | Environment variables specific to Cluster Checks Runner |
| clusterChecksRunner.envFrom | list | `[]` | Set environment variables specific to Cluster Checks Runner from configMaps and/or secrets |
| clusterChecksRunner.healthPort | int | `5557` | Port number to use in the Cluster Checks Runner for the healthz endpoint |
| clusterChecksRunner.image.name | string | `"agent"` | Datadog Agent image name to use (relative to `registry`) |
| clusterChecksRunner.image.pullPolicy | string | `"IfNotPresent"` | Datadog Agent image pull policy |
| clusterChecksRunner.image.pullSecrets | list | `[]` | Datadog Agent repository pullSecret (ex: specify docker registry credentials) |
| clusterChecksRunner.image.repository | string | `nil` | Override default registry + image.name for Cluster Check Runners |
| clusterChecksRunner.image.tag | string | `"7.31.1"` | Define the Agent version to use |
| clusterChecksRunner.image.tagSuffix | string | `""` | Suffix to append to Agent tag |
| clusterChecksRunner.livenessProbe | object | Every 15s / 6 KO / 1 OK | Override default agent liveness probe settings |
| clusterChecksRunner.networkPolicy.create | bool | `false` | If true, create a NetworkPolicy for the cluster checks runners. DEPRECATED. Use datadog.networkPolicy.create instead |
| clusterChecksRunner.nodeSelector | object | `{}` | Allow the ClusterChecks Deployment to schedule on selected nodes |
| clusterChecksRunner.podAnnotations | object | `{}` | Annotations to add to the cluster-checks-runner's pod(s) |
| clusterChecksRunner.ports | list | `[]` | Allows to specify extra ports (hostPorts for instance) for this container |
| clusterChecksRunner.priorityClassName | string | `nil` | Name of the priorityClass to apply to the Cluster checks runners |
| clusterChecksRunner.rbac.create | bool | `true` | If true, create & use RBAC resources |
| clusterChecksRunner.rbac.dedicated | bool | `false` | If true, use a dedicated RBAC resource for the cluster checks agent(s) |
| clusterChecksRunner.rbac.serviceAccountAnnotations | object | `{}` | Annotations to add to the ServiceAccount if clusterChecksRunner.rbac.dedicated is true |
| clusterChecksRunner.rbac.serviceAccountName | string | `"default"` | Specify a preexisting ServiceAccount to use if clusterChecksRunner.rbac.create is false |
| clusterChecksRunner.readinessProbe | object | Every 15s / 6 KO / 1 OK | Override default agent readiness probe settings |
| clusterChecksRunner.replicas | int | `2` | Number of Cluster Checks Runner instances |
| clusterChecksRunner.resources | object | `{}` | Datadog clusterchecks-agent resource requests and limits. |
| clusterChecksRunner.securityContext | object | `{}` | Allows you to overwrite the default PodSecurityContext on the clusterchecks pods. |
| clusterChecksRunner.strategy | object | `{"rollingUpdate":{"maxSurge":1,"maxUnavailable":0},"type":"RollingUpdate"}` | Allow the ClusterChecks deployment to perform a rolling update on helm update |
| clusterChecksRunner.tolerations | list | `[]` | Tolerations for pod assignment |
| clusterChecksRunner.volumeMounts | list | `[]` | Specify additional volumes to mount in the cluster checks container |
| clusterChecksRunner.volumes | list | `[]` | Specify additional volumes to mount in the cluster checks container |
| datadog-crds.crds.datadogMetrics | bool | `true` | Set to true to deploy the DatadogMetrics CRD |
| datadog.apiKey | string | `"<DATADOG_API_KEY>"` | Your Datadog API key ref: https://app.datadoghq.com/account/settings#agent/kubernetes |
| datadog.apiKeyExistingSecret | string | `nil` | Use existing Secret which stores API key instead of creating a new one |
| datadog.apm.enabled | bool | `false` | Enable this to enable APM and tracing, on port 8126 DEPRECATED. Use datadog.apm.portEnabled instead |
| datadog.apm.hostSocketPath | string | `"/var/run/datadog/"` | Host path to the trace-agent socket |
| datadog.apm.port | int | `8126` | Override the trace Agent port |
| datadog.apm.portEnabled | bool | `false` | Enable APM over TCP communication (port 8216 by default |
| datadog.apm.socketEnabled | bool | `true` | Enable APM over Socket (Unix Socket or windows named pipe) |
| datadog.apm.socketPath | string | `"/var/run/datadog/apm.socket"` | Path to the trace-agent socket |
| datadog.apm.useSocketVolume | bool | `false` | Enable APM over Unix Domain Socket DEPRECATED. Use datadog.apm.socketEnabled instead |
| datadog.appKey | string | `nil` | Datadog APP key required to use metricsProvider |
| datadog.appKeyExistingSecret | string | `nil` | Use existing Secret which stores APP key instead of creating a new one |
| datadog.checksCardinality | string | `nil` | Sets the tag cardinality for the checks run by the Agent. |
| datadog.checksd | object | `{}` | Provide additional custom checks as python code |
| datadog.clusterChecks.enabled | bool | `true` | Enable the Cluster Checks feature on both the cluster-agents and the daemonset |
| datadog.clusterName | string | `nil` | Set a unique cluster name to allow scoping hosts and Cluster Checks easily |
| datadog.collectEvents | bool | `true` | Enables this to start event collection from the kubernetes API |
| datadog.confd | object | `{}` | Provide additional check configurations (static and Autodiscovery) |
| datadog.containerExclude | string | `nil` | Exclude containers from the Agent Autodiscovery, as a space-sepatered list |
| datadog.containerExcludeLogs | string | `nil` | Exclude logs from the Agent Autodiscovery, as a space-separated list |
| datadog.containerExcludeMetrics | string | `nil` | Exclude metrics from the Agent Autodiscovery, as a space-separated list |
| datadog.containerInclude | string | `nil` | Include containers in the Agent Autodiscovery, as a space-separated list.  If a container matches an include rule, it’s always included in the Autodiscovery |
| datadog.containerIncludeLogs | string | `nil` | Include logs in the Agent Autodiscovery, as a space-separated list |
| datadog.containerIncludeMetrics | string | `nil` | Include metrics in the Agent Autodiscovery, as a space-separated list |
| datadog.criSocketPath | string | `nil` | Path to the container runtime socket (if different from Docker) |
| datadog.dd_url | string | `nil` | The host of the Datadog intake server to send Agent data to, only set this option if you need the Agent to send data to a custom URL |
| datadog.dockerSocketPath | string | `nil` | Path to the docker socket |
| datadog.dogstatsd.hostSocketPath | string | `"/var/run/datadog/"` | Host path to the DogStatsD socket |
| datadog.dogstatsd.nonLocalTraffic | bool | `true` | Enable this to make each node accept non-local statsd traffic (from outside of the pod) |
| datadog.dogstatsd.originDetection | bool | `false` | Enable origin detection for container tagging |
| datadog.dogstatsd.port | int | `8125` | Override the Agent DogStatsD port |
| datadog.dogstatsd.socketPath | string | `"/var/run/datadog/dsd.socket"` | Path to the DogStatsD socket |
| datadog.dogstatsd.tagCardinality | string | `"low"` | Sets the tag cardinality relative to the origin detection |
| datadog.dogstatsd.tags | list | `[]` | List of static tags to attach to every custom metric, event and service check collected by Dogstatsd. |
| datadog.dogstatsd.useHostPID | bool | `false` | Run the agent in the host's PID namespace |
| datadog.dogstatsd.useHostPort | bool | `false` | Sets the hostPort to the same value of the container port |
| datadog.dogstatsd.useSocketVolume | bool | `true` | Enable dogstatsd over Unix Domain Socket with an HostVolume |
| datadog.env | list | `[]` | Set environment variables for all Agents |
| datadog.envFrom | list | `[]` | Set environment variables for all Agents directly from configMaps and/or secrets |
| datadog.excludePauseContainer | bool | `true` | Exclude pause containers from the Agent Autodiscovery. |
| datadog.hostVolumeMountPropagation | string | `"None"` | Allow to specify the `mountPropagation` value on all volumeMounts using HostPath |
| datadog.ignoreAutoConfig | list | `[]` | List of integration to ignore auto_conf.yaml. |
| datadog.kubeStateMetricsCore.collectSecretMetrics | bool | `true` | Enable watching secret objects and collecting their corresponding metrics kubernetes_state.secret.* |
| datadog.kubeStateMetricsCore.enabled | bool | `false` | Enable the kubernetes_state_core check in the Cluster Agent (Requires Cluster Agent 1.12.0+) |
| datadog.kubeStateMetricsCore.ignoreLegacyKSMCheck | bool | `true` | Disable the auto-configuration of legacy kubernetes_state check (taken into account only when datadog.kubeStateMetricsCore.enabled is true) |
| datadog.kubeStateMetricsEnabled | bool | `true` | If true, deploys the kube-state-metrics deployment |
| datadog.kubeStateMetricsNetworkPolicy.create | bool | `false` | If true, create a NetworkPolicy for kube state metrics |
| datadog.kubelet.agentCAPath | string | /var/run/host-kubelet-ca.crt if hostCAPath else /var/run/secrets/kubernetes.io/serviceaccount/ca.crt | Path (inside Agent containers) where the Kubelet CA certificate is stored |
| datadog.kubelet.host | object | `{"valueFrom":{"fieldRef":{"fieldPath":"status.hostIP"}}}` | Override kubelet IP |
| datadog.kubelet.hostCAPath | string | None (no mount from host) | Path (on host) where the Kubelet CA certificate is stored |
| datadog.kubelet.tlsVerify | string | true | Toggle kubelet TLS verification |
| datadog.leaderElection | bool | `true` | Enables leader election mechanism for event collection |
| datadog.leaderLeaseDuration | string | `nil` | Set the lease time for leader election in second |
| datadog.logLevel | string | `"INFO"` | Set logging verbosity, valid log levels are: trace, debug, info, warn, error, critical, off |
| datadog.logs.containerCollectAll | bool | `false` | Enable this to allow log collection for all containers |
| datadog.logs.containerCollectUsingFiles | bool | `true` | Collect logs from files in /var/log/pods instead of using container runtime API |
| datadog.logs.enabled | bool | `false` | Enables this to activate Datadog Agent log collection |
| datadog.namespaceLabelsAsTags | object | `{}` | Provide a mapping of Kubernetes Namespace Labels to Datadog Tags |
| datadog.networkMonitoring.enabled | bool | `false` | Enable network performance monitoring |
| datadog.networkPolicy.cilium.dnsSelector | object | kube-dns in namespace kube-system | Cilium selector of the DNS server entity |
| datadog.networkPolicy.create | bool | `false` | If true, create NetworkPolicy for all the components |
| datadog.networkPolicy.flavor | string | `"kubernetes"` | Flavor of the network policy to use. Can be: * kubernetes for networking.k8s.io/v1/NetworkPolicy * cilium     for cilium.io/v2/CiliumNetworkPolicy |
| datadog.nodeLabelsAsTags | object | `{}` | Provide a mapping of Kubernetes Node Labels to Datadog Tags |
| datadog.orchestratorExplorer.container_scrubbing | object | `{"enabled":true}` | Enable the scrubbing of containers in the kubernetes resource YAML for sensitive information |
| datadog.orchestratorExplorer.enabled | bool | `true` | Set this to false to disable the orchestrator explorer |
| datadog.podAnnotationsAsTags | object | `{}` | Provide a mapping of Kubernetes Annotations to Datadog Tags |
| datadog.podLabelsAsTags | object | `{}` | Provide a mapping of Kubernetes Labels to Datadog Tags |
| datadog.processAgent.enabled | bool | `true` | Set this to true to enable live process monitoring agent |
| datadog.processAgent.processCollection | bool | `false` | Set this to true to enable process collection in process monitoring agent |
| datadog.processAgent.stripProcessArguments | bool | `false` | Set this to scrub all arguments from collected processes |
| datadog.prometheusScrape.additionalConfigs | list | `[]` | Allows adding advanced openmetrics check configurations with custom discovery rules. (Requires Agent version 7.27+) |
| datadog.prometheusScrape.enabled | bool | `false` | Enable autodiscovering pods and services exposing prometheus metrics. |
| datadog.prometheusScrape.serviceEndpoints | bool | `false` | Enable generating dedicated checks for service endpoints. |
| datadog.securityAgent.compliance.checkInterval | string | `"20m"` | Compliance check run interval |
| datadog.securityAgent.compliance.configMap | string | `nil` | Contains CSPM compliance benchmarks that will be used |
| datadog.securityAgent.compliance.enabled | bool | `false` | Set to true to enable Cloud Security Posture Management (CSPM) |
| datadog.securityAgent.runtime.enabled | bool | `false` | Set to true to enable Cloud Workload Security (CWS) |
| datadog.securityAgent.runtime.policies.configMap | string | `nil` | Contains CWS policies that will be used |
| datadog.securityAgent.runtime.syscallMonitor.enabled | bool | `false` | Set to true to enable the Syscall monitoring (recommended for troubleshooting only) |
| datadog.securityContext | object | `{}` | Allows you to overwrite the default PodSecurityContext on the Daemonset or Deployment |
| datadog.site | string | `nil` | The site of the Datadog intake to send Agent data to |
| datadog.systemProbe.apparmor | string | `"unconfined"` | Specify a apparmor profile for system-probe |
| datadog.systemProbe.bpfDebug | bool | `false` | Enable logging for kernel debug |
| datadog.systemProbe.collectDNSStats | bool | `true` | Enable DNS stat collection |
| datadog.systemProbe.conntrackInitTimeout | string | `"10s"` | the time to wait for conntrack to initialize before failing |
| datadog.systemProbe.conntrackMaxStateSize | int | `131072` | the maximum size of the userspace conntrack cache |
| datadog.systemProbe.debugPort | int | `0` | Specify the port to expose pprof and expvar for system-probe agent |
| datadog.systemProbe.enableConntrack | bool | `true` | Enable the system-probe agent to connect to the netlink/conntrack subsystem to add NAT information to connection data |
| datadog.systemProbe.enableOOMKill | bool | `false` | Enable the OOM kill eBPF-based check |
| datadog.systemProbe.enableTCPQueueLength | bool | `false` | Enable the TCP queue length eBPF-based check |
| datadog.systemProbe.maxTrackedConnections | int | `131072` | the maximum number of tracked connections |
| datadog.systemProbe.seccomp | string | `"localhost/system-probe"` | Apply an ad-hoc seccomp profile to the system-probe agent to restrict its privileges |
| datadog.systemProbe.seccompRoot | string | `"/var/lib/kubelet/seccomp"` | Specify the seccomp profile root directory |
| datadog.tags | list | `[]` | List of static tags to attach to every metric, event and service check collected by this Agent. |
| existingClusterAgent.clusterchecksEnabled | bool | `true` | set this to false if you don’t want the agents to run the cluster checks of the joined external cluster agent |
| existingClusterAgent.join | bool | `false` | set this to true if you want the agents deployed by this chart to connect to a Cluster Agent deployed independently |
| existingClusterAgent.serviceName | string | `nil` | Existing service name to use for reaching the external Cluster Agent |
| existingClusterAgent.tokenSecretName | string | `nil` | Existing secret name to use for external Cluster Agent token |
| fullnameOverride | string | `nil` | Override the full qualified app name |
| kube-state-metrics.nodeSelector | object | `{"kubernetes.io/os":"linux"}` | Node selector for KSM. KSM only supports Linux. |
| kube-state-metrics.rbac.create | bool | `true` | If true, create & use RBAC resources |
| kube-state-metrics.resources | object | `{}` | Resource requests and limits for the kube-state-metrics container. |
| kube-state-metrics.serviceAccount.create | bool | `true` | If true, create ServiceAccount, require rbac kube-state-metrics.rbac.create true |
| kube-state-metrics.serviceAccount.name | string | `nil` | The name of the ServiceAccount to use. |
| nameOverride | string | `nil` | Override name of app |
| providers.gke.autopilot | bool | `false` | Enables Datadog Agent deployment on GKE Autopilot |
| registry | string | `"gcr.io/datadoghq"` | Registry to use for all Agent images (default gcr.io) |
| targetSystem | string | `"linux"` | Target OS for this deployment (possible values: linux, windows) |

## Configuration options for Windows deployments
<a name="windows-config"></a>
Some options above are not working/not available on Windows, here is the list of **unsupported** options:

| Parameter                                | Reason                                           |
|------------------------------------------|--------------------------------------------------|
| `datadog.dogstatsd.useHostPID`           | Host PID not supported by Windows Containers     |
| `datadog.dogstatsd.useSocketVolume`      | Unix sockets not supported on Windows            |
| `datadog.dogstatsd.socketPath`           | Unix sockets not supported on Windows            |
| `datadog.processAgent.processCollection` | Unable to access host/other containers processes |
| `datadog.systemProbe.seccomp`            | System probe is not available for Windows        |
| `datadog.systemProbe.seccompRoot`        | System probe is not available for Windows        |
| `datadog.systemProbe.debugPort`          | System probe is not available for Windows        |
| `datadog.systemProbe.enableConntrack`    | System probe is not available for Windows        |
| `datadog.systemProbe.bpfDebug`           | System probe is not available for Windows        |
| `datadog.systemProbe.apparmor`           | System probe is not available for Windows        |
| `agents.useHostNetwork`                  | Host network not supported by Windows Containers |

### How to join a Cluster Agent from another helm chart deployment (Linux)

Because the Cluster Agent can only be deployed on Linux Node, the communication between
the Agents deployed on the Windows nodes with the a Cluster Agent need to be configured.

The following `datadog-values.yaml` file contains all the parameters needed to configure this communication.

```yaml
targetSystem: windows

existingClusterAgent:
  join: true
  serviceName: "<EXISTING_DCA_SECRET_NAME>" # from the other datadog helm chart release
  tokenSecretName: "<EXISTING_DCA_SERVICE_NAME>" # from the other datadog helm chart release

# Disabled datadogMetrics deployment since it should have been already deployed with the other chart release.
datadog-crds:
  crds:
    datadogMetrics: false

# Disable kube-state-metrics deployment
datadog:
  kubeStateMetricsEnabled: false
```
