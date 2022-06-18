{{/* vim: set filetype=mustache: */}}

{{/*
Expand the name of the chart.
*/}}
{{- define "aws-node-termination-handler.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "aws-node-termination-handler.fullname" -}}
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
Equivalent to "aws-node-termination-handler.fullname" except that "-win" indicator is appended to the end.
Name will not exceed 63 characters.
*/}}
{{- define "aws-node-termination-handler.fullnameWindows" -}}
{{- include "aws-node-termination-handler.fullname" . | trunc 59 | trimSuffix "-" | printf "%s-win" -}}
{{- end -}}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "aws-node-termination-handler.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Common labels
*/}}
{{- define "aws-node-termination-handler.labels" -}}
{{ include "aws-node-termination-handler.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/part-of: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ include "aws-node-termination-handler.chart" . }}
{{- with .Values.customLabels }}
{{ toYaml . }}
{{- end }}
{{- end -}}

{{/*
Deployment labels
*/}}
{{- define "aws-node-termination-handler.labelsDeployment" -}}
{{ include "aws-node-termination-handler.labels" . }}
app.kubernetes.io/component: deployment
{{- end -}}

{{/*
Daemonset labels
*/}}
{{- define "aws-node-termination-handler.labelsDaemonset" -}}
{{ include "aws-node-termination-handler.labels" . }}
app.kubernetes.io/component: daemonset
{{- end -}}

{{/*
Selector labels
*/}}
{{- define "aws-node-termination-handler.selectorLabels" -}}
app.kubernetes.io/name: {{ include "aws-node-termination-handler.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/*
Selector labels for the deployment
*/}}
{{- define "aws-node-termination-handler.selectorLabelsDeployment" -}}
{{ include "aws-node-termination-handler.selectorLabels" . }}
app.kubernetes.io/component: deployment
{{- end -}}

{{/*
Selector labels for the daemonset
*/}}
{{- define "aws-node-termination-handler.selectorLabelsDaemonset" -}}
{{ include "aws-node-termination-handler.selectorLabels" . }}
app.kubernetes.io/component: daemonset
{{- end -}}

{{/*
Create the name of the service account to use
*/}}
{{- define "aws-node-termination-handler.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
    {{ default (include "aws-node-termination-handler.fullname" .) .Values.serviceAccount.name }}
{{- else -}}
    {{ default "default" .Values.serviceAccount.name }}
{{- end -}}
{{- end -}}

{{/*
The image to use
*/}}
{{- define "aws-node-termination-handler.image" -}}
{{- printf "%s:%s" .Values.image.repository (default (printf "v%s" .Chart.AppVersion) .Values.image.tag) }}
{{- end }}

{{/* Get PodDisruptionBudget API Version */}}
{{- define "aws-node-termination-handler.pdb.apiVersion" -}}
  {{- if and (.Capabilities.APIVersions.Has "policy/v1") (semverCompare ">= 1.21-0" .Capabilities.KubeVersion.Version) -}}
      {{- print "policy/v1" -}}
  {{- else -}}
    {{- print "policy/v1beta1" -}}
  {{- end -}}
{{- end -}}
