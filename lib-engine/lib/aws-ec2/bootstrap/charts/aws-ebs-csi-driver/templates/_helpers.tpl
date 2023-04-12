{{/* vim: set filetype=mustache: */}}
{{/*
Expand the name of the chart.
*/}}
{{- define "aws-ebs-csi-driver.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "aws-ebs-csi-driver.fullname" -}}
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
{{- define "aws-ebs-csi-driver.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Common labels
*/}}
{{- define "aws-ebs-csi-driver.labels" -}}
{{ include "aws-ebs-csi-driver.selectorLabels" . }}
{{- if ne .Release.Name "kustomize" }}
helm.sh/chart: {{ include "aws-ebs-csi-driver.chart" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/component: csi-driver
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}
{{- if .Values.customLabels }}
{{ toYaml .Values.customLabels }}
{{- end }}
{{- end -}}

{{/*
Common selector labels
*/}}
{{- define "aws-ebs-csi-driver.selectorLabels" -}}
app.kubernetes.io/name: {{ include "aws-ebs-csi-driver.name" . }}
{{- if ne .Release.Name "kustomize" }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}
{{- end -}}

{{/*
Convert the `--extra-tags` command line arg from a map.
*/}}
{{- define "aws-ebs-csi-driver.extra-volume-tags" -}}
{{- $result := dict "pairs" (list) -}}
{{- range $key, $value := .Values.controller.extraVolumeTags -}}
{{- $noop := printf "%s=%v" $key $value | append $result.pairs | set $result "pairs" -}}
{{- end -}}
{{- if gt (len $result.pairs) 0 -}}
{{- printf "- \"--extra-tags=%s\"" (join "," $result.pairs) -}}
{{- end -}}
{{- end -}}

{{/*
Handle http proxy env vars
*/}}
{{- define "aws-ebs-csi-driver.http-proxy" -}}
- name: HTTP_PROXY
  value: {{ .Values.proxy.http_proxy | quote }}
- name: HTTPS_PROXY
  value: {{ .Values.proxy.http_proxy | quote }}
- name: NO_PROXY
  value: {{ .Values.proxy.no_proxy | quote }}
{{- end -}}
