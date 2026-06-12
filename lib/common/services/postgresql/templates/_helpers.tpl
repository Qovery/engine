{{/*
Chart name (overridable).
*/}}
{{- define "postgresql18.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Fully qualified app name. Qovery always sets fullnameOverride to the sanitized_name (kube_name),
so the StatefulSet, headless service and PVCs all derive from it.
*/}}
{{- define "postgresql18.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name (include "postgresql18.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{/*
Primary service name. Defaults to fullname but Qovery overrides it with `service_name` (fqdn_id),
matching the Bitnami chart's primary-service naming so connection hosts stay identical.
*/}}
{{- define "postgresql18.primary.svc.name" -}}
{{- default (include "postgresql18.fullname" .) .Values.service.name -}}
{{- end -}}

{{/*
Headless service name: `<fullname>-hl`.
*/}}
{{- define "postgresql18.headless.svc.name" -}}
{{- printf "%s-hl" (include "postgresql18.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Selector labels — stable and immutable; must be a subset of the pod template labels.
*/}}
{{- define "postgresql18.selectorLabels" -}}
app.kubernetes.io/name: {{ include "postgresql18.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: primary
{{- end -}}

{{/*
Common metadata labels = selector labels + chart-managed labels + Qovery commonLabels
(which carry `qovery.com/service-id`, the engine's workload selector).
*/}}
{{- define "postgresql18.labels" -}}
{{ include "postgresql18.selectorLabels" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- with .Values.commonLabels }}
{{ toYaml . }}
{{- end }}
{{- end -}}
