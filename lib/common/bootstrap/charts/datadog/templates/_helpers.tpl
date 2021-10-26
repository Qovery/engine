{{/* vim: set filetype=mustache: */}}

{{- define "check-version" -}}
{{- if not .Values.agents.image.doNotCheckTag -}}
{{- $version := .Values.agents.image.tag | toString | trimSuffix "-jmx" -}}
{{- $length := len (split "." $version) -}}
{{- if and (eq $length 1) (eq $version "6") -}}
{{- $version = "6.19.0" -}}
{{- end -}}
{{- if and (eq $length 1) (eq $version "7") -}}
{{- $version = "7.19.0" -}}
{{- end -}}
{{- if and (eq $length 1) (eq $version "latest") -}}
{{- $version = "7.19.0" -}}
{{- end -}}
{{- if not (semverCompare "^6.19.0-0 || ^7.19.0-0" $version) -}}
{{- fail "This version of the chart requires an agent image 7.19.0 or greater. If you want to force and skip this check, use `--set agents.image.doNotCheckTag=true`" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{/*
Check if target cluster is running OpenShift.
*/}}
{{- define "is-openshift" -}}
{{- if .Capabilities.APIVersions.Has "quota.openshift.io/v1/ClusterResourceQuota" -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{- define "agent-has-env-ad" -}}
{{- if not .Values.agents.image.doNotCheckTag -}}
{{- $version := .Values.agents.image.tag | toString | trimSuffix "-jmx" -}}
{{- $length := len (split "." $version) -}}
{{- if and (eq $length 1) (eq $version "6") -}}
{{- $version = "6.27.0" -}}
{{- end -}}
{{- if and (eq $length 1) (eq $version "7") -}}
{{- $version = "7.27.0" -}}
{{- end -}}
{{- if and (eq $length 1) (eq $version "latest") -}}
{{- $version = "7.27.0" -}}
{{- end -}}
{{- if semverCompare "^6.27.0-0 || ^7.27.0-0" $version -}}
true
{{- else -}}
false
{{- end -}}
{{- else -}}
true
{{- end -}}
{{- end -}}

{{- define "check-cluster-name" }}
{{- $length := len .Values.datadog.clusterName -}}
{{- if (gt $length 80)}}
{{- fail "Your `clusterName` isn’t valid it has to be below 81 chars." -}}
{{- end}}
{{- if not (regexMatch "^([a-z]([a-z0-9\\-]*[a-z0-9])?\\.)*([a-z]([a-z0-9\\-]*[a-z0-9])?)$" .Values.datadog.clusterName) -}}
{{- fail "Your `clusterName` isn’t valid. It must be dot-separated tokens where a token start with a lowercase letter followed by lowercase letters, numbers, or hyphens, can only end with a with [a-z0-9] and has to be below 80 chars." -}}
{{- end -}}
{{- end -}}

{{/*
Expand the name of the chart.
*/}}
{{- define "datadog.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
And depending on the resources the name is completed with an extension.
If release name contains chart name it will be used as a full name.
*/}}
{{- define "datadog.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "datadog.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Return secret name to be used based on provided values.
*/}}
{{- define "datadog.apiSecretName" -}}
{{- $fullName := include "datadog.fullname" . -}}
{{- default $fullName .Values.datadog.apiKeyExistingSecret | quote -}}
{{- end -}}

{{/*
Return secret name to be used based on provided values.
*/}}
{{- define "datadog.appKeySecretName" -}}
{{- $fullName := printf "%s-appkey" (include "datadog.fullname" .) -}}
{{- default $fullName .Values.datadog.appKeyExistingSecret | quote -}}
{{- end -}}

{{/*
Return secret name to be used based on provided values.
*/}}
{{- define "clusterAgent.tokenSecretName" -}}
{{- if not .Values.clusterAgent.tokenExistingSecret -}}
{{- include "datadog.fullname" . -}}-cluster-agent
{{- else -}}
{{- .Values.clusterAgent.tokenExistingSecret -}}
{{- end -}}
{{- end -}}

{{/*
Return the appropriate apiVersion for RBAC APIs.
*/}}
{{- define "rbac.apiVersion" -}}
{{- if .Capabilities.APIVersions.Has "rbac.authorization.k8s.io/v1" -}}
"rbac.authorization.k8s.io/v1"
{{- else -}}
"rbac.authorization.k8s.io/v1beta1"
{{- end -}}
{{- end -}}

{{/*
Return the appropriate os label
*/}}
{{- define "label.os" -}}
{{- if semverCompare "^1.14-0" .Capabilities.KubeVersion.GitVersion -}}
kubernetes.io/os
{{- else -}}
beta.kubernetes.io/os
{{- end -}}
{{- end -}}

{{/*
Correct `clusterAgent.metricsProvider.service.port` if Kubernetes <= 1.15
*/}}
{{- define "clusterAgent.metricsProvider.port" -}}
{{- if semverCompare "^1.15-0" .Capabilities.KubeVersion.GitVersion -}}
{{- .Values.clusterAgent.metricsProvider.service.port -}}
{{- else -}}
443
{{- end -}}
{{- end -}}

{{/*
Return the container runtime socket
*/}}
{{- define "datadog.dockerOrCriSocketPath" -}}
{{- if eq .Values.targetSystem "linux" -}}
{{- if .Values.providers.gke.autopilot -}}
/var/run/containerd/containerd.sock
{{- else -}}
{{- .Values.datadog.dockerSocketPath | default .Values.datadog.criSocketPath | default "/var/run/docker.sock" -}}
{{- end -}}
{{- end -}}
{{- if eq .Values.targetSystem "windows" -}}
{{- .Values.datadog.dockerSocketPath | default .Values.datadog.criSocketPath | default `\\.\pipe\docker_engine` -}}
{{- end -}}
{{- end -}}

{{/*
Return agent config path
*/}}
{{- define "datadog.confPath" -}}
{{- if eq .Values.targetSystem "linux" -}}
/etc/datadog-agent
{{- end -}}
{{- if eq .Values.targetSystem "windows" -}}
C:/ProgramData/Datadog
{{- end -}}
{{- end -}}

{{/*
Return agent host mount root
*/}}
{{- define "datadog.hostMountRoot" -}}
{{- if .Values.providers.gke.autopilot -}}
/var/autopilot/addon/datadog
{{- else -}}
/var/lib/datadog-agent
{{- end -}}
{{- end -}}

{{/*
Return true if we are installing on a GKE cluster without RBAC setup (versions older than GKE R26)
*/}}
{{- define "is-gke-without-external-metrics" -}}
{{- if contains "-gke." .Capabilities.KubeVersion.GitVersion -}}
{{- if semverCompare ">=1.17.9-gke.600 || >=1.16.13-gke.1" .Capabilities.KubeVersion.GitVersion -}}
false
{{- else -}}
true
{{- end -}}
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Returns probe definition based on user settings and default HTTP port.
Accepts a map with `port` (default port), `path` (probe handler URI) and `settings` (probe settings).
*/}}
{{- define "probe.http" -}}
{{- if or .settings.httpGet .settings.tcpSocket .settings.exec -}}
{{ toYaml .settings }}
{{- else -}}
{{- $handler := dict "httpGet" (dict "port" .port "path" .path "scheme" "HTTP") -}}
{{ toYaml (merge $handler .settings) }}
{{- end -}}
{{- end -}}

