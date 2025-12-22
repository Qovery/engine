{{/* vim: set filetype=mustache: */}}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "keda.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Generate basic labels for CRD
*/}}
{{- define "keda.crd-labels" }}
helm.sh/chart: {{ include "keda.chart" . }}
app.kubernetes.io/component: operator
app.kubernetes.io/managed-by: {{ .Values.customManagedBy | default .Release.Service }}
app.kubernetes.io/part-of: {{ .Values.operator.name }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion }}
{{- end }}
{{- end }}

{{/*
Generate basic labels
*/}}
{{- define "keda.labels" -}}
{{- include "keda.crd-labels" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- if .Values.additionalLabels }}
{{ toYaml .Values.additionalLabels }}
{{- end }}
{{- end }}
