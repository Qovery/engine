{{/*
Chart name (overridable).
*/}}
{{- define "mongodb.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Fully qualified app name. Qovery always sets fullnameOverride to the sanitized_name (kube_name),
so the StatefulSet, headless service and PVCs all derive from it.
*/}}
{{- define "mongodb.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name (include "mongodb.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{/*
Primary service name. Defaults to fullname but Qovery overrides it with `service_name` (fqdn_id),
matching the Bitnami chart's primary-service naming so connection hosts stay identical.
*/}}
{{- define "mongodb.primary.svc.name" -}}
{{- default (include "mongodb.fullname" .) .Values.service.name -}}
{{- end -}}

{{/*
Headless service name: `<fullname>-hl`.
*/}}
{{- define "mongodb.headless.svc.name" -}}
{{- printf "%s-hl" (include "mongodb.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Selector labels — stable and immutable; must be a subset of the pod template labels.
*/}}
{{- define "mongodb.selectorLabels" -}}
app.kubernetes.io/name: {{ include "mongodb.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: primary
{{- end -}}

{{/*
Common metadata labels = selector labels + chart-managed labels + Qovery commonLabels
(which carry `qovery.com/service-id`, the engine's workload selector).
*/}}
{{- define "mongodb.labels" -}}
{{ include "mongodb.selectorLabels" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- with .Values.commonLabels }}
{{ toYaml . }}
{{- end }}
{{- end -}}