{{/*
Returns probe definition based on user settings and default TCP socket port.
Accepts a map with `port` (default port) and `settings` (probe settings).
*/}}
{{- define "probe.tcp" -}}
{{- if or .settings.httpGet .settings.tcpSocket .settings.exec -}}
{{ toYaml .settings }}
{{- else -}}
{{- $handler := dict "tcpSocket" (dict "port" .port) -}}
{{- toYaml (merge $handler .settings) -}}
{{- end -}}
{{- end -}}

{{/*
Return a remote image path based on `.Values` (passed as root) and `.` (any `.image` from `.Values` passed as parameter)
*/}}
{{- define "image-path" -}}
{{- $tagSuffix := "" -}}
{{- if .image.tagSuffix -}}
{{- $tagSuffix = printf "-%s" .image.tagSuffix -}}
{{- end -}}
{{- if .image.repository -}}
{{- .image.repository -}}:{{ .image.tag }}{{ $tagSuffix }}
{{- else -}}
{{ .root.registry }}/{{ .image.name }}:{{ .image.tag }}{{ $tagSuffix }}
{{- end -}}
{{- end -}}

{{/*
Return true if a system-probe feature is enabled.
*/}}
{{- define "system-probe-feature" -}}
{{- if or .Values.datadog.securityAgent.runtime.enabled .Values.datadog.networkMonitoring.enabled .Values.datadog.systemProbe.enableTCPQueueLength .Values.datadog.systemProbe.enableOOMKill -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if the system-probe container should be created.
*/}}
{{- define "should-enable-system-probe" -}}
{{- if and (not .Values.providers.gke.autopilot) (eq (include "system-probe-feature" .) "true") -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}


{{/*
Return true if a security-agent feature is enabled.
*/}}
{{- define "security-agent-feature" -}}
{{- if or .Values.datadog.securityAgent.compliance.enabled .Values.datadog.securityAgent.runtime.enabled -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if the security-agent container should be created.
*/}}
{{- define "should-enable-security-agent" -}}
{{- if and (not .Values.providers.gke.autopilot) (eq (include "security-agent-feature" .) "true") -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if the compliance features should be enabled.
*/}}
{{- define "should-enable-compliance" -}}
{{- if and (not .Values.providers.gke.autopilot) .Values.datadog.securityAgent.compliance.enabled -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if the runtime security features should be enabled.
*/}}
{{- define "should-enable-runtime-security" -}}
{{- if and (not .Values.providers.gke.autopilot) .Values.datadog.securityAgent.runtime.enabled -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if .Values.existingClusterAgent is fully configured
*/}}
{{- define "existingClusterAgent-configured" -}}
{{- if and .Values.existingClusterAgent.join .Values.existingClusterAgent.serviceName .Values.existingClusterAgent.tokenSecretName -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if the ClusterAgent is enabled
*/}}
{{- define "cluster-agent-enabled" -}}
{{- if or (eq (include "existingClusterAgent-configured" .) "true") .Values.clusterAgent.enabled -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}


{{/*
Return true if the ClusterAgent needs to be deployed
*/}}
{{- define "should-deploy-cluster-agent" -}}
{{- if and .Values.clusterAgent.enabled (not .Values.existingClusterAgent.join) -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}


{{/*
Return true if a trace-agent needs to be deployed.
*/}}
{{- define "should-enable-trace-agent" -}}
{{- if or (eq  (include "trace-agent-use-tcp-port" .) "true") (eq  (include "trace-agent-use-uds" .) "true") -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true hostPath should be use for DSD socket. Return always false on GKE autopilot.
*/}}
{{- define "should-mount-hostPath-for-dsd-socket" -}}
{{- if or .Values.providers.gke.autopilot (eq .Values.targetSystem "windows") -}}
false
{{- end -}}
{{- if .Values.datadog.dogstatsd.useSocketVolume -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if a APM over UDS is configured. Return always false on GKE autopilot.
*/}}
{{- define "trace-agent-use-uds" -}}
{{- if or .Values.providers.gke.autopilot (eq .Values.targetSystem "windows") -}}
false
{{- end -}}
{{- if or .Values.datadog.apm.socketEnabled .Values.datadog.apm.useSocketVolume -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if a traffic over TCP is configured for APM.
*/}}
{{- define "trace-agent-use-tcp-port" -}}
{{- if or .Values.datadog.apm.portEnabled .Values.datadog.apm.enabled -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}


{{/*
Return true if Kubernetes resource monitoring (orchestrator explorer) should be enabled.
*/}}
{{- define "should-enable-k8s-resource-monitoring" -}}
{{- if and .Values.datadog.orchestratorExplorer.enabled (or .Values.clusterAgent.enabled (eq (include "existingClusterAgent-configured" .) "true")) -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Returns provider kind
*/}}
{{- define "provider-kind" -}}
{{- if .Values.providers.gke.autopilot -}}
gke-autopilot
{{- end -}}
{{- end -}}


{{/*
Common template labels
*/}}
{{- define "datadog.template-labels" -}}
app.kubernetes.io/name: "{{ template "datadog.fullname" . }}"
app.kubernetes.io/instance: {{ .Release.Name | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{/*
Common labels
*/}}
{{- define "datadog.labels" -}}
helm.sh/chart: '{{ include "datadog.chart" . }}'
{{ include "datadog.template-labels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
{{- end -}}

{{/*
Returns provider-specific labels if any
*/}}
{{- define "provider-labels" -}}
{{- if include "provider-kind" . -}}
env.datadoghq.com/kind: {{ include "provider-kind" . }}
{{- end -}}
{{- end -}}

{{/*
Returns provider-specific env vars if any
*/}}
{{- define "provider-env" -}}
{{- if include "provider-kind" . -}}
- name: DD_PROVIDER_KIND
  value: {{ include "provider-kind" . }}
{{- end -}}
{{- end -}}

{{/*
Return Kubelet CA path inside Agent containers
*/}}
{{- define "datadog.kubelet.mountPath" -}}
{{- if .Values.datadog.kubelet.agentCAPath -}}
{{- .Values.datadog.kubelet.agentCAPath -}}
{{- else if .Values.datadog.kubelet.hostCAPath -}}
{{- if eq .Values.targetSystem "windows" -}}
C:/var/kubelet-ca/{{ base .Values.datadog.kubelet.hostCAPath }}
{{- else -}}
/var/run/kubelet-ca/{{ base .Values.datadog.kubelet.hostCAPath }}
{{- end -}}
{{- end -}}
{{- end -}}

{{/*
Return Kubelet volumeMount
*/}}
{{- define "datadog.kubelet.volumeMount" -}}
- name: kubelet-ca
  {{- if eq .Values.targetSystem "linux" }}
  mountPath: {{ include "datadog.kubelet.mountPath" . }}
  {{- end }}
  {{- if eq .Values.targetSystem "windows" }}
  mountPath: {{ dir (include "datadog.kubelet.mountPath" .) }}
  {{- end }}
  readOnly: true
{{- end -}}

{{/*
Return true if the Cluster Agent needs a confd configmap
*/}}
{{- define "need-cluster-agent-confd" -}}
{{- if (or (.Values.clusterAgent.confd) (.Values.datadog.kubeStateMetricsCore.enabled)) -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}

{{/*
Return true if we can enable Service Internal Traffic Policy
*/}}
{{- define "enable-service-internal-traffic-policy" -}}
{{- if semverCompare "^1.22-0" .Capabilities.KubeVersion.GitVersion -}}
true
{{- else -}}
false
{{- end -}}
{{- end -}}
