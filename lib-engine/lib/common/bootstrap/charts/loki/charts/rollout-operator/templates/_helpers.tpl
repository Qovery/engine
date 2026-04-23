{{/*
Expand the name of the chart.
*/}}
{{- define "rollout-operator.name" -}}
{{- default (include "rollout-operator.chartName" .) .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "rollout-operator.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default (include "rollout-operator.chartName" .) .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Recalculate the chart name, because it may be sub-chart included as rollout_operator,
and _ is not valid in resource names.
*/}}
{{- define "rollout-operator.chartName" -}}
{{- print .Chart.Name | replace "_" "-" -}}
{{- end -}}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "rollout-operator.chart" -}}
{{- printf "%s-%s" (include "rollout-operator.chartName" .) .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "rollout-operator.labels" -}}
helm.sh/chart: {{ include "rollout-operator.chart" . }}
{{ include "rollout-operator.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- with .Values.global.commonLabels }}
{{ toYaml . }}
{{- end }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "rollout-operator.selectorLabels" -}}
app.kubernetes.io/name: {{ include "rollout-operator.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "rollout-operator.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "rollout-operator.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}


{{- define "cli.labels" -}}
{{- $list := list -}}
{{- range $k, $v := ( include "rollout-operator.selectorLabels" . | fromYaml ) -}}
{{- $list = append $list (printf "%s=%s" $k $v) -}}
{{- end -}}
{{ join "," $list }}
{{- end -}}

{{/*
Create the image name
*/}}
{{- define "rollout-operator.imageName" -}}
{{- $imageTag := (.Values.image.tag | default .Chart.AppVersion) }}
{{- if .Values.image.registry }}
{{- (printf "%s/%s:%s" .Values.image.registry .Values.image.repository $imageTag) }}
{{- else }}
{{- (printf "%s:%s" .Values.image.repository $imageTag) }}
{{- end -}}
{{- end -}}
