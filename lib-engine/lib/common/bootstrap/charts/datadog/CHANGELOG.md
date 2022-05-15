# Datadog changelog

# 2.22.17

* Update general installation documentation and add how to disable APM.

# 2.22.16

* Support containerd on windows node with logs enabled.

# 2.22.15

* Add a new configuration field `datadog.kubeStateMetricsCore.collectSecretMetrics` to allow disabling the collection of `kubernetes_state.secret.*` metrics by the `kubernetes_state_core` check.

# 2.22.14

* Apply security context capabilities to security-agent only if compliance is enabled.

# 2.22.13

* Add configurable conntrack_init_timeout to sysprobe config.

## 2.22.12

* Replace the `prometheus` check targetting the Datadog Cluster Agent by the new `datadog_cluster_agent` integration. (Requires Datadog Agent 7.31+)

## 2.22.11

* Adds missing configuration option `DD_STRIP_PROCESS_ARGS` for the process agent.

## 2.22.10

* Default Datadog Agent image to `7.31.1`.
* Default Datadog Cluster Agent image to `1.15.1`.

## 2.22.9

* Makes the runtime socket configurable when running on Windows instead of defaulting to `\\.\pipe\docker_engine`.

## 2.22.8

* Add a service with local [internal traffic policy](https://kubernetes.io/docs/concepts/services-networking/service-traffic-policy/) for traces and dogstatsd.
  This works only on Kubernetes 1.22 or more recent. 

## 2.22.7

* Add a default required pod anti-affinity for the cluster agent.

## 2.22.6

* Adds missing configuration option for `DD_KUBERNETES_NAMESPACE_LABELS_AS_TAGS`.

## 2.22.5

* Add support for using `envFrom` on all container definitions.

## 2.22.4

* Cluster Agent: `DD_TAGS` are included even when Datadog is not set as metrics provider.

## 2.22.3

* CiliumNetworkPolicy: Grant access to the agent to ECS container agent via localhost.

## 2.22.2

* Bind mount host /etc/os-release in system probe container.

## 2.22.1

* Fix CiliumNetworkPolicy `port` field.

## 2.22.0

* Default Datadog Agent image to 7.31.0.
* Default Datadog Cluster Agent image to 1.15.0.

## 2.21.5

* Update descriptions for securityAgent configuration.

## 2.21.4

* Fix condition for including `sysprobe-socket-dir` and `sysprobe-config` volume mounts for `agent`.

## 2.21.3

* Default Datadog Agent image to 7.30.1.

## 2.21.2

* Fix Dogstatsd UDS socket configuration with a HostVolume when `useSocketVolume: true`.

## 2.21.1

* Disable by default UDS socket for dogstastd and apm on GKE autopilot.

## 2.21.0

* Enable APM by default with using a Unix Domain socket for communication.

## 2.20.4

* Skip KSM network policy creation when KSM creation is disabled.

## 2.20.3

* Add `agents.image.tagSuffix` and `clusterChecksRunner.image.tagSuffix` to be able to request JMX or Windows servercore images without having to explicitly specify the full version.

## 2.20.2

* Add an additional way to configure cluster check allowing multiple configs for the same check.

## 2.20.1

* Add Statefulsets RBAC rules for the Cluster Agent in order to collect new resources in the Orchestrator Explorer.

## 2.20.0

* Update default Agent image tag to `7.30.0`
* Update default Cluster-Agent image tag to `1.14.0`

## 2.19.9

* Print a configuration notice to clarify the containers filtering behavior when a misconfiguration is detected.

## 2.19.8

* Update `datadog-crds` to `0.3.2`.

## 2.19.7

* Fix test value files in datadog/ci directory.

## 2.19.6

* Update `agent` image tag to `7.29.1`.
* Update `clusterChecksRunner` image tag to `7.29.1`.

## 2.19.5

* Update link toe `kube-state-metrics` in README.md.

## 2.19.4

* Fix `runtimesocket` volumeMount for the `trace-agent` on windows deployment.

## 2.19.3

* Fix condition defining `should-enable-k8s-resource-monitoring`, which toggles the orchestrator explorer feature.

## 2.19.2

* Fix `dsdsocket` volumeMount for the `trace-agent` on windows deployment.

## 2.19.1

* Fix chart release process after updating the `kube-state-metrics` chart registry.

## 2.19.0

* Move to the new `kube-state-metrics` chart registry, but keep the version `2.13.2`.

## 2.18.2

* Update `kube-state-metrics` requirement chart documentation.
* Add missing `DD_TAGS` envvar in `cluster-agent` deployment (Fix #304).

## 2.18.1

* Honor `doNotCheckTag` in Env AD detection, preventing install failures with custom images using non semver tags.

## 2.18.0

* Configure and activate the Dogstatsd UDS socket in an "emptyDir" volume by default. It will allow JMX-Fetch to use UDS by default.

## 2.17.1

* Update `cluster-agent` image tag to `1.13.1`.

## 2.17.0

* Update `agent` image tag to `7.29.0`.
* Update `cluster-agent` image tag to `1.13.0`.

## 2.16.6

* Support template expansion for `clusterAgent.podAnnotations`
* Support template expansion for `clusterAgent.rbac.serviceAccountAnnotations`

## 2.16.5

* Remove other way of detecting OpenShift cluster as it's not supported by Helm2.

## 2.16.4

* Rename the `Role` and `RoleBinding` of the Datadog Cluster Agent to avoid edge cases where `helm upgrade` can fail because of object name conflict.

## 2.16.3

* Add Daemonsets RBAC rules for the Cluster Agent in order to collect new resources in the Orchestrator Explorer.

## 2.16.2

* Document Autodiscovery management parameters: `datadog.containerExclude`, `datadog.containerInclude`, `datadog.containerExcludeMetrics`, `datadog.containerIncludeMetrics`, `datadog.containerExcludeLogs` and `datadog.containerIncludeLogs`.
* Introduce `datadog.includePauseContainer` to control autodiscovery of pause containers.
* Introduce a deprecation noticed for the undocumented and long deprecated `datadog.acInclude` and `datadog.acExclude`.

## 2.16.1

* Use the pod name as cluster check runner ID to allow deploying multiple cluster check runners on the same node. (Requires agent 7.27.0+)

## 2.16.0

* Always mount `/var/log/containers` for the Datadog Agent to better handle logs file scanning with short-lived containers. (See [datadog-agent#8143](https://github.com/DataDog/datadog-agent/pull/8143))

## 2.15.6

* Set `GODEBUG=x509ignoreCN=0` to revert Agent SSL certificates validation to behaviour to Golang <= 1.14. Notably it fixes issues with Kubelet certificates on AKS with Agent >= 7.28.

## 2.15.5

* Add RBAC rules for the Cluster Agent in order to collect new resources in the Orchestrator Explorer.

## 2.15.4

* Bump Agent version to `7.28.1`.

## 2.15.3

* Fix Cilium network policies.

## 2.15.2

* OpenShift: Automatically use built-in SCCs instead of failing if create SCC option is not used

## 2.15.1

* Add parameter `clusterAgent.rbac.serviceAccountAnnotations` for specifying annotations for dedicated ServiceAccount for Cluster Agent.
* Add parameter `agents.rbac.serviceAccountAnnotations` for specifying annotations for dedicated ServiceAccount for Agents.
* Support template expansion for `agents.podAnnotations`

## 2.15.0

* Bump Agent version to `7.28.0`.

## 2.14.0

* Improve resources labels with kubermetes/helm standard labels.

## 2.13.3

* Add `datadog.checksCardinality` field to configure `DD_CHECKS_TAG_CARDINALITY`.
* Add a reminder to set the `datadog.site` field if needed.

## 2.13.2

* Fix `YAML parse error on datadog/templates/daemonset.yaml` when autopilot is enabled.
* Fix "README.md" generation.

## 2.13.1

* Fix Kubelet connection on GKE-autopilot environment: force `http` endpoint to retrieves pods information.

## 2.13.0

* Update `kube-state-metrics` chart version to `2.13.2` that include `kubernetes/kube-state-metrics#1442` fix for `helm2`.

## 2.12.4

* Fix missing namespaces in chart templates

## 2.12.3

* Added `datadog.ignoreAutoConfig` config option to ignore `auto_conf.yaml` configurations.

## 2.12.2

* The Datadog Cluster Agent's Admission Controller now uses a `Role` to watch secrets instead of a `ClusterRole`. (Requires Datadog Cluster Agent v1.12+)

## 2.12.1

* Add more kube-state-metrics core check documentation

## 2.12.0

* Update the Cluster Agent version to `1.12.0`
* Support kube-state-metrics core check (Requires Datadog Cluster Agent v1.12+)

## 2.11.6

* Improve support for environment autodiscovery by removing explicit setting of `DOCKER_HOST` by default with Agent 7.27+.
Starting Agent 7.27, the recommended setup is to never set `datadog.dockerSocketPath` or `datadog.criSocketPath`, except if your setup is using non-standard paths.

## 2.11.5

* Remove comment in the `seccomp` json profile, which is break the json parsing.

## 2.11.4

* Add missing system calls to system-probe `seccomp` profile.

## 2.11.3

* Update the documentation with the new path of the `kube-state-metrics` chart

## 2.11.2

* Update `agent.customAgentConfig` config example in the `values.yaml`: removes reference to APM configuration.

## 2.11.1

* Enable `collectDNSStats` by default

## 2.11.0

* Bump Agent version to `7.27.0`.
* Support configuring advanced openmetrics check parameters via `datadog.prometheusScrape.additionalConfigs`.

## 2.10.14

* Add Kubelet `hostCAPath` and `agentCAPath` parameters to automatically mount and use CA cert from host filesystem for Kubelet connection.
* Fix default value for DCA hostNetwork

## 2.10.13

* Fix `security-agent-feature` helper function to support `helm2`.
* Fix `provider-labels` helper function to support `helm2`.
* Fix `provider-env` helper function to support `helm2`.

## 2.10.12

* Add the possibility to specify securityContext for cluster-agent containers

## 2.10.11

* Fix RBAC needed for the external metrics provider for the future release of the DCA.

## 2.10.10

* Fix system-probe version check when using `datadog.networkMonitoring.enabled`

## 2.10.9

* Add the possibility to specify a priority class name for the cluster checks runner pods.

## 2.10.8

* When node agents are joining an existing DCA managed by another Helm release, we must control if they should be eligible to cluster checks dispatch or not depending on whether CLC have been deployed with the external DCA.

## 2.10.7

* Fix bug regarding using "Metric collection with Prometheus annotations".

## 2.10.6

* Add provider labels on pods, warning on dogstatsd with UDS on GKE Autopilot.

## 2.10.5

* Increase default `datadog.systemProbe.maxTrackedConnections` to 131072.

## 2.10.4

* Fix several bugs with OpenShift SCC and hostNetwork.

## 2.10.3

* Bump version of KSM chart to get rid of `rbac.authorization.k8s.io/v1beta1 ClusterRole is deprecated in v1.17+, unavailable in v1.22+; use rbac.authorization.k8s.io/v1` warnings

## 2.10.2

* Use an EmptyDir volume shared between all the agents for logs so that `agent flare` can gather the logs of all of them.

## 2.10.1

* Remove the cluster-id configmap mount for process-agent. (Requires Datadog Agent 7.25+ and Datadog Cluster Agent 1.11+, otherwise collection of pods for the Kubernetes Resources page will fail).

## 2.10.0

* Remove the cluster-id configmap mount for process-agent. (Requires Datadog Agent 7.26+ and Datadog Cluster Agent 1.11+, otherwise collection of pods for the Kubernetes Resources page will fail).

## 2.9.11

* Allow system-probe container to send flares by adding main agent config file to container.

## 2.9.10

* Support configuring Prometheus Autodiscovery. (Requires Datadog Agent 7/6.26+ and Datadog Cluster Agent 1.11+).

## 2.9.9

* Update "agent" image tag to `7.26.0` and "cluster-agent" to `1.11.0`.
* Fix nit comments

## 2.9.8

* Make pod collection for the Kubernetes Explorer work with an external Cluster Agent deployment.

## 2.9.7

* Allow cluster-agent to override metrics provider endpoint with `clusterAgent.metricsProvider.endpoint`.

## 2.9.6

* Add missing `NET_RAW` capability to `System-probe` to support `CVE-2020-14386` mitigation.

## 2.9.5

* Fix typo in variable name. `agents.podSecurity.capabilities` replaces `agents.podSecurity.capabilites`.

## 2.9.4

* Remove uses of `systemProbe.enabled`.

## 2.9.3

* Enable support for GKE Autopilot.

## 2.9.2

* Fixed a bug where `datadog.leaderElection` would not configure the cluster-agent environment variable `DD_LEADER_ELECTION` correctly.

## 2.9.1

* add `datadog.systemProbe.conntrackMaxStateSize` and  `datadog.systemProbe.maxTrackedConnections`.

## 2.9.0

* Remove `systemProbe.enabled` config param in favor of `networkMonitoring.enabled`, `securityAgent.runtime.enabled`, `systemProbe.enableOOMKill`, and `systemProbe.enableTCPQueueLength`.
* Fix bug preventing network monitoring to be disabled by setting `datadog.networkMonitoring.enabled` to `false`.

## 2.8.6

* Add support for Service Topology to target the Datadog Agent via a kubernetes service instead of host ports. This will allow sending traces and custom metrics without using host ports. Note: Service Topology is a new Kubernetes feature, it's still in alpha and disabled by default.

## 2.8.5

* Allow `namespaces` in RBAC for `kubernetes_namespace_labels_as_tags`.

## 2.8.4

* Grant access to the `Lease` objects.
  `Lease` objects can be read by the `kube_scheduler` and `kube_controller_manager` checks on agent 7.27+ on Kubernetes clusters 1.14+.

## 2.8.3

* Fix potential duplicate `DD_KUBERNETES_KUBELET_TLS_VERIFY` env var due to new parameter `kubelet.tlsVerify`. Parameter has now 3 states and env var won't be added if not set, improving backward compatibility.
* Fix activation of Cluster Checks while Cluster Agent is disabled.
* Change default value for `clusterAgent.metricsProvider.useDatadogMetrics` from `true` to `false` as it may trigger CRD ownership issues in several situations.

## 2.8.2

* Open port 5000/TCP for ingress on cluster agent for Prometheus check from the agent.

## 2.8.1

* Fix `datadog.kubelet.tlsVerify` value when set to `false`

## 2.8.0

* Enable the orchestrator explorer by default.

## 2.7.2

* Add a new fields `datadog.kubelet.host` (to override `DD_KUBERNETES_KUBELET_HOST`) and `datadog.kubelet.tlsVerify` (to toggle kubelet TLS verification)

## 2.7.1

* Open port 8000/TCP for ingress on cluster agent for Admission Controller communication.

## 2.7.0

* Changes default values to activate a maximum of built-in features to ease configuration.
  Notable changes:
  - Cluster Agent, cluster checks and event collection are activated by default
  - DatadogMetrics CRD usage is activated by default if ExternalMetrics are used
  - Dogstatsd non-local traffic is activated by default (hostPort usage is not)
* Bump Agent version to `7.25.0` and Cluster Agent version to `1.10.0`
* Introduce `.registry` parameter to quickly change registry for all Datadog images. Image name is retrieved from `.image.name`, however setting `.image.repository` still allows to override per image, ensuring backward compatibility

## 2.6.15

* Add `ports` options to all Agent containers to allow users to add any binding they'd like for integrations

## 2.6.14

* Opens port 6443/TCP on kube-state-metrics netpol.

## 2.6.13

* Opens ports 6443/TCP and 53/UDP for egress on cluster agent.
* Adds PodSecurityPolicy support for Cluster Agents.

## 2.6.12

* Mount `/etc/passwd` as `readOnly` in the `process-agent`.

## 2.6.11

* Adds `unconfined` as a default value for `agents.podSecurity.apparmorProfiles`. It now aligns with `datadog.systemProbe.apparmor` default value.
* Updates `hostPID` for PodSecurityPolicy, bringing it in line with SCC.

## 2.6.10

* Allow cluster-agent to access apps/daemonsets when admissionController is enabled.

## 2.6.9

* Add `/tmp` in Agent POD as an emptyDir to allow VOLUME removal from Agent Dockerfile
* Clarify documentation of `datadog.dogstatsd.nonLocalTraffic`

## 2.6.8

* Fix `helm lint` by renaming YAML files lacking metadata info.

## 2.6.7

* Change the default agent version to `7.24.1`

## 2.6.6

* Add `agents.containers.systemProbe.securityContext` option.

## 2.6.5

* Make sure all agents are rolled out on API key update and the Cluster agents on Application key update.

## 2.6.4

* Fix agent container volumeMounts when oom kill check or tcp queue length check is enabled.

## 2.6.3

* Add a new field `datadog.dogstatsd.tags` to configure `DD_DOGSTATSD_TAGS`.

## 2.6.2

* Make sure KSM deploys on Linux nodes

## 2.6.1

* Fix `process-agent` and `trace-agent` communication with the `cluster-agent`: When the `cluster-agent` is activated,
  the agents should communicated with the `cluster-agent` to retrived tags like `kube_service` instead of communicating
  directly with the Kubernetes API-Server.

## 2.6.0

* deprecates `systemProbe.enabled` in favor of `networkMonitoring.enabled`, `securityAgent.runtime.enabled`, `systemProbe.enableOOMKill`, and `systemProbe.enableTCPQueueLength`.
* fixes a bug where network performance monitoring would be enabled if any systemProbe feature was enabled.

## 2.5.5

* Add CiliumNetworkPolicy

## 2.5.4

* Supports `clusterChecksRunner` pod annotations

## 2.5.3

* Add "datadog-crds" chart as dependency. It is used to install the `DatadogMetrics` CRD if needed.

## 2.5.2

* Change `datadog.tags` to a `tpl` value

## 2.5.0

* Use `gcr.io` instead of Dockerhub
* Change the default agent version `7.23.1`
* Change the default cluster agent version `1.9.1`
* Change the default cluster checks runner version `7.23.1`

## 2.4.39

* Fixed a bug where `networkMonitoring.enabled` would not configure the process-agent correctly, causing network data to not be reported.

## 2.4.38

* Move the kube-state-metrics subchart from google's helm registry to charts.helm.sh/stable.

## 2.4.37

* Fix incorrect link for Event Collection in `values.yaml`.

## 2.4.36

* Fix `should-enable-system-probe` helper function to support `helm2`.

## 2.4.35

* Add options to set pod and container securityContext

## 2.4.34

* Add `datadog.networkMonitoring` section to allow the system-probe to be run without network performance monitoring. Deprecates `systemProbe.enabled`.

## 2.4.33

* Introduce overall cluster-name limit of 80
* Remove character limit of single parts of the cluster-name

## 2.4.32

* The `agents.volumeMounts` option is now properly propagated to all agent containers.

## 2.4.31

* Support adding labels to the Agent pods and daemonset via `agents.additionalLabels`.
* Support adding labels to the Cluster Agent pods and deployment via `clusterAgent.additionalLabels`.
* Support adding labels to the Cluster Checks Runner pods and deployment via `clusterChecksRunner.additionalLabels`.

## 2.4.30

* Refactor liveness and readiness probes with helpers to allow user overrides with other types of probes or disabling
  probes entirely.
* Introduce `clusterChecksRunner.healthPort` default setting.
* Use health port defaults instead of hardcoded values.

## 2.4.29

* Add `common-env-vars` to `system-probe` container

## 2.4.28

* Make sure we rollout Agent/CLC/DCA when an upgrade is done (thus triggering a change in token secret)

## 2.4.27

* Remove port defaults from liveness/readiness probes and show error notices on misconfiguration if user overrides are supplying custom node settings.

## 2.4.26

* Revert to Helm2 hash in `requirements.yaml` to retain compatibility with Helm 2

## 2.4.25

* Update default `datadog/agent` image tag to `7.23.0`
* Update default `datadog/cluster-agent` image tag to `1.9.0`

## 2.4.24

* Fix the Cluster Agent's network policy (allow ingress from node Agents)
* Add kube-state-metrics network policy

## 2.4.23

* Add `datadog.envFrom` parameter to support passing references to secrets and/or configmaps for environment
variables, instead of passing one by one.

## 2.4.22

* Add automatic README.md generation from `Values.yaml`

## 2.4.21

* Change `securityContext` variable name to `seLinuxContext` allow setting the PSP/SCC seLinux `type` or `rule`. Backward compatible.

## 2.4.20

* Add NetworkPolicy ingress rules for dogstatsd and APM

## 2.4.19

* Add NetworkPolicy
  Add the following parameters to control the creation of NetworkPolicy:
  * `agents.networkPolicy.create`
  * `clusterAgent.networkPolicy.create`
  * `clusterChecksRunner.networkPolicy.create`
  The NetworkPolicy managed by the Helm chart are designed to work out-of-the-box on most setups.
  In particular, the agents need to connect to the datadog intakes. NetworkPolicy can be restricted
  by IP but the datadog intake IP cannot be guaranteed to be stable.
  The agents are also susceptible to connect to any pod, on any port, depending on the "auto-discovery" annotations
  that can be dynamically added to them.

## 2.4.18

* Fix `config` volume not being mounted in clusterChecksRunner pods.

## 2.4.17

* Update default `Agent` and `Cluster-Agent` image tags: `7.22` and `1.18`.

## 2.4.16

* Add `External Metric` Aggregator config on Chart.

## 2.4.15

* Add `agents.podSecurity.apparmor.enabled` flag (defaulted to `true`).

## 2.4.14

* Fix external metrics on GKE due to Google fix on recent versions (introduced in 2.4.1).

## 2.4.13

* fix Agent `PodSecurityPolicy` with `hostPorts` definition, and missing RBAC.

## 2.4.12

* Add `compliance` and `runtime` `security-agent` support.

## 2.4.11

* Add `NET_BROADCAST` capability for `system-probe`.

## 2.4.10

* Add `scrubbing` option for helm charts to "Orchestrator Explorer" support.

## 2.4.9

* Add `DD_DOGSTATSD_TAG_CARDINALITY` capability.

## 2.4.8

* Fix, Only try to mount `/lib/modules` and `/usr/src` when needed.

## 2.4.7

* Add `eventfd` and `eventfd2` to allowed syscalls for `system-probe`.

## 2.4.6

* Fix Windows deployment support (fixes #15).

## 2.4.5

* Add mount propagation option for `hostVolumes`.

## 2.4.4

* Fix typo in `allowHostPorts`.
* Add support of `MustRunAs` in Agent `PodSecurityPolicy` and `SecurityContextConstraints`.

## 2.4.3

* Fix `Cluster-Agent` RBAC to collect new resources for the "Orchestrator Explorer" support.

## 2.4.2

* Add `install_info` file.

## 2.4.1

* Fix MetricsProvider RBAC setup on GKE clusters

## 2.4.0

* First release on github.com/datadog/helm-charts

## 2.3.41

* Fix issue with Kubernetes <= 1.14 and Cluster Agent's External Metrics Provider (must be 443)

## 2.3.40

* Update documentation for resource requests & limits default values.

## 2.3.39

* Propagate `datadog.checksd` to the clusterchecks runner to support custom checks there.

## 2.3.38

* Add support of DD\_CONTAINER\_{INCLUDE,EXCLUDE}\_{METRICS,LOGS}

## 2.3.37

* Add NET\_BROADCAST capability

## 2.3.36

* Bump default Agent version to `7.21.1`

## 2.3.35

* Add support for configuring the Datadog Admission Controller

## 2.3.34

* Add support for scaling based on `DatadogMetric` CRD

## 2.3.33

* Create new `datadog.podSecurity.securityContext` field to fix windows agent daemonset config.

## 2.3.32

* Always add os in nodeSelector based on `targetSystem`

## 2.3.31

* Fixed daemonset template for go 1.14

## 2.3.29

* Change the default port for the Cluster Agent's External Metrics Provider
  from 443 to 8443.
* Document usage of `clusterAgent.env`

## 2.3.28

* fix daemonset template generation if `datadog.securityContext` is set to `nil`

## 2.3.27

* add systemProbe.collectDNSStats option

## 2.3.26

* fix PodSecurityContext configuration

## 2.3.25

* Use directly .env var YAML block for all agents (was already the case for Cluster Agent)

## 2.3.24

* Allow enabling Orchestrator Explorer data collection from the process-agent

## 2.3.23

* Add the possibility to create a `PodSecurityPolicy` or a `SecurityContextConstraints` (Openshift) for the Agent's Daemonset Pods.

## 2.3.22

* Remove duplicate imagePullSecrets
* Fix DataDog location to useConfigMap in docs
* Adding explanation for metricsProvider.enabled

## 2.3.21

* Fix additional default values in `values.yaml` to prevent errors with Helm 2.x

## 2.3.20

* Fix process-agent <> system-probe communication

## 2.3.19

* Fix the container-trace-agent.yaml template creates invalid yaml when  `useSocketVolume` is enabled.

## 2.3.18

* Support arguments in the cluster-agent container `command` value

## 2.3.17

* grammar edits to datadog helm docs!
* Typo in log config

## 2.3.16

* Add parameter `clusterChecksRunner.rbac.serviceAccountAnnotations` for specifying annotations for dedicated ServiceAccount for Cluster Checks runners.
* Add parameters `clusterChecksRunner.volumes` and `clusterChecksRunner.volumeMounts` that can be used for providing a secret backend to Cluster Checks runners.

## 2.3.15

* Mount kernel headers in system-probe container
* Fix the mount of the `system-probe` socket in core agent
* Add parameters to enable eBPF based checks

## 2.3.14

* Allow overriding the `command` to run in the cluster-agent container

## 2.3.13

* Use two distinct health endpoints for liveness and readiness probes.

## 2.3.12

* Fix endpoints checks scheduling between agent and cluster check runners
* Cluster Check Runner now runs without s6 (similar to other agents)

## 2.3.11

* Bump the default version of the agent docker images

## 2.3.10

* Add dnsConfig options to all containers

## 2.3.9

* Add `clusterAgent.podLabels` variable to add labels to the Cluster Agent Pod(s)

## 2.3.8

* Fix templating errors when `clusterAgent.datadog_cluster_yaml` is being used.

## 2.3.7

* Fix an agent warning at startup because of a deprecated parameter

## 2.3.6

* Add `affinity` parameter in `values.yaml` for cluster agent deployment

## 2.3.5

* Add `DD_AC_INCLUDE` and `DD_AC_EXCLUDE` to all containers
* Add "Unix Domain Socket" support in trace-agent
* Add new parameter to specify the dogstatsd socket path on the host
* Fix typos in values.yaml
* Update "tags:" example in values.yaml
* Add "rate_limit_queries_*" in the datadog.cluster-agent prometheus check configuration

## 2.3.4

* Fix default values in `values.yaml` to prevent warnings with Helm 2.x

## 2.3.3

* Allow pre-release versions as docker image tag

## 2.3.2

* Update the DCA RBAC to allow it to create events in the HPA

## 2.3.1

* Update the example for `datadog.securityContext`

## 2.3.0

* Mount the directory containing the CRI socket instead of the socket itself
  This is to handle the cases where the docker daemon is restarted.
  In this case, the docker daemon will recreate its docker socket and,
  if the container bind-mounted directly the socket, the container would
  still have access to the old socket instead of the one of the new docker
  daemon.
  ⚠ This version of the chart requires an agent image 7.19.0 or more recent

## 2.2.12

* Adding resources for `system-probe` init container

## 2.2.11

* Add documentations around secret management in the datadog helm chart. It is to upstream
  requested changes in the IBM charts repository: https://github.com/IBM/charts/pull/690#discussion_r411702458
* update `kube-state-metrics` dependency
* uncomment every values.yaml parameters for IBM chart compliancy

## 2.2.10

* Remove `kubeStateMetrics` section from `values.yaml` as not used anymore

## 2.2.9

* Fixing variables description in README and Migration documentation (#22031)
* Avoid volumes mount conflict between `system-probe` and `logs` volumes in the `agent`.

## 2.2.8

* Mount `system-probe` socket in `agent` container when system-probe is enabled

## 2.2.7

* Add "Cluster-Agent" `Event` `create` RBAC permission

## 2.2.6

* Ensure the `trace-agent` computes the same hostname as the core `agent`.
  by giving it access to all the elements that might be used to compute the hostname:
  the `DD_CLUSTER_NAME` environment variable and the docker socket.

## 2.2.5

* Fix RBAC

## 2.2.4

* Move several EnvVars to `common-env-vars` to be accessible by the `trace-agent` #21991.
* Fix discrepancies migration-guide and readme reporded in #21806 and #21920.
* Fix EnvVars with integer value due to yaml. serialization, reported by #21853.
* Fix .Values.datadog.tags encoding, reported by #21663.
* Add Checksum to `xxx-cluster-agent-config` config map, reported by #21622 and contribution #21656.

## 2.2.3

* Fix `datadog.dockerOrCriSocketPath` helper #21992

## 2.2.2

* Fix indentation for `clusterAgent.volumes`.

## 2.2.1

* Updating `agents.useConfigMap` and `agents.customAgentConfig` parameter descriptions in the chart and main readme.

## 2.2.0

* Add Windows support
* Update documentation to reflect some changes that were made default
* Enable endpoint checks by default in DCA/Agent

## 2.1.2

* Fixed a bug where `DD_LEADER_ELECTION` was not set in the config init container, leading to a failure to adapt
config to this environment variable.

## 2.1.1

* Add option to enable WPA in the Cluster Agent.

## 2.1.0

* Changed the default for `processAgent.enabled` to `true`.

## 2.0.14

* Fixed a bug where the `trace-agent` runs in the same container as `dd-agent`

## 2.0.13

* Fix `system-probe` startup on latest versions of containerd.
  Here is the error that this change fixes:
  ```    State:          Waiting
      Reason:       CrashLoopBackOff
    Last State:     Terminated
      Reason:       StartError
      Message:      failed to create containerd task: OCI runtime create failed: container_linux.go:349: starting container process caused "close exec fds: ensure /proc/self/fd is on procfs: operation not permitted": unknown
      Exit Code:    128
   ```

## 2.0.11

* Add missing syscalls in the `system-probe` seccomp profile

## 2.0.10

* Do not enable the `cri` check when running on a `docker` setup.

## 2.0.7

* Pass expected `DD_DOGSTATSD_PORT` to datadog-agent rather than invalid `DD_DOGSTATD_PORT`

## 2.0.6

* Introduces `procesAgent.processCollection` to correctly configure `DD_PROCESS_AGENT_ENABLED` for the process agent.

## 2.0.5

* Honor the `datadog.env` parameter in all containers.

## 2.0.4

* Honor the image pull policy in init containers.
* Pass the `DD_CRI_SOCKET_PATH` environment variable to the config init container so that it can adapt the agent config based on the CRI.

## 2.0.3

* Fix templating error when `agents.useConfigMap` is set to true.
* Add DD\_APM\_ENABLED environment variable to trace agent container.

## 2.0.2

* Revert the docker socket path inside the agent container to its standard location to fix #21223.

## 2.0.1

* Add parameters `datadog.logs.enabled` and `datadog.logs.containerCollectAll` to replace `datadog.logsEnabled` and `datadog.logsConfigContainerCollectAll`.
* Update the migration document link in the `Readme.md`.

### 2.0.0

* Remove Datadog agent deployment configuration.
* Cleanup resources labels, to fit with recommended labels.
* Cleanup useless or unused values parameters.
* each component have its own RBAC configuration (create,configuration).
* container runtime socket update values configuration simplification.
* `nameOverride` `fullnameOverride` is now optional in values.yaml.
