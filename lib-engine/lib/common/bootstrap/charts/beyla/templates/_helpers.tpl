{{/*
Expand the name of the chart.
*/}}
{{- define "beyla.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "beyla.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Allow the release namespace to be overridden for multi-namespace deployments in combined charts
*/}}
{{- define "beyla.namespace" -}}
{{- if .Values.namespaceOverride }}
{{- .Values.namespaceOverride }}
{{- else }}
{{- .Release.Namespace }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "beyla.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "beyla.labels" -}}
helm.sh/chart: {{ include "beyla.chart" . }}
{{ include "beyla.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: beyla
{{- end }}

{{/*
Selector (pod) labels
*/}}
{{- define "beyla.selectorLabels" -}}
app.kubernetes.io/name: {{ include "beyla.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- with .Values.podLabels }}
{{ toYaml . }}
{{- end }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "beyla.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "beyla.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Calculate name of image ID to use for "beyla".
*/}}
{{- define "beyla.imageId" -}}
{{- if .Values.image.digest }}
{{- $digest := .Values.image.digest }}
{{- if not (hasPrefix "sha256:" $digest) }}
{{- $digest = printf "sha256:%s" $digest }}
{{- end }}
{{- printf "@%s" $digest }}
{{- else if .Values.image.tag }}
{{- printf ":%s" .Values.image.tag }}
{{- else }}
{{- printf ":%s" .Chart.AppVersion }}
{{- end }}
{{- end }}

{{/*
Calculate name of image ID to use for "beyla-cache".
*/}}
{{- define "beyla.k8sCache.imageId" -}}
{{- if .Values.k8sCache.image.digest }}
{{- $digest := .Values.k8sCache.image.digest }}
{{- if not (hasPrefix "sha256:" $digest) }}
{{- $digest = printf "sha256:%s" $digest }}
{{- end }}
{{- printf "@%s" $digest }}
{{- else if .Values.k8sCache.image.tag }}
{{- printf ":%s" .Values.k8sCache.image.tag }}
{{- else }}
{{- printf ":%s" .Chart.AppVersion }}
{{- end }}
{{- end }}

{{/*
Common kube cache labels
*/}}
{{- define "beyla.cache.labels" -}}
helm.sh/chart: {{ include "beyla.chart" . }}
{{ include "beyla.cache.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: beyla
{{- end }}

{{/*
Selector (pod) labels
*/}}
{{- define "beyla.cache.selectorLabels" -}}
app.kubernetes.io/name: {{ .Values.k8sCache.service.name }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- with .Values.k8sCache.podLabels }}
{{ toYaml . }}
{{- end }}
{{- end }}